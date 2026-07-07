//! LLM provider — the OpenAI-compatible data-plane's metering,
//! pre-authorization, and model-resolution core. The HTTP layer authenticates a
//! bearer to a contract, calls [`LlmProvider::preauth_cost_wei`] before
//! forwarding a request to the model backend (llama.cpp / vLLM / Ollama), then
//! [`LlmProvider::cost_wei`] on the returned `usage` and charges the ledger via
//! `ContractLedger::draw_down`.
//!
//! Design: `docs/chiral-book.md` → "Data-Plane API: Inference (LLM)" and
//! "Provider Implementation" → LLM. The forward to the backend is the wire
//! layer's job (reqwest); this module owns the billing logic, which is what
//! must be exact — token counts come from the backend's `usage` block, and
//! pre-auth bounds the worst case so a request never exhausts mid-stream.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::resource_offer::{ResourceClass, ResourceOffer};

#[derive(Debug, Clone, Copy)]
pub struct TokenRates {
    pub per_1k_input_wei: u128,
    pub per_1k_output_wei: u128,
}

/// The `usage` block an OpenAI-compatible backend returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

/// Per-provider model catalog + pricing.
#[derive(Debug, Default)]
pub struct LlmProvider {
    models: HashMap<String, TokenRates>,
}

impl LlmProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model(mut self, id: &str, rates: TokenRates) -> Self {
        self.models.insert(id.to_string(), rates);
        self
    }

    /// Build from an inference offer: `capacity.models = [{ "id": ... }]` and
    /// `price_schedule = { "<id>": { "per_1k_input_wei": "..", "per_1k_output_wei": ".." } }`.
    pub fn from_offer(offer: &ResourceOffer) -> Result<Self, String> {
        if offer.resource_class != ResourceClass::Inference {
            return Err("offer is not an inference offer".to_string());
        }
        let models = offer
            .capacity
            .get("models")
            .and_then(|m| m.as_array())
            .ok_or("offer.capacity.models missing")?;
        let mut p = LlmProvider::new();
        for m in models {
            let id = m
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or("model entry missing id")?;
            let rate = offer
                .price_schedule
                .get(id)
                .ok_or_else(|| format!("no price for model {}", id))?;
            let per_in = wei_field(rate, "per_1k_input_wei")?;
            let per_out = wei_field(rate, "per_1k_output_wei")?;
            p.models.insert(
                id.to_string(),
                TokenRates {
                    per_1k_input_wei: per_in,
                    per_1k_output_wei: per_out,
                },
            );
        }
        Ok(p)
    }

    pub fn has_model(&self, model: &str) -> bool {
        self.models.contains_key(model)
    }

    pub fn model_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.models.keys().cloned().collect();
        v.sort();
        v
    }

    /// Exact cost of a completed request, ceil-rounded per direction so the
    /// provider never under-collects. `None` if the model isn't served.
    pub fn cost_wei(&self, model: &str, prompt_tokens: u64, completion_tokens: u64) -> Option<u128> {
        let r = self.models.get(model)?;
        Some(
            ceil_div(prompt_tokens as u128 * r.per_1k_input_wei, 1000)
                + ceil_div(completion_tokens as u128 * r.per_1k_output_wei, 1000),
        )
    }

    /// Worst-case pre-authorization: the prompt plus the full `max_tokens` of
    /// output. The caller refuses the request (`402 insufficient_quota`) if this
    /// exceeds the contract balance, so a request never exhausts mid-stream.
    pub fn preauth_cost_wei(&self, model: &str, est_prompt_tokens: u64, max_tokens: u64) -> Option<u128> {
        self.cost_wei(model, est_prompt_tokens, max_tokens)
    }
}

/// Cheap token estimate for pre-authorization (~4 chars / token). The real
/// tokenizer runs at the backend; this only *bounds* pre-auth, and actual
/// billing always uses the backend's returned `usage`.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

fn wei_field(v: &serde_json::Value, key: &str) -> Result<u128, String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("missing {}", key))?
        .parse::<u128>()
        .map_err(|e| format!("bad {}: {}", key, e))
}

fn ceil_div(n: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    n.div_ceil(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider() -> LlmProvider {
        LlmProvider::new().with_model(
            "llama-3.1-70b-instruct",
            TokenRates {
                per_1k_input_wei: 1_000_000,
                per_1k_output_wei: 2_000_000,
            },
        )
    }

    #[test]
    fn cost_math_ceil_per_direction() {
        let p = provider();
        // 1500 in * 1e6/1k + 500 out * 2e6/1k = 1_500_000 + 1_000_000
        assert_eq!(
            p.cost_wei("llama-3.1-70b-instruct", 1500, 500),
            Some(2_500_000)
        );
        // Sub-1k rounds up, not down.
        assert_eq!(p.cost_wei("llama-3.1-70b-instruct", 1, 0), Some(1_000)); // ceil(1*1e6/1000)
    }

    #[test]
    fn unknown_model_has_no_cost() {
        let p = provider();
        assert!(!p.has_model("gpt-4"));
        assert_eq!(p.cost_wei("gpt-4", 10, 10), None);
        assert_eq!(p.preauth_cost_wei("gpt-4", 10, 10), None);
    }

    #[test]
    fn preauth_is_worst_case() {
        let p = provider();
        // Pre-auth charges prompt + full max_tokens; actual is usually less.
        let pre = p.preauth_cost_wei("llama-3.1-70b-instruct", 100, 1000).unwrap();
        let actual = p.cost_wei("llama-3.1-70b-instruct", 100, 200).unwrap();
        assert!(pre > actual);
    }

    #[test]
    fn from_offer_parses_models_and_rates() {
        let offer = ResourceOffer {
            provider_wallet: "0xp".into(),
            resource_class: ResourceClass::Inference,
            capacity: json!({ "models": [ { "id": "llama-3.1-70b-instruct", "context_len": 131072 } ] }),
            price_schedule: json!({
                "llama-3.1-70b-instruct": { "per_1k_input_wei": "1000000", "per_1k_output_wei": "2000000" }
            }),
            endpoint: "https://p".into(),
            region: "".into(),
            min_funding_wei: "0".into(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        };
        let p = LlmProvider::from_offer(&offer).unwrap();
        assert_eq!(p.model_ids(), vec!["llama-3.1-70b-instruct".to_string()]);
        assert_eq!(p.cost_wei("llama-3.1-70b-instruct", 1000, 1000), Some(3_000_000));
    }

    #[test]
    fn from_offer_rejects_wrong_class() {
        let mut offer = ResourceOffer {
            provider_wallet: "0xp".into(),
            resource_class: ResourceClass::Storage,
            capacity: json!({}),
            price_schedule: json!({}),
            endpoint: "https://p".into(),
            region: "".into(),
            min_funding_wei: "0".into(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        };
        assert!(LlmProvider::from_offer(&offer).is_err());
        offer.resource_class = ResourceClass::Inference;
        offer.capacity = json!({ "models": [] });
        assert!(LlmProvider::from_offer(&offer).unwrap().model_ids().is_empty());
    }

    #[test]
    fn token_estimate() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2); // ceil(5/4)
    }
}

//! Concrete private llama.cpp boundary, pinned to cd959bae99ff4bed22bef2604f45241c09bbb8c8.
//! Only bounded OpenAI routes are public. Native template/tokenizer and sampled
//! token metadata remain private. Disconnect cancels work without a final usage
//! checkpoint; unknown usage must never become a customer debit.
use crate::error::ApiError;
use crate::protocol::{ModelSpec, TokenUsage};
use crate::provider_config::{self, ModelConfig};
use axum::http::StatusCode;
use futures_util::StreamExt;
use reqwest::{Client, Response};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_EMBEDDING_BATCH: usize = 32;
pub const MAX_SSE_EVENT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Chat,
    Completions,
    Embeddings,
}
impl Endpoint {
    pub fn capability(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Completions => "completions",
            Self::Embeddings => "embeddings",
        }
    }
    pub fn path(self) -> &'static str {
        match self {
            Self::Chat => "/v1/chat/completions",
            Self::Completions => "/v1/completions",
            Self::Embeddings => "/v1/embeddings",
        }
    }
}

pub struct PreparedRequest {
    pub endpoint: Endpoint,
    pub body: Value,
    pub stream: bool,
    pub include_usage: bool,
    pub max_output_tokens: u64,
    pub prompt_tokens: u64,
    pub embedding_count: usize,
    pub logprobs: Option<usize>,
    pub echo_prompt: Option<String>,
}


fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::bad_request("unsupported_parameter", message)
}
pub fn upstream_error() -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "engine_error", "The inference engine failed or returned an invalid response; inspect request status before retrying")
}
fn object(value: &Value) -> Result<&Map<String, Value>, ApiError> {
    value
        .as_object()
        .ok_or_else(|| invalid("Expected a JSON object"))
}
fn fields(value: &Value, allowed: &[&str]) -> Result<(), ApiError> {
    if object(value)?
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(invalid("Request contains an unsupported field"));
    }
    Ok(())
}
fn text(value: Option<&Value>, max: usize) -> Result<&str, ApiError> {
    value
        .and_then(Value::as_str)
        .filter(|s| s.len() <= max)
        .ok_or_else(|| invalid("Expected a bounded string"))
}
fn boolean(value: Option<&Value>, default: bool) -> Result<bool, ApiError> {
    match value {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| invalid("Expected a boolean")),
    }
}
fn unsigned(value: &Value, min: u64, max: u64) -> Result<u64, ApiError> {
    value
        .as_u64()
        .filter(|v| (min..=max).contains(v))
        .ok_or_else(|| invalid("Integer parameter exceeds its supported range"))
}
fn number(value: &Value, min: f64, max: f64) -> Result<(), ApiError> {
    if value
        .as_f64()
        .is_some_and(|v| v.is_finite() && (min..=max).contains(&v))
    {
        Ok(())
    } else {
        Err(invalid("Numeric parameter exceeds its supported range"))
    }
}
fn capability(model: &ModelSpec, name: &str) -> Result<(), ApiError> {
    if model.capabilities.iter().any(|c| c == name) {
        Ok(())
    } else {
        Err(invalid(
            "This model does not advertise the requested capability",
        ))
    }
}
fn schema(value: &Value, depth: usize) -> Result<(), ApiError> {
    if depth > 32 {
        return Err(invalid("Schema nesting exceeds 32 levels"));
    }
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref") {
                if !reference.as_str().is_some_and(|r| r.starts_with('#')) {
                    return Err(invalid("Remote schema references are not supported"));
                }
            }
            for child in map.values() {
                schema(child, depth + 1)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                schema(child, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn function_name(value: Option<&Value>) -> Result<(), ApiError> {
    let name = text(value, 64)?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(invalid("Invalid function name"));
    }
    Ok(())
}
fn validate_messages(body: &Value, model: &ModelSpec) -> Result<(), ApiError> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .filter(|m| !m.is_empty() && m.len() <= 256)
        .ok_or_else(|| invalid("messages must contain between 1 and 256 text messages"))?;
    for message in messages {
        fields(
            message,
            &["role", "content", "name", "tool_calls", "tool_call_id"],
        )?;
        let role = text(message.get("role"), 16)?;
        if !matches!(role, "system" | "developer" | "user" | "assistant" | "tool") {
            return Err(invalid("Unsupported message role"));
        }
        if let Some(name) = message.get("name") {
            text(Some(name), 64)?;
        }
        match message.get("content") {
            Some(Value::String(_)) => {}
            Some(Value::Array(parts)) if !parts.is_empty() && parts.len() <= 64 => {
                for part in parts {
                    fields(part, &["type", "text"])?;
                    if part.get("type").and_then(Value::as_str) != Some("text") {
                        return Err(invalid(
                            "Only text content is supported; remote media is forbidden",
                        ));
                    }
                    text(part.get("text"), MAX_REQUEST_BYTES)?;
                }
            }
            None | Some(Value::Null)
                if role == "assistant" && message.get("tool_calls").is_some() => {}
            _ => return Err(invalid("Messages require text content")),
        }
        if role == "tool" {
            capability(model, "tools")?;
            text(message.get("tool_call_id"), 128)?;
        } else if message.get("tool_call_id").is_some() {
            return Err(invalid("tool_call_id requires the tool role"));
        }
        if let Some(calls) = message.get("tool_calls") {
            capability(model, "tools")?;
            if role != "assistant" {
                return Err(invalid("tool_calls requires the assistant role"));
            }
            let calls = calls
                .as_array()
                .filter(|c| !c.is_empty() && c.len() <= 32)
                .ok_or_else(|| invalid("Invalid tool_calls"))?;
            for call in calls {
                fields(call, &["id", "type", "function"])?;
                text(call.get("id"), 128)?;
                if call.get("type").and_then(Value::as_str) != Some("function") {
                    return Err(invalid("Only function tool calls are supported"));
                }
                let function = call
                    .get("function")
                    .ok_or_else(|| invalid("Missing tool function"))?;
                fields(function, &["name", "arguments"])?;
                function_name(function.get("name"))?;
                text(function.get("arguments"), 32 * 1024)?;
            }
        }
    }
    Ok(())
}

/// Strictly allowlisted, bounded OpenAI subset. Backend extensions that can
/// alter tokenization, expand generation, access URLs or execute code are absent.
pub fn prepare(
    mut body: Value,
    endpoint: Endpoint,
    model: &ModelSpec,
) -> Result<PreparedRequest, ApiError> {
    let common = [
        "model",
        "stream",
        "stream_options",
        "max_tokens",
        "n",
        "temperature",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
        "seed",
        "stop",
        "user",
        "logprobs",
        "logit_bias",
    ];
    let chat = [
        "messages",
        "max_completion_tokens",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "response_format",
        "top_logprobs",
    ];
    let completion = ["prompt", "echo"];
    let embedding = ["model", "input", "encoding_format", "dimensions", "user"];
    for key in object(&body)?.keys() {
        let permitted = if endpoint == Endpoint::Embeddings {
            embedding.contains(&key.as_str())
        } else {
            common.contains(&key.as_str())
                || if endpoint == Endpoint::Chat {
                    chat.contains(&key.as_str())
                } else {
                    completion.contains(&key.as_str())
                }
        };
        if !permitted {
            return Err(invalid("Request contains an unsupported parameter (including generation expansion or engine-only options)"));
        }
    }
    if text(body.get("model"), 256)? != model.id {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "model_scope",
            "Credential is not authorized for this model",
        ));
    }
    capability(model, endpoint.capability())?;
    // Optional OpenAI nulls have default semantics, but required input cannot be null.
    body.as_object_mut().unwrap().retain(|key, v| {
        !v.is_null() || matches!(key.as_str(), "model" | "messages" | "prompt" | "input")
    });
    if let Some(user) = body.get("user") {
        text(Some(user), 256)?;
    }
    let stream = boolean(body.get("stream"), false)?;
    let include_usage = if let Some(options) = body.get("stream_options") {
        if !stream {
            return Err(invalid("stream_options requires stream=true"));
        }
        fields(options, &["include_usage"])?;
        boolean(options.get("include_usage"), false)?
    } else {
        false
    };
    let mut embedding_count = 0;
    if endpoint == Endpoint::Embeddings {
        let input = body
            .get("input")
            .ok_or_else(|| invalid("Missing embedding input"))?;
        match input {
            Value::String(s) if !s.is_empty() => embedding_count = 1,
            Value::Array(values) if !values.is_empty() && values.len() <= MAX_EMBEDDING_BATCH => {
                for value in values {
                    if text(Some(value), MAX_REQUEST_BYTES)?.is_empty() {
                        return Err(invalid("Embedding inputs must not be empty"));
                    }
                }
                embedding_count = values.len();
            }
            _ => {
                return Err(invalid(
                    "Embedding input must be text or a batch of at most 32 strings",
                ))
            }
        }
        if let Some(format) = body.get("encoding_format") {
            if !matches!(format.as_str(), Some("float" | "base64")) {
                return Err(invalid("encoding_format must be float or base64"));
            }
        }
        if let Some(dimensions) = body.get("dimensions") {
            unsigned(dimensions, 1, 16_384)?;
        }
    } else {
        if let Some(n) = body.get("n") {
            unsigned(n, 1, 1)?;
        }
        for (name, min, max) in [
            ("temperature", 0.0, 2.0),
            ("top_p", 0.0, 1.0),
            ("frequency_penalty", -2.0, 2.0),
            ("presence_penalty", -2.0, 2.0),
        ] {
            if let Some(value) = body.get(name) {
                number(value, min, max)?;
            }
        }
        if let Some(seed) = body.get("seed") {
            if seed.as_i64().is_none() {
                return Err(invalid("seed must be an i64"));
            }
        }
        if let Some(stop) = body.get("stop") {
            match stop {
                Value::String(s) if !s.is_empty() && s.len() <= 256 => {}
                Value::Array(values) if values.len() <= 4 => {
                    for v in values {
                        if text(Some(v), 256)?.is_empty() {
                            return Err(invalid("Stop strings must not be empty"));
                        }
                    }
                }
                _ => return Err(invalid("stop allows at most four strings of 256 bytes")),
            }
        }
        if let Some(bias) = body.get("logit_bias") {
            let map = object(bias)?;
            if map.len() > 256 {
                return Err(invalid("logit_bias exceeds 256 entries"));
            }
            for (key, value) in map {
                key.parse::<u32>()
                    .map_err(|_| invalid("Invalid bias token id"))?;
                number(value, -100.0, 100.0)?;
            }
        }
        if endpoint == Endpoint::Chat {
            validate_messages(&body, model)?;
            boolean(body.get("logprobs"), false)?;
            if let Some(top) = body.get("top_logprobs") {
                unsigned(top, 0, 5)?;
            }
            boolean(body.get("parallel_tool_calls"), true)?;
            if let Some(tools) = body.get("tools") {
                capability(model, "tools")?;
                let tools = tools
                    .as_array()
                    .filter(|t| !t.is_empty() && t.len() <= 32)
                    .ok_or_else(|| invalid("tools requires 1 to 32 functions"))?;
                for tool in tools {
                    fields(tool, &["type", "function"])?;
                    if tool.get("type").and_then(Value::as_str) != Some("function") {
                        return Err(invalid(
                            "Only function descriptions are allowed; tools are never executed",
                        ));
                    }
                    let function = tool
                        .get("function")
                        .ok_or_else(|| invalid("Missing function definition"))?;
                    fields(function, &["name", "description", "parameters", "strict"])?;
                    function_name(function.get("name"))?;
                    if let Some(description) = function.get("description") {
                        text(Some(description), 4096)?;
                    }
                    if boolean(function.get("strict"), false)? {
                        return Err(invalid(
                            "Strict tool schemas are not part of the qualified auto-tool profile",
                        ));
                    }
                    if let Some(parameters) = function.get("parameters") {
                        object(parameters)?;
                        schema(parameters, 0)?;
                    }
                }
                // Preserve the qualified auto-tool public contract. The private
                // template endpoint receives the exact same choices as generation.
                if body
                    .get("tool_choice")
                    .is_some_and(|v| v.as_str() != Some("auto"))
                {
                    return Err(invalid(
                        "Qualified tool calling supports tool_choice=auto only",
                    ));
                }
                body["tool_choice"] = json!("auto");
            } else if body
                .get("tool_choice")
                .is_some_and(|v| v.as_str() != Some("none"))
            {
                return Err(invalid("tool_choice requires tools"));
            }
            if let Some(format) = body.get("response_format") {
                fields(format, &["type", "json_schema"])?;
                match format.get("type").and_then(Value::as_str) {
                    Some("text") => {}
                    Some("json_object") => capability(model, "structured_output")?,
                    Some("json_schema") => {
                        capability(model, "structured_output")?;
                        let spec = format
                            .get("json_schema")
                            .ok_or_else(|| invalid("Missing JSON schema"))?;
                        fields(spec, &["name", "description", "schema", "strict"])?;
                        function_name(spec.get("name"))?;
                        if let Some(description) = spec.get("description") {
                            text(Some(description), 4096)?;
                        }
                        boolean(spec.get("strict"), false)?;
                        let schema_value = spec
                            .get("schema")
                            .ok_or_else(|| invalid("Missing schema object"))?;
                        object(schema_value)?;
                        schema(schema_value, 0)?;
                    }
                    _ => return Err(invalid("Unsupported response format")),
                }
            }
            body["add_generation_prompt"] = json!(true);
            body["continue_final_message"] = json!(false);
            body["chat_template_kwargs"] = json!({"enable_thinking":false});
        } else {
            text(body.get("prompt"), MAX_REQUEST_BYTES)?;
            boolean(body.get("echo"), false)?;
            if let Some(logprobs) = body.get("logprobs") {
                unsigned(logprobs, 0, 5)?;
            }
        }
    }
    let max_output_tokens = if endpoint == Endpoint::Embeddings {
        0
    } else {
        if body.get("max_tokens").is_some() && body.get("max_completion_tokens").is_some() {
            return Err(invalid("Specify only one output token limit"));
        }
        let value = body
            .get("max_completion_tokens")
            .or_else(|| body.get("max_tokens"));
        let maximum = value
            .map(|v| unsigned(v, 1, model.max_output_tokens))
            .transpose()?
            .unwrap_or(model.max_output_tokens.min(256));
        body.as_object_mut()
            .unwrap()
            .remove("max_completion_tokens");
        body["max_tokens"] = json!(maximum);
        body["n"] = json!(1);
        body["stream"] = json!(stream);
        if stream {
            body["stream_options"] = json!({"include_usage":true});
        }
        maximum
    };
    let logprobs = if endpoint == Endpoint::Chat {
        (body["logprobs"].as_bool() == Some(true))
            .then(|| body["top_logprobs"].as_u64().unwrap_or(0) as usize)
    } else {
        body["logprobs"].as_u64().map(|n| n as usize)
    };
    let echo_prompt = if endpoint == Endpoint::Completions && body["echo"].as_bool() == Some(true) {
        Some(text(body.get("prompt"), MAX_REQUEST_BYTES)?.to_owned())
    } else { None };
    if endpoint != Endpoint::Embeddings {
        // n_probs is the native sampled-token metadata setting. Unlike injecting
        // OpenAI logprobs it also works with the pinned streaming tool parser.
        let map = body.as_object_mut().unwrap();
        map.remove("logprobs");
        map.remove("top_logprobs");
        map.remove("echo");
        body["n_probs"] = json!(logprobs.map(|n| n.max(1)).unwrap_or(usize::from(endpoint == Endpoint::Chat)));
        body["cache_prompt"] = json!(false);
        body["return_tokens"] = json!(true);
        if endpoint == Endpoint::Completions {
            body["verbose"] = json!(true);
            // The final verbose envelope otherwise repeats the entire prompt and
            // model settings, needlessly exceeding the bounded SSE event size.
            body["response_fields"] = json!(["tokens", "truncated"]);
        }
    }
    Ok(PreparedRequest {
        endpoint,
        body,
        stream,
        include_usage,
        max_output_tokens,
        prompt_tokens: 0,
        embedding_count,
        logprobs,
        echo_prompt,
    })
}

#[derive(Clone)]
pub struct Backend {
    client: Client,
    base: String,
    pub config: ModelConfig,
}
impl Backend {
    pub fn new(config: &ModelConfig) -> Result<Self, ApiError> {
        let url =
            provider_config::parse_origin(&config.engine_url).map_err(|_| upstream_error())?;
        let private = match url.host() {
            Some(url::Host::Ipv4(ip)) => provider_config::is_private_engine_ip(ip.into()),
            Some(url::Host::Ipv6(ip)) => provider_config::is_private_engine_ip(ip.into()),
            Some(url::Host::Domain("localhost")) => true,
            _ => false,
        };
        if !private || !matches!(url.scheme(), "http" | "https") {
            return Err(upstream_error());
        }
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(path) = &config.engine_api_key_file {
            let secret = provider_config::read_secret(path).map_err(|_| upstream_error())?;
            let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {secret}"))
                .map_err(|_| upstream_error())?;
            header.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, header);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .pool_max_idle_per_host(config.max_concurrency as usize)
            .build()
            .map_err(|_| upstream_error())?;
        Ok(Self {
            client,
            base: config.engine_url.trim_end_matches('/').into(),
            config: config.clone(),
        })
    }
    async fn get_json(&self, path: &str) -> Result<Value, ApiError> {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .map_err(|_| upstream_error())?;
        read_json(response, MAX_RESPONSE_BYTES).await
    }
    pub async fn ready(&self) -> Result<(), ApiError> {
        let response = self
            .client
            .get(format!("{}/health", self.base))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|_| upstream_error())?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "engine_unavailable",
                "Model engine is not ready",
            ))
        }
    }
    async fn post_json(&self, path: &str, body: &Value) -> Result<Value, ApiError> {
        let response = self.client.post(format!("{}{path}", self.base))
            .json(body).send().await.map_err(|_| upstream_error())?;
        read_json(response, MAX_RESPONSE_BYTES).await
    }
    async fn tokenize_one(&self, content: &Value) -> Result<Vec<Value>, ApiError> {
        let mut value = self.post_json("/tokenize", &json!({
            "content": content, "add_special": true, "parse_special": true
        })).await?;
        let tokens = value.get_mut("tokens").and_then(Value::as_array_mut)
            .ok_or_else(upstream_error)?;
        // An input the engine prepares into no tokens is invalid input, as an empty
        // embedding input is in `prepare`; only a longer list is a context overflow.
        if tokens.is_empty() {
            return Err(invalid("Input is prepared into no tokens; supply non-empty input"));
        }
        if tokens.len() as u64 > self.config.model.context_length {
            return Err(ApiError::bad_request("context_length_exceeded", "Tokenized prompt exceeds the advertised context"));
        }
        if tokens.iter().any(|token| token.as_u64().is_none_or(|id| id > i32::MAX as u64)) {
            return Err(upstream_error());
        }
        Ok(std::mem::take(tokens))
    }
    pub async fn tokenize(&self, request: &mut PreparedRequest) -> Result<(), ApiError> {
        if request.endpoint == Endpoint::Chat {
            // The pinned endpoint calls exactly the same chat session/template
            // renderer as generation, including tools and structured-output options.
            let rendered = self.post_json("/apply-template", &request.body).await?;
            let prompt = rendered.get("prompt").filter(|v| v.is_string()).ok_or_else(upstream_error)?;
            request.prompt_tokens = self.tokenize_one(prompt).await?.len() as u64;
        } else if request.endpoint == Endpoint::Completions {
            let tokens = self.tokenize_one(&request.body["prompt"]).await?;
            request.prompt_tokens = tokens.len() as u64;
            // Arrays of IDs bypass any second tokenization or BOS insertion.
            request.body["prompt"] = Value::Array(tokens);
        } else {
            let input = request.body.as_object_mut().unwrap().remove("input").ok_or_else(upstream_error)?;
            let inputs = match input {
                Value::String(_) => vec![input],
                Value::Array(values) => values,
                _ => return Err(upstream_error()),
            };
            let mut token_inputs = Vec::with_capacity(inputs.len());
            for input in inputs {
                let tokens = self.tokenize_one(&input).await?;
                request.prompt_tokens = request.prompt_tokens.checked_add(tokens.len() as u64).ok_or_else(upstream_error)?;
                token_inputs.push(Value::Array(tokens));
            }
            request.body["input"] = Value::Array(token_inputs);
        }
        let context = self.config.model.context_length;
        let total_limit = if request.endpoint == Endpoint::Embeddings {
            context.checked_mul(MAX_EMBEDDING_BATCH as u64)
        } else {
            Some(context)
        };
        if request
            .prompt_tokens
            .checked_add(request.max_output_tokens)
            .zip(total_limit)
            .is_none_or(|(tokens, limit)| tokens > limit)
        {
            return Err(ApiError::bad_request(
                "context_length_exceeded",
                "Prompt plus output limit exceeds the contracted context",
            ));
        }
        Ok(())
    }
    pub async fn dispatch(
        &self,
        request: &PreparedRequest,
        request_id: &str,
    ) -> Result<Response, ApiError> {
        let response = self
            .client
            .post(format!("{}{}", self.base, request.endpoint.path()))
            .header("X-Request-Id", request_id)
            .json(&request.body)
            .send()
            .await
            .map_err(|_| upstream_error())?;
        if !response.status().is_success() {
            // Do not echo engine exception text, model paths, or credentials to clients.
            return Err(if response.status().as_u16() == 429 {
                ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "engine_saturated",
                    "Model engine is saturated",
                )
            } else {
                upstream_error()
            });
        }
        let expected = if request.stream {
            "text/event-stream"
        } else {
            "application/json"
        };
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.split(';').next() == Some(expected))
        {
            return Err(upstream_error());
        }
        Ok(response)
    }
    /// Readiness for one model. With `probe_inference`, also runs real inference
    /// for every advertised capability and returns the native embedding dimension
    /// its embedding probes measured; `None` for a model without `embeddings`, or
    /// when no inference ran.
    async fn qualification_probe(&self, probe_inference: bool) -> Result<Option<u64>, ApiError> {
        self.ready().await?;
        if self.config.model.serving.is_some() {
            let path = self.config.qualification_evidence_file.as_ref().ok_or_else(upstream_error)?;
            crate::performance::validate_evidence(
                &self.config.model, path, crate::protocol::now().map_err(|_| upstream_error())?,
            ).map_err(|_| upstream_error())?;
            if self.config.model.serving.as_ref().is_none_or(|p| p.engine != "llama.cpp" || p.engine_version != crate::engine_runtime::ENGINE_VERSION) {
                return Err(upstream_error());
            }
        }
        let catalog = self.get_json("/v1/models").await?;
        let model = catalog
            .get("data")
            .and_then(Value::as_array)
            .and_then(|models| {
                models
                    .iter()
                    .find(|m| m.get("id").and_then(Value::as_str) == Some(&self.config.model.id))
            })
            .ok_or_else(upstream_error)?;
        if model.get("id").and_then(Value::as_str) != Some(self.config.model.id.as_str()) {
            return Err(upstream_error());
        }
        let props = self.get_json("/props").await?;
        // A version string alone does not identify the reviewed private protocol.
        let commit = props["build_info"].as_str().and_then(|s| s.split_once('-')).map(|(_, c)| c);
        if commit.is_none_or(|c| c.len() < 7 || !"cd959bae99ff4bed22bef2604f45241c09bbb8c8".starts_with(c)) {
            return Err(upstream_error());
        }
        if props["default_generation_settings"]["n_ctx"].as_u64() != Some(self.config.model.context_length)
            || props["total_slots"].as_u64() != Some(u64::from(self.config.max_concurrency))
            || props["model_alias"].as_str() != Some(self.config.model.id.as_str())
        {
            return Err(upstream_error());
        }
        if self.config.model.capabilities.iter().any(|c| c == "chat") {
            let template = props["chat_template"].as_str().ok_or_else(upstream_error)?;
            let digest = hex::encode(Sha256::digest(template.as_bytes()));
            if !digest.eq_ignore_ascii_case(self.config.model.chat_template_sha256.trim_start_matches("0x"))
                || (self.config.model.capabilities.iter().any(|c| c == "tools")
                    && props.get("chat_template_tool_use").and_then(Value::as_str)
                        .is_some_and(|tools| tools != template))
            {
                return Err(upstream_error());
            }
        }
        // Every advertised capability needs a probe on an endpoint the model serves:
        // `streaming` is exercised by the streamed chat or completion probes below,
        // `tools` and `structured_output` by chat probes. Checked here so that it does
        // not depend on the probe order or on whether inference is repeated.
        if !unprobeable_capabilities(&self.config.model.capabilities).is_empty() {
            return Err(upstream_error());
        }
        if !probe_inference { return Ok(None); }
        let mut embedding_dimensions = None;
        for endpoint in [Endpoint::Chat, Endpoint::Completions, Endpoint::Embeddings] {
            if !self
                .config
                .model
                .capabilities
                .iter()
                .any(|c| c == endpoint.capability())
            {
                continue;
            }
            let body = match endpoint {
                Endpoint::Chat => {
                    json!({"model":self.config.model.id,"messages":[{"role":"user","content":"Reply with OK."}],"max_tokens":1})
                }
                Endpoint::Completions => {
                    json!({"model":self.config.model.id,"prompt":"A","max_tokens":1})
                }
                Endpoint::Embeddings => {
                    json!({"model":self.config.model.id,"input":["A","MiXeD case"],"encoding_format":"float"})
                }
            };
            let dimensions = self.probe(body.clone(), endpoint).await?;
            if endpoint == Endpoint::Embeddings {
                let mut encoded = body;
                encoded["encoding_format"] = json!("base64");
                // Both encodings must agree on the one native dimension that later
                // requests naming `dimensions` are checked against.
                if dimensions.is_none() || self.probe(encoded, endpoint).await? != dimensions {
                    return Err(upstream_error());
                }
                embedding_dimensions = dimensions;
            } else {
                let mut streaming = body;
                streaming["stream"] = json!(true);
                streaming["logprobs"] = if endpoint == Endpoint::Chat { json!(true) } else { json!(1) };
                if endpoint == Endpoint::Chat { streaming["top_logprobs"] = json!(1); }
                self.probe(streaming, endpoint).await?;
                if endpoint == Endpoint::Completions {
                    self.probe(json!({"model":self.config.model.id,"prompt":ECHO_PROBE_PROMPT,
                        "max_tokens":1,"temperature":0,"echo":true,"logprobs":0,"stream":true}), endpoint).await?;
                }
            }
        }
        if self.config.model.capabilities.iter().any(|c| c == "tools") {
            let mut body = json!({"model":self.config.model.id,"messages":[{"role":"user","content":"Call the ping function now. Do not respond with text."}],"max_tokens":self.config.model.max_output_tokens.min(64),"temperature":0,"tools":[{"type":"function","function":{"name":"ping","parameters":{"type":"object","properties":{}}}}]});
            self.probe(body.clone(), Endpoint::Chat).await?;
            body["stream"] = json!(true);
            self.probe(body, Endpoint::Chat).await?;
        }
        if self
            .config
            .model
            .capabilities
            .iter()
            .any(|c| c == "structured_output")
        {
            let mut body = json!({"model":self.config.model.id,"messages":[{"role":"user","content":"Return JSON {}."}],"max_tokens":self.config.model.max_output_tokens.min(64),"temperature":0,"response_format":{"type":"json_schema","json_schema":{"name":"probe","schema":{"type":"object","properties":{},"additionalProperties":false},"strict":true}}});
            self.probe(body.clone(), Endpoint::Chat).await?;
            body["stream"] = json!(true);
            self.probe(body, Endpoint::Chat).await?;
        }
        Ok(embedding_dimensions)
    }
    /// One readiness request. Returns the vectors' common length for an embedding
    /// probe, and `None` otherwise.
    async fn probe(&self, body: Value, endpoint: Endpoint) -> Result<Option<u64>, ApiError> {
        let mut request = prepare(body, endpoint, &self.config.model)?;
        self.tokenize(&mut request).await?;
        let response = self
            .dispatch(&request, &uuid::Uuid::new_v4().to_string())
            .await?;
        if request.stream {
            let spec = StreamSpec {
                endpoint,
                model: self.config.model.id.clone(),
                prompt: request.prompt_tokens,
                output: request.max_output_tokens,
                include_usage: false,
            };
            let mut stream = response.bytes_stream();
            let mut decoder = SseDecoder::default();
            let mut validation = StreamValidation::default();
            let mut bytes = 0usize;
            let mut content = String::new();
            let mut tool_name = String::new();
            let mut arguments = String::new();
            let expected_echo = request.echo_prompt.as_deref().is_some();
            let mut echo_seen = false;
            let mut normalization = StreamNormalization::new(request.logprobs, request.echo_prompt.take(), self.config.model.context_length);
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| upstream_error())?;
                bytes = bytes.checked_add(chunk.len()).ok_or_else(upstream_error)?;
                if bytes > MAX_RESPONSE_BYTES {
                    return Err(upstream_error());
                }
                for byte in chunk {
                    if let Some(data) = decoder.byte(byte)? {
                        if data == b"[DONE]" {
                            if !validation.finished || !validation.final_usage || (expected_echo && !echo_seen) {
                                return Err(upstream_error());
                            }
                            if request.body.get("tools").is_some()
                                && (tool_name != "ping" || serde_json::from_str::<Value>(&arguments).ok() != Some(json!({}))) {
                                return Err(upstream_error());
                            }
                            if request.body.get("response_format").is_some()
                                && serde_json::from_str::<Value>(&content).ok() != Some(json!({})) {
                                return Err(upstream_error());
                            }
                            return Ok(None);
                        }
                        let value = serde_json::from_slice(&data).map_err(|_| upstream_error())?;
                        for mut value in normalization.events(value, endpoint)?.into_iter().flatten() {
                            validation.event(&mut value, &spec)?;
                            if value["chiral_echo"].as_bool() == Some(true) {
                                echo_seen = value["choices"][0]["text"].as_str() == Some(ECHO_PROBE_PROMPT);
                            }
                            let delta = &value["choices"][0]["delta"];
                            if let Some(text) = delta["content"].as_str() { content.push_str(text); }
                            if let Some(calls) = delta["tool_calls"].as_array() {
                                for call in calls {
                                    if call["index"].as_u64() != Some(0) { return Err(upstream_error()); }
                                    if let Some(name) = call["function"]["name"].as_str() { tool_name.push_str(name); }
                                    if let Some(fragment) = call["function"]["arguments"].as_str() { arguments.push_str(fragment); }
                                }
                            }
                        }
                    }
                }
            }
            return Err(upstream_error());
        }
        let mut value = read_json(response, MAX_RESPONSE_BYTES).await?;
        normalize_embedding_usage(&mut value, endpoint)?;
        let (usage, dimensions) = validate_response_dimensions(&value, &request)?;
        validate_authorized_usage(&usage, request.prompt_tokens, request.max_output_tokens)?;
        if request.body.get("tools").is_some() {
            let choice = &value["choices"][0];
            if choice["finish_reason"].as_str() != Some("tool_calls") {
                return Err(upstream_error());
            }
            let calls = choice["message"]["tool_calls"]
                .as_array()
                .filter(|calls| calls.len() == 1)
                .ok_or_else(upstream_error)?;
            if calls[0]["function"]["name"].as_str() != Some("ping") {
                return Err(upstream_error());
            }
            let arguments: Value = serde_json::from_str(
                calls[0]["function"]["arguments"]
                    .as_str()
                    .ok_or_else(upstream_error)?,
            )
            .map_err(|_| upstream_error())?;
            if arguments != json!({}) {
                return Err(upstream_error());
            }
        }
        if request.body.get("response_format").is_some() {
            let content = value["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(upstream_error)?;
            if serde_json::from_str::<Value>(content).map_err(|_| upstream_error())? != json!({}) {
                return Err(upstream_error());
            }
        }
        Ok(dimensions)
    }
}

pub async fn read_json(response: Response, limit: usize) -> Result<Value, ApiError> {
    if !response.status().is_success()
        || response.content_length().is_some_and(|n| n > limit as u64)
    {
        return Err(upstream_error());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| upstream_error())?;
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(upstream_error());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| upstream_error())
}

/// Embeddings have no generation phase. The pinned engine omits the redundant
/// completion count; require its two authoritative input totals to agree.
pub fn normalize_embedding_usage(value: &mut Value, endpoint: Endpoint) -> Result<(), ApiError> {
    if endpoint != Endpoint::Embeddings { return Ok(()); }
    let usage = value.get_mut("usage").and_then(Value::as_object_mut).ok_or_else(upstream_error)?;
    let prompt = usage.get("prompt_tokens").and_then(Value::as_u64).ok_or_else(upstream_error)?;
    if usage.get("total_tokens").and_then(Value::as_u64) != Some(prompt)
        || usage.get("completion_tokens").is_some_and(|n| n.as_u64() != Some(0))
    { return Err(upstream_error()); }
    usage.insert("completion_tokens".into(), json!(0));
    Ok(())
}

/// Normalizes the pinned server's two OpenAI stream dialects. Token IDs are
/// sampled engine IDs, never re-tokenized text or an SSE event count.
pub(crate) struct StreamNormalization {
    logprobs: Option<usize>,
    echo_prompt: Option<String>,
    text_offset: usize,
    context_length: u64,
}
impl StreamNormalization {
    pub(crate) fn new(logprobs: Option<usize>, echo_prompt: Option<String>, context_length: u64) -> Self {
        let text_offset = echo_prompt.as_ref().map_or(0, |p| p.chars().count());
        Self { logprobs, echo_prompt, text_offset, context_length }
    }
    pub(crate) fn events(&mut self, mut value: Value, endpoint: Endpoint) -> Result<[Option<Value>; 2], ApiError> {
        let mut ids = None;
        if endpoint == Endpoint::Completions {
            // Upstream also marks an ordinary no-context-shift stop at the exact
            // context boundary as "truncated". Accept only that terminal case;
            // StreamValidation still verifies exact input and authorized output.
            if value["__verbose"]["truncated"].as_bool() == Some(true)
                && (!matches!(value["choices"][0]["finish_reason"].as_str(), Some("length" | "stop"))
                    || value["usage"]["total_tokens"].as_u64() != Some(self.context_length))
            {
                return Err(upstream_error());
            }
            if let Some(verbose) = value.get_mut("__verbose") {
                ids = verbose.get_mut("tokens").map(Value::take);
            }
        }
        let root = value.as_object_mut().ok_or_else(upstream_error)?;
        root.retain(|key, _| matches!(key.as_str(),
            "id" | "object" | "created" | "model" | "choices" | "usage" | "system_fingerprint" | "service_tier"));
        let choices = value.get_mut("choices").and_then(Value::as_array_mut).ok_or_else(upstream_error)?;
        let has_choices = !choices.is_empty();
        if let Some(choice) = choices.first_mut() {
            choice.as_object_mut().ok_or_else(upstream_error)?.retain(|key, _| matches!(
                key.as_str(), "index" | "text" | "delta" | "logprobs" | "finish_reason" | "stop_reason"));
            let mut probabilities = choice.get_mut("logprobs")
                .and_then(|v| v.get_mut("content")).map(Value::take);
            if let Some(probabilities) = probabilities.as_mut() {
                let probabilities = probabilities.as_array_mut().ok_or_else(upstream_error)?;
                if ids.is_none() {
                    let mut sampled = Vec::with_capacity(probabilities.len());
                    for p in probabilities.iter() {
                        let id = p["id"].as_u64().filter(|id| *id <= i32::MAX as u64).ok_or_else(upstream_error)?;
                        sampled.push(json!(id));
                    }
                    ids = Some(Value::Array(sampled));
                }
                for p in probabilities.iter_mut().filter(|_| self.logprobs.is_some()) {
                    let p = p.as_object_mut().ok_or_else(upstream_error)?;
                    p.retain(|key, _| matches!(key.as_str(), "token" | "bytes" | "logprob" | "top_logprobs"));
                    if p.get("token").and_then(Value::as_str).is_none()
                        || p.get("logprob").and_then(Value::as_f64).is_none_or(|n| !n.is_finite()) {
                        return Err(upstream_error());
                    }
                    let top = p.get_mut("top_logprobs").and_then(Value::as_array_mut).ok_or_else(upstream_error)?;
                    top.truncate(self.logprobs.unwrap_or(0));
                    for candidate in top {
                        let candidate = candidate.as_object_mut().ok_or_else(upstream_error)?;
                        candidate.retain(|key, _| matches!(key.as_str(), "token" | "bytes" | "logprob"));
                        if candidate.get("token").and_then(Value::as_str).is_none()
                            || candidate.get("logprob").and_then(Value::as_f64).is_none_or(|n| !n.is_finite()) {
                            return Err(upstream_error());
                        }
                    }
                }
            }
            if let Some(ids) = ids {
                if ids.as_array().is_none_or(|ids| ids.iter().any(|id| id.as_u64().is_none_or(|id| id > i32::MAX as u64))) {
                    return Err(upstream_error());
                }
                choice["token_ids"] = ids;
            }
            if self.logprobs.is_none() {
                choice.as_object_mut().ok_or_else(upstream_error)?.remove("logprobs");
            } else if endpoint == Endpoint::Chat {
                if let Some(probabilities) = probabilities {
                    choice["logprobs"] = json!({"content": probabilities});
                }
            } else if let Some(Value::Array(probabilities)) = probabilities {
                let mut tokens = Vec::with_capacity(probabilities.len());
                let mut token_logprobs = Vec::with_capacity(probabilities.len());
                let mut top_logprobs = Vec::with_capacity(probabilities.len());
                let mut offsets = Vec::with_capacity(probabilities.len());
                for mut p in probabilities {
                    let token = p["token"].as_str().ok_or_else(upstream_error)?;
                    offsets.push(json!(self.text_offset));
                    self.text_offset += token.chars().count();
                    tokens.push(p["token"].take());
                    token_logprobs.push(p["logprob"].take());
                    let mut top = Map::new();
                    for candidate in p["top_logprobs"].as_array().ok_or_else(upstream_error)? {
                        top.insert(candidate["token"].as_str().ok_or_else(upstream_error)?.into(), candidate["logprob"].clone());
                    }
                    top_logprobs.push(Value::Object(top));
                }
                choice["logprobs"] = json!({"tokens":tokens,"token_logprobs":token_logprobs,
                    "top_logprobs":top_logprobs,"text_offset":offsets});
            }
        }
        // Text completions attach authoritative final usage to the finish choice;
        // chat supplies a separate empty-choice checkpoint. Canonicalize once.
        if endpoint == Endpoint::Completions
            && value["choices"][0]["finish_reason"].as_str().is_some()
            && value.get("usage").is_some()
        {
            if let Some(prompt) = self.echo_prompt.take() {
                if value["choices"][0]["text"].as_str() != Some("") { return Err(upstream_error()); }
                value["choices"][0]["text"] = json!(prompt);
                value["chiral_echo"] = json!(true);
            }
            let usage = value.as_object_mut().unwrap().remove("usage").unwrap();
            let mut checkpoint = json!({"choices":[],"usage":usage});
            for field in ["id", "object", "created", "model"] {
                checkpoint[field] = value[field].clone();
            }
            return Ok([Some(value), Some(checkpoint)]);
        }
        if endpoint == Endpoint::Completions && has_choices {
            if let Some(prompt) = self.echo_prompt.take() {
                // Input probabilities are unavailable from this engine; never
                // invent them or confuse echoed input with sampled output.
                let mut echo = json!({"chiral_echo":true,"choices":[{"index":0,"text":prompt,"logprobs":null,
                    "finish_reason":null,"token_ids":[]}]});
                for field in ["id", "object", "created", "model"] { echo[field] = value[field].clone(); }
                return Ok([Some(echo), Some(value)]);
            }
        }
        Ok([Some(value), None])
    }
}

pub fn parse_usage(value: &Value) -> Result<TokenUsage, ApiError> {
    let prompt_tokens = value
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(upstream_error)?;
    let completion_tokens = value
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(upstream_error)?;
    if value.get("total_tokens").and_then(Value::as_u64)
        != prompt_tokens.checked_add(completion_tokens)
    {
        return Err(upstream_error());
    }
    Ok(TokenUsage {
        prompt_tokens,
        completion_tokens,
    })
}
pub fn validate_authorized_usage(
    usage: &TokenUsage,
    prompt: u64,
    output: u64,
) -> Result<(), ApiError> {
    if usage.prompt_tokens != prompt || usage.completion_tokens > output {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "engine_usage_mismatch", "Engine usage does not match the authorized tokenization; the request will not be charged"));
    }
    Ok(())
}
pub fn validate_response(value: &Value, request: &PreparedRequest) -> Result<TokenUsage, ApiError> {
    validate_response_dimensions(value, request).map(|(usage, _)| usage)
}
/// `validate_response`, also returning an embedding batch's common vector length.
pub(crate) fn validate_response_dimensions(
    value: &Value,
    request: &PreparedRequest,
) -> Result<(TokenUsage, Option<u64>), ApiError> {
    if value.get("error").is_some()
        || value.get("model").and_then(Value::as_str)
            != request.body.get("model").and_then(Value::as_str)
    {
        return Err(upstream_error());
    }
    let expected = match request.endpoint {
        Endpoint::Chat => "chat.completion",
        Endpoint::Completions => "text_completion",
        Endpoint::Embeddings => "list",
    };
    if value.get("object").and_then(Value::as_str) != Some(expected) {
        return Err(upstream_error());
    }
    let mut batch_dimensions = None;
    if request.endpoint == Endpoint::Embeddings {
        let data = value
            .get("data")
            .and_then(Value::as_array)
            .filter(|d| d.len() == request.embedding_count)
            .ok_or_else(upstream_error)?;
        let format = request
            .body
            .get("encoding_format")
            .and_then(Value::as_str)
            .unwrap_or("float");
        let requested_dimensions = request.body.get("dimensions").and_then(Value::as_u64);
        for (index, item) in data.iter().enumerate() {
            if item.get("index").and_then(Value::as_u64) != Some(index as u64)
                || item.get("object").and_then(Value::as_str) != Some("embedding")
            {
                return Err(upstream_error());
            }
            let dimensions = match (format, item.get("embedding")) {
                ("float", Some(Value::Array(values)))
                    if !values.is_empty()
                        && values.len() <= 16_384
                        && values
                            .iter()
                            .all(|v| v.as_f64().is_some_and(f64::is_finite)) =>
                {
                    values.len()
                }
                ("base64", Some(Value::String(encoded)))
                    if !encoded.is_empty() && encoded.len() <= 16_384 * 8 =>
                {
                    // Validate bounded float32 data without allocating another vector.
                    let mut decoder = base64::read::DecoderReader::new(
                        encoded.as_bytes(),
                        &base64::engine::general_purpose::STANDARD,
                    );
                    let mut dimensions = 0;
                    let mut scalar = [0u8; 4];
                    while std::io::Read::read(&mut decoder, &mut scalar[..1])
                        .map_err(|_| upstream_error())?
                        != 0
                    {
                        std::io::Read::read_exact(&mut decoder, &mut scalar[1..])
                            .map_err(|_| upstream_error())?;
                        if dimensions == 16_384 || !f32::from_le_bytes(scalar).is_finite() {
                            return Err(upstream_error());
                        }
                        dimensions += 1;
                    }
                    if dimensions == 0 {
                        return Err(upstream_error());
                    }
                    dimensions
                }
                _ => return Err(upstream_error()),
            };
            if requested_dimensions.is_some_and(|expected| expected != dimensions as u64)
                || batch_dimensions.is_some_and(|expected| expected != dimensions)
            {
                return Err(upstream_error());
            }
            batch_dimensions = Some(dimensions);
        }
    } else {
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .filter(|c| c.len() == 1)
            .ok_or_else(upstream_error)?;
        let choice = &choices[0];
        if choice.get("index").and_then(Value::as_u64) != Some(0)
            || !matches!(
                choice.get("finish_reason").and_then(Value::as_str),
                Some("stop" | "length" | "tool_calls" | "content_filter")
            )
        {
            return Err(upstream_error());
        }
        if request.endpoint == Endpoint::Chat {
            let message = choice.get("message").ok_or_else(upstream_error)?;
            object(message)?;
            if message.get("role").and_then(Value::as_str) != Some("assistant")
                || (!message.get("content").is_some_and(Value::is_string)
                    && !message
                        .get("tool_calls")
                        .and_then(Value::as_array)
                        .is_some_and(|calls| !calls.is_empty()))
            {
                return Err(upstream_error());
            }
        } else if choice.get("text").and_then(Value::as_str).is_none() {
            return Err(upstream_error());
        }
    }
    let usage = parse_usage(value.get("usage").ok_or_else(upstream_error)?)?;
    Ok((usage, batch_dimensions.map(|dimensions: usize| dimensions as u64)))
}

/// Incremental SSE framing shared by live responses and readiness probes.
#[derive(Default)]
pub(crate) struct SseDecoder {
    line: Vec<u8>,
    data: Vec<u8>,
    event_bytes: usize,
}
impl SseDecoder {
    pub(crate) fn byte(&mut self, byte: u8) -> Result<Option<Vec<u8>>, ApiError> {
        self.event_bytes += 1;
        if self.event_bytes > MAX_SSE_EVENT_BYTES {
            return Err(upstream_error());
        }
        if byte != b'\n' {
            self.line.push(byte);
            return Ok(None);
        }
        if self.line.last() == Some(&b'\r') {
            self.line.pop();
        }
        if self.line.is_empty() {
            self.event_bytes = 0;
            if self.data.is_empty() {
                return Ok(None);
            }
            self.data.pop();
            return Ok(Some(std::mem::take(&mut self.data)));
        }
        if let Some(value) = self.line.strip_prefix(b"data:") {
            let value = value.strip_prefix(b" ").unwrap_or(value);
            self.data.extend_from_slice(value);
            self.data.push(b'\n');
        } else if !self.line.starts_with(b":")
            && !self.line.starts_with(b"event:")
            && !self.line.starts_with(b"id:")
            && !self.line.starts_with(b"retry:")
        {
            return Err(upstream_error());
        }
        self.line.clear();
        Ok(None)
    }
}
const ECHO_PROBE_PROMPT: &str = "Chiral verifies exact echoed completion text without counting the input as generated output.";
pub(crate) struct StreamSpec {
    pub endpoint: Endpoint,
    pub model: String,
    pub prompt: u64,
    pub output: u64,
    pub include_usage: bool,
}
#[derive(Default)]
pub(crate) struct StreamValidation {
    pub finished: bool,
    pub final_usage: bool,
    response_id: Option<String>,
}
impl StreamValidation {
    pub(crate) fn event(
        &mut self,
        value: &mut Value,
        spec: &StreamSpec,
    ) -> Result<(bool, Option<TokenUsage>), ApiError> {
        if value.get("error").is_some()
            || value.get("model").and_then(Value::as_str) != Some(spec.model.as_str())
        {
            return Err(upstream_error());
        }
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or_else(upstream_error)?;
        if self
            .response_id
            .as_ref()
            .is_some_and(|previous| previous != id)
        {
            return Err(upstream_error());
        }
        if self.response_id.is_none() {
            self.response_id = Some(id.into());
        }
        let expected = if spec.endpoint == Endpoint::Chat {
            "chat.completion.chunk"
        } else {
            "text_completion"
        };
        if value.get("object").and_then(Value::as_str) != Some(expected) {
            return Err(upstream_error());
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(upstream_error)?;
        if choices.is_empty() {
            if !self.finished || self.final_usage {
                return Err(upstream_error());
            }
            let usage = parse_usage(value.get("usage").ok_or_else(upstream_error)?)?;
            validate_authorized_usage(&usage, spec.prompt, spec.output)?;
            self.final_usage = true;
            return Ok((spec.include_usage, Some(usage)));
        }
        if self.finished
            || self.final_usage
            || choices.len() != 1
            || choices[0].get("index").and_then(Value::as_u64) != Some(0)
        {
            return Err(upstream_error());
        }
        if spec.endpoint == Endpoint::Chat {
            if !choices[0].get("delta").is_some_and(Value::is_object) {
                return Err(upstream_error());
            }
        } else if choices[0].get("text").and_then(Value::as_str).is_none() {
            return Err(upstream_error());
        }
        if let Some(finish) = choices[0].get("finish_reason").filter(|v| !v.is_null()) {
            if !matches!(
                finish.as_str(),
                Some("stop" | "length" | "tool_calls" | "content_filter")
            ) {
                return Err(upstream_error());
            }
            self.finished = true;
        }
        // Continuous statistics are not an authoritative final-usage checkpoint.
        if !spec.include_usage {
            value
                .as_object_mut()
                .ok_or_else(upstream_error)?
                .remove("usage");
        }
        Ok((true, None))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelHealth {
    pub model_id: String,
    pub ready: bool,
    pub detail: String,
    /// The native embedding dimension this pass's embedding probes measured.
    /// Private readiness state: never reported, listed or signed.
    #[serde(skip)]
    pub embedding_dimensions: Option<u64>,
}
/// The advertised capabilities that no readiness probe can exercise on the model's
/// endpoints (public-inference-api requirement 8): `streaming` is probed only by the
/// streamed chat or completion probes, `tools` and `structured_output` only by chat
/// probes. A model advertising any of them can never become ready.
pub(crate) fn unprobeable_capabilities(capabilities: &[String]) -> Vec<&'static str> {
    let advertises = |name: &str| capabilities.iter().any(|c| c == name);
    let mut unprobeable = Vec::new();
    if advertises("streaming") && !advertises("chat") && !advertises("completions") {
        unprobeable.push("streaming");
    }
    for name in ["tools", "structured_output"] {
        if advertises(name) && !advertises("chat") {
            unprobeable.push(name);
        }
    }
    unprobeable
}
/// The readiness reason of a model whose managed engine has exited, from the readiness
/// pass or, until the next pass, from the engine watcher.
pub const ENGINE_EXITED: &str = "Managed engine exited";
/// Parent caches publication readiness; this performs real inference, not a TCP probe.
pub async fn model_health(state: &crate::app::ProviderState) -> Vec<ModelHealth> {
    let config = &state.config;
    let mut models = config.models.iter();
    let mut probes = futures_util::stream::FuturesUnordered::new();
    let mut health = Vec::with_capacity(config.models.len());
    for model in models.by_ref().take(4) {
        probes.push(probe_model(model, state));
    }
    while let Some(result) = probes.next().await {
        health.push(result);
        if let Some(model) = models.next() {
            probes.push(probe_model(model, state));
        }
    }
    health
}

async fn probe_model(model: &ModelConfig, state: &crate::app::ProviderState) -> ModelHealth {
    if !state.engine_alive(&model.model.id) {
        return ModelHealth { model_id: model.model.id.clone(), ready: false, detail: ENGINE_EXITED.into(), embedding_dimensions: None };
    }
    // A capability set that can never pass readiness is a configuration problem; name it
    // for the operator instead of reporting a generic probe failure (task 2.5).
    let unprobeable = unprobeable_capabilities(&model.model.capabilities);
    if !unprobeable.is_empty() {
        return ModelHealth {
            model_id: model.model.id.clone(),
            ready: false,
            detail: format!(
                "Not ready: advertised capabilities without a probe on this model's endpoints: {}; streaming needs chat or completions, and tools and structured_output need chat",
                unprobeable.join(", ")
            ),
            embedding_dimensions: None,
        };
    }
    let permit = state.model_slots[&model.model.id].clone().try_acquire_many_owned(model.max_concurrency).ok();
    if permit.is_none() && state.qualified_model(&model.model).is_none() {
        return ModelHealth { model_id: model.model.id.clone(), ready: false, detail: "Initial qualification awaits idle engine capacity".into(), embedding_dimensions: None };
    }
    let result = match Backend::new(model) {
        Ok(backend) => tokio::time::timeout(
            Duration::from_secs(model.request_timeout_secs.min(60)),
            backend.qualification_probe(permit.is_some()),
        )
        .await
        .map_err(|_| upstream_error())
        .and_then(|r| r),
        Err(error) => Err(error),
    };
    ModelHealth {
        model_id: model.model.id.clone(),
        ready: result.is_ok(),
        detail: if result.is_ok() {
            "Ready: private health, tokenizer/template identity and advertised inference probes passed; artifact/hardware qualification remains operator-owned".into()
        } else {
            "Not ready: private engine, tokenizer/template, capability probe or usage accounting failed".into()
        },
        embedding_dimensions: result.ok().flatten(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> ModelSpec {
        ModelSpec {
            id: "model".into(),
            revision: "a".repeat(40),
            tokenizer_revision: "a".repeat(40),
            chat_template_sha256: "c".repeat(64),
            quantization: None,
            context_length: 4096,
            max_output_tokens: 512,
            serving: None,
            capabilities: vec![
                "chat".into(),
                "completions".into(),
                "embeddings".into(),
                "tools".into(),
                "structured_output".into(),
            ],
        }
    }
    #[test]
    fn rejects_expansion_and_media_before_tokenization() {
        for field in [
            "best_of",
            "use_beam_search",
            "chat_template",
            "chat_template_kwargs",
            "truncate_prompt_tokens",
            "logits_processors",
            "mm_processor_kwargs",
            "return_tokens",
            "n_probs",
            "verbose",
            "cache_prompt",
            "reasoning_format",
            "response_fields",
        ] {
            let mut body = json!({"model":"model","messages":[{"role":"user","content":"hello"}]});
            body[field] = json!(1);
            assert!(prepare(body, Endpoint::Chat, &model()).is_err());
        }
        assert!(prepare(
            json!({"model":"model","n":2,"prompt":"hi"}),
            Endpoint::Completions,
            &model()
        )
        .is_err());
        assert!(prepare(json!({"model":"model","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"http://127.0.0.1"}}]}]}), Endpoint::Chat, &model()).is_err());
        assert!(prepare(
            json!({"model":"model","prompt":"hi","max_tokens":513}),
            Endpoint::Completions,
            &model()
        )
        .is_err());
    }
    #[test]
    fn embedding_batch_and_schema_boundaries() {
        assert!(prepare(
            json!({"model":"model","input":vec!["x";33]}),
            Endpoint::Embeddings,
            &model()
        )
        .is_err());
        assert!(prepare(
            json!({"model":"model","input":[1,2,3]}),
            Endpoint::Embeddings,
            &model()
        )
        .is_err());
        assert!(schema(&json!({"$ref":"https://attacker/schema"}), 0).is_err());
        assert!(schema(&json!({"$ref":"#/$defs/local"}), 0).is_ok());
    }
    #[test]
    fn invalid_embedding_payloads_cannot_be_settled_as_success() {
        use base64::Engine as _;
        let mut request = prepare(
            json!({"model":"model","input":["x"],"encoding_format":"base64","dimensions":2}),
            Endpoint::Embeddings,
            &model(),
        )
        .unwrap();
        let response = |embedding: Value| json!({"object":"list","model":"model","data":[{"object":"embedding","index":0,"embedding":embedding}],"usage":{"prompt_tokens":1,"completion_tokens":0,"total_tokens":1}});
        let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        for bad in [
            json!("not base64!"),
            json!(encode(&[1, 2, 3])),
            json!(encode(&f32::INFINITY.to_le_bytes())),
            json!(encode(&[0u8; 4])),
            json!([0.25, -0.5]),
        ] {
            assert!(validate_response(&response(bad), &request).is_err());
        }
        let valid = [0.25f32.to_le_bytes(), (-0.5f32).to_le_bytes()].concat();
        assert_eq!(
            validate_response(&response(json!(encode(&valid))), &request)
                .unwrap()
                .prompt_tokens,
            1
        );
        request.body["encoding_format"] = json!("float");
        assert!(validate_response(&response(json!(encode(&valid))), &request).is_err());
        assert!(validate_response(&response(json!([0.25])), &request).is_err());
        assert_eq!(
            validate_response(&response(json!([0.25, -0.5])), &request)
                .unwrap()
                .completion_tokens,
            0
        );
    }
    #[test]
    fn accounting_requires_exact_prompt_and_bounded_output() {
        let usage =
            parse_usage(&json!({"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}))
                .unwrap();
        assert!(validate_authorized_usage(&usage, 12, 3).is_ok());
        assert!(validate_authorized_usage(&usage, 13, 3).is_err());
        assert!(validate_authorized_usage(&usage, 12, 2).is_err());
        assert!(
            parse_usage(&json!({"prompt_tokens":12,"completion_tokens":3,"total_tokens":14}))
                .is_err()
        );
    }
    fn stream_spec(include_usage: bool) -> StreamSpec {
        StreamSpec {
            endpoint: Endpoint::Chat,
            model: "model".into(),
            prompt: 12,
            output: 8,
            include_usage,
        }
    }
    fn stream_chunk(choices: Value, usage: Value) -> Value {
        json!({"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"model","choices":choices,"usage":usage})
    }
    #[test]
    fn hidden_usage_is_retained_once_not_estimated_from_chunks() {
        let mut state = StreamValidation::default();
        let spec = stream_spec(false);
        for content in ["a", "many words inside one network event", ""] {
            let mut chunk = stream_chunk(
                json!([{"index":0,"delta":{"content":content},"finish_reason":null}]),
                json!({"prompt_tokens":12,"completion_tokens":999,"total_tokens":1011}),
            );
            let (forward, usage) = state.event(&mut chunk, &spec).unwrap();
            assert!(forward);
            assert!(usage.is_none());
            assert!(chunk.get("usage").is_none());
        }
        let mut finish = stream_chunk(
            json!([{"index":0,"delta":{},"finish_reason":"stop"}]),
            Value::Null,
        );
        assert!(state.event(&mut finish, &spec).unwrap().1.is_none());
        assert!(!state.final_usage);
        let mut final_chunk = stream_chunk(
            json!([]),
            json!({"prompt_tokens":12,"completion_tokens":2,"total_tokens":14}),
        );
        let (forward, usage) = state.event(&mut final_chunk, &spec).unwrap();
        assert!(!forward);
        assert_eq!(usage.unwrap().completion_tokens, 2);
        assert!(state.final_usage);
        assert!(state.event(&mut final_chunk, &spec).is_err());
    }
    #[test]
    fn final_usage_requires_finished_single_choice_and_authorized_counts() {
        let spec = stream_spec(true);
        let mut final_chunk = stream_chunk(
            json!([]),
            json!({"prompt_tokens":12,"completion_tokens":2,"total_tokens":14}),
        );
        assert!(StreamValidation::default()
            .event(&mut final_chunk, &spec)
            .is_err());
        for usage in [
            json!({"prompt_tokens":13,"completion_tokens":2,"total_tokens":15}),
            json!({"prompt_tokens":12,"completion_tokens":9,"total_tokens":21}),
            json!({"prompt_tokens":12,"total_tokens":12}),
        ] {
            let mut state = StreamValidation::default();
            let mut finish = stream_chunk(
                json!([{"index":0,"delta":{},"finish_reason":"length"}]),
                Value::Null,
            );
            state.event(&mut finish, &spec).unwrap();
            assert!(state
                .event(&mut stream_chunk(json!([]), usage), &spec)
                .is_err());
            assert!(!state.final_usage);
        }
        let mut wrong_choice = stream_chunk(
            json!([{"index":1,"delta":{},"finish_reason":null}]),
            Value::Null,
        );
        assert!(StreamValidation::default()
            .event(&mut wrong_choice, &spec)
            .is_err());
        let mut engine_error = json!({"error":{"message":"out of memory"}});
        assert!(StreamValidation::default()
            .event(&mut engine_error, &spec)
            .is_err());
    }
    #[test]
    fn native_embedding_usage_is_canonical_without_inventing_input_totals() {
        let mut response = json!({"usage":{"prompt_tokens":7,"total_tokens":7}});
        normalize_embedding_usage(&mut response, Endpoint::Embeddings).unwrap();
        assert_eq!(parse_usage(&response["usage"]).unwrap().completion_tokens, 0);
        for usage in [
            json!({"prompt_tokens":7,"total_tokens":8}),
            json!({"prompt_tokens":7,"total_tokens":7,"completion_tokens":1}),
            json!({"total_tokens":7}),
        ] {
            assert!(normalize_embedding_usage(&mut json!({"usage":usage}), Endpoint::Embeddings).is_err());
        }
    }
    fn completion_chunk(text: &str, finish: Value, usage: Value) -> Value {
        json!({"id":"completion","object":"text_completion","created":1,"model":"model",
            "choices":[{"index":0,"text":text,"finish_reason":finish,"logprobs":null}],
            "__verbose":{"tokens":[42],"prompt":"private input","generation_settings":{"private":"value"}},
            "timings":{"predicted_ms":1},"usage":usage})
    }
    #[test]
    fn completion_finish_is_split_into_one_authoritative_checkpoint() {
        let mut normalization = StreamNormalization::new(None, None, 4096);
        let events = normalization.events(completion_chunk("", json!("length"),
            json!({"prompt_tokens":12,"completion_tokens":2,"total_tokens":14})), Endpoint::Completions).unwrap();
        let spec = StreamSpec { endpoint: Endpoint::Completions, ..stream_spec(true) };
        let mut validation = StreamValidation::default();
        let mut events = events.into_iter().flatten();
        let mut finish = events.next().unwrap();
        assert!(finish.get("__verbose").is_none());
        assert!(finish.get("timings").is_none());
        assert!(finish.get("usage").is_none());
        assert_eq!(finish["choices"][0]["token_ids"], json!([42]));
        assert!(validation.event(&mut finish, &spec).unwrap().1.is_none());
        let mut checkpoint = events.next().unwrap();
        assert_eq!(validation.event(&mut checkpoint, &spec).unwrap().1.unwrap().completion_tokens, 2);
        assert!(validation.event(&mut checkpoint, &spec).is_err());
    }
    #[test]
    fn context_limit_stop_preserves_valid_usage_but_rejects_early_truncation_and_false_totals() {
        for (context, prompt, output, total, finish, accepted) in [
            (20, 12, 8, 20, "length", true),
            (20, 12, 8, 20, "stop", true),
            (20, 12, 7, 19, "length", false),
            (20, 12, 7, 20, "length", false),
            (20, 13, 7, 20, "length", false),
            (21, 12, 9, 21, "length", false),
            (20, 12, 8, 20, "tool_calls", false),
        ] {
            let mut chunk = completion_chunk("", json!(finish),
                json!({"prompt_tokens":prompt,"completion_tokens":output,"total_tokens":total}));
            chunk["__verbose"]["truncated"] = json!(true);
            chunk["__verbose"]["tokens"] = json!([]);
            let mut validation = StreamValidation::default();
            let spec = StreamSpec { endpoint: Endpoint::Completions, ..stream_spec(true) };
            let result = StreamNormalization::new(None, None, context).events(chunk, Endpoint::Completions)
                .and_then(|events| {
                    for mut event in events.into_iter().flatten() { validation.event(&mut event, &spec)?; }
                    Ok(validation.final_usage)
                });
            assert_eq!(result.unwrap_or(false), accepted, "{context}/{prompt}/{output}/{total}/{finish}");
        }
    }
    #[test]
    fn chat_sample_ids_are_private_and_requested_logprobs_are_preserved() {
        let chunk = || json!({"id":"chat","object":"chat.completion.chunk","model":"model",
            "choices":[{"index":0,"delta":{"content":"OK"},"finish_reason":null,
                "logprobs":{"content":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5,
                    "top_logprobs":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5}]}]}}],
            "timings":{"predicted_n":1}});
        let hidden = StreamNormalization::new(None, None, 4096).events(chunk(), Endpoint::Chat).unwrap()[0].take().unwrap();
        assert_eq!(hidden["choices"][0]["token_ids"], json!([42]));
        assert!(hidden["choices"][0].get("logprobs").is_none());
        assert!(hidden.get("timings").is_none());
        let public = StreamNormalization::new(Some(1), None, 4096).events(chunk(), Endpoint::Chat).unwrap()[0].take().unwrap();
        let probability = &public["choices"][0]["logprobs"]["content"][0];
        assert_eq!(probability["logprob"], -0.5);
        assert!(probability.get("id").is_none());
        assert!(probability["top_logprobs"][0].get("id").is_none());
    }
    #[test]
    fn echo_is_not_sampled_output_and_completion_logprobs_use_public_shape() {
        let mut chunk = completion_chunk(" world", Value::Null, Value::Null);
        chunk["choices"][0]["logprobs"] = json!({"content":[{
            "id":42,"token":" world","bytes":[32,119,111,114,108,100],"logprob":-0.25,"top_logprobs":[]
        }]});
        let mut normalization = StreamNormalization::new(Some(0), Some("Hello".into()), 4096);
        let [echo, output] = normalization.events(chunk, Endpoint::Completions).unwrap();
        let echo = echo.unwrap();
        assert_eq!(echo["choices"][0]["text"], "Hello");
        assert_eq!(echo["choices"][0]["token_ids"], json!([]));
        assert!(echo["choices"][0]["logprobs"].is_null());
        let output = output.unwrap();
        assert_eq!(output["choices"][0]["text"], " world");
        assert_eq!(output["choices"][0]["logprobs"]["text_offset"], json!([5]));
        assert_eq!(output["choices"][0]["logprobs"]["token_logprobs"], json!([-0.25]));
    }
    #[tokio::test]
    async fn tokenized_input_must_leave_room_for_authorized_output() {
        use axum::{routing::post, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = Router::new().route("/tokenize", post(|| async {
            Json(json!({"tokens":vec![42;128]}))
        }));
        let server = tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
        let mut bounded = model();
        bounded.context_length = 128;
        let backend = Backend::new(&ModelConfig {
            model: bounded.clone(),
            price: crate::protocol::PriceSchedule { input_wei_per_1k:"1".into(), output_wei_per_1k:"2".into() },
            engine_url: base, engine_api_key_file: None, max_concurrency:1,
            request_timeout_secs:5, qualification_evidence_file:None, runtime:None,
        }).unwrap();
        let mut request = prepare(json!({"model":"model","prompt":"A ".repeat(128),"max_tokens":1}),
            Endpoint::Completions, &bounded).unwrap();
        let error = backend.tokenize(&mut request).await.unwrap_err();
        assert_eq!((error.status, error.code.as_str()), (StatusCode::BAD_REQUEST, "context_length_exceeded"));
        server.abort();
    }
    /// public-inference-api requirement 6 (task 2.4, design code difference 4): an empty
    /// completion prompt that the engine prepares into no tokens is invalid input, not a
    /// context overflow; one that it prepares into a beginning-of-sequence token is valid.
    #[tokio::test]
    async fn an_input_prepared_into_no_tokens_is_invalid_input_not_a_context_overflow() {
        use axum::{routing::post, Json, Router};
        for tokens in [json!([]), json!([1])] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let prepared = json!({"tokens": tokens});
            let routes = Router::new().route("/tokenize", post(move || async move { Json(prepared) }));
            let server = tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
            let backend = Backend::new(&ModelConfig {
                model: model(),
                price: crate::protocol::PriceSchedule { input_wei_per_1k:"1".into(), output_wei_per_1k:"2".into() },
                engine_url: base, engine_api_key_file: None, max_concurrency:1,
                request_timeout_secs:5, qualification_evidence_file:None, runtime:None,
            }).unwrap();
            let mut request = prepare(json!({"model":"model","prompt":"","max_tokens":1}),
                Endpoint::Completions, &model()).unwrap();
            let result = backend.tokenize(&mut request).await;
            if tokens == json!([]) {
                let error = result.unwrap_err();
                assert_eq!(error.status, StatusCode::BAD_REQUEST);
                assert_ne!(error.code, "context_length_exceeded");
                assert_eq!(error.code, "unsupported_parameter");
                assert!(!error.message.contains("context"), "{}", error.message);
            } else {
                result.unwrap();
                assert_eq!(request.prompt_tokens, 1);
                assert_eq!(request.body["prompt"], json!([1]));
            }
            server.abort();
        }
    }
    /// public-inference-api task 2.5: the pure rule names exactly the advertised
    /// capabilities that no probe can exercise, so the health detail can name them.
    #[test]
    fn unprobeable_capabilities_names_each_capability_without_a_probe() {
        let set = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        for (capabilities, expected) in [
            (&["embeddings", "streaming"][..], &["streaming"][..]),
            (&["streaming"], &["streaming"]),
            (&["embeddings", "tools", "structured_output", "streaming"], &["streaming", "tools", "structured_output"]),
            (&["completions", "streaming", "tools"], &["tools"]),
            (&["completions", "structured_output"], &["structured_output"]),
            (&["chat", "completions", "streaming", "tools", "structured_output"], &[]),
            (&["completions", "streaming"], &[]),
            (&["chat", "streaming"], &[]),
            (&["embeddings"], &[]),
        ] {
            assert_eq!(unprobeable_capabilities(&set(capabilities)), expected, "{capabilities:?}");
        }
    }
    /// public-inference-api requirement 8 (task 2.1, design code difference 1): the rule
    /// that every advertised capability has a probe on an endpoint the model serves is
    /// checked before any inference probe, so a busy re-check that repeats no inference
    /// applies it too; chat and completion models that advertise `streaming`, `tools` or
    /// `structured_output` beside their endpoint stay eligible.
    #[tokio::test]
    async fn every_advertised_capability_needs_a_probe_on_the_model_endpoints() {
        use axum::{routing::get, Json, Router};
        let template = "{{ messages }}";
        let props = json!({"build_info":"b1-cd959ba","total_slots":1,"model_alias":"model",
            "default_generation_settings":{"n_ctx":4096},"chat_template":template});
        let routes = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/v1/models", get(|| async { Json(json!({"data":[{"id":"model"}]})) }))
            .route("/props", get(move || async move { Json(props) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
        for (capabilities, eligible) in [
            (&["chat", "completions", "streaming"][..], true),
            (&["completions", "streaming"], true),
            (&["chat", "streaming"], true),
            (&["chat", "completions", "tools", "structured_output", "streaming"], true),
            (&["embeddings"], true),
            (&["embeddings", "streaming"], false),
            (&["streaming"], false),
            (&["completions", "streaming", "tools"], false),
            (&["completions", "structured_output"], false),
            (&["embeddings", "tools"], false),
            (&["embeddings", "structured_output"], false),
        ] {
            let mut spec = model();
            spec.chat_template_sha256 = hex::encode(Sha256::digest(template.as_bytes()));
            spec.capabilities = capabilities.iter().map(|c| c.to_string()).collect();
            let backend = Backend::new(&ModelConfig {
                model: spec,
                price: crate::protocol::PriceSchedule { input_wei_per_1k:"1".into(), output_wei_per_1k:"2".into() },
                engine_url: base.clone(), engine_api_key_file: None, max_concurrency:1,
                request_timeout_secs:5, qualification_evidence_file:None, runtime:None,
            }).unwrap();
            // Without inference: the identity checks pass, so only the rule decides.
            let result = backend.qualification_probe(false).await;
            assert_eq!(result.as_ref().map_err(|error| error.code.as_str()).copied(),
                if eligible { Ok(None) } else { Err("engine_error") }, "{capabilities:?}");
        }
        server.abort();
    }
    /// outbound-http-safeguards requirement 7 scenario "An engine redirect is not followed"
    /// (task 3.7). An engine that answers every request with `302` and a `Location` naming a
    /// second loopback listener: the readiness probe reports the engine unavailable, and
    /// template rendering (the tokenization path) and an inference dispatch fail as engine
    /// failures. The engine received each request with its key; the second listener accepted
    /// no connection, so neither the engine key nor any request content reached it.
    #[tokio::test]
    async fn an_engine_redirect_is_an_engine_failure_and_its_target_is_never_contacted() {
        use std::io::Write;
        use std::sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex};
        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_address = target.local_addr().unwrap();
        let target_connections = Arc::new(AtomicUsize::new(0));
        let counter = target_connections.clone();
        let target_task = tokio::spawn(async move {
            while let Ok((stream, _)) = target.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorded = seen.clone();
        let engine = axum::Router::new().fallback(
            move |method: axum::http::Method, uri: axum::http::Uri, headers: axum::http::HeaderMap| {
                let recorded = recorded.clone();
                async move {
                    let authorization = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    recorded.lock().unwrap().push(format!("{method} {} {authorization}", uri.path()));
                    (
                        StatusCode::FOUND,
                        [(axum::http::header::LOCATION, format!("http://{target_address}{uri}"))],
                    )
                }
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, engine).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let key = "engine-redirect-key-0123456789abcdef0123";
        let key_file = directory.path().join("engine.token");
        crate::wallet::create_secret_file(&key_file).unwrap().write_all(key.as_bytes()).unwrap();
        let backend = Backend::new(&ModelConfig {
            model: model(),
            price: crate::protocol::PriceSchedule { input_wei_per_1k:"1".into(), output_wei_per_1k:"2".into() },
            engine_url: base, engine_api_key_file: Some(key_file), max_concurrency:1,
            request_timeout_secs:5, qualification_evidence_file:None, runtime:None,
        }).unwrap();
        let ready = backend.ready().await.unwrap_err();
        assert_eq!((ready.status, ready.code.as_str()), (StatusCode::SERVICE_UNAVAILABLE, "engine_unavailable"));
        let body = json!({"model":"model","messages":[{"role":"user","content":"redirected engine prompt"}],"max_tokens":1});
        let mut request = PreparedRequest {
            endpoint: Endpoint::Chat, body, stream: false, include_usage: false, max_output_tokens: 1,
            prompt_tokens: 0, embedding_count: 0, logprobs: None, echo_prompt: None,
        };
        let tokenized = backend.tokenize(&mut request).await.unwrap_err();
        assert_eq!((tokenized.status, tokenized.code.as_str()), (StatusCode::BAD_GATEWAY, "engine_error"));
        let dispatched = backend.dispatch(&request, "request-a").await.err().unwrap();
        assert_eq!((dispatched.status, dispatched.code.as_str()), (StatusCode::BAD_GATEWAY, "engine_error"));
        assert_eq!(*seen.lock().unwrap(), [
            format!("GET /health Bearer {key}"),
            format!("POST /apply-template Bearer {key}"),
            format!("POST /v1/chat/completions Bearer {key}"),
        ]);
        assert_eq!(target_connections.load(Ordering::SeqCst), 0, "the redirect target was contacted");
        server.abort();
        target_task.abort();
    }
}

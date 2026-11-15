//! Thin desktop adapter. All network, wallet, funding and credential logic is shared with the CLI.
use crate::{
    consumer::{Consumer, StreamEvent},
    protocol::{InferenceOffer, Quote, Signed},
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path, sync::Arc};
use tauri::{ipc::Channel, State};
use tokio::sync::{watch, Mutex, RwLock};

#[derive(Default)]
struct DesktopState {
    consumer: RwLock<Option<Arc<Consumer>>>,
    streams: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
}
impl DesktopState {
    async fn cancel_all(&self) {
        for (_, cancellation) in self.streams.lock().await.drain() {
            cancellation.send_replace(true);
        }
    }
}
fn field<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(
        args.get(name)
            .cloned()
            .ok_or_else(|| format!("missing {name}"))?,
    )
    .map_err(|_| format!("invalid {name}"))
}
fn text<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("missing {name}"))
}
fn optional_text<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
}
fn value<T: serde::Serialize>(result: T) -> Result<Value, String> {
    serde_json::to_value(result).map_err(|_| "cannot serialize native response".into())
}
fn ensure_supported() -> Result<(), String> {
    let policy = crate::version::effective_policy();
    if crate::version::compare_to_policy(crate::version::CURRENT_VERSION, &policy)? == "required" {
        return Err("client update required before network operations".into());
    }
    Ok(())
}

#[tauri::command]
async fn consumer_command(
    state: State<'_, DesktopState>,
    operation: String,
    args: Value,
) -> Result<Value, String> {
    if operation == "logout" {
        let mut current = state.consumer.write().await;
        state.cancel_all().await;
        *current = None;
        return Ok(Value::Null);
    }
    if operation == "configure" {
        let next = Arc::new(Consumer::load(Path::new(text(&args, "path")?))?);
        let address = next.address().ok();
        let mut current = state.consumer.write().await;
        state.cancel_all().await;
        *current = Some(next);
        return Ok(json!({"address":address}));
    }
    let current = state.consumer.read().await;
    let consumer = current
        .as_ref()
        .ok_or("load an explicit consumer configuration in Settings first")?;
    match operation.as_str() {
        "wallet_create" => value(consumer.wallet_create()?),
        "wallet_import" => value(consumer.wallet_import_file(Path::new(text(&args, "path")?))?),
        "wallet_export" => {
            consumer.wallet_export_file(Path::new(text(&args, "path")?))?;
            Ok(Value::Null)
        }
        "balance" => consumer.balance().await,
        "history" => consumer.history().await,
        "network_status" => consumer.network_status().await,
        "network_profile" => Ok(consumer.network_profile()),
        "provider_info" => {
            let offer: Signed<InferenceOffer> = field(&args, "offer")?;
            consumer.provider_info(&offer).await
        }
        "accounts" => value(consumer.accounts()?),
        "pending_accounts" => consumer.pending_accounts(),
        "local_requests" => consumer.local_requests(text(&args, "contract_id")?),
        "funding" => consumer.funding(text(&args, "contract_id")?).await,
        "account" => value(consumer.account(text(&args, "contract_id")?).await?),
        "usage" => consumer.usage(text(&args, "contract_id")?).await,
        "receipt" => value(consumer.receipt(text(&args, "contract_id")?).await?),
        "funding_recovery" => consumer.funding_recovery(text(&args, "contract_id")?).await,
        "credentials" => consumer.credentials(text(&args, "contract_id")?).await,
        "credential_export" => consumer.credential_export(
            text(&args, "contract_id")?,
            optional_text(&args, "credential_id"),
        ),
        "credential_revoke" => {
            consumer
                .credential_revoke(text(&args, "contract_id")?, text(&args, "credential_id")?)
                .await
        }
        "request_status" => {
            consumer
                .request_status(text(&args, "contract_id")?, text(&args, "request_id")?)
                .await
        }
        "cancel_request" => {
            consumer
                .cancel_request(text(&args, "contract_id")?, text(&args, "request_id")?)
                .await
        }
        "cancel_funding" => {
            if !field::<bool>(&args, "accept_gas_and_race_risk")? {
                return Err(
                    "explicit acceptance of cancellation gas and transaction race risk is required"
                        .into(),
                );
            }
            consumer
                .cancel_funding(
                    text(&args, "contract_id")?,
                    optional_text(&args, "operation_id"),
                )
                .await
        }
        "provider_admin" => {
            consumer
                .provider_admin(
                    text(&args, "endpoint")?,
                    Path::new(text(&args, "token_file")?),
                    text(&args, "action")?,
                )
                .await
        }
        "diagnostics" => Ok(json!({
            "version":crate::version::CURRENT_VERSION,
            "network":consumer.network_config().identity,
            "chain_status":consumer.network_status().await.map_err(|_| "configured chain unavailable"),
            "active_streams":state.streams.lock().await.len(),
            "wallet_loaded":consumer.address().is_ok(),
        })),
        "discover" | "quote" | "fund" | "topup" | "resume" | "credential_issue"
        | "credential_rotate" | "feedback" | "infer" => {
            ensure_supported()?;
            match operation.as_str() {
                "discover" => value(consumer.discover(optional_text(&args, "model")).await?),
                "quote" => {
                    let offer: Signed<InferenceOffer> = field(&args, "offer")?;
                    value(consumer.quote(&offer, text(&args, "funding_wei")?).await?)
                }
                "fund" => {
                    let offer: Signed<InferenceOffer> = field(&args, "offer")?;
                    let quote: Signed<Quote> = field(&args, "quote")?;
                    value(consumer.fund(&offer, &quote).await?)
                }
                "topup" => value(
                    consumer
                        .topup(text(&args, "contract_id")?, text(&args, "amount_wei")?)
                        .await?,
                ),
                "resume" => value(consumer.resume(text(&args, "contract_id")?).await?),
                "credential_issue" => {
                    consumer
                        .credential_issue(text(&args, "contract_id")?, field(&args, "options")?)
                        .await
                }
                "credential_rotate" => {
                    consumer
                        .credential_rotate(
                            text(&args, "contract_id")?,
                            text(&args, "credential_id")?,
                            field(&args, "options")?,
                        )
                        .await
                }
                "feedback" => {
                    consumer
                        .feedback(
                            text(&args, "contract_id")?,
                            field(&args, "score")?,
                            field(&args, "revision")?,
                        )
                        .await
                }
                "infer" => {
                    consumer
                        .infer(
                            text(&args, "contract_id")?,
                            text(&args, "kind")?,
                            field(&args, "request")?,
                        )
                        .await
                }
                _ => unreachable!("operation matched above"),
            }
        }
        _ => Err("unsupported desktop operation".into()),
    }
}

#[tauri::command]
async fn inference_stream(
    state: State<'_, DesktopState>,
    contract_id: String,
    request: Value,
    on_event: Channel<StreamEvent>,
    kind: Option<String>,
) -> Result<String, String> {
    ensure_supported()?;
    let current = state.consumer.read().await;
    let consumer = current
        .as_ref()
        .ok_or("load a native consumer configuration first")?
        .clone();
    if !matches!(kind.as_deref().unwrap_or("chat"), "chat" | "completions") {
        return Err("only chat and completions support streaming".into());
    }
    let mut streams = state.streams.lock().await;
    if streams.len() >= 8 {
        return Err("desktop concurrent stream limit reached".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (cancel, mut cancelled) = watch::channel(false);
    streams.insert(id.clone(), cancel);
    let active = state.streams.clone();
    let task_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let opening = tokio::select! {
            biased;
            _ = cancelled.changed() => None,
            result = consumer.stream_kind(&contract_id, kind.as_deref().unwrap_or("chat"), request) => Some(result),
        };
        match opening {
            None => {
                let _ = on_event.send(StreamEvent {
                    kind: "cancelled".into(),
                    data: Value::Null,
                });
            }
            Some(Err(message)) => {
                let _ = on_event.send(StreamEvent {
                    kind: "error".into(),
                    data: json!({"message":message}),
                });
            }
            Some(Ok(mut stream)) => {
                let mut terminal = false;
                loop {
                    tokio::select! {
                        biased;
                        _ = cancelled.changed() => {
                            stream.cancel.cancel();
                            let _ = on_event.send(StreamEvent { kind: "cancelled".into(), data: Value::Null });
                            break;
                        },
                        event = stream.events.recv() => match event {
                            Some(event) => {
                                terminal |= matches!(event.kind.as_str(), "done" | "error" | "cancelled");
                                if on_event.send(event).is_err() { break; }
                            },
                            None => {
                                if !terminal {
                                    let _ = on_event.send(StreamEvent { kind: "error".into(), data: json!({"message":"stream ended without completion; inspect request accounting before retrying"}) });
                                }
                                break;
                            },
                        }
                    }
                }
                stream.cancel.cancel();
            }
        }
        active.lock().await.remove(&task_id);
    });
    Ok(id)
}

#[tauri::command]
async fn inference_cancel(state: State<'_, DesktopState>, stream_id: String) -> Result<(), String> {
    // Idempotent: a stream may have completed between the UI click and this command.
    if let Some(cancellation) = state.streams.lock().await.remove(&stream_id) {
        cancellation.send_replace(true);
    }
    Ok(())
}

#[tauri::command]
async fn get_version_status(state: State<'_, DesktopState>) -> Result<Value, String> {
    let current = state.consumer.read().await;
    if let Some(consumer) = current.as_ref() {
        return consumer.version_status().await;
    }
    let policy = crate::version::effective_policy();
    Ok(
        json!({"currentVersion":crate::version::CURRENT_VERSION,"status":crate::version::compare_to_policy(crate::version::CURRENT_VERSION,&policy)?,"policy":policy}),
    )
}

pub fn run() {
    crate::version::log_policy_key_status();
    tauri::Builder::default()
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            consumer_command,
            inference_stream,
            inference_cancel,
            get_version_status
        ])
        .run(tauri::generate_context!())
        .expect("desktop runtime failed");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_invalid_command_fields_are_rejected() {
        assert!(text(&json!({"path":""}), "path").is_err());
        assert!(field::<u8>(&json!({"score":256}), "score").is_err());
        assert!(field::<Signed<Quote>>(&json!({"quote":{"id":"untrusted"}}), "quote").is_err());
        assert_eq!(optional_text(&json!({"model":null}), "model"), None);
    }

    #[tokio::test]
    async fn logout_cancellation_reaches_pending_streams_and_is_idempotent() {
        let state = DesktopState::default();
        let (sender, mut receiver) = watch::channel(false);
        state.streams.lock().await.insert("pending".into(), sender);
        state.cancel_all().await;
        receiver.changed().await.expect("cancellation notification");
        assert!(*receiver.borrow());
        assert!(state.streams.lock().await.is_empty());
        state.cancel_all().await;
    }
}

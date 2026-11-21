//! Thin desktop adapter. All network, wallet, funding and credential logic is shared with the CLI.
use crate::{
    consumer::{Consumer, StreamEvent},
    protocol::{InferenceOffer, Quote, Signed},
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path, sync::Arc};
use tauri::{ipc::Channel, AppHandle, Emitter, Manager, State};
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

/// JavaScript's `Number.MAX_SAFE_INTEGER`, 2^53 - 1: every integer of at most this magnitude
/// survives the webview's IPC JSON parsing into an IEEE-754 double unchanged.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Operations whose results present an account snapshot, a signed receipt, request records or
/// usage. The webview only displays them; unlike discovered offers and quotes, which it hands
/// back verbatim to `quote` and `fund`, it never returns them for native verification.
const EXACT_PRESENTATION: [&str; 11] = [
    "accounts",
    "account",
    "fund",
    "topup",
    "resume",
    "receipt",
    "usage",
    "request_status",
    "cancel_request",
    "local_requests",
    "infer",
];

/// The webview's copy of a native statement. Its IPC JSON is parsed into doubles, so an integer
/// beyond ±(2^53 - 1), such as an unsigned 64-bit token counter, receipt sequence or timestamp,
/// is delivered as its exact decimal string instead of a silently rounded number. Safe
/// integers, floats and strings (canonical wei included) are unchanged. Only this presentation
/// copy changes: the wire, signatures, persisted state and the CLI's JSON integers do not.
fn exact_integers(value: Value) -> Value {
    match value {
        Value::Number(number) => {
            let beyond = match (number.as_u64(), number.as_i64()) {
                (Some(unsigned), _) => unsigned > MAX_SAFE_INTEGER,
                (None, Some(signed)) => signed.unsigned_abs() > MAX_SAFE_INTEGER,
                // A float is already the double JavaScript parses.
                (None, None) => false,
            };
            if beyond {
                Value::String(number.to_string())
            } else {
                Value::Number(number)
            }
        }
        Value::Array(items) => Value::Array(items.into_iter().map(exact_integers).collect()),
        Value::Object(members) => Value::Object(
            members
                .into_iter()
                .map(|(name, member)| (name, exact_integers(member)))
                .collect(),
        ),
        other => other,
    }
}

/// A native result as `consumer_command` delivers it to the webview.
fn presented(operation: &str, result: Value) -> Value {
    if EXACT_PRESENTATION.contains(&operation) {
        exact_integers(result)
    } else {
        result
    }
}

/// A provider stream event as the webview receives it: its usage counters and timestamps are
/// presented like an `infer` result.
fn webview_event(event: StreamEvent) -> StreamEvent {
    StreamEvent {
        kind: event.kind,
        data: exact_integers(event.data),
    }
}

/// The forwarding loop of `inference_stream`: delivers each event of an opened provider stream to
/// the webview channel as its `webview_event` copy until the stream ends, the channel closes or
/// the desktop cancels it. `cancel_provider` stops the provider stream before the cancellation
/// event is sent.
async fn forward_stream(
    events: &mut tokio::sync::mpsc::Receiver<StreamEvent>,
    cancel_provider: impl Fn(),
    cancelled: &mut watch::Receiver<bool>,
    on_event: &Channel<StreamEvent>,
) {
    let mut terminal = false;
    loop {
        tokio::select! {
            biased;
            _ = cancelled.changed() => {
                cancel_provider();
                let _ = on_event.send(StreamEvent { kind: "cancelled".into(), data: Value::Null });
                break;
            },
            event = events.recv() => match event {
                Some(event) => {
                    terminal |= matches!(event.kind.as_str(), "done" | "error" | "cancelled");
                    if on_event.send(webview_event(event)).is_err() { break; }
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
    consumer_operation(&state, operation, args).await
}

/// The body of `consumer_command`, on the desktop state itself, so that tests drive the same
/// dispatch without a Tauri application.
async fn consumer_operation(
    state: &DesktopState,
    operation: String,
    args: Value,
) -> Result<Value, String> {
    let result = dispatch(state, operation.clone(), args).await?;
    Ok(presented(&operation, result))
}

/// The native result of one consumer operation, before its webview presentation.
async fn dispatch(state: &DesktopState, operation: String, args: Value) -> Result<Value, String> {
    if operation == "logout" {
        end_session(state).await;
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
        "agreement" => consumer.agreement(text(&args, "contract_id")?),
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
                forward_stream(&mut stream.events, || stream.cancel.cancel(), &mut cancelled, &on_event).await;
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

/// Close the native session: cancel streams, drop the consumer and forget the network's
/// effective version policy, so no status from that network is reported afterwards.
async fn end_session(state: &DesktopState) {
    let mut current = state.consumer.write().await;
    state.cancel_all().await;
    *current = None;
    crate::version::reset_effective_policy();
}

/// The version status when no profile is loaded: the effective (after logout, bundled) policy.
fn status_without_profile() -> Result<Value, String> {
    let policy = crate::version::effective_policy();
    Ok(
        json!({"currentVersion":crate::version::CURRENT_VERSION,"status":crate::version::compare_to_policy(crate::version::CURRENT_VERSION,&policy)?,"policy":policy}),
    )
}

#[tauri::command]
async fn get_version_status(state: State<'_, DesktopState>) -> Result<Value, String> {
    let current = state.consumer.read().await;
    if let Some(consumer) = current.as_ref() {
        return consumer.version_status().await;
    }
    status_without_profile()
}

// ---- System tray (book Appendix A.2; OpenSpec desktop-window-lifecycle) ----
// Closing the window only minimizes it; the tray offers Show and a confirmed
// Quit. The tray is native, so the webview needs no tray permission.

/// Event the webview listens for to open its "Quit Chiral?" confirmation.
const QUIT_REQUEST_EVENT: &str = "chiral://quit-requested";
const TRAY_SHOW_ID: &str = "tray-show";
const TRAY_QUIT_ID: &str = "tray-quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayAction {
    Show,
    RequestQuit,
}

fn tray_action(menu_id: &str) -> Option<TrayAction> {
    match menu_id {
        TRAY_SHOW_ID => Some(TrayAction::Show),
        TRAY_QUIT_ID => Some(TrayAction::RequestQuit),
        _ => None,
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Show always restores the window. Quit restores it too and asks the webview
/// to open the confirmation; the tray never ends the process by itself.
fn handle_tray_action(app: &AppHandle, action: TrayAction) {
    show_main_window(app);
    if action == TrayAction::RequestQuit {
        if let Err(error) = app.emit_to("main", QUIT_REQUEST_EVENT, ()) {
            eprintln!("[TRAY] quit request not delivered to the window: {error}");
        }
    }
}

fn build_tray(app: &tauri::App) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("no application icon is bundled")?;
    let show = MenuItem::with_id(app, TRAY_SHOW_ID, "Show Chiral", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit Chiral…", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&show, &quit]).map_err(|e| e.to_string())?;
    TrayIconBuilder::with_id("chiral-tray")
        .icon(icon)
        .tooltip("Chiral Network")
        .menu(&menu)
        .on_menu_event(|app, event| {
            if let Some(action) = tray_action(event.id().as_ref()) {
                handle_tray_action(app, action);
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// A missing tray (e.g. no AppIndicator library on a Linux desktop) must not
/// stop the app: log it and keep running without one.
fn install_tray(app: &tauri::App) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_tray(app))) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("[TRAY] system tray unavailable: {error}"),
        Err(_) => eprintln!("[TRAY] system tray unavailable: tray initialization panicked"),
    }
}

pub fn run() {
    crate::version::log_policy_key_status();
    tauri::Builder::default()
        .manage(DesktopState::default())
        .setup(|app| {
            install_tray(app);
            Ok(())
        })
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

    /// Serializes the tests that set or read the process-wide effective version policy: loading
    /// a profile restores that profile's policy watermark into it.
    static EFFECTIVE_POLICY: Mutex<()> = Mutex::const_new(());

    #[test]
    fn tray_menu_ids_map_to_show_and_confirmed_quit_only() {
        assert_eq!(tray_action(TRAY_SHOW_ID), Some(TrayAction::Show));
        assert_eq!(tray_action(TRAY_QUIT_ID), Some(TrayAction::RequestQuit));
        for unknown in ["", "quit", "show", "tray-exit", "TRAY-QUIT", "tray-quit "] {
            assert_eq!(tray_action(unknown), None, "{unknown:?}");
        }
        assert_ne!(TRAY_SHOW_ID, TRAY_QUIT_ID);
        assert_eq!(QUIT_REQUEST_EVENT, "chiral://quit-requested");
    }

    /// Whether JavaScript's JSON parsing keeps every number in `value` exact: no integer beyond
    /// `Number.MAX_SAFE_INTEGER` (written out, independently of the product constant).
    fn javascript_exact(value: &Value) -> bool {
        const SAFE: u64 = 9_007_199_254_740_991;
        match value {
            Value::Number(number) => match (number.as_u64(), number.as_i64()) {
                (Some(unsigned), _) => unsigned <= SAFE,
                (None, Some(signed)) => signed.unsigned_abs() <= SAFE,
                (None, None) => true,
            },
            Value::Array(items) => items.iter().all(javascript_exact),
            Value::Object(members) => members.values().all(javascript_exact),
            _ => true,
        }
    }

    /// inference-account-snapshot task 2.5. The webview parses IPC JSON into doubles, so
    /// 2^53 + 1 would arrive as 2^53 and 2^64 - 1 as 18446744073709552000. Integers beyond
    /// 2^53 - 1 become their exact decimal strings; 2^53 itself is beyond (2^53 + 1 parses to
    /// it). Safe integers, floats and strings, canonical wei among them, are unchanged.
    #[test]
    fn webview_integers_beyond_double_precision_become_exact_decimal_strings() {
        let safe = (1u64 << 53) - 1;
        let native = json!({
            "zero": 0, "safe": safe, "negative_safe": -(safe as i64), "u32_max": u32::MAX,
            "two_53": 1u64 << 53, "two_53_plus_1": (1u64 << 53) + 1, "u64_max": u64::MAX,
            "negative_two_53": -(1i64 << 53), "i64_min": i64::MIN,
            "half": 0.5, "large_float": 1e300,
            "wei": u128::MAX.to_string(), "digits": "9007199254740993", "status": "provider_review",
            "nested": [{"prompt_tokens": u64::MAX, "completion_tokens": safe}, [ (1u64 << 53) + 1 ]],
            "none": null, "flag": true,
        });
        let delivered = exact_integers(native);
        assert_eq!(delivered, json!({
            "zero": 0, "safe": safe, "negative_safe": -(safe as i64), "u32_max": u32::MAX,
            "two_53": "9007199254740992", "two_53_plus_1": "9007199254740993",
            "u64_max": "18446744073709551615",
            "negative_two_53": "-9007199254740992", "i64_min": "-9223372036854775808",
            "half": 0.5, "large_float": 1e300,
            "wei": "340282366920938463463374607431768211455", "digits": "9007199254740993",
            "status": "provider_review",
            "nested": [{"prompt_tokens": "18446744073709551615", "completion_tokens": safe},
                ["9007199254740993"]],
            "none": null, "flag": true,
        }));
        assert!(javascript_exact(&delivered));
        // The IPC text carries the exact digits as JSON strings.
        let text = serde_json::to_string(&delivered).unwrap();
        assert!(text.contains(r#""u64_max":"18446744073709551615""#), "{text}");
        assert!(text.contains(r#""safe":9007199254740991"#), "{text}");
    }

    fn boundary_account() -> crate::protocol::AccountView {
        use crate::protocol::{AccountView, ModelSpec, NetworkIdentity, PriceSchedule, TokenUsage};
        let max = u128::MAX.to_string();
        AccountView {
            contract_id: format!("0x{}", "01".repeat(32)),
            network: NetworkIdentity {
                chain_id: crate::network::ESPACE_TESTNET_CHAIN_ID,
                genesis_hash: crate::network::ESPACE_TESTNET_GENESIS.into(),
                deployment_id: "exact-presentation".into(),
            },
            provider_wallet: format!("0x{}", "11".repeat(20)),
            consumer_wallet: format!("0x{}", "22".repeat(20)),
            model: ModelSpec {
                id: "test/exact-counters".into(),
                revision: "ab".repeat(20),
                tokenizer_revision: "cd".repeat(20),
                chat_template_sha256: "ef".repeat(32),
                quantization: None,
                context_length: u64::MAX,
                max_output_tokens: u64::MAX,
                capabilities: vec!["chat".into()],
                serving: None,
            },
            price: PriceSchedule { input_wei_per_1k: max.clone(), output_wei_per_1k: "1".into() },
            status: "provider_review".into(),
            // funded + loss = fee + reversed + spent + reserved + available = 2 * u128::MAX.
            funded_wei: max.clone(),
            fee_wei: "5".into(),
            spent_wei: (u128::MAX - 15).to_string(),
            reserved_wei: "3".into(),
            available_wei: max.clone(),
            reversed_wei: "7".into(),
            provider_loss_wei: max.clone(),
            usage: TokenUsage { prompt_tokens: u64::MAX, completion_tokens: (1 << 53) + 1 },
            waived_wei: max,
            max_concurrency: u32::MAX,
        }
    }

    fn boundary_receipt() -> Signed<crate::protocol::UsageReceipt> {
        use crate::{billing_store::RequestView, protocol::{PerformanceOutcome, TokenUsage, UsageReceipt}};
        let account = boundary_account();
        let max = u128::MAX.to_string();
        let request = RequestView {
            id: "request-exact".into(),
            request_id: "chatcmpl-exact".into(),
            contract_id: account.contract_id.clone(),
            credential_id: "credential-exact".into(),
            status: "settled".into(),
            usage: TokenUsage { prompt_tokens: (1 << 53) - 1, completion_tokens: u64::MAX },
            cost_wei: "0".into(),
            max_cost_wei: max.clone(),
            provider_loss_wei: max.clone(),
            prompt_tokens_reserved: (1 << 53) + 1,
            max_output_tokens: u64::MAX,
            created_at: 1_763_683_100,
            settled_at: Some(1 << 53),
            performance: Some(PerformanceOutcome {
                evidence_sha256: "ab".repeat(32),
                policy: "unqualified".into(),
                eligibility: "not_applicable".into(),
                ttft_us: Some(u64::MAX),
                decode_us: Some((1 << 53) + 1),
                elapsed_us: Some((1 << 53) - 1),
                output_tokens: Some(u64::MAX),
                attained: None,
                remedy: "none".into(),
            }),
            waived_wei: max,
        };
        Signed {
            payload: UsageReceipt {
                network: account.network.clone(),
                provider_wallet: account.provider_wallet.clone(),
                account,
                requests: vec![request],
                sequence: u64::MAX,
                issued_at: 1_763_683_200,
            },
            // The webview never verifies; the native receipt read did, before presentation.
            signature: format!("0x{}", "5a".repeat(65)),
        }
    }

    /// inference-account-snapshot task 2.5, desktop boundary. Every account-, receipt-, request-
    /// and usage-bearing operation delivers the shared fixture that tests/exactStatements.test.ts
    /// renders: u64 counters, sequences and timestamps beyond 2^53 - 1 as exact decimal strings,
    /// u128::MAX wei unchanged. The native typed statements keep their JSON integers; discovered
    /// offers and quotes, handed back verbatim for native verification, are not converted.
    #[test]
    fn account_and_receipt_statements_reach_the_webview_as_the_shared_exact_fixture() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/desktop-exact-statements.json"))
                .unwrap();
        let account = value(boundary_account()).unwrap();
        let receipt = value(boundary_receipt()).unwrap();
        // The native statements are exact JSON integers; only the webview copy changes.
        assert_eq!(account["usage"]["prompt_tokens"], json!(u64::MAX));
        assert_eq!(receipt["payload"]["sequence"], json!(u64::MAX));
        assert!(!javascript_exact(&account) && !javascript_exact(&receipt));

        for operation in ["account", "fund", "topup", "resume"] {
            assert_eq!(presented(operation, account.clone()), fixture["account"], "{operation}");
        }
        assert_eq!(presented("accounts", json!([account.clone()])), json!([fixture["account"]]));
        assert_eq!(presented("receipt", receipt.clone()), fixture["receipt"]);
        let requests = receipt["payload"]["requests"].clone();
        let presented_requests = fixture["receipt"]["payload"]["requests"].clone();
        assert_eq!(presented("usage", requests.clone()), presented_requests);
        for operation in ["request_status", "cancel_request"] {
            assert_eq!(presented(operation, requests[0].clone()), presented_requests[0], "{operation}");
        }
        let keys = json!({"requests": [{"contract_id": "c", "request_key": "k", "created_at": u64::MAX}],
            "scope": "local_dispatch_keys_not_settlement"});
        assert_eq!(presented("local_requests", keys)["requests"][0]["created_at"], "18446744073709551615");
        let completion = json!({"created": 1_763_683_200u64, "data": [{"index": 0, "embedding": [0.25, -1.5]}],
            "usage": {"prompt_tokens": u64::MAX, "completion_tokens": (1u64 << 53) + 1, "total_tokens": u64::MAX}});
        assert_eq!(presented("infer", completion), json!({"created": 1_763_683_200u64,
            "data": [{"index": 0, "embedding": [0.25, -1.5]}],
            "usage": {"prompt_tokens": "18446744073709551615", "completion_tokens": "9007199254740993",
                "total_tokens": "18446744073709551615"}}));
        for delivered in [presented("receipt", receipt.clone()), fixture["account"].clone()] {
            assert!(javascript_exact(&delivered));
        }
        // Signed offers and quotes return to `quote`/`fund` as received; never converted here.
        for operation in ["discover", "quote", "provider_info", "agreement", "network_status"] {
            assert_eq!(presented(operation, receipt.clone()), receipt, "{operation}");
        }
    }

    /// A provider stream event reaches the webview channel as JSON text whose usage counters
    /// beyond 2^53 - 1 are exact strings. The channel would otherwise evaluate
    /// `18446744073709551615` as a JavaScript number literal and round it.
    #[test]
    fn stream_usage_reaches_the_webview_channel_as_exact_text() {
        use tauri::ipc::InvokeResponseBody;
        let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
        let channel: Channel<StreamEvent> = Channel::new({
            let sent = sent.clone();
            move |body| {
                if let InvokeResponseBody::Json(text) = body {
                    sent.lock().unwrap().push(text);
                }
                Ok(())
            }
        });
        let event = StreamEvent {
            kind: "chunk".into(),
            data: json!({"created": 1_763_683_200u64, "choices": [{"index": 0, "delta": {"content": "hi"}}],
                "usage": {"prompt_tokens": u64::MAX, "completion_tokens": (1u64 << 53) + 1,
                    "total_tokens": (1u64 << 53) - 1}}),
        };
        channel.send(webview_event(event)).unwrap();
        let text = sent.lock().unwrap().pop().expect("one JSON message");
        assert!(text.contains(r#""prompt_tokens":"18446744073709551615""#), "{text}");
        let delivered: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(delivered, json!({"kind": "chunk", "data": {"created": 1_763_683_200u64,
            "choices": [{"index": 0, "delta": {"content": "hi"}}],
            "usage": {"prompt_tokens": "18446744073709551615", "completion_tokens": "9007199254740993",
                "total_tokens": 9_007_199_254_740_991u64}}}));
        assert!(javascript_exact(&delivered));
    }

    /// The forwarding loop `inference_stream` runs for an opened provider stream: each provider
    /// event reaches the actual webview channel, in order, with usage counters beyond 2^53 - 1 as
    /// exact strings. Catches the loop sending the provider event itself instead of its webview
    /// copy, which the helper-level test above cannot see.
    #[tokio::test]
    async fn inference_stream_forwards_provider_usage_to_the_channel_exactly() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tauri::ipc::InvokeResponseBody;
        let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
        let channel: Channel<StreamEvent> = Channel::new({
            let sent = sent.clone();
            move |body| {
                if let InvokeResponseBody::Json(text) = body {
                    sent.lock().unwrap().push(text);
                }
                Ok(())
            }
        });
        let usage = json!({"prompt_tokens": u64::MAX, "completion_tokens": (1u64 << 53) + 1,
            "total_tokens": (1u64 << 53) - 1});
        let (provider, mut events) = tokio::sync::mpsc::channel(4);
        for (kind, data) in [
            ("started", json!({"request_id": "chatcmpl-exact"})),
            ("chunk", json!({"choices": [{"index": 0, "delta": {"content": "hi"}}], "usage": usage})),
            ("done", json!({"request_id": "chatcmpl-exact", "usage": usage})),
        ] {
            provider.send(StreamEvent { kind: kind.into(), data }).await.unwrap();
        }
        drop(provider);
        // The desktop's cancellation sender stays open: this stream is not cancelled.
        let (_desktop, mut cancelled) = watch::channel(false);
        let provider_cancelled = AtomicBool::new(false);
        forward_stream(&mut events, || provider_cancelled.store(true, Ordering::SeqCst), &mut cancelled, &channel)
            .await;

        let texts = sent.lock().unwrap().clone();
        assert_eq!(texts.len(), 3, "{texts:?}");
        for text in &texts[1..] {
            for exact in [r#""prompt_tokens":"18446744073709551615""#, r#""completion_tokens":"9007199254740993""#,
                r#""total_tokens":9007199254740991"#] {
                assert!(text.contains(exact), "{text} lacks {exact}");
            }
        }
        let delivered: Vec<Value> = texts.iter().map(|text| serde_json::from_str(text).unwrap()).collect();
        // Exactly the events tests/exactStatements.test.ts evaluates on the Playground.
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/desktop-exact-statements.json"))
                .unwrap();
        assert_eq!(Value::Array(delivered.clone()), fixture["stream"]);
        assert!(delivered.iter().all(javascript_exact));
        // A completed stream is neither cancelled nor followed by a synthetic error.
        assert!(!provider_cancelled.load(Ordering::SeqCst));
    }

    /// The actual `consumer_command` dispatch: `accounts` and `local_requests` read native
    /// protected files that keep exact u64 counters, receipt watermark and u128::MAX wei, and
    /// deliver them exactly to the webview without changing those files. The signed terms are a
    /// restored prerequisite written through public APIs, not an observed opening.
    #[tokio::test]
    async fn native_accounts_and_request_keys_reach_the_webview_exactly_through_dispatch() {
        use crate::consumer::ConsumerConfig;
        use crate::network::NetworkConfig;
        use crate::protocol::{InferenceOffer, Quote, OFFER_DOMAIN, QUOTE_DOMAIN};
        use std::io::Write;
        let _policy = EFFECTIVE_POLICY.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let mut account = boundary_account();
        let config = ConsumerConfig {
            network: NetworkConfig {
                identity: account.network.clone(),
                rpc_url: "http://127.0.0.1:9".into(),
                rpc_bearer_file: None,
                faucet_url: None,
                explorer_url: None,
                policy_url: None,
            },
            data_dir: root.join("data"),
            catalog_url: None,
            development: false,
            dht: None,
        };
        let profile = root.join("consumer.json");
        std::fs::write(&profile, serde_json::to_vec(&config).unwrap()).unwrap();
        let state = DesktopState::default();
        consumer_operation(&state, "configure".into(), json!({"path": profile.display().to_string()}))
            .await
            .unwrap();
        let payer: String = serde_json::from_value(
            consumer_operation(&state, "wallet_create".into(), json!({})).await.unwrap(),
        )
        .unwrap();
        let key = crate::wallet::generate_private_key();
        let provider = crate::wallet::address_from_private_key(&key).unwrap();
        let now = crate::protocol::now().unwrap();
        let offer = InferenceOffer {
            network: account.network.clone(),
            provider_wallet: provider.clone(),
            peer_id: "QmPVGjYFugq4XUyBfoTHG6c3qxfBS26jEdaFM1gdAVuMZ2".into(),
            deployment_id: "exact-presentation".into(),
            endpoint: "https://provider.example".into(),
            model: account.model.clone(),
            price: account.price.clone(),
            min_funding_wei: "1".into(),
            platform_wallet: format!("0x{}", "33".repeat(20)),
            fee_bps: 50,
            max_concurrency: u32::MAX,
            revision: 1,
            issued_at: now - 120,
            expires_at: now + 600,
        };
        let quote = Quote {
            id: account.contract_id.clone(),
            network: offer.network.clone(),
            offer_id: crate::protocol::offer_id(&offer).unwrap(),
            provider_wallet: provider.clone(),
            consumer_wallet: payer.clone(),
            model: offer.model.clone(),
            price: offer.price.clone(),
            funding_wei: u128::MAX.to_string(),
            platform_wallet: offer.platform_wallet.clone(),
            fee_bps: 50,
            max_concurrency: u32::MAX,
            nonce: "exact-presentation".into(),
            issued_at: now - 120,
            expires_at: now + 300,
        };
        account.provider_wallet = provider;
        account.consumer_wallet = payer.clone();
        let saved = json!({
            "offer": crate::protocol::sign(OFFER_DOMAIN, offer, &key).unwrap(),
            "quote": crate::protocol::sign(QUOTE_DOMAIN, quote, &key).unwrap(),
            "account": account, "raw_transaction": null, "tx_hash": null, "credentials": [],
            "active_credential": null, "topup": null, "receipt_sequence": u64::MAX,
        });
        let folder = config.network.scoped_dir(&config.data_dir).join("consumer").join(&payer);
        let write = |path: &Path, value: &Value| {
            crate::wallet::secure_directory(path.parent().unwrap()).unwrap();
            let mut file = crate::wallet::create_secret_file(path).unwrap();
            file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
        };
        let id = account.contract_id.clone();
        let account_path = folder.join(format!("{}.json", hex::encode(crate::protocol::digest(id.as_bytes()))));
        write(&account_path, &saved);
        let request_path = folder.join("requests").join("exact.json");
        write(&request_path, &json!({"contract_id": id, "request_key": "exact", "created_at": u64::MAX}));
        let stored = (std::fs::read(&account_path).unwrap(), std::fs::read(&request_path).unwrap());

        // The native consumer itself holds the exact integers.
        let native = state.consumer.read().await.clone().unwrap().accounts().unwrap();
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].usage.prompt_tokens, u64::MAX);
        assert_eq!(native[0].usage.completion_tokens, (1 << 53) + 1);

        let delivered = consumer_operation(&state, "accounts".into(), json!({})).await.unwrap();
        let mut expected = value(&account).unwrap();
        expected["usage"] = json!({"prompt_tokens": "18446744073709551615", "completion_tokens": "9007199254740993"});
        expected["model"]["context_length"] = json!("18446744073709551615");
        expected["model"]["max_output_tokens"] = json!("18446744073709551615");
        assert_eq!(delivered, json!([expected]));
        assert_eq!(delivered[0]["available_wei"], "340282366920938463463374607431768211455");
        assert_eq!(delivered[0]["max_concurrency"], json!(u32::MAX));
        assert!(javascript_exact(&delivered));

        let keys = consumer_operation(&state, "local_requests".into(), json!({"contract_id": id}))
            .await
            .unwrap();
        assert_eq!(keys, json!({"requests": [{"contract_id": id, "request_key": "exact",
            "created_at": "18446744073709551615"}], "scope": "local_dispatch_keys_not_settlement"}));
        // Presentation reads change no protected native file.
        assert_eq!((std::fs::read(&account_path).unwrap(), std::fs::read(&request_path).unwrap()), stored);
        let saved: Value = serde_json::from_slice(&stored.0).unwrap();
        assert_eq!(saved["receipt_sequence"], json!(u64::MAX));
        assert_eq!(saved["account"]["usage"]["prompt_tokens"], json!(u64::MAX));
    }

    #[test]
    fn missing_and_invalid_command_fields_are_rejected() {
        assert!(text(&json!({"path":""}), "path").is_err());
        assert!(field::<u8>(&json!({"score":256}), "score").is_err());
        assert!(field::<Signed<Quote>>(&json!({"quote":{"id":"untrusted"}}), "quote").is_err());
        assert_eq!(optional_text(&json!({"model":null}), "model"), None);
    }

    #[tokio::test]
    async fn logout_drops_the_previous_networks_version_status() {
        let _policy = EFFECTIVE_POLICY.lock().await;
        let mut policy = crate::version::bundled_policy();
        policy.recommended = "999.0.0".into();
        crate::version::set_effective_policy_for_test(policy);
        assert_eq!(status_without_profile().unwrap()["status"], "recommended");
        end_session(&DesktopState::default()).await;
        let status = status_without_profile().unwrap();
        assert_eq!(status["status"], "ok");
        assert_eq!(status["policy"], json!(crate::version::bundled_policy()));
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

    /// profile-path-resolution requirement 5 scenario "Desktop Settings shows a refusal" (task
    /// 3.5), native half. With profile P1 loaded (its wallet created and a stream pending),
    /// `configure` with a profile P2 that is refused because this network's state exists only at
    /// the working-directory interpretation of a relative `data_dir`, `network.rpc_bearer_file`
    /// or `dht.identity_file` returns the refusal (the string Settings shows), creates nothing,
    /// leaves the old state unchanged, keeps the very same P1 consumer loaded with its wallet,
    /// and cancels no stream. The deliberate absolute-path remedy then loads, replacing P1 and
    /// cancelling the stream, as every successful `configure` does.
    #[tokio::test]
    async fn configure_returns_a_refusal_and_the_loaded_profile_stays_loaded() {
        use crate::consumer::ConsumerConfig;
        use crate::network::NetworkConfig;
        use crate::protocol::NetworkIdentity;
        use crate::provider_config::DhtConfig;
        use std::path::{Component, PathBuf};
        let _policy = EFFECTIVE_POLICY.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let network = |deployment: &str| NetworkConfig {
            identity: NetworkIdentity {
                chain_id: crate::network::ESPACE_TESTNET_CHAIN_ID,
                genesis_hash: crate::network::ESPACE_TESTNET_GENESIS.into(),
                deployment_id: deployment.into(),
            },
            rpc_url: "http://127.0.0.1:9".into(),
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        };
        let profile = |network: NetworkConfig, data_dir: PathBuf| ConsumerConfig {
            network,
            data_dir,
            catalog_url: None,
            development: false,
            dht: None,
        };
        let write = |path: &Path, config: &ConsumerConfig| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, serde_json::to_vec(config).unwrap()).unwrap();
        };
        let snapshot = |path: &Path| {
            let mut entries = std::collections::BTreeMap::new();
            let mut pending = vec![path.to_path_buf()];
            while let Some(directory) = pending.pop() {
                for entry in std::fs::read_dir(&directory).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        entries.insert(path.clone(), None);
                        pending.push(path);
                    } else {
                        // A digest, so that a failure never shows the wallet key.
                        let digest = <sha2::Sha256 as sha2::Digest>::digest(std::fs::read(&path).unwrap());
                        entries.insert(path.clone(), Some(hex::encode(digest)));
                    }
                }
            }
            entries
        };
        // The working directory of a test binary is fixed (nothing changes it), so a relative
        // path that climbs from it to the file-system root and descends to an absolute `target`
        // names `target` from the working directory, and a place inside `root` that does not
        // exist from a profile directory deeper than the working directory: exactly the old
        // working-directory location of an earlier client. The working directory is only read.
        let working = std::env::current_dir().unwrap().canonicalize().unwrap();
        let climb = working.components().filter(|c| matches!(c, Component::Normal(_))).count();
        let from_working_directory = |target: &Path| -> PathBuf {
            let mut path: PathBuf = std::iter::repeat("..").take(climb).collect();
            path.extend(target.components().filter(|c| matches!(c, Component::Normal(_))));
            assert!(std::fs::symlink_metadata(&path).is_ok(), "{path:?} names {target:?}");
            path
        };
        let deep = (0..=climb).fold(root.join("p2"), |path, level| path.join(format!("d{level}")));

        // P1 is loaded, its wallet created, and a stream is pending.
        let state = DesktopState::default();
        let p1 = root.join("p1").join("consumer.json");
        write(&p1, &profile(network("desktop-p1"), root.join("p1-data")));
        let configure = |path: &Path| json!({"path": path.display().to_string()});
        assert_eq!(
            consumer_operation(&state, "configure".into(), configure(&p1)).await,
            Ok(json!({"address": null}))
        );
        let address = consumer_operation(&state, "wallet_create".into(), json!({})).await.unwrap();
        let loaded = state.consumer.read().await.clone().unwrap();
        let (sender, mut receiver) = watch::channel(false);
        state.streams.lock().await.insert("pending".into(), sender);

        // State that an earlier client run from the working directory left for P2's network.
        let old = root.join("old");
        let old_data = old.join("data");
        let p2_network = network("desktop-p2");
        let old_address = Consumer::new(profile(p2_network.clone(), old_data.clone()))
            .unwrap()
            .wallet_create()
            .unwrap();
        assert_ne!(json!(old_address), address);
        std::fs::write(old.join("rpc.token"), b"read from the old working directory").unwrap();
        std::fs::write(old.join("peer.key"), b"read from the old working directory").unwrap();
        let old_before = snapshot(&old);

        let unused = root.join("p2-data");
        for field in ["data_dir", "network.rpc_bearer_file", "dht.identity_file"] {
            let mut config = profile(p2_network.clone(), unused.clone());
            match field {
                "data_dir" => config.data_dir = from_working_directory(&old_data),
                "network.rpc_bearer_file" => {
                    config.network.rpc_bearer_file = Some(from_working_directory(&old.join("rpc.token")))
                }
                _ => {
                    config.dht = Some(DhtConfig {
                        listen: "/ip4/127.0.0.1/tcp/0".into(),
                        bootstrap: vec![],
                        identity_file: from_working_directory(&old.join("peer.key")),
                    })
                }
            }
            let p2 = deep.join(format!("{}.json", field.replace('.', "-")));
            write(&p2, &config);
            let p2_before = snapshot(&root.join("p2"));

            let error = consumer_operation(&state, "configure".into(), configure(&p2))
                .await
                .expect_err(field);

            let start = format!("consumer profile {}: relative {field} ", p2.display());
            assert!(error.starts_with(&start), "{field}: {error}");
            for required in [
                "now resolves against the profile's directory",
                "nothing was created, moved or deleted",
                &format!("set {field} to the absolute path"),
                "deliberately move",
            ] {
                assert!(error.contains(required), "{field}: missing {required:?} in {error}");
            }
            // Nothing was created, and the old state is unchanged.
            assert_eq!(snapshot(&root.join("p2")), p2_before, "{field}");
            assert!(!unused.exists(), "{field}");
            assert_eq!(snapshot(&old), old_before, "{field}");
            // P1 stays the loaded profile: the same consumer, its network and wallet, and no
            // stream was cancelled.
            let current = state.consumer.read().await.clone();
            let current = current.unwrap_or_else(|| panic!("{field}: P1 was unloaded"));
            assert!(Arc::ptr_eq(&current, &loaded), "{field}: P1 was replaced");
            assert_eq!(json!(current.address().unwrap()), address, "{field}");
            let shown = consumer_operation(&state, "network_profile".into(), json!({})).await.unwrap();
            assert_eq!(shown["identity"]["deployment_id"], "desktop-p1", "{field}");
            assert!(!receiver.has_changed().unwrap(), "{field}: a stream was cancelled");
            assert!(state.streams.lock().await.contains_key("pending"), "{field}");
        }

        // The deliberate remedy, an absolute data_dir naming the old state, loads it: a
        // successful configure replaces P1 and cancels its stream.
        let remedy = root.join("p2-absolute.json");
        write(&remedy, &profile(p2_network, old_data));
        assert_eq!(
            consumer_operation(&state, "configure".into(), configure(&remedy)).await,
            Ok(json!({"address": old_address}))
        );
        receiver.changed().await.expect("cancellation notification");
        assert!(*receiver.borrow());
        assert!(!Arc::ptr_eq(&state.consumer.read().await.clone().unwrap(), &loaded));
    }
}

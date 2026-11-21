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

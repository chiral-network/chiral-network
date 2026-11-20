use super::*;
use serde_json::json;
use std::io::Write;

fn fixture() -> (tempfile::TempDir, ModelConfig) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    let artifact = directory.path().join("artifact");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::create_dir(&artifact).unwrap();
    let key = directory.path().join("engine.key");
    crate::wallet::create_secret_file(&key).unwrap().write_all(b"test-private-engine-token-32-characters\n").unwrap();
    let mut executable_bytes = vec![0; 4096];
    let n = std::fs::File::open(std::env::current_exe().unwrap()).unwrap().read(&mut executable_bytes).unwrap();
    executable_bytes.truncate(n);
    let executable_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    std::fs::write(runtime.join(executable_name), &executable_bytes).unwrap();
    let runtime_manifest = json!({"schema":"chiral-llama-runtime-v1", "platform":platform(), "architecture":std::env::consts::ARCH,
        "backend":"cpu", "engine_version":ENGINE_VERSION, "source_revision":SOURCE_REVISION, "executable":executable_name,
        "files":{executable_name:{"sha256":hex::encode(Sha256::digest(&executable_bytes)),"size_bytes":executable_bytes.len()}}});
    std::fs::write(runtime.join("chiral-runtime.json"), protocol::canonical_json(&runtime_manifest).unwrap()).unwrap();
    let template = b"{{ messages }}";
    let gguf = b"GGUF-test-inventory-bytes";
    std::fs::write(artifact.join("model-Q4_K_M.gguf"), gguf).unwrap();
    std::fs::write(artifact.join("chiral-chat-template.jinja"), template).unwrap();
    let model = ModelSpec { id:"test/repo/model-Q4_K_M.gguf".into(), revision:"ab".repeat(20), tokenizer_revision:"ab".repeat(20), chat_template_sha256:hex::encode(Sha256::digest(template)), quantization:Some("Q4_K_M".into()), context_length:512, max_output_tokens:64, capabilities:vec!["chat".into(), "completions".into()], serving:None };
    std::fs::write(artifact.join("chiral-model.json"), protocol::canonical_json(&model).unwrap()).unwrap();
    let manifest = json!({"schema":"chiral-gguf-artifact-v1", "model":{"repo_id":"test/repo","revision":model.revision,"license":"test"},
        "task":"generation","quantization":"Q4_K_M","context_length":512,"max_output_tokens":64,"capabilities":model.capabilities,
        "chat_template_name":"default","model_file":"model-Q4_K_M.gguf",
        "files":{"model-Q4_K_M.gguf":{"sha256":hex::encode(Sha256::digest(gguf)),"size_bytes":gguf.len(),"upstream_repo":"test/repo","upstream_path":"model-Q4_K_M.gguf","upstream_digest":"ab".repeat(32)},
        "chiral-chat-template.jinja":{"sha256":hex::encode(Sha256::digest(template)),"size_bytes":template.len(),"upstream_repo":null,"upstream_path":null,"upstream_digest":null}}});
    std::fs::write(artifact.join("chiral-artifact-sha256.json"), protocol::canonical_json(&manifest).unwrap()).unwrap();
    let config = ModelConfig { model, price:crate::protocol::PriceSchedule { input_wei_per_1k:"0".into(), output_wei_per_1k:"0".into() }, engine_url:"http://127.0.0.1:18080".into(), engine_api_key_file:Some(key), max_concurrency:2, request_timeout_secs:30, qualification_evidence_file:None,
        runtime:Some(EngineRuntimeConfig { runtime_dir:runtime, artifact_dir:artifact, backend:"cpu".into(), device_ids:vec![], memory_reserve_mib:0, gpu_layers:0, split_mode:"none".into(), tensor_split:vec![], cache_type_k:"f16".into(), cache_type_v:"f16".into(), flash_attention:false, batch_size:128, ubatch_size:64, startup_timeout_secs:1 }) };
    (directory, config)
}

fn embedding_fixture() -> (tempfile::TempDir, ModelConfig) {
    let (directory, mut config) = fixture();
    let root = &config.runtime.as_ref().unwrap().artifact_dir;
    let path = root.join("chiral-artifact-sha256.json");
    let mut manifest = read_json(&path).unwrap();
    config.model.capabilities = vec!["embeddings".into()];
    config.model.max_output_tokens = 0;
    config.model.chat_template_sha256 = hex::encode(Sha256::digest(b""));
    manifest["task"] = json!("embeddings");
    manifest["capabilities"] = json!(config.model.capabilities);
    manifest["max_output_tokens"] = json!(0);
    manifest["chat_template_name"] = Value::Null;
    manifest["files"].as_object_mut().unwrap().remove("chiral-chat-template.jinja");
    std::fs::remove_file(root.join("chiral-chat-template.jinja")).unwrap();
    std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
    std::fs::write(path, protocol::canonical_json(&manifest).unwrap()).unwrap();
    (directory, config)
}

#[test]
fn embedding_inventory_rejects_selected_templates_even_when_hashes_match() {
    for source in [b"{{ messages }}".as_slice(), b"".as_slice()] {
        for selected in [Value::Null, json!("default")] {
            let (_directory, mut config) = embedding_fixture();
            let root = &config.runtime.as_ref().unwrap().artifact_dir;
            let path = root.join("chiral-artifact-sha256.json");
            let mut manifest = read_json(&path).unwrap();
            // Bind the exact selected bytes in both metadata and inventory; this
            // must not be rejected merely for a stale digest or missing file.
            config.model.chat_template_sha256 = hex::encode(Sha256::digest(source));
            manifest["chat_template_name"] = selected;
            manifest["files"]["chiral-chat-template.jinja"] = json!({
                "sha256": config.model.chat_template_sha256, "size_bytes": source.len(),
                "upstream_repo": null, "upstream_path": null, "upstream_digest": null
            });
            std::fs::write(root.join("chiral-chat-template.jinja"), source).unwrap();
            std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
            std::fs::write(path, protocol::canonical_json(&manifest).unwrap()).unwrap();
            assert!(validate_inventory(&config).is_err());
        }
    }
}

#[test]
fn embedding_inventory_requires_explicit_null_template_selection() {
    let (_directory, config) = embedding_fixture();
    let path = config.runtime.as_ref().unwrap().artifact_dir.join("chiral-artifact-sha256.json");
    let mut manifest = read_json(&path).unwrap();
    for selected in [
        Some(json!("default")), Some(json!("tool_use")), Some(json!("")),
        Some(json!(false)), Some(json!(0)), Some(json!([])), Some(json!({})), None,
    ] {
        if let Some(value) = selected {
            manifest["chat_template_name"] = value;
        } else {
            manifest.as_object_mut().unwrap().remove("chat_template_name");
        }
        std::fs::write(&path, protocol::canonical_json(&manifest).unwrap()).unwrap();
        assert!(validate_inventory(&config).is_err());
    }
    manifest["chat_template_name"] = Value::Null;
    std::fs::write(path, protocol::canonical_json(&manifest).unwrap()).unwrap();
    let validated = validate_inventory(&config).unwrap();
    assert_eq!(validated.task, "embeddings");
    assert!(validated.template.is_none());
}

#[test]
fn template_free_embedding_inventory_still_binds_model_bytes_and_empty_digest() {
    // Reuse synthetic inventory bytes, not an executable GGUF or engine qualification.
    let (_directory, mut config) = embedding_fixture();
    let root = &config.runtime.as_ref().unwrap().artifact_dir;
    let model_path = root.join("model-Q4_K_M.gguf");
    let validated = validate_inventory(&config).unwrap();
    assert_eq!(validated.task, "embeddings");
    assert!(validated.template.is_none());
    assert_eq!(validated.model_file, model_path.canonicalize().unwrap());

    let mut bytes = std::fs::read(&model_path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&model_path, &bytes).unwrap();
    assert!(validate_inventory(&config).is_err());
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&model_path, &bytes).unwrap();

    config.model.chat_template_sha256 = hex::encode(Sha256::digest(b"{{ messages }}"));
    std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
    assert!(validate_inventory(&config).is_err());
    config.model.chat_template_sha256 = hex::encode(Sha256::digest(b""));
    std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
    assert!(validate_inventory(&config).unwrap().template.is_none());
}

#[test]
fn inventories_bind_every_selected_byte_and_reject_unlisted_libraries() {
    let (_directory, config) = fixture();
    let validated = validate_inventory(&config).unwrap();
    assert_eq!(validated.task, "generation");
    let runtime = config.runtime.as_ref().unwrap();
    std::fs::write(runtime.runtime_dir.join("unlisted.dll"), b"untrusted loader input").unwrap();
    assert!(validate_inventory(&config).err().unwrap().contains("unexpected file"));
    std::fs::remove_file(runtime.runtime_dir.join("unlisted.dll")).unwrap();
    std::fs::write(runtime.artifact_dir.join("model-Q4_K_M.gguf"), b"GGUF-test-inventory-byteX").unwrap();
    assert!(validate_inventory(&config).is_err());
}

#[test]
fn immutable_template_and_base_model_must_match_configuration() {
    let (_directory, mut config) = fixture();
    config.model.context_length += 1;
    assert!(validate_inventory(&config).err().unwrap().contains("ModelSpec"));
    config.model.context_length -= 1;
    config.model.chat_template_sha256 = "00".repeat(32);
    assert!(validate_inventory(&config).is_err());
}

#[test]
fn effective_template_preserves_whitespace_except_pinned_lexer_normalization() {
    for (source, expected) in [
        (" \r\n{{ messages }}\r\n\r\n", " \n{{ messages }}\n"),
        ("{{ messages }}\r", "{{ messages }}\n"),
        ("{{ messages }}\n ", "{{ messages }}\n "),
        ("", ""),
    ] {
        assert_eq!(template_digest(source.as_bytes()), hex::encode(Sha256::digest(expected.as_bytes())));
    }
}

#[test]
fn runtime_rejects_floating_identity_and_unsafe_inventory_paths() {
    let (_directory, config) = fixture();
    let root = &config.runtime.as_ref().unwrap().runtime_dir;
    let path = root.join("chiral-runtime.json");
    let mut manifest = read_json(&path).unwrap();
    manifest["source_revision"] = json!("master");
    std::fs::write(&path, protocol::canonical_json(&manifest).unwrap()).unwrap();
    assert!(validate_inventory(&config).err().unwrap().contains("pinned upstream"));
    for path in ["../engine", "/absolute", "C:\\engine.exe", "folder/../engine", "a\\b"] { assert!(relative_path(path).is_err()); }
}


#[test]
fn managed_endpoint_and_key_require_literal_loopback_and_single_protected_token() {
    let (_directory, mut config) = fixture();
    for endpoint in ["http://localhost:18080", "http://192.168.1.2:18080", "https://127.0.0.1:18080", "http://127.0.0.1"] {
        config.engine_url = endpoint.into();
        assert!(config.runtime.as_ref().unwrap().validate(&config).is_err());
    }
    config.engine_url = "http://[::1]:18080".into();
    assert!(config.runtime.as_ref().unwrap().validate(&config).is_ok());
    let key = config.engine_api_key_file.as_ref().unwrap();
    std::fs::write(key, b"#comment-is-not-an-engine-auth-key-123456789").unwrap();
    assert!(validate_inventory(&config).is_err());
    std::fs::write(key, b" test-private-engine-token-32-characters\n").unwrap();
    assert!(validate_inventory(&config).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_inventory_entries_are_rejected_before_execution() {
    let (_directory, config) = fixture();
    let root = &config.runtime.as_ref().unwrap().artifact_dir;
    std::os::unix::fs::symlink("model-Q4_K_M.gguf", root.join("alias.gguf")).unwrap();
    assert!(validate_inventory(&config).err().unwrap().contains("symlink"));
}

#[test]
#[ignore = "native child helper, invoked only by lifecycle tests"]
fn managed_child_helper() { std::thread::sleep(Duration::from_secs(60)); }

fn child() -> ManagedEngine {
    ManagedEngine::helper_for_test("test/helper", "http://127.0.0.1:1")
}

#[tokio::test]
async fn shutdown_reaps_child_and_permanently_withdraws_readiness() {
    let engine = child();
    assert!(engine.is_alive());
    engine.stop().await;
    assert!(!engine.is_alive());
    assert!(engine.child.lock().unwrap().try_wait().unwrap().is_some());
    engine.stop().await;
    assert!(!engine.is_alive());
}

#[tokio::test]
async fn process_exit_is_observed_without_waiting_for_health_refresh() {
    let engine = child();
    engine.child.lock().unwrap().start_kill().unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while engine.is_alive() && tokio::time::Instant::now() < deadline { tokio::time::sleep(Duration::from_millis(20)).await; }
    assert!(!engine.is_alive());
}

#[tokio::test]
async fn startup_timeout_kills_child_instead_of_admitting_or_retrying() {
    let engine = child();
    engine.ready.store(false, Ordering::Release);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let result = engine.await_health(&url, "private-test-token", Duration::from_millis(50)).await;
    assert!(result.err().unwrap().contains("timed out"));
    assert!(!engine.is_alive());
    assert!(engine.child.lock().unwrap().try_wait().unwrap().is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_runtime_does_not_strand_the_native_child() {
    let engine = child();
    let pid = engine.observation.pid as i32;
    drop(engine);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 && tokio::time::Instant::now() < deadline { tokio::time::sleep(Duration::from_millis(20)).await; }
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

/// Errors `ManagedEngine::start` can return with the CPU fixture once the port check has
/// passed: its `llama-server` is only the first 4,096 bytes of this test binary.
fn failed_after_port_check(error: &str) -> bool {
    error != OCCUPIED_ENGINE_PORT
        && ["cannot start pinned native llama-server", "managed engine exited before readiness", "managed engine startup timed out"].contains(&error)
}

/// Another process's listener: an IPv6 one is `IPV6_V6ONLY`, so that it occupies only its family.
/// With `reuseaddr` it has `SO_REUSEADDR`, as `std::net::TcpListener` sets on Unix (task 2.2).
async fn foreign_listener(address: std::net::SocketAddr, reuseaddr: bool) -> tokio::net::TcpListener {
    let socket = if address.is_ipv4() { tokio::net::TcpSocket::new_v4() } else { tokio::net::TcpSocket::new_v6() }.unwrap();
    socket.set_reuseaddr(reuseaddr).unwrap();
    #[cfg(unix)]
    if address.is_ipv6() {
        use std::os::fd::AsRawFd;
        let on: libc::c_int = 1;
        assert_eq!(unsafe { libc::setsockopt(socket.as_raw_fd(), libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, (&on as *const libc::c_int).cast(), std::mem::size_of::<libc::c_int>() as libc::socklen_t) }, 0);
    }
    socket.bind(address).unwrap();
    socket.listen(8).unwrap()
}

async fn assert_still_accepts(listener: &tokio::net::TcpListener, endpoint: std::net::SocketAddr) {
    let client = tokio::net::TcpStream::connect(endpoint).await.unwrap();
    let (_accepted, peer) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
    assert_eq!(peer, client.local_addr().unwrap());
}

#[tokio::test]
async fn a_listener_on_the_engine_address_or_its_wildcard_is_refused_before_spawn() {
    let (_directory, mut config) = fixture();
    let mut cases = vec![("127.0.0.1:0", "127.0.0.1"), ("0.0.0.0:0", "127.0.0.1")];
    // Where IPv6 loopback exists, the same holds for `[::1]` beside `[::1]` and `[::]`.
    if std::net::TcpListener::bind("[::1]:0").is_ok() { cases.extend([("[::1]:0", "[::1]"), ("[::]:0", "[::1]")]); }
    // On Windows `SO_REUSEADDR` lets a socket share a port another one holds; not that case.
    let options: &[bool] = if cfg!(windows) { &[false] } else { &[false, true] };
    for (bound, host) in cases {
        for &reuseaddr in options {
            let listener = foreign_listener(bound.parse().unwrap(), reuseaddr).await;
            let port = listener.local_addr().unwrap().port();
            config.engine_url = format!("http://{host}:{port}");
            let error = ManagedEngine::start(&config).await.err().unwrap();
            assert_eq!(error, OCCUPIED_ENGINE_PORT, "{bound} (SO_REUSEADDR {reuseaddr}) occupies {}", config.engine_url);
            // Not adopted, signalled or disturbed: the other process still accepts connections.
            assert_still_accepts(&listener, format!("{host}:{port}").parse().unwrap()).await;
        }
    }
}

#[tokio::test]
async fn a_port_holding_only_time_wait_connections_passes_the_port_check() {
    use std::io::Read as _;
    let (_directory, mut config) = fixture();
    // `127.0.0.1`, and `[::1]` where IPv6 loopback exists (its probe is `IPV6_V6ONLY`).
    let mut hosts = vec!["127.0.0.1:0"];
    if std::net::TcpListener::bind("[::1]:0").is_ok() { hosts.push("[::1]:0"); }
    for host in hosts {
        let listener = std::net::TcpListener::bind(host).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let mut client = std::net::TcpStream::connect(endpoint).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        // The listening side closes first, so its end of the connection waits in TIME_WAIT on
        // the endpoint, as a stopped engine's keep-alive connections do.
        drop(accepted);
        assert_eq!(client.read(&mut [0u8; 1]).unwrap(), 0);
        drop(client);
        drop(listener);
        std::thread::sleep(Duration::from_millis(200));
        // The port is not free of sockets: a bind without `SO_REUSEADDR` is refused (measured
        // on macOS, design decision 5), so dropping the option would refuse a prompt restart.
        #[cfg(target_os = "macos")]
        assert!(if endpoint.is_ipv4() { tokio::net::TcpSocket::new_v4() } else { tokio::net::TcpSocket::new_v6() }.unwrap().bind(endpoint).is_err(), "no TIME_WAIT connection was left on {endpoint}");
        config.engine_url = format!("http://{endpoint}");
        let error = ManagedEngine::start(&config).await.err().unwrap();
        assert!(failed_after_port_check(&error), "{endpoint}: {error}");
    }
}

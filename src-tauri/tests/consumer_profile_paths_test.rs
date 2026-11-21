//! Relative paths in a consumer profile name places next to the profile, never in the process
//! working directory (book line 449), so one profile reaches one `data_dir`, wallet and nonce
//! journal from anywhere (book line 470). State found only at the old working-directory location
//! is refused with both paths, never silently replaced or moved (book lines 472 and 652).
//!
//! The `chiral` binary runs with an explicit temporary profile and a different working directory.
//! Only offline wallet commands are used; the loopback RPC is never contacted.
use chiral_network::consumer::{Consumer, ConsumerConfig};
use chiral_network::network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS};
use chiral_network::protocol::{InferenceOffer, NetworkIdentity, Signed};
use chiral_network::provider_config::ProviderConfig;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn network(deployment: &str) -> NetworkConfig {
    NetworkConfig {
        identity: NetworkIdentity {
            chain_id: ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: ESPACE_TESTNET_GENESIS.into(),
            deployment_id: deployment.into(),
        },
        rpc_url: "http://127.0.0.1:9".into(),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    }
}

/// A temporary root whose paths match what a child process reports for its working directory
/// (macOS temporary directories sit behind a symbolic link).
fn root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let path = directory.path().canonicalize().unwrap();
    #[cfg(not(unix))]
    let path = directory.path().to_path_buf();
    (directory, path)
}

fn folder(path: PathBuf) -> PathBuf {
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write_profile(path: &Path, profile: Value) {
    std::fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
}

fn consumer_profile(network: &NetworkConfig, data_dir: impl serde::Serialize) -> Value {
    json!({"network": network, "data_dir": data_dir, "catalog_url": null})
}

fn chiral(current_dir: &Path, config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_chiral"))
        .env_remove("CHIRAL_CONSUMER_CONFIG")
        .current_dir(current_dir)
        .arg("--config")
        .arg(config)
        .args(args)
        .output()
        .unwrap()
}

fn address(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["address"].as_str().unwrap().to_owned()
}

fn refused(output: &Output) -> String {
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(
        !output.status.success(),
        "the profile must be refused: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "stdout carries only machine-readable JSON"
    );
    stderr
}

/// Every directory and file under `path`, with the SHA-256 of each file's contents, to prove
/// nothing was touched; a failure shows digests, never a key or bearer.
fn snapshot(path: &Path) -> BTreeMap<PathBuf, Option<String>> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                entries.insert(path.clone(), None);
                pending.push(path);
            } else {
                let digest = <sha2::Sha256 as sha2::Digest>::digest(std::fs::read(&path).unwrap());
                entries.insert(path.clone(), Some(hex::encode(digest)));
            }
        }
    }
    entries
}

fn is_empty(path: &Path) -> bool {
    std::fs::read_dir(path).unwrap().next().is_none()
}

#[test]
fn relative_profile_paths_resolve_next_to_the_profile_not_the_working_directory() {
    let (_guard, root) = root();
    let network = network("profile-path-resolution");
    let profiles = folder(root.join("profiles"));
    let elsewhere = folder(root.join("elsewhere"));
    let profile = profiles.join("consumer.json");
    write_profile(&profile, consumer_profile(&network, "data"));

    let created = address(&chiral(&elsewhere, &profile, &["wallet", "create"]));

    let state = network.scoped_dir(&profiles.join("data"));
    assert!(state.join("consumer").join("wallet.key").is_file());
    assert!(state
        .join("wallet-journal")
        .join("transactions.sqlite3")
        .is_file());
    assert!(state.join("version-policy").is_dir());
    assert!(
        is_empty(&elsewhere),
        "nothing may be created in the working directory: {:?}",
        snapshot(&elsewhere)
    );
    assert_eq!(
        address(&chiral(&elsewhere, &profile, &["wallet", "address"])),
        created
    );
}

#[test]
fn one_profile_reaches_one_wallet_from_every_working_directory() {
    let (_guard, root) = root();
    let network = network("profile-path-working-directories");
    let profiles = folder(root.join("profiles"));
    let first = folder(root.join("first"));
    let second = folder(root.join("second"));
    let profile = profiles.join("consumer.json");
    write_profile(&profile, consumer_profile(&network, "data"));

    let created = address(&chiral(&first, &profile, &["wallet", "create"]));
    assert_eq!(
        address(&chiral(&second, &profile, &["wallet", "address"])),
        created
    );
    // A relative --config is read from the working directory; the paths inside it still resolve
    // against the profile's own directory, however the same location is spelled.
    for (current_dir, config) in [
        (&root, Path::new("profiles").join("consumer.json")),
        (&profiles, PathBuf::from("consumer.json")),
        (&profiles, Path::new(".").join("consumer.json")),
        (
            &second,
            Path::new("..").join("profiles").join("consumer.json"),
        ),
    ] {
        assert_eq!(
            address(&chiral(current_dir, &config, &["wallet", "address"])),
            created,
            "from {current_dir:?} with --config {config:?}"
        );
    }
    assert!(is_empty(&first) && is_empty(&second));
    let names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        names.len(),
        3,
        "no state beside the profile folder: {names:?}"
    );
}

#[test]
fn state_only_at_the_old_working_directory_location_is_refused_untouched() {
    let (_guard, root) = root();
    let network = network("profile-path-stranded-state");
    let profiles = folder(root.join("profiles"));
    let old_cwd = folder(root.join("old-cwd"));
    let profile = profiles.join("consumer.json");
    write_profile(&profile, consumer_profile(&network, "data"));

    // The state an earlier client created when run from `old-cwd` with this profile.
    let old_data = old_cwd.join("data");
    let old_address = Consumer::new(ConsumerConfig {
        network: network.clone(),
        data_dir: old_data.clone(),
        catalog_url: None,
        development: false,
        dht: None,
    })
    .unwrap()
    .wallet_create()
    .unwrap();
    let before = snapshot(&old_cwd);
    assert!(before.len() > 3);

    let new_data = profiles.join("data");
    for args in [["wallet", "create"], ["wallet", "address"]] {
        let stderr = refused(&chiral(&old_cwd, &profile, &args));
        for required in [
            "relative data_dir \"data\"",
            "Refusing to start with empty state",
            "nothing was created, moved or deleted",
            "set data_dir to the absolute path",
            "deliberately move",
        ] {
            assert!(
                stderr.contains(required),
                "missing {required:?} in {stderr}"
            );
        }
        for path in [
            Path::new("old-cwd").join("data"),
            Path::new("profiles").join("data"),
        ] {
            let path = path.display().to_string();
            assert!(stderr.contains(&path), "missing {path} in {stderr}");
        }
        #[cfg(unix)]
        for path in [
            &old_data,
            &new_data,
            &network.scoped_dir(&old_data),
            &network.scoped_dir(&new_data),
        ] {
            let path = path.display().to_string();
            assert!(stderr.contains(&path), "missing {path} in {stderr}");
        }
        assert!(!new_data.exists(), "a refused profile creates nothing");
        assert_eq!(
            snapshot(&old_cwd),
            before,
            "old state is neither moved nor changed"
        );
    }

    // An absolute data_dir keeps reaching the old state from anywhere, unchanged.
    let absolute = profiles.join("absolute.json");
    write_profile(&absolute, consumer_profile(&network, &old_data));
    assert_eq!(
        address(&chiral(&profiles, &absolute, &["wallet", "address"])),
        old_address
    );
    assert!(!new_data.exists());

    // A deliberate move by the user makes the relative profile load the same wallet.
    std::fs::rename(&old_data, &new_data).unwrap();
    assert_eq!(
        address(&chiral(&old_cwd, &profile, &["wallet", "address"])),
        old_address
    );

    // Once the profile's own location holds state, it is used; another working-directory copy
    // is left untouched.
    Consumer::new(ConsumerConfig {
        network: network.clone(),
        data_dir: old_data.clone(),
        catalog_url: None,
        development: false,
        dht: None,
    })
    .unwrap()
    .wallet_create()
    .unwrap();
    let other = snapshot(&old_cwd);
    assert_eq!(
        address(&chiral(&old_cwd, &profile, &["wallet", "address"])),
        old_address
    );
    assert_eq!(snapshot(&old_cwd), other);
}

#[test]
fn an_empty_location_created_from_another_directory_does_not_hide_an_old_wallet() {
    let (_guard, root) = root();
    let network = network("profile-path-scaffold");
    let profiles = folder(root.join("profiles"));
    let old_cwd = folder(root.join("old-cwd"));
    let other = folder(root.join("other"));
    let profile = profiles.join("consumer.json");
    write_profile(&profile, consumer_profile(&network, "data"));
    let old_data = old_cwd.join("data");
    let old_address = Consumer::new(ConsumerConfig {
        network: network.clone(),
        data_dir: old_data.clone(),
        catalog_url: None,
        development: false,
        dht: None,
    })
    .unwrap()
    .wallet_create()
    .unwrap();

    // Any command run from a directory without old state loads the profile, which creates the
    // network-scoped directories, an empty journal and the policy directory, but no wallet.
    let shown = chiral(&other, &profile, &["network", "show"]);
    assert!(
        shown.status.success(),
        "{}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let new_data = profiles.join("data");
    let new_state = network.scoped_dir(&new_data);
    assert!(new_state.is_dir());
    assert!(!new_state.join("consumer").join("wallet.key").exists());

    // That empty scaffold is not this profile's wallet: the old wallet is still the only one, so
    // a later run from the old working directory is refused instead of creating a second wallet.
    let old_before = snapshot(&old_cwd);
    let new_before = snapshot(&profiles);
    for args in [["wallet", "create"], ["wallet", "address"]] {
        let stderr = refused(&chiral(&old_cwd, &profile, &args));
        for required in [
            "relative data_dir \"data\"",
            "wallet key",
            "nothing was created, moved or deleted",
            "set data_dir to the absolute path",
            "deliberately move",
        ] {
            assert!(
                stderr.contains(required),
                "missing {required:?} in {stderr}"
            );
        }
        for path in [
            Path::new("old-cwd").join("data"),
            Path::new("profiles").join("data"),
        ] {
            let path = path.display().to_string();
            assert!(stderr.contains(&path), "missing {path} in {stderr}");
        }
        assert_eq!(snapshot(&old_cwd), old_before, "old state is untouched");
        assert_eq!(snapshot(&profiles), new_before, "no wallet is created");
    }

    // The absolute remedy still reaches the old wallet.
    let absolute = profiles.join("absolute.json");
    write_profile(&absolute, consumer_profile(&network, &old_data));
    assert_eq!(
        address(&chiral(&other, &absolute, &["wallet", "address"])),
        old_address
    );
}

#[test]
fn stranded_relative_bearer_and_identity_files_are_refused_untouched() {
    for (field, name) in [
        ("network.rpc_bearer_file", "rpc.token"),
        ("dht.identity_file", "peer.key"),
    ] {
        let (_guard, root) = root();
        let network = network("profile-path-stranded-file");
        let profiles = folder(root.join("profiles"));
        let old_cwd = folder(root.join("old-cwd"));
        let profile = profiles.join("consumer.json");
        let mut document = consumer_profile(&network, "data");
        if field == "dht.identity_file" {
            document["dht"] =
                json!({"listen": "/ip4/127.0.0.1/tcp/0", "bootstrap": [], "identity_file": name});
        } else {
            document["network"]["rpc_bearer_file"] = json!(name);
        }
        write_profile(&profile, document);
        std::fs::write(
            old_cwd.join(name),
            b"a file an earlier client read from here",
        )
        .unwrap();
        let before = snapshot(&old_cwd);

        let stderr = refused(&chiral(&old_cwd, &profile, &["wallet", "create"]));
        for required in [
            format!("relative {field} \"{name}\""),
            "nothing was created, moved or deleted".into(),
            format!("set {field} to the absolute path"),
        ] {
            assert!(
                stderr.contains(&required),
                "missing {required:?} in {stderr}"
            );
        }
        for path in [
            Path::new("old-cwd").join(name),
            Path::new("profiles").join(name),
        ] {
            let path = path.display().to_string();
            assert!(stderr.contains(&path), "missing {path} in {stderr}");
        }
        assert_eq!(snapshot(&old_cwd), before, "{field}");
        assert!(
            !profiles.join(name).exists() && !profiles.join("data").exists(),
            "{field}"
        );
    }
}

fn development_provider(network: &NetworkConfig) -> Value {
    json!({
        "network": network,
        "data_dir": "data", "signing_key_file": "wallet.key", "admin_token_file": "admin.token",
        "public_endpoint": "http://127.0.0.1:18082",
        "public_bind": "127.0.0.1:18082", "admin_bind": "127.0.0.1:18083",
        "platform_wallet": "0x2222222222222222222222222222222222222222",
        "fee_bps": 50, "min_funding_wei": "1000",
        "models": [{
            "model": {
                "id": "test/model", "revision": "ab".repeat(20),
                "tokenizer_revision": "cd".repeat(20), "chat_template_sha256": "ef".repeat(32),
                "quantization": null, "context_length": 4096, "max_output_tokens": 256,
                "capabilities": ["chat"], "serving": null
            },
            "price": { "input_wei_per_1k": "1000", "output_wei_per_1k": "1000" },
            "engine_url": "http://127.0.0.1:18080", "engine_api_key_file": "engine.key",
            "runtime": null, "max_concurrency": 1, "request_timeout_secs": 30,
            "qualification_evidence_file": null
        }],
        "dht": { "listen": "/ip4/127.0.0.1/tcp/0", "bootstrap": [], "identity_file": "dht.key" },
        "development": true
    })
}

#[test]
fn a_consumer_profile_and_a_provider_config_in_one_folder_share_one_journal() {
    let (_guard, root) = root();
    let network = network("profile-path-shared-journal");
    let shared = folder(root.join("shared"));
    let elsewhere = folder(root.join("elsewhere"));
    let profile = shared.join("consumer.json");
    write_profile(&profile, consumer_profile(&network, "data"));
    let provider_path = shared.join("provider.json");
    write_profile(&provider_path, development_provider(&network));

    address(&chiral(&elsewhere, &profile, &["wallet", "create"]));
    let provider = ProviderConfig::load(&provider_path).unwrap();

    assert_eq!(provider.data_dir, shared.join("data"));
    let state = provider.network.scoped_dir(&provider.data_dir);
    assert!(
        state
            .join("wallet-journal")
            .join("transactions.sqlite3")
            .is_file(),
        "the consumer's journal is the provider's journal (book line 470)"
    );
    assert!(state.join("consumer").join("wallet.key").is_file());
    assert!(is_empty(&elsewhere));
}

/// The subcommand names a `chiral` help screen lists, without `help`.
fn subcommands(path: &[&str]) -> Vec<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_chiral"))
        .env_remove("CHIRAL_CONSUMER_CONFIG")
        .args(path)
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success(), "{path:?}");
    let help = String::from_utf8(output.stdout).unwrap();
    let listed = help
        .split("Commands:\n")
        .nth(1)
        .unwrap_or_else(|| panic!("{path:?}: {help}"));
    listed
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_owned)
        .collect()
}

/// profile-path-resolution requirement 5 scenario "The CLI reports a refusal on stderr" (task
/// 3.5) for every command: each subcommand the CLI lists, with arguments that would otherwise
/// reach the chain, a provider or a file, is given a refused profile (this network's wallet only
/// at the old working-directory location). Each exits with status 1, writes nothing to standard
/// output, writes the refusal as one `chiral: ` diagnostic line to standard error, creates
/// nothing (neither the profile's `data_dir` nor the export file) and leaves the old state
/// unchanged. The list is checked against the help screens, so a new command must be added here.
#[test]
fn every_cli_command_reports_a_refused_profile_on_stderr_only() {
    let (_guard, root) = root();
    let network = network("profile-path-every-command");
    let profiles = folder(root.join("profiles"));
    let old_cwd = folder(root.join("old-cwd"));
    let profile = profiles.join("consumer.json");
    write_profile(&profile, consumer_profile(&network, "data"));
    Consumer::new(ConsumerConfig {
        network: network.clone(),
        data_dir: old_cwd.join("data"),
        catalog_url: None,
        development: false,
        dht: None,
    })
    .unwrap()
    .wallet_create()
    .unwrap();
    let before = snapshot(&old_cwd);
    let profiles_before = snapshot(&profiles);

    let contract = "c".repeat(64);
    let c = contract.as_str();
    let commands: Vec<Vec<&str>> = vec![
        vec!["network", "show"],
        vec!["network", "status"],
        vec!["network", "policy"],
        vec!["wallet", "create"],
        vec!["wallet", "import", "--file", "import.key"],
        vec!["wallet", "export", "--file", "export.key"],
        vec!["wallet", "address"],
        vec!["wallet", "balance"],
        vec!["wallet", "history"],
        vec!["discover"],
        vec!["discover", "--model", "test/model"],
        vec!["quote", "--offer", "offer.json", "--amount-wei", "1000"],
        vec!["fund", "--offer", "offer.json", "--quote", "quote.json"],
        vec!["resume", c],
        vec!["cancel-funding", c, "--accept-gas-and-race-risk"],
        vec!["topup", c, "--amount-wei", "1000"],
        vec!["accounts"],
        vec!["pending"],
        vec!["account", c],
        vec!["usage", c],
        vec!["funding", c],
        vec!["funding-recovery", c],
        vec!["receipt", c],
        vec!["credentials", c, "list"],
        vec!["credentials", c, "issue"],
        vec!["credentials", c, "revoke", "credential"],
        vec!["credentials", c, "rotate", "credential"],
        vec!["credentials", c, "export"],
        vec!["infer", c, "--request", "request.json"],
        vec!["infer", c, "--request", "request.json", "--stream"],
        vec!["request", c, "request-1"],
        vec!["request", c, "request-1", "--cancel"],
        vec!["requests", c],
        vec!["provider-admin", "--endpoint", "https://provider.example", "--token-file", "admin.token", "status"],
        vec!["feedback", c, "--score", "5", "--revision", "1"],
    ];

    // Every listed command and nested command is exercised.
    let named = |command: &Vec<&str>| match command[0] {
        "network" | "wallet" => format!("{} {}", command[0], command[1]),
        "credentials" => format!("credentials {}", command[2]),
        name => name.to_owned(),
    };
    let exercised: std::collections::BTreeSet<_> = commands.iter().map(named).collect();
    let mut listed = std::collections::BTreeSet::new();
    for name in subcommands(&[]) {
        if matches!(name.as_str(), "network" | "wallet" | "credentials") {
            for nested in subcommands(&[&name]) {
                listed.insert(format!("{name} {nested}"));
            }
        } else {
            listed.insert(name);
        }
    }
    assert_eq!(listed.len(), 32, "{listed:?}");
    assert_eq!(exercised, listed);

    for args in &commands {
        let output = chiral(&old_cwd, &profile, args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        let stderr = refused(&output);
        let start = format!("chiral: consumer profile {}: relative data_dir \"data\"", profile.display());
        assert!(stderr.starts_with(&start), "{args:?}: {stderr}");
        assert_eq!(stderr.lines().count(), 1, "{args:?}: one diagnostic line: {stderr}");
        for required in ["Refusing to start with empty state", "nothing was created, moved or deleted"] {
            assert!(stderr.contains(required), "{args:?}: missing {required:?} in {stderr}");
        }
        assert_eq!(snapshot(&old_cwd), before, "{args:?}: old state changed");
        assert_eq!(snapshot(&profiles), profiles_before, "{args:?}: state created");
    }
}

/// profile-path-resolution requirement 1 (task 3.1): path arguments given on the `chiral` command
/// line are not configuration paths and follow the process working directory, while the
/// profile's own `data_dir` still resolves next to the profile. A wallet export file and the
/// import of it, an offer document (`quote --offer`, `fund --offer`), a quote document (`fund
/// --quote`), an inference request document, credential options documents (`credentials issue`
/// and `rotate --options`) and the provider administration `--token-file` are each found in (or
/// written to) the working directory, never beside the profile.
#[test]
fn command_line_path_arguments_follow_the_working_directory() {
    let (_guard, root) = root();
    let network = network("profile-path-command-line");
    let profiles = folder(root.join("profiles"));
    let others = folder(root.join("others"));
    let work = folder(root.join("work"));
    let profile = profiles.join("consumer.json");
    write_profile(&profile, consumer_profile(&network, "data"));
    let other = others.join("consumer.json");
    write_profile(&other, consumer_profile(&network, "data"));

    let created = address(&chiral(&work, &profile, &["wallet", "create"]));
    let exported = chiral(&work, &profile, &["wallet", "export", "--file", "backup.key"]);
    assert!(exported.status.success(), "{}", String::from_utf8_lossy(&exported.stderr));
    assert!(work.join("backup.key").is_file(), "the export is written to the working directory");
    assert!(!profiles.join("backup.key").exists());
    // The import reads the working directory's file into the other profile's own data_dir.
    assert_eq!(
        address(&chiral(&work, &other, &["wallet", "import", "--file", "backup.key"])),
        created
    );
    assert!(!others.join("backup.key").exists());
    assert!(network.scoped_dir(&others.join("data")).join("consumer").join("wallet.key").is_file());

    // Documents are read from the working directory: an unparsable one there is reported as
    // such; were they read beside the profile, where none exists, the file would be unreadable.
    std::fs::write(work.join("offer.json"), b"not a signed offer").unwrap();
    std::fs::write(work.join("request.json"), b"not a request").unwrap();
    std::fs::write(work.join("options.json"), b"not credential options").unwrap();
    for args in [
        vec!["quote", "--offer", "offer.json", "--amount-wei", "1000"],
        vec!["infer", "contract", "--request", "request.json"],
        vec!["credentials", "contract", "issue", "--options", "options.json"],
        vec!["credentials", "contract", "rotate", "credential", "--options", "options.json"],
    ] {
        let output = chiral(&work, &profile, &args);
        let stderr = refused(&output);
        assert_eq!(stderr.trim(), "chiral: invalid JSON input document", "{args:?}");
    }
    // `fund` reads its offer and then its quote document: with a parsable signed offer in the
    // working directory, the unparsable quote there is the reported failure.
    let signed_offer = json!({
        "payload": {
            "network": network.identity,
            "provider_wallet": "0x1111111111111111111111111111111111111111",
            "peer_id": "peer", "deployment_id": "deployment", "endpoint": "https://provider.example",
            "model": {
                "id": "test/model", "revision": "ab".repeat(20), "tokenizer_revision": "cd".repeat(20),
                "chat_template_sha256": "ef".repeat(32), "quantization": null, "context_length": 4096,
                "max_output_tokens": 256, "capabilities": ["chat"], "serving": null
            },
            "price": {"input_wei_per_1k": "1000", "output_wei_per_1k": "1000"},
            "min_funding_wei": "1000", "platform_wallet": "0x2222222222222222222222222222222222222222",
            "fee_bps": 50, "max_concurrency": 1, "revision": 1, "issued_at": 1, "expires_at": 2
        },
        "signature": "00"
    });
    serde_json::from_value::<Signed<InferenceOffer>>(signed_offer.clone()).expect("the offer document parses");
    std::fs::write(work.join("signed-offer.json"), serde_json::to_vec(&signed_offer).unwrap()).unwrap();
    std::fs::write(work.join("quote.json"), b"not a signed quote").unwrap();
    let output = chiral(&work, &profile, &["fund", "--offer", "signed-offer.json", "--quote", "quote.json"]);
    let stderr = refused(&output);
    assert_eq!(stderr.lines().last(), Some("chiral: invalid JSON input document"), "fund: {stderr}");
    // The administration token file is read from the working directory before any request: an
    // empty protected file there is refused as empty, where a missing one would be unreadable.
    drop(chiral_network::wallet::create_secret_file(&work.join("admin.token")).unwrap());
    let output = chiral(
        &work,
        &profile,
        &["provider-admin", "--endpoint", "http://127.0.0.1:9", "--token-file", "admin.token", "status"],
    );
    assert_eq!(refused(&output).trim(), "chiral: empty admin token");
    let mut names: Vec<_> = std::fs::read_dir(&work)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["admin.token", "backup.key", "offer.json", "options.json", "quote.json", "request.json", "signed-offer.json"]
    );
    assert!(!profiles.join("admin.token").exists() && !profiles.join("quote.json").exists());
}

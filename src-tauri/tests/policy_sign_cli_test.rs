//! `chiral-policy-sign` keeps the policy authority's private key in a protected file:
//! never on the command line, in the environment or on standard output (book lines 474
//! and 648; client-version-policy requirement 8).
use chiral_network::version::{bundled_policy, canonical_signing_payload, VersionPolicy};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn tool(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_chiral-policy-sign"))
        .env_remove("CHIRAL_POLICY_SECRET")
        .args(args)
        .output()
        .unwrap()
}
fn keygen(path: &Path) -> (Output, Option<String>) {
    let output = tool(&["keygen", "--out", path.to_str().unwrap()]);
    let public = serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .and_then(|json| json["public_key"].as_str().map(str::to_owned));
    (output, public)
}
fn verifies(policy: &VersionPolicy, public: &str) -> bool {
    let key: [u8; 32] = hex::decode(public).unwrap().try_into().unwrap();
    let signature: [u8; 64] = hex::decode(&policy.signature).unwrap().try_into().unwrap();
    VerifyingKey::from_bytes(&key)
        .unwrap()
        .verify(&canonical_signing_payload(policy), &Signature::from_bytes(&signature))
        .is_ok()
}

#[test]
fn keygen_writes_an_owner_only_key_file_and_prints_only_the_public_key() {
    let directory = tempfile::tempdir().unwrap();
    let key_file = directory.path().join("authority.key");
    let (output, public) = keygen(&key_file);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let public = public.expect("JSON with the public key on stdout");
    assert_eq!(public.len(), 64);
    let secret = std::fs::read_to_string(&key_file).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains(secret.trim()), "the private key must never be printed");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["secret_file"], key_file.display().to_string());
    assert_eq!(json.as_object().unwrap().len(), 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&key_file).unwrap().permissions().mode() & 0o777, 0o600);
    }
    // An existing path is refused, and the existing key is untouched.
    let (again, _) = keygen(&key_file);
    assert!(!again.status.success());
    assert!(again.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&key_file).unwrap(), secret);
}

#[test]
fn sign_reads_the_key_only_from_a_protected_file() {
    let directory = tempfile::tempdir().unwrap();
    let key_file = directory.path().join("authority.key");
    let other_file = directory.path().join("other.key");
    let (_, public) = keygen(&key_file);
    let (_, other) = keygen(&other_file);
    let (public, other) = (public.unwrap(), other.unwrap());
    let unsigned = directory.path().join("policy.json");
    let mut policy = bundled_policy();
    policy.signature.clear();
    std::fs::write(&unsigned, serde_json::to_string(&policy).unwrap()).unwrap();

    let signed = tool(&["sign", "--key-file", key_file.to_str().unwrap(), "--in", unsigned.to_str().unwrap()]);
    assert!(signed.status.success(), "{}", String::from_utf8_lossy(&signed.stderr));
    let signed_policy: VersionPolicy = serde_json::from_slice(&signed.stdout).unwrap();
    assert!(verifies(&signed_policy, &public));
    assert!(!verifies(&signed_policy, &other));
    let signed_file = directory.path().join("signed.json");
    std::fs::write(&signed_file, &signed.stdout).unwrap();
    let accepted = tool(&["verify", "--pub", &public, "--in", signed_file.to_str().unwrap()]);
    assert!(accepted.status.success());
    let rejected = tool(&["verify", "--pub", &other, "--in", signed_file.to_str().unwrap()]);
    assert!(!rejected.status.success());

    // No argument or environment variable carries the key itself.
    let secret = std::fs::read_to_string(&key_file).unwrap();
    let on_argv = tool(&["sign", "--key", secret.trim(), "--in", unsigned.to_str().unwrap()]);
    assert!(!on_argv.status.success());
    assert!(on_argv.stdout.is_empty());
    let from_env = Command::new(env!("CARGO_BIN_EXE_chiral-policy-sign"))
        .env("CHIRAL_POLICY_SECRET", secret.trim())
        .args(["sign", "--in", unsigned.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!from_env.status.success());
    assert!(from_env.stdout.is_empty());

    // A key file other users can read is refused before anything is signed.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let exposed = tool(&["sign", "--key-file", key_file.to_str().unwrap(), "--in", unsigned.to_str().unwrap()]);
        assert!(!exposed.status.success());
        assert!(exposed.stdout.is_empty());
        assert!(String::from_utf8_lossy(&exposed.stderr).contains("no group/other permissions"));
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = directory.path().join("link.key");
        std::os::unix::fs::symlink(&key_file, &link).unwrap();
        let linked = tool(&["sign", "--key-file", link.to_str().unwrap(), "--in", unsigned.to_str().unwrap()]);
        assert!(!linked.status.success());
        assert!(linked.stdout.is_empty());
    }
}

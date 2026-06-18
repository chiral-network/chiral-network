use std::{env, fs, path::PathBuf};

#[path = "src/policy_key_validation.rs"]
mod policy_key_validation;

fn main() {
    println!("cargo:rerun-if-env-changed=CHIRAL_POLICY_PUBLIC_KEY");
    let policy_key = resolve_build_policy_key();
    write_policy_key(policy_key);
    tauri_build::build()
}

fn resolve_build_policy_key() -> [u8; 32] {
    match env::var("CHIRAL_POLICY_PUBLIC_KEY") {
        Ok(raw) => match policy_key_validation::parse_policy_public_key_hex(&raw) {
            Ok(key) if !policy_key_validation::is_placeholder_policy_key(&key) => key,
            Ok(_) if is_release_profile() => {
                panic!("CHIRAL_POLICY_PUBLIC_KEY must not be the all-zero placeholder in release builds")
            }
            Ok(key) => key,
            Err(err) if is_release_profile() => {
                panic!("CHIRAL_POLICY_PUBLIC_KEY is required for release builds: {err}")
            }
            Err(_) => [0u8; 32],
        },
        Err(_) if is_release_profile() => {
            panic!("CHIRAL_POLICY_PUBLIC_KEY must be set to a nonzero 32-byte hex Ed25519 public key for release builds")
        }
        Err(_) => [0u8; 32],
    }
}

fn is_release_profile() -> bool {
    env::var("PROFILE")
        .map(|profile| profile == "release")
        .unwrap_or(false)
}

fn write_policy_key(key: [u8; 32]) {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by Cargo"));
    let bytes = key
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("pub const POLICY_PUBLIC_KEY: [u8; 32] = [{bytes}];\n");
    fs::write(out_dir.join("policy_public_key.rs"), source)
        .expect("write generated policy_public_key.rs");
}

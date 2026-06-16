use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=CHIRAL_POLICY_PUBLIC_KEY");
    let policy_key = resolve_build_policy_key();
    write_policy_key(policy_key);
    tauri_build::build()
}

fn resolve_build_policy_key() -> [u8; 32] {
    match env::var("CHIRAL_POLICY_PUBLIC_KEY") {
        Ok(raw) => match parse_policy_key_hex(&raw) {
            Ok(key) if key != [0u8; 32] => key,
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

fn parse_policy_key_hex(raw: &str) -> Result<[u8; 32], String> {
    let cleaned = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
    if cleaned.len() != 64 {
        return Err("expected exactly 64 hex characters".to_string());
    }

    let mut out = [0u8; 32];
    for (index, chunk) in cleaned.as_bytes().chunks_exact(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[index] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("key contains non-hex characters".to_string()),
    }
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

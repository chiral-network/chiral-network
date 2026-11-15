//! Local wallet material and exact CFX units. Chain orchestration lives in chain_client.
use rand::RngCore;
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, PublicKey, Secp256k1, SecretKey,
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;
#[cfg(windows)]
#[path = "wallet_windows.rs"]
mod windows;

pub(crate) fn secret(key: &str) -> Result<SecretKey, String> {
    let key = key.strip_prefix("0x").unwrap_or(key);
    if key.len() != 64 {
        return Err("private key must contain exactly 32 bytes".into());
    }
    let bytes = Zeroizing::new(hex::decode(key).map_err(|_| "invalid private key encoding")?);
    SecretKey::from_slice(&bytes).map_err(|_| "invalid private key scalar".into())
}
fn public_address(key: &PublicKey) -> String {
    let hash = crate::protocol::digest(&key.serialize_uncompressed()[1..]);
    format!("0x{}", hex::encode(&hash[12..]))
}
pub fn address_from_private_key(key: &str) -> Result<String, String> {
    Ok(public_address(&PublicKey::from_secret_key(
        &Secp256k1::new(),
        &secret(key)?,
    )))
}
pub fn generate_private_key() -> String {
    let mut bytes = Zeroizing::new([0u8; 32]);
    loop {
        rand::rngs::OsRng.fill_bytes(&mut *bytes);
        if SecretKey::from_slice(&*bytes).is_ok() {
            return hex::encode(&*bytes);
        }
    }
}
/// Validates the opened object, never an earlier path metadata snapshot.
pub fn validate_secret_file(file: &File) -> Result<(), String> {
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect protected file")?;
    if !metadata.is_file() {
        return Err("protected secret must be a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(
                "secret file must be owned by this user with no group/other permissions".into(),
            );
        }
    }
    #[cfg(windows)]
    windows::validate(file)?;
    #[cfg(not(any(unix, windows)))]
    return Err("secure file permissions are unsupported on this platform".into());
    Ok(())
}
pub fn create_secret_file(path: &Path) -> Result<File, String> {
    #[cfg(windows)]
    {
        return windows::create(path);
    }
    #[cfg(not(windows))]
    {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|_| "cannot create protected secret file (must not exist)")?;
        validate_secret_file(&file)?;
        Ok(file)
    }
}
/// Creates a private application directory or rejects an insecure existing one.
pub fn secure_directory(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        return windows::directory(path);
    }
    #[cfg(not(windows))]
    {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|_| "cannot create private directory")?;
        let meta =
            std::fs::symlink_metadata(path).map_err(|_| "cannot inspect private directory")?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("private directory cannot be a symlink".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.mode() & 0o077 != 0 || meta.uid() != unsafe { libc::geteuid() } {
                return Err("private directory must belong to this user and have mode 0700".into());
            }
        }
        #[cfg(not(unix))]
        return Err("secure directories are unsupported on this platform".into());
        Ok(())
    }
}
/// Opens a bounded credential without following symlinks or Windows reparse points.
pub(crate) fn protected_file(path: &Path) -> Result<File, String> {
    let file = open_secret_file(path)?;
    if file
        .metadata()
        .map_err(|_| "cannot inspect credential")?
        .len()
        > 4096
    {
        return Err("credential file exceeds size limit".into());
    }
    Ok(file)
}
pub fn open_secret_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|_| "cannot open protected secret file")?;
    validate_secret_file(&file)?;
    Ok(file)
}
pub fn load_private_key(path: &Path) -> Result<String, String> {
    let mut text = Zeroizing::new(String::new());
    protected_file(path)?
        .take(4097)
        .read_to_string(&mut text)
        .map_err(|_| "cannot read private key")?;
    let key = text.trim();
    secret(key)?;
    Ok(key.strip_prefix("0x").unwrap_or(key).to_ascii_lowercase())
}
/// Never replaces an existing wallet, symlink, or legacy key/history file.
pub fn save_private_key(path: &Path, key: &str) -> Result<(), String> {
    secret(key)?;
    let mut file = create_secret_file(path)?;
    file.write_all(key.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| "cannot persist private key")?;
    #[cfg(unix)]
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| "cannot persist private key directory")?;
    }
    Ok(())
}
pub fn parse_units(value: &str) -> Result<u128, String> {
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    if parts.next().is_some() || whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err("amount must be an unsigned decimal CFX string".into());
    }
    let fraction = fraction.unwrap_or("");
    if fraction.len() > 18 || !fraction.bytes().all(|b| b.is_ascii_digit()) || value.ends_with('.')
    {
        return Err("amount must have at most 18 fractional digits".into());
    }
    let whole = whole.parse::<u128>().map_err(|_| "amount overflow")?;
    let fractional = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u128>().map_err(|_| "invalid fraction")?
            * 10u128.pow(18 - fraction.len() as u32)
    };
    whole
        .checked_mul(10u128.pow(18))
        .and_then(|n| n.checked_add(fractional))
        .ok_or_else(|| "amount overflow".into())
}
pub fn format_units(value: u128) -> String {
    let scale = 10u128.pow(18);
    let whole = value / scale;
    let fraction = value % scale;
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{:018}", fraction)
            .trim_end_matches('0')
            .to_owned()
    }
}
fn personal_digest(data: &[u8]) -> [u8; 32] {
    use tiny_keccak::{Hasher, Keccak};
    let mut hash = Keccak::v256();
    hash.update(format!("\x19Ethereum Signed Message:\n{}", data.len()).as_bytes());
    hash.update(data);
    let mut out = [0; 32];
    hash.finalize(&mut out);
    out
}
pub fn sign_message(key: &str, data: &[u8]) -> Result<String, String> {
    let message = Message::from_digest(personal_digest(data));
    let (id, signature) = Secp256k1::new()
        .sign_ecdsa_recoverable(&message, &secret(key)?)
        .serialize_compact();
    let mut bytes = [0; 65];
    bytes[..64].copy_from_slice(&signature);
    bytes[64] = id.to_i32() as u8 + 27;
    Ok(hex::encode(bytes))
}
pub(crate) fn recover_digest(
    hash: [u8; 32],
    bytes: &[u8; 64],
    parity: u8,
) -> Result<String, String> {
    if parity > 1 {
        return Err("signature recovery parity must be zero or one".into());
    }
    let signature = RecoverableSignature::from_compact(
        bytes,
        RecoveryId::from_i32(parity as i32).map_err(|_| "invalid parity")?,
    )
    .map_err(|_| "invalid signature scalar")?;
    let original = signature.to_standard();
    let mut normalized = original;
    normalized.normalize_s();
    if original != normalized {
        return Err("non-canonical high-s signature".into());
    }
    let public = Secp256k1::new()
        .recover_ecdsa(&Message::from_digest(hash), &signature)
        .map_err(|_| "signature recovery failed")?;
    Ok(public_address(&public))
}
pub fn recover_signer(data: &[u8], signature: &str) -> Result<String, String> {
    let signature = signature.strip_prefix("0x").unwrap_or(signature);
    if signature.len() != 130 {
        return Err("signature must be 65 bytes".into());
    }
    let bytes = hex::decode(signature).map_err(|_| "invalid signature encoding")?;
    let parity = bytes[64]
        .checked_sub(27)
        .ok_or("invalid personal-sign recovery id")?;
    recover_digest(
        personal_digest(data),
        bytes[..64]
            .try_into()
            .map_err(|_| "invalid signature size")?,
        parity,
    )
}
pub fn verify_signature(data: &[u8], signature: &str, expected_address: &str) -> bool {
    recover_signer(data, signature)
        .is_ok_and(|address| address == expected_address.to_ascii_lowercase())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_units() {
        for n in [0, 1, 999999999999999999, 1000000000000000000, u128::MAX] {
            assert_eq!(parse_units(&format_units(n)).unwrap(), n);
        }
        for bad in [
            "-1",
            "+1",
            "1e2",
            "NaN",
            "1.",
            ".1",
            " 1",
            "0.0000000000000000001",
            "340282366920938463464",
        ] {
            assert!(parse_units(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn personal_sign_roundtrip_and_tamper() {
        let key = "01".repeat(32);
        let sig = sign_message(&key, b"hello").unwrap();
        assert_eq!(
            recover_signer(b"hello", &sig).unwrap(),
            address_from_private_key(&key).unwrap()
        );
        assert!(!verify_signature(
            b"other",
            &sig,
            &address_from_private_key(&key).unwrap()
        ));
        let mut bytes = hex::decode(sig).unwrap();
        // n-s is the alternate, malleable signature: it must be rejected.
        let order = primitive_types::U256::from_big_endian(
            &hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                .unwrap(),
        );
        let high = order - primitive_types::U256::from_big_endian(&bytes[32..64]);
        high.to_big_endian(&mut bytes[32..64]);
        bytes[64] = if bytes[64] == 27 { 28 } else { 27 };
        assert!(recover_signer(b"hello", &hex::encode(bytes)).is_err());
    }
    #[test]
    fn key_files_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        let key = generate_private_key();
        save_private_key(&path, &key).unwrap();
        assert_eq!(load_private_key(&path).unwrap(), key);
        assert!(save_private_key(&path, &key).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(load_private_key(&path).is_err());
        }
    }
    #[test]
    fn shared_secret_helpers_preserve_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("private");
        secure_directory(&dir).unwrap();
        secure_directory(&dir).unwrap();
        let path = dir.join("credential");
        let mut file = create_secret_file(&path).unwrap();
        file.write_all(b"test-only").unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(open_secret_file(&path).is_ok());
        assert!(create_secret_file(&path).is_err());
        #[cfg(unix)]
        {
            let link = dir.join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(open_secret_file(&link).is_err());
            assert!(create_secret_file(&link).is_err());
        }
        #[cfg(windows)]
        {
            let inherited = root.path().join("inherited");
            std::fs::write(&inherited, b"not protected").unwrap();
            assert!(open_secret_file(&inherited).is_err());
        }
    }
}

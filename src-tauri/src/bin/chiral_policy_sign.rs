//! `chiral-policy-sign` — operator CLI for Phase 5 of the version-enforcement
//! plan. Signs a `VersionPolicy` JSON with the project's offline Ed25519 key
//! so that a relay can serve it and clients will accept it via
//! `version::is_acceptable_remote_policy`.
//!
//! Subcommands:
//!   keygen                   — print a fresh Ed25519 keypair (hex).
//!   sign --key <hex>         — read policy JSON from stdin (or --in <path>),
//!                              fill `signature`, write to stdout (or --out).
//!   verify --pub <hex>       — verify a signed policy against a public key.
//!
//! The matching public key is supplied as `CHIRAL_POLICY_PUBLIC_KEY` when
//! building release binaries. The private key stays offline / in a CI
//! secret — never checked in.

use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use chiral_network::version::{
    canonical_signing_payload, verify_policy, VersionPolicy, POLICY_PUBLIC_KEY,
};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;

#[derive(Parser)]
#[command(name = "chiral-policy-sign", about = "Sign Chiral version policies")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a fresh Ed25519 keypair. Prints `public=<hex>` and
    /// `secret=<hex>` to stdout. Paste the public into
    /// release builds as `CHIRAL_POLICY_PUBLIC_KEY`; keep the secret offline.
    Keygen,
    /// Sign a policy JSON. Reads from `--in` or stdin; writes the signed
    /// JSON to `--out` or stdout.
    Sign {
        /// 64-hex-char (32-byte) Ed25519 secret key.
        #[arg(long, env = "CHIRAL_POLICY_SECRET")]
        key: String,
        #[arg(long)]
        r#in: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
    /// Verify a signed policy. Defaults to the public key compiled into
    /// this binary (`version::POLICY_PUBLIC_KEY`); `--pub` overrides.
    Verify {
        #[arg(long = "pub")]
        public_key: Option<String>,
        #[arg(long)]
        r#in: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Keygen => {
            let signing = SigningKey::generate(&mut OsRng);
            let verifying: VerifyingKey = signing.verifying_key();
            println!("public={}", hex::encode(verifying.to_bytes()));
            println!("secret={}", hex::encode(signing.to_bytes()));
            ExitCode::SUCCESS
        }
        Cmd::Sign { key, r#in, out } => match run_sign(&key, r#in.as_deref(), out.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("sign: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Verify { public_key, r#in } => {
            match run_verify(public_key.as_deref(), r#in.as_deref()) {
                Ok(true) => {
                    println!("ok");
                    ExitCode::SUCCESS
                }
                Ok(false) => {
                    eprintln!("verify: signature does not match");
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("verify: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn read_input(path: Option<&str>) -> io::Result<String> {
    match path {
        Some(p) => fs::read_to_string(p),
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

fn write_output(path: Option<&str>, s: &str) -> io::Result<()> {
    match path {
        Some(p) => fs::write(p, s),
        None => {
            io::stdout().write_all(s.as_bytes())?;
            io::stdout().write_all(b"\n")
        }
    }
}

fn parse_secret(hex_str: &str) -> Result<SigningKey, String> {
    let bytes = hex::decode(hex_str.trim()).map_err(|e| format!("bad hex secret: {e}"))?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "secret must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&arr))
}

fn run_sign(key: &str, in_path: Option<&str>, out_path: Option<&str>) -> Result<(), String> {
    let signing = parse_secret(key)?;
    let raw = read_input(in_path).map_err(|e| format!("read input: {e}"))?;
    let mut policy: VersionPolicy =
        serde_json::from_str(&raw).map_err(|e| format!("parse policy: {e}"))?;
    let payload = canonical_signing_payload(&policy);
    let sig = signing.sign(&payload);
    policy.signature = hex::encode(sig.to_bytes());
    let out =
        serde_json::to_string_pretty(&policy).map_err(|e| format!("serialize signed: {e}"))?;
    write_output(out_path, &out).map_err(|e| format!("write output: {e}"))?;
    Ok(())
}

fn run_verify(public_key: Option<&str>, in_path: Option<&str>) -> Result<bool, String> {
    let raw = read_input(in_path).map_err(|e| format!("read input: {e}"))?;
    let policy: VersionPolicy =
        serde_json::from_str(&raw).map_err(|e| format!("parse policy: {e}"))?;
    match public_key {
        None => Ok(verify_policy(&policy)),
        Some(hex_str) => {
            let bytes = hex::decode(hex_str.trim()).map_err(|e| format!("bad hex pub: {e}"))?;
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| "pub must be 32 bytes".to_string())?;
            verify_against(&policy, &arr)
        }
    }
}

fn verify_against(policy: &VersionPolicy, pubkey: &[u8; 32]) -> Result<bool, String> {
    use ed25519_dalek::{Signature, Verifier};
    if policy.signature.is_empty() {
        return Ok(false);
    }
    let sig_bytes =
        hex::decode(&policy.signature).map_err(|e| format!("bad hex signature: {e}"))?;
    if sig_bytes.len() != 64 {
        return Err("signature must be 64 bytes".to_string());
    }
    let signature = Signature::from_slice(&sig_bytes).map_err(|e| format!("bad sig: {e}"))?;
    let key = VerifyingKey::from_bytes(pubkey).map_err(|e| format!("bad pubkey: {e}"))?;
    Ok(key
        .verify(&canonical_signing_payload(policy), &signature)
        .is_ok())
}

// Silence dead-code warning on POLICY_PUBLIC_KEY — referenced indirectly via
// `verify_policy` when `--pub` is omitted, but keep an explicit reference so
// `cargo check` on this bin alone still touches it.
#[allow(dead_code)]
const _POLICY_PK_REF: &[u8; 32] = &POLICY_PUBLIC_KEY;

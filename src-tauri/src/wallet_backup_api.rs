use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::{Deserialize, Serialize};
use std::env;

const DEFAULT_SMTP_PORT: u16 = 587;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct WalletBackupEmailRequest {
    email: String,
    wallet_address: String,
    encrypted_backup: EncryptedWalletBackupPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct EncryptedWalletBackupPayload {
    version: String,
    algorithm: String,
    kdf: String,
    iterations: u32,
    salt: String,
    iv: String,
    ciphertext: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletBackupEmailResponse {
    ok: bool,
}

#[derive(Debug, Clone)]
struct SmtpSettings {
    host: String,
    port: u16,
    username: String,
    password: String,
    from: String,
    starttls: bool,
}

fn is_valid_wallet_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn base64_decoded_len(value: &str) -> Option<usize> {
    STANDARD.decode(value.trim()).ok().map(|bytes| bytes.len())
}

fn is_valid_encrypted_backup_payload(value: &EncryptedWalletBackupPayload) -> bool {
    value.version == "chiral-wallet-backup-v1"
        && value.algorithm == "AES-256-GCM"
        && value.kdf == "PBKDF2-SHA256"
        && value.iterations >= 100_000
        && base64_decoded_len(&value.salt) == Some(16)
        && base64_decoded_len(&value.iv) == Some(12)
        && base64_decoded_len(&value.ciphertext).is_some_and(|len| len > 0)
}

fn parse_smtp_settings() -> Result<SmtpSettings, String> {
    let host = env::var("CHIRAL_WALLET_EMAIL_SMTP_HOST")
        .map_err(|_| "Email backup is not configured".to_string())?;
    let username = env::var("CHIRAL_WALLET_EMAIL_SMTP_USERNAME").unwrap_or_default();
    let password = env::var("CHIRAL_WALLET_EMAIL_SMTP_PASSWORD").unwrap_or_default();
    let from = env::var("CHIRAL_WALLET_EMAIL_FROM")
        .map_err(|_| "Email backup is not configured".to_string())?;

    let port = env::var("CHIRAL_WALLET_EMAIL_SMTP_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_SMTP_PORT);
    let starttls = env::var("CHIRAL_WALLET_EMAIL_SMTP_STARTTLS")
        .ok()
        .map(|v| {
            let lowered = v.trim().to_ascii_lowercase();
            lowered != "0" && lowered != "false" && lowered != "no"
        })
        .unwrap_or(true);

    Ok(SmtpSettings {
        host,
        port,
        username,
        password,
        from,
        starttls,
    })
}

fn build_email_body(req: &WalletBackupEmailRequest) -> String {
    format!(
        "This is your encrypted one-time Chiral wallet backup email.\n\nWallet Address:\n{}\n\nEncrypted Backup:\nversion={}\nalgorithm={}\nkdf={}\niterations={}\nsalt={}\niv={}\nciphertext={}\n\nSecurity reminders:\n- Keep this email private and secure.\n- Keep your backup key separate from this email.\n- The relay did not receive your recovery phrase or private key.",
        req.wallet_address.trim(),
        req.encrypted_backup.version.trim(),
        req.encrypted_backup.algorithm.trim(),
        req.encrypted_backup.kdf.trim(),
        req.encrypted_backup.iterations,
        req.encrypted_backup.salt.trim(),
        req.encrypted_backup.iv.trim(),
        req.encrypted_backup.ciphertext.trim(),
    )
}

async fn send_wallet_backup_email(Json(req): Json<WalletBackupEmailRequest>) -> Response {
    let email = req.email.trim();
    if email.is_empty() {
        return (StatusCode::BAD_REQUEST, "Email is required").into_response();
    }
    if !is_valid_wallet_address(req.wallet_address.trim()) {
        return (StatusCode::BAD_REQUEST, "Invalid wallet address").into_response();
    }
    if !is_valid_encrypted_backup_payload(&req.encrypted_backup) {
        return (
            StatusCode::BAD_REQUEST,
            "Invalid encrypted wallet backup payload",
        )
            .into_response();
    }

    let to_mailbox: Mailbox = match email.parse() {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid email address").into_response(),
    };

    let smtp = match parse_smtp_settings() {
        Ok(v) => v,
        Err(e) => return (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    };

    let from_mailbox: Mailbox = match smtp.from.parse() {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Email sender is misconfigured",
            )
                .into_response()
        }
    };

    let message = match Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject("Your Chiral Wallet Backup (One-Time)")
        .header(ContentType::TEXT_PLAIN)
        .body(build_email_body(&req))
    {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build email message",
            )
                .into_response()
        }
    };

    let transport_builder = if smtp.starttls {
        match AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host) {
            Ok(builder) => builder,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "SMTP host does not support STARTTLS",
                )
                    .into_response()
            }
        }
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
    };

    let mut builder = transport_builder.port(smtp.port);
    if !smtp.username.is_empty() {
        builder = builder.credentials(Credentials::new(smtp.username, smtp.password));
    }
    let mailer = builder.build();

    if let Err(_) = mailer.send(message).await {
        return (
            StatusCode::BAD_GATEWAY,
            "Failed to send wallet backup email",
        )
            .into_response();
    }

    (StatusCode::OK, Json(WalletBackupEmailResponse { ok: true })).into_response()
}

/// Create routes for one-time wallet backup email delivery.
pub fn wallet_backup_routes() -> Router {
    Router::new().route("/api/wallet/backup-email", post(send_wallet_backup_email))
}

#[cfg(test)]
mod tests {
    use super::{
        build_email_body, is_valid_encrypted_backup_payload, is_valid_wallet_address,
        EncryptedWalletBackupPayload, WalletBackupEmailRequest,
    };

    #[test]
    fn validates_wallet_address_format() {
        assert!(is_valid_wallet_address(
            "0x1234567890abcdef1234567890abcdef12345678"
        ));
        assert!(!is_valid_wallet_address("0x123"));
        assert!(!is_valid_wallet_address("xyz"));
    }

    fn encrypted_payload() -> EncryptedWalletBackupPayload {
        EncryptedWalletBackupPayload {
            version: "chiral-wallet-backup-v1".to_string(),
            algorithm: "AES-256-GCM".to_string(),
            kdf: "PBKDF2-SHA256".to_string(),
            iterations: 210_000,
            salt: "MDEyMzQ1Njc4OWFiY2RlZg==".to_string(),
            iv: "MTIzNDU2Nzg5MDEy".to_string(),
            ciphertext: "ZW5jcnlwdGVkLXdhbGxldC1iYWNrdXA=".to_string(),
        }
    }

    #[test]
    fn validates_encrypted_backup_payload() {
        let valid = encrypted_payload();
        assert!(is_valid_encrypted_backup_payload(&valid));

        let too_weak = EncryptedWalletBackupPayload {
            iterations: 1,
            ..encrypted_payload()
        };
        assert!(!is_valid_encrypted_backup_payload(&too_weak));

        let bad_base64 = EncryptedWalletBackupPayload {
            ciphertext: "not base64".to_string(),
            ..encrypted_payload()
        };
        assert!(!is_valid_encrypted_backup_payload(&bad_base64));

        let wrong_iv_length = EncryptedWalletBackupPayload {
            iv: "c2hvcnQ=".to_string(),
            ..encrypted_payload()
        };
        assert!(!is_valid_encrypted_backup_payload(&wrong_iv_length));
    }

    #[test]
    fn email_body_never_contains_raw_wallet_secrets() {
        let raw_phrase = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
        let raw_private_key = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let req = WalletBackupEmailRequest {
            email: "user@example.com".to_string(),
            wallet_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            encrypted_backup: encrypted_payload(),
        };

        let body = build_email_body(&req);

        assert!(body.contains("ZW5jcnlwdGVkLXdhbGxldC1iYWNrdXA="));
        assert!(!body.contains(raw_phrase));
        assert!(!body.contains(raw_private_key));
    }

    #[test]
    fn plaintext_wallet_secret_fields_are_rejected() {
        let raw = serde_json::json!({
            "email": "user@example.com",
            "walletAddress": "0x1234567890abcdef1234567890abcdef12345678",
            "recoveryPhrase": "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu",
            "privateKey": "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "encryptedBackup": {
                "version": "chiral-wallet-backup-v1",
                "algorithm": "AES-256-GCM",
                "kdf": "PBKDF2-SHA256",
                "iterations": 210000,
                "salt": "MDEyMzQ1Njc4OWFiY2RlZg==",
                "iv": "MTIzNDU2Nzg5MDEy",
                "ciphertext": "ZW5jcnlwdGVkLXdhbGxldC1iYWNrdXA="
            }
        });

        let err = serde_json::from_value::<WalletBackupEmailRequest>(raw)
            .expect_err("plaintext-era wallet secrets should be unknown fields");

        assert!(err.to_string().contains("unknown field"));
    }
}

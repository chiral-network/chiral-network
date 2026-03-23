use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::{Deserialize, Serialize};
use std::env;

const DEFAULT_SMTP_PORT: u16 = 587;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WalletBackupEmailRequest {
    email: String,
    recovery_phrase: String,
    wallet_address: String,
    private_key: String,
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

fn is_valid_private_key(value: &str) -> bool {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_valid_recovery_phrase(value: &str) -> bool {
    let words: Vec<&str> = value.split_whitespace().collect();
    words.len() == 12
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
        "This is your one-time Chiral wallet backup email.\n\nRecovery Phrase (12 words):\n{}\n\nWallet Address:\n{}\n\nPrivate Key:\n{}\n\nSecurity reminders:\n- Keep this email private and secure.\n- Delete this email after saving these credentials in a secure password manager.\n- Anyone with this information can fully control your wallet.",
        req.recovery_phrase.trim(),
        req.wallet_address.trim(),
        req.private_key.trim(),
    )
}

async fn send_wallet_backup_email(Json(req): Json<WalletBackupEmailRequest>) -> Response {
    let email = req.email.trim();
    if email.is_empty() {
        return (StatusCode::BAD_REQUEST, "Email is required").into_response();
    }
    if !is_valid_recovery_phrase(&req.recovery_phrase) {
        return (
            StatusCode::BAD_REQUEST,
            "Recovery phrase must contain exactly 12 words",
        )
            .into_response();
    }
    if !is_valid_wallet_address(req.wallet_address.trim()) {
        return (StatusCode::BAD_REQUEST, "Invalid wallet address").into_response();
    }
    if !is_valid_private_key(req.private_key.trim()) {
        return (StatusCode::BAD_REQUEST, "Invalid private key").into_response();
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
    use super::{is_valid_private_key, is_valid_recovery_phrase, is_valid_wallet_address};

    #[test]
    fn validates_wallet_address_format() {
        assert!(is_valid_wallet_address(
            "0x1234567890abcdef1234567890abcdef12345678"
        ));
        assert!(!is_valid_wallet_address("0x123"));
        assert!(!is_valid_wallet_address("xyz"));
    }

    #[test]
    fn validates_private_key_format() {
        assert!(is_valid_private_key(
            "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(is_valid_private_key(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_valid_private_key("not-a-key"));
    }

    #[test]
    fn validates_recovery_phrase_word_count() {
        let phrase = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
        assert!(is_valid_recovery_phrase(phrase));
        assert!(!is_valid_recovery_phrase("too short"));
    }
}

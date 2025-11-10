use chrono::{serde::ts_seconds, DateTime, Duration, Utc};
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};

pub const HANDSHAKE_PROTOCOL_ID: &str = "/chiral/handshake/1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeRequest {
    pub file_id: String,
    pub download_id: String,
    pub epoch: u64,
    pub requester_peer_id: String,
}

impl HandshakeRequest {
    pub fn new(
        file_id: impl Into<String>,
        download_id: impl Into<String>,
        epoch: u64,
        requester_peer_id: impl Into<String>,
    ) -> Self {
        Self {
            file_id: file_id.into(),
            download_id: download_id.into(),
            epoch,
            requester_peer_id: requester_peer_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeAck {
    pub file_id: String,
    pub download_id: String,
    pub epoch: u64,
    pub etag: String,
    pub size: u64,
    #[serde(with = "ts_seconds")]
    pub lease_exp: DateTime<Utc>,
    #[serde(with = "ts_seconds")]
    pub lease_issued_at: DateTime<Utc>,
    pub resume_token: String,
}

impl HandshakeAck {
    pub fn lease_window(&self) -> LeaseWindow {
        LeaseWindow {
            issued_at: self.lease_issued_at,
            expires_at: self.lease_exp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseWindow {
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl LeaseWindow {
    pub fn duration(&self) -> Duration {
        self.expires_at - self.issued_at
    }

    pub fn remaining_from(&self, now: DateTime<Utc>) -> Duration {
        self.expires_at - now
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }
}

#[derive(Debug, Clone)]
pub struct LeaseRenewalPolicy {
    pub jitter_max_ratio: f64,
    pub min_lead_time: Duration,
    pub lead_time_fraction: f64,
}

impl Default for LeaseRenewalPolicy {
    fn default() -> Self {
        Self {
            jitter_max_ratio: 0.15,
            min_lead_time: Duration::seconds(60),
            lead_time_fraction: 0.10,
        }
    }
}

impl LeaseRenewalPolicy {
    pub fn compute_trigger_instant(&self, window: &LeaseWindow) -> DateTime<Utc> {
        let lease_len = window.duration();
        let mut lead = scale_duration(lease_len, self.lead_time_fraction);
        if lead < self.min_lead_time {
            lead = self.min_lead_time;
        }
        let jitter = self.random_jitter(lead);
        window.expires_at - (lead + jitter)
    }

    pub fn is_renewal_due(&self, window: &LeaseWindow, now: DateTime<Utc>) -> bool {
        now >= self.compute_trigger_instant(window)
    }

    fn random_jitter(&self, base: Duration) -> Duration {
        if self.jitter_max_ratio <= 0.0 {
            return Duration::zero();
        }
        let mut rng = OsRng;
        let ratio = rng.gen_range(0.0..self.jitter_max_ratio.min(1.0));
        scale_duration(base, ratio)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeErrorKind {
    Timeout,
    InvalidResponse,
    Transport(u16),
    Unauthorized,
    Retryable,
}

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("handshake failed: {message}")]
    Failed {
        kind: HandshakeErrorKind,
        message: String,
    },
}

impl HandshakeError {
    pub fn new(kind: HandshakeErrorKind, message: impl Into<String>) -> Self {
        Self::Failed {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> HandshakeErrorKind {
        match self {
            HandshakeError::Failed { kind, .. } => *kind,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HandshakeBackoff {
    attempt: u32,
    base: Duration,
    max: Duration,
    jitter_ratio: f64,
}

impl HandshakeBackoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            attempt: 0,
            base,
            max,
            jitter_ratio: 0.2,
        }
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn next_delay(&mut self, kind: HandshakeErrorKind) -> Duration {
        let should_retry = matches!(
            kind,
            HandshakeErrorKind::Timeout
                | HandshakeErrorKind::Retryable
                | HandshakeErrorKind::Transport(500..=599)
        );

        if !should_retry {
            self.attempt = 0;
            return Duration::zero();
        }

        let capped_attempt = self.attempt.min(16);
        let factor = 1i32
            .checked_shl(capped_attempt)
            .unwrap_or(i32::MAX);
        let mut delay = self.base * factor;
        if delay > self.max {
            delay = self.max;
        }
        self.attempt = self.attempt.saturating_add(1);
        delay + self.random_jitter(delay)
    }

    fn random_jitter(&self, base: Duration) -> Duration {
        if self.jitter_ratio <= 0.0 {
            return Duration::zero();
        }
        let mut rng = OsRng;
        let ratio = rng.gen_range(0.0..self.jitter_ratio.min(1.0));
        scale_duration(base, ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes() {
        let req = HandshakeRequest::new("file", "dl", 42, "peer");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"fileId\""));
    }

    #[test]
    fn ack_lease_window_duration() {
        let issued = DateTime::from_timestamp(1_000_000, 0).unwrap();
        let expires = issued + Duration::hours(4);
        let ack = HandshakeAck {
            file_id: "f".into(),
            download_id: "d".into(),
            epoch: 1,
            etag: "\"abc\"".into(),
            size: 1024,
            lease_exp: expires,
            lease_issued_at: issued,
            resume_token: "t".into(),
        };
        let window = ack.lease_window();
        assert_eq!(window.duration(), Duration::hours(4));
        assert!(!window.is_expired(issued));
    }

    #[test]
    fn renewal_policy_enforces_minimum() {
        let policy = LeaseRenewalPolicy {
            jitter_max_ratio: 0.0,
            min_lead_time: Duration::seconds(60),
            lead_time_fraction: 0.10,
        };
        let issued = Utc::now();
        let window = LeaseWindow {
            issued_at: issued,
            expires_at: issued + Duration::minutes(5),
        };
        let trigger = policy.compute_trigger_instant(&window);
        assert!(window.expires_at - trigger >= Duration::seconds(60));
    }

    #[test]
    fn backoff_increases() {
        let mut backoff = HandshakeBackoff::new(Duration::milliseconds(500), Duration::seconds(5));
        let first = backoff.next_delay(HandshakeErrorKind::Timeout);
        let second = backoff.next_delay(HandshakeErrorKind::Timeout);
        assert!(second >= first);
        backoff.reset();
        let reset_val = backoff.next_delay(HandshakeErrorKind::Timeout);
        assert!(reset_val <= Duration::seconds(5));
    }
}

fn scale_duration(duration: Duration, factor: f64) -> Duration {
    if factor <= 0.0 || duration <= Duration::zero() {
        return Duration::zero();
    }
    let millis = duration.num_milliseconds();
    if millis <= 0 {
        return Duration::zero();
    }
    let scaled = ((millis as f64) * factor).round();
    if !scaled.is_finite() {
        return duration;
    }
    let capped = scaled.clamp(0.0, i64::MAX as f64) as i64;
    Duration::milliseconds(capped)
}

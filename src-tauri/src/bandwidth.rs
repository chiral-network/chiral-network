use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::sleep;

/// Simple token-bucket based bandwidth controller shared between upload and download paths.
pub struct BandwidthController {
    inner: Mutex<Inner>,
}

struct Inner {
    upload: TokenBucket,
    download: TokenBucket,
}

impl BandwidthController {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                upload: TokenBucket::unlimited(),
                download: TokenBucket::unlimited(),
            }),
        }
    }

    pub async fn set_limits(&self, upload_kbps: u64, download_kbps: u64) {
        let mut inner = self.inner.lock().await;
        inner.upload.set_limit(upload_kbps);
        inner.download.set_limit(download_kbps);
    }

    pub async fn acquire_upload(&self, bytes: usize) {
        self.acquire(bytes, Direction::Upload).await;
    }

    pub async fn acquire_download(&self, bytes: usize) {
        self.acquire(bytes, Direction::Download).await;
    }

    async fn acquire(&self, bytes: usize, direction: Direction) {
        if bytes == 0 {
            return;
        }

        loop {
            let wait = {
                let mut inner = self.inner.lock().await;
                let bucket = match direction {
                    Direction::Upload => &mut inner.upload,
                    Direction::Download => &mut inner.download,
                };
                bucket.consume(bytes)
            };

            match wait {
                None => break,
                Some(delay) if delay.is_zero() => break,
                Some(delay) => sleep(delay).await,
            }
        }
    }
}

enum Direction {
    Upload,
    Download,
}

struct TokenBucket {
    limit_bytes_per_sec: Option<f64>,
    tokens: f64,
    capacity: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn unlimited() -> Self {
        Self {
            limit_bytes_per_sec: None,
            tokens: f64::INFINITY,
            capacity: f64::INFINITY,
            last_refill: Instant::now(),
        }
    }

    fn set_limit(&mut self, limit_kbps: u64) {
        if limit_kbps == 0 {
            self.limit_bytes_per_sec = None;
            self.tokens = f64::INFINITY;
            self.capacity = f64::INFINITY;
            self.last_refill = Instant::now();
            return;
        }

        let limit = (limit_kbps as f64) * 1024.0; // Convert KB/s to bytes/s.
        self.limit_bytes_per_sec = Some(limit);
        self.capacity = limit * 2.0; // Allow up to ~2 seconds of burst.
        self.tokens = self.tokens.min(self.capacity);
        self.last_refill = Instant::now();
    }

    fn consume(&mut self, bytes: usize) -> Option<Duration> {
        let limit = match self.limit_bytes_per_sec {
            None => return None,
            Some(limit) if limit <= f64::EPSILON => return None,
            Some(limit) => limit,
        };

        self.refill(limit);

        let required = bytes as f64;
        if self.tokens >= required {
            self.tokens -= required;
            None
        } else {
            let deficit = required - self.tokens;
            self.tokens = 0.0;
            let wait_secs = deficit / limit;
            if wait_secs <= 0.0 {
                None
            } else {
                Some(Duration::from_secs_f64(wait_secs))
            }
        }
    }

    fn refill(&mut self, limit: f64) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed <= 0.0 {
            return;
        }

        let new_tokens = elapsed * limit;
        self.tokens = (self.tokens + new_tokens).min(self.capacity);
        self.last_refill = now;
    }
}

// src-tauri/src/http_range_client.rs
// HTTP Range Client implementing DownloadSource trait for download-restart baseline
//
// Implements §5.2 Minimal source trait and §5.3 Expected HTTP server behaviour
// from docs/download-restart.md
//
// References:
// - §5.2: Minimal source trait definition
// - §5.3: Expected HTTP server behaviour (HEAD, Range requests, ETag handling)
// - §7: Client algorithm (PreparingHead, Streaming, Resume path)

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use std::pin::Pin;
use std::str::FromStr;
use thiserror::Error;
use tokio_stream::StreamExt;
use tracing::{debug, warn};
use url::Url;

/// Minimal source trait for byte-range-capable transports
/// 
/// Defined in docs/download-restart.md §5.2
#[async_trait]
pub trait DownloadSource: Send + Sync {
    /// Fetch file metadata via HEAD request (or probe GET if HEAD fails)
    /// 
    /// Per §5.3: Reply to HEAD with 200 OK, Content-Length, Accept-Ranges: bytes,
    /// strong ETag, and optional Last-Modified. If Content-Length is unavailable
    /// or HEAD returns 405/501, probe with GET Range: bytes=0-0.
    async fn head(&self, url: &Url) -> Result<SourceMetadata, SourceError>;

    /// Fetch file data starting from byte offset
    /// Returns a stream of Bytes chunks
    /// 
    /// Per §5.3: Ranged GET requests should return 206 Partial Content with
    /// Content-Range header. 200 OK without Content-Range indicates range
    /// unsupported. 416 indicates range exceeds file size.
    async fn fetch_range(
        &self,
        url: &Url,
        start: u64,
    ) -> Result<Pin<Box<dyn tokio_stream::Stream<Item = Result<Bytes, SourceError>> + Send>>, SourceError>;
}

/// File metadata from source
/// 
/// Defined in docs/download-restart.md §5.2
#[derive(Debug, Clone)]
pub struct SourceMetadata {
    pub expected_size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<DateTime<Utc>>,
}

/// Errors that can occur during source operations
/// 
/// Defined in docs/download-restart.md §5.2
#[derive(Debug, Error)]
pub enum SourceError {
    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("unreachable")]
    Unreachable,

    #[error("range unsupported")]
    RangeUnsupported,

    #[error("unexpected status: {0}")]
    UnexpectedStatus(String),
}

/// HTTP implementation of DownloadSource trait
/// 
/// Implements §5.3 Expected HTTP server behaviour:
/// - HEAD requests with fallback to Range: bytes=0-0 probe
/// - Range requests with If-Range header support
/// - Weak ETag detection (W/ prefix)
/// - 206/200/416 response handling
pub struct HttpRangeClient {
    client: Client,
}

impl HttpRangeClient {
    /// Create a new HTTP range client with default timeout (30s)
    /// 
    /// Per §5.3: Redirects are disabled - we treat 3xx as errors.
    pub fn new() -> Result<Self, SourceError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none()) // Per §5.3: treat redirects as errors
            .build()
            .map_err(|e| SourceError::Protocol(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { client })
    }

    /// Create with custom timeout
    /// 
    /// Per §5.3: Redirects are disabled - we treat 3xx as errors.
    pub fn with_timeout(timeout_secs: u64) -> Result<Self, SourceError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .redirect(reqwest::redirect::Policy::none()) // Per §5.3: treat redirects as errors
            .build()
            .map_err(|e| SourceError::Protocol(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { client })
    }

    /// Extract ETag from response headers, checking for weak ETags
    /// 
    /// Per §5.3: Weak ETags (W/ prefix) signal that safe resume is impossible;
    /// the client should restart from zero.
    fn extract_etag(&self, response: &reqwest::Response) -> Option<String> {
        if let Some(etag) = response.headers().get("ETag") {
            if let Ok(etag_str) = etag.to_str() {
                // Per §5.3: Weak ETags signal unsafe resume
                if etag_str.starts_with("W/") {
                    warn!("Weak ETag detected: {}. Per §5.3, resume is unsafe - will restart from zero.", etag_str);
                    // Return None to signal weak ETag (caller should restart from zero)
                    return None;
                }
                return Some(etag_str.to_string());
            }
        }
        None
    }

    /// Parse Content-Range header to extract total size
    /// Format: "bytes start-end/total" or "bytes */total"
    /// 
    /// Used when probing with Range: bytes=0-0 per §5.3
    /// Per RFC 7233: Content-Range = "bytes" SP (byte-range-resp / unsatisfied-range)
    ///               byte-range-resp = byte-range "/" (complete-length / "*")
    fn parse_content_range(&self, content_range: &str) -> Option<u64> {
        // Must start with "bytes "
        if !content_range.starts_with("bytes ") {
            return None;
        }
        
        // Find the slash separating range from total
        if let Some(slash_pos) = content_range.rfind('/') {
            let total_str = content_range[slash_pos + 1..].trim();
            // Per §5.3: "*" means unknown length, which we don't support
            if total_str == "*" {
                return None;
            }
            if let Ok(total) = u64::from_str(total_str) {
                return Some(total);
            }
        }
        None
    }

    /// Probe file size using GET Range: bytes=0-0
    /// 
    /// Per §5.3: If HEAD fails or Content-Length is missing, probe with
    /// GET Range: bytes=0-0 and expect 206 Partial Content with
    /// Content-Range: bytes 0-0/total
    async fn probe_size(&self, url: &Url) -> Result<SourceMetadata, SourceError> {
        debug!("Probing file size with Range: bytes=0-0 (per §5.3 fallback)");

        let response = self
            .client
            .get(url.as_str())
            .header("Range", "bytes=0-0")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() || e.is_connect() {
                    SourceError::Unreachable
                } else {
                    SourceError::Protocol(format!("Probe request failed: {}", e))
                }
            })?;

        match response.status() {
            StatusCode::PARTIAL_CONTENT => {
                // Per §5.3: 206 responses MUST include Content-Range header
                let content_range_str = response
                    .headers()
                    .get("Content-Range")
                    .and_then(|h| h.to_str().ok())
                    .ok_or_else(|| {
                        SourceError::Protocol("206 response missing Content-Range header (per §5.3)".to_string())
                    })?;

                // Per §5.3: Parse Content-Range to get total size
                let total_size = self.parse_content_range(content_range_str)
                    .ok_or_else(|| {
                        SourceError::Protocol(format!(
                            "Failed to parse Content-Range: {} (per §5.3)",
                            content_range_str
                        ))
                    })?;

                let etag = self.extract_etag(&response);
                let last_modified = response
                    .headers()
                    .get("Last-Modified")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                Ok(SourceMetadata {
                    expected_size: total_size,
                    etag,
                    last_modified,
                })
            }
            StatusCode::OK => {
                // Per §5.3: Server doesn't support ranges
                Err(SourceError::RangeUnsupported)
            }
            status if status.is_redirection() => {
                // Per §5.3: Treat redirects as errors (mirrors not supported)
                Err(SourceError::Protocol(
                    format!("Probe request returned redirect ({}): mirrors not supported per §5.3", status)
                ))
            }
            status => Err(SourceError::UnexpectedStatus(format!(
                "Probe request returned {} (expected 206 per §5.3)",
                status
            ))),
        }
    }

    /// Fetch range with If-Range header for resume safety
    /// 
    /// Per §5.3 and §7: Resumes use Range: bytes={bytes_downloaded}- with
    /// If-Range containing the strong ETag. This method wraps fetch_range
    /// to add If-Range support.
    /// 
    /// This is an HTTP-specific extension beyond the minimal trait.
    pub async fn fetch_range_with_etag(
        &self,
        url: &Url,
        start: u64,
        etag: &str,
    ) -> Result<Pin<Box<dyn tokio_stream::Stream<Item = Result<Bytes, SourceError>> + Send>>, SourceError>
    {
        debug!("Fetching range from byte {} with If-Range: {} (per §5.3, §7)", start, etag);

        let response = self
            .client
            .get(url.as_str())
            .header("Range", format!("bytes={}-", start))
            .header("If-Range", etag) // Per §5.3, §7: Use If-Range for resume safety
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() || e.is_connect() {
                    SourceError::Unreachable
                } else {
                    SourceError::Protocol(format!("Range request failed: {}", e))
                }
            })?;

        match response.status() {
            StatusCode::PARTIAL_CONTENT => {
                // Per §5.3: 206 responses MUST include Content-Range header
                let content_range = response
                    .headers()
                    .get("Content-Range")
                    .and_then(|h| h.to_str().ok())
                    .ok_or_else(|| {
                        SourceError::Protocol("206 response missing Content-Range header (per §5.3)".to_string())
                    })?;

                // Per §5.3: Verify Content-Range matches requested start
                if !content_range.starts_with(&format!("bytes {}-", start)) {
                    return Err(SourceError::Protocol(format!(
                        "Content-Range mismatch: expected bytes {}-, got {} (per §5.3)",
                        start, content_range
                    )));
                }

                // Per §5.3: Verify ETag matches (for integrity)
                if let Some(response_etag) = self.extract_etag(&response) {
                    if response_etag != etag {
                        return Err(SourceError::Protocol(format!(
                            "ETag mismatch: expected {}, got {} (per §5.3: file may have changed)",
                            etag, response_etag
                        )));
                    }
                }

                // Convert response body to stream
                let stream = response
                    .bytes_stream()
                    .map(|result| {
                        result
                            .map_err(|e| SourceError::Protocol(format!("Stream error: {}", e)))
                            .map(Bytes::from)
                    });

                Ok(Box::pin(stream))
            }
            StatusCode::OK => {
                // Per §5.3: 200 OK without Content-Range indicates range unsupported
                warn!("Server returned 200 OK to range request - range not supported (per §5.3)");
                Err(SourceError::RangeUnsupported)
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                // Per §5.3 and §7: 416 indicates range exceeds file size
                // Per §7: Re-probe size and restart from zero
                Err(SourceError::Protocol(format!(
                    "416 Range Not Satisfiable: requested start {} is beyond file size (per §5.3, §7: will re-probe and restart)",
                    start
                )))
            }
            status if status.is_redirection() => {
                // Per §5.3: Treat redirects as errors (mirrors not supported)
                Err(SourceError::Protocol(
                    format!("Range request with If-Range returned redirect ({}): mirrors not supported per §5.3", status)
                ))
            }
            status => Err(SourceError::UnexpectedStatus(format!(
                "Range request returned {} (expected 206 per §5.3)",
                status
            ))),
        }
    }
}

#[async_trait]
impl DownloadSource for HttpRangeClient {
    /// Fetch file metadata via HEAD request with fallback to probe
    /// 
    /// Per §5.3: Reply to HEAD with 200 OK, Content-Length, Accept-Ranges: bytes,
    /// strong ETag, and optional Last-Modified. If Content-Length is unavailable
    /// or HEAD returns 405/501, probe with GET Range: bytes=0-0.
    /// 
    /// Per §7 PreparingHead: Issue HEAD, record strong ETag, Last-Modified, and
    /// Content-Length. If length is missing, probe with GET Range: bytes=0-0.
    async fn head(&self, url: &Url) -> Result<SourceMetadata, SourceError> {
        debug!("Sending HEAD request to {} (per §5.3, §7 PreparingHead)", url);

        // Per §5.3: Try HEAD first
        let response = match self.client.head(url.as_str()).send().await {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() || e.is_connect() {
                    return Err(SourceError::Unreachable);
                }
                // Per §5.3: If HEAD fails, try probe
                warn!("HEAD request failed: {}. Trying probe GET per §5.3.", e);
                return self.probe_size(url).await;
            }
        };

        match response.status() {
            StatusCode::OK => {
                // Per §5.3: Extract Content-Length
                let expected_size = match response
                    .headers()
                    .get("Content-Length")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| u64::from_str(s).ok())
                {
                    Some(size) => size,
                    None => {
                        // Per §5.3 and §7: If Content-Length missing, probe
                        debug!("HEAD response missing Content-Length, probing per §5.3, §7");
                        return self.probe_size(url).await;
                    }
                };

                let etag = self.extract_etag(&response);
                let last_modified = response
                    .headers()
                    .get("Last-Modified")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                // Per §5.3: Check Accept-Ranges header (optional but good to verify)
                if let Some(accept_ranges) = response.headers().get("Accept-Ranges") {
                    if accept_ranges.to_str().unwrap_or("") != "bytes" {
                        warn!("Server Accept-Ranges is not 'bytes': {:?} (per §5.3)", accept_ranges);
                    }
                }

                // Per §7: When only Last-Modified is available, proceed but log risk
                if etag.is_none() && last_modified.is_some() {
                    warn!("Only Last-Modified available (no ETag) - resume risk higher per §7");
                }

                Ok(SourceMetadata {
                    expected_size,
                    etag,
                    last_modified,
                })
            }
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED => {
                // Per §5.3: HEAD not supported (405/501), try probe
                debug!("HEAD not supported ({}), probing per §5.3", response.status());
                self.probe_size(url).await
            }
            status if status.is_redirection() => {
                // Per §5.3: Treat redirects as errors (mirrors not supported)
                warn!("HEAD returned redirect ({}), treating as error per §5.3", status);
                Err(SourceError::Protocol(
                    format!("Server returned redirect ({}): mirrors not supported per §5.3", status)
                ))
            }
            status => {
                // Other error, try probe as fallback per §5.3
                warn!("HEAD returned {}, trying probe per §5.3", status);
                self.probe_size(url).await
            }
        }
    }

    /// Fetch file data starting from byte offset
    /// 
    /// Per §5.3: Ranged GET requests should return 206 Partial Content with
    /// Content-Range header. 200 OK without Content-Range indicates range
    /// unsupported. 416 indicates range exceeds file size.
    /// 
    /// Note: For resume with ETag validation, use fetch_range_with_etag() instead.
    /// Per §7 Streaming: Resumes use If-Range header.
    async fn fetch_range(
        &self,
        url: &Url,
        start: u64,
    ) -> Result<Pin<Box<dyn tokio_stream::Stream<Item = Result<Bytes, SourceError>> + Send>>, SourceError>
    {
        debug!("Fetching range from byte {} (per §5.3)", start);

        let response = self
            .client
            .get(url.as_str())
            .header("Range", format!("bytes={}-", start))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() || e.is_connect() {
                    SourceError::Unreachable
                } else {
                    SourceError::Protocol(format!("Range request failed: {}", e))
                }
            })?;

        match response.status() {
            StatusCode::PARTIAL_CONTENT => {
                // Per §5.3: 206 responses MUST include Content-Range header
                let content_range = response
                    .headers()
                    .get("Content-Range")
                    .and_then(|h| h.to_str().ok())
                    .ok_or_else(|| {
                        SourceError::Protocol("206 response missing Content-Range header (per §5.3)".to_string())
                    })?;

                // Per §5.3: Verify Content-Range matches requested start
                if !content_range.starts_with(&format!("bytes {}-", start)) {
                    return Err(SourceError::Protocol(format!(
                        "Content-Range mismatch: expected bytes {}-, got {} (per §5.3)",
                        start, content_range
                    )));
                }

                // Convert response body to stream
                let stream = response
                    .bytes_stream()
                    .map(|result| {
                        result
                            .map_err(|e| SourceError::Protocol(format!("Stream error: {}", e)))
                            .map(Bytes::from)
                    });

                Ok(Box::pin(stream))
            }
            StatusCode::OK => {
                // Per §5.3: 200 OK without Content-Range indicates range unsupported
                // Per §7: This triggers Restarting state
                warn!("Server returned 200 OK to range request - range not supported (per §5.3, §7 Restarting)");
                Err(SourceError::RangeUnsupported)
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                // Per §5.3 and §7: 416 indicates range exceeds file size
                // Per §7 Restarting: Re-probe size and restart from zero
                Err(SourceError::Protocol(format!(
                    "416 Range Not Satisfiable: requested start {} is beyond file size (per §5.3, §7: will re-probe and restart)",
                    start
                )))
            }
            status if status.is_redirection() => {
                // Per §5.3: Treat redirects as errors (mirrors not supported)
                Err(SourceError::Protocol(
                    format!("Range request returned redirect ({}): mirrors not supported per §5.3", status)
                ))
            }
            status => Err(SourceError::UnexpectedStatus(format!(
                "Range request returned {} (expected 206 per §5.3)",
                status
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests would go here - see §10 Testing plan in download-restart.md
    // Integration tests should verify:
    // - HEAD request with fallback to probe
    // - Weak ETag detection
    // - 206/200/416 response handling
    // - If-Range header behavior
}
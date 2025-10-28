# 1. Title, Owner, Reviewers, Status

- **Title:** Whole-File Pause & Resume Baseline
- **Owner:** Team Hawks
- **Reviewers:** Prof. Mu, Steven, Team Whales, Team Pandas
- **Status:** Draft for approval

## 2. Why

We do not yet have a reliable, restart-safe download path: a user can start an HTTP transfer today but loses progress if the process stops. This proposal delivers a minimal whole-file workflow where a downloader can pause, close the app, reopen it, and resume from the saved offset using persisted metadata, creating a boring, verifiable baseline that the class can demo and extend.

## 3. Non-goals

- Multi-source scheduling, BitTorrent/Bitswap logic, or any coordinated swarming features.
- DHT-based search, peer discovery, or URL distribution.
- Payments, receipts, reputation scoring, or blockchain hooks.
- Database-backed persistence or indexing beyond a JSON sidecar file.
- Partial-file streaming, media preview, or chunk-level prioritisation.

## 4. User stories

- A downloader selects an HTTP file, starts the transfer, sees progress, pauses, and resumes later without re-downloading completed bytes.
- A user can quit Chiral, relaunch it, and have the download resume automatically from the stored offset with integrity checks.
- A demo runner can show Node A uploading and Node B downloading over plain HTTP, interrupt the process, restart Node B, and complete the file with a final hash match.

## 5. Interfaces

### 5.1 Tauri commands

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartDownloadRequest {
    pub download_id: Option<String>,        // Optional client-provided UUID; generates one if None.
    pub source_url: String,                 // HTTP or FTP URL.
    pub destination_path: String,           // Absolute path under the user's downloads directory.
    pub expected_sha256: Option<String>,    // Optional final hash for verification.
}

pub type DownloadId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DownloadState {
    Idle,
    PreparingHead,
    HeadBackoff,
    Restarting,
    PreflightStorage,
    ValidatingMetadata,
    Downloading,
    PersistingProgress,
    Paused,
    AwaitingResume,
    VerifyingSha,
    FinalizingIo,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStatus {
    pub download_id: DownloadId,
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub expected_size: Option<u64>,
    pub etag: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("download not found")]
    NotFound,
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("source error: {0}")]
    Source(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("already completed")]
    AlreadyCompleted,
}

#[tauri::command]
pub async fn start_download(request: StartDownloadRequest) -> Result<DownloadId, DownloadError>;

#[tauri::command]
pub async fn pause_download(download_id: DownloadId) -> Result<(), DownloadError>;

#[tauri::command]
pub async fn resume_download(download_id: DownloadId) -> Result<(), DownloadError>;

#[tauri::command]
pub async fn get_download_status(download_id: DownloadId) -> Result<DownloadStatus, DownloadError>;
```

### 5.2 Minimal source trait

```rust
#[async_trait]
pub trait DownloadSource: Send + Sync {
    async fn head(&self, url: &Url) -> Result<SourceMetadata, SourceError>;
    async fn fetch_range(
        &self,
        url: &Url,
        start: u64,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, SourceError>> + Send>>, SourceError>;
}

#[derive(Debug, Clone)]
pub struct SourceMetadata {
    pub expected_size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
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
```

HTTP and FTP implementations conform to this trait so the resume core can swap protocols without changing state management.

### 5.3 Expected HTTP server behaviour

- Respond to `HEAD /file` with `200 OK`, `Content-Length`, `Accept-Ranges: bytes`, `ETag`, and optional `Last-Modified`.
- Respond to `GET /file` with `Range: bytes=start-` and `If-Range: <etag>` by returning `206 Partial Content` with `Content-Range: bytes start-end/total`.
- When `Range` is absent or unsupported, return `200 OK` with the full payload; the client will detect the missing `Accept-Ranges` header and restart from zero.
- Preserve a stable `ETag` for the lifetime of the file so clients can detect file changes between pauses.

## 6. On-disk format

- `<destination_path>.part`: binary data stream written sequentially from byte `0`.
- `<destination_path>.meta.json`: UTF-8 JSON record with the following schema:
  ```json
  {
    "download_id": "uuid-string",
    "url": "https://node-a/files/report.pdf",
    "etag": "\"abc123\"",
    "expected_size": 123456789,
    "bytes_downloaded": 52428800,
    "last_modified": "2025-10-25T04:12:33Z",
    "sha256_final": null
  }
  ```
  Fields:
  - `download_id`: stable identifier stored with the metadata.
  - `url`, `etag`, `expected_size`, `last_modified`: copied from `HEAD`.
  - `bytes_downloaded`: last durable write offset flushed to disk.
  - `sha256_final`: populated only after final hash verification succeeds.

## 7. Client algorithm

- **Start / `PreparingHead`:** issue `HEAD`, reject if `Content-Length` missing, initialise `.part` and `.meta.json` with zero offset, and cache response headers for later validation.
- **Retry / `HeadBackoff`:** on transient network or 5xx responses, retry `HEAD` with exponential backoff (base 1 s, cap 30 s) and surface the attempt count in telemetry.
- **Reset / `Restarting`:** when headers disagree with existing metadata (`ETag`, `Content-Length`, `Last-Modified`), delete stale artifacts, zero the offset, and return to `PreparingHead`.
- **Storage preflight / `PreflightStorage`:** before any body bytes are read, ensure the destination directory exists, free disk space >= `expected_size - bytes_downloaded`, and open the `.part` file with `O_APPEND|O_CREAT`, truncating only when restarting from zero.
- **Metadata validation / `ValidatingMetadata`:** load `.meta.json`, confirm schema version, and reconcile any drift between persisted offset and actual `.part` length; discrepancies trigger a downgrade to `Restarting`.
- **Streaming / `Downloading`:** request the body (`GET` without `Range` on first start, `Range` with `If-Range` on resumes), read chunks into an 8 MiB buffer, and hand them to the writer.
- **Persistence loop / `PersistingProgress`:** after each buffer write, flush and optionally `fsync` according to platform policy, then update `bytes_downloaded` and timestamps in `.meta.json`; failures roll back to the last safe offset and record `last_error`.
- **Pause & await resume / `Paused` + `AwaitingResume`:** cancelling the stream or detecting an app shutdown flushes all buffers, closes descriptors, and moves to `Paused`; on relaunch, metadata hydration transitions to `AwaitingResume` until the UI or auto-resume policy calls `resume_download`.
- **Resume path:** re-run `HEAD`, compare headers, and if they still match, re-enter `Downloading` with `Range: bytes={bytes_downloaded}-` and `If-Range` set to the stored `ETag`; otherwise fall back to `Restarting`.
- **Finish / `VerifyingSha` + `FinalizingIo`:** when `bytes_downloaded == expected_size`, compute the SHA-256 (if provided) over the `.part` file, cache the digest in `.meta.json`, perform an `fsync`, atomically rename `<dest>.part` to `<dest>`, clean up metadata, and notify the frontend.
- **Failure handling / `Failed`:** non-recoverable errors (persistent HTTP rejection, disk full, hash mismatch, permission failure) mark the download as `Failed` with a structured `last_error`; users can press resume to retry from the last valid offset or start fresh.

## 8. State machine

```
Stage 1 – setup, header, and storage validation
┌────────┐ start_download ┌───────────────┐ head_ok ┌──────────────────┐ storage_ok ┌─────────────────────┐ validation_ok ┌──────────────────────┐
│ Idle   │──────────────►│ PreparingHead │───────►│ PreflightStorage │──────────►│ ValidatingMetadata │──────────────►│ Downloading (entry) │
└──┬─────┘               └──────┬────────┘        └──────┬───────────┘          └──────┬──────────────┘                └──────────┬──────────┘
   │                             │ head_retry/backoff      │ storage_retry/backoff        │ validation_fail                         │ pause/app_close/app_crash
   │                             v                         v                              v                                      v
   │                       ┌──────────────┐          ┌──────────────┐                ┌──────────────┐                         ┌──────────────────┐
   │                       │ HeadBackoff  │◄──retry──│ Restarting   │◄─reset_meta────│     Failed   │                         │     Paused       │
   │                       └──────┬───────┘          └──────┬───────┘                └──────────────┘                         └──────────┬──────┘
   │                             │ unrecoverable_head      │ recreate_meta                                                   resume    │
   │                             v                        v                                                                  │          │ auto-resume timer
   │                       ┌──────────────┐         ┌──────────────────────┐                                               ┌───────────▼─────────┐
   │                       │     Failed   │         │ ValidatingMetadata   │                                               │ AwaitingResume       │
   │                       └──────────────┘         └──────────┬──────────┘                                               └───────────┬─────────┘
   │                                                           │ validation_fail                                                      │ resume_timeout/user_cancel
   │                                                           v                                                                      v
   │                                                     ┌──────────────┐                                                      ┌──────────────┐
   │                                                     │     Failed   │                                                      │     Failed   │
   │                                                     └──────────────┘                                                      └──────────────┘
   │
   └─► app_close/app_crash persists `.meta.json` with the last state for relaunch.

Stage 2 – streaming, persistence loop, resume, and completion
┌──────────────────────┐ chunk_written ┌──────────────────────┐ flush_ok ┌────────────────────────┐ hash_ok ┌──────────────────┐ rename_ok ┌─────────────┐
│ Downloading (active) │─────────────►│ PersistingProgress   │────────►│ VerifyingSha           │──────►│ FinalizingIo    │───────►│ Completed   │
└──────┬───────────────┘             └──────┬────────────────┘         └──────┬──────────────────┘       └──────┬───────────┘         └──────┬──────┘
       │ pause/app_close/app_crash          │ persist_error                   │ hash_fail                     │ rename_fail              │ telemetry
       v                                    v                                 v                              v                          v
┌──────────────┐                      ┌──────────────┐                  ┌──────────────┐               ┌──────────────┐          ┌──────────────┐
│     Paused   │────resume──────────►│ AwaitingResume│────resume──────►│     Failed   │◄─hash_mismatch│     Failed   │◄─io_fail│     Failed   │
└──────┬───────┘   auto-resume timer └──────────────┘  resume_timeout   └──────────────┘               └──────────────┘          └──────────────┘
       │ user_cancel/abort                        ▲                                       ▲                                 ▲
       v                                          │                                       │                                 │
┌──────────────┐                                  │                               unrecoverable_network/disk         telemetry reports
│     Failed   │◄─────────────────────────────────┘
└──────────────┘
```

Transitions and recovery notes:
- `start_download` moves `Idle → PreparingHead`, issuing `HEAD` and caching headers.
- `PreparingHead → HeadBackoff` on transient network issues; the loop applies bounded exponential backoff before retrying `HEAD`.
- If headers disagree with existing metadata, we enter `Restarting`, wipe partial artifacts, and return to `ValidatingMetadata` with a fresh zero offset; unrecoverable header failures drop to `Failed`.
- `PreflightStorage` ensures directories exist and free space is available; if not, the client retries with backoff or surfaces an actionable error before falling to `Failed`.
- `ValidatingMetadata` reconciles `.meta.json` with the on-disk `.part` length; mismatches produce a controlled `Restarting` cycle, while schema/permission issues mark `Failed`.
- The streaming loop alternates between `Downloading` and `PersistingProgress`, guaranteeing that each flushed buffer is durably recorded before fetching the next chunk.
- `pause_download`, `app_close`, or crashes move to `Paused`; on restart we hydrate into `AwaitingResume`. Automatic resume policies or UI-triggered resumes move back to `Downloading` after revalidating headers.
- Persistent errors in `AwaitingResume` (e.g., repeated resume timeouts, user cancellation) surface `Failed` but keep metadata so the user can start over.
- `VerifyingSha` runs after the last byte is written; mismatches mark `Failed` and flag the metadata for a clean restart.
- `FinalizingIo` fsyncs metadata and performs an atomic rename into the final destination; rename failures (e.g., target locked) keep the download in `Failed` with guidance to the user.
- Any unrecoverable condition bubbles to `Failed` with `last_error` populated; `Completed` is reached only after the rename succeeds and all temporary artifacts are removed.

App crashes or manual exits persist `.meta.json` at every state transition (`PreparingHead`, `PreflightStorage`, `ValidatingMetadata`, `Downloading`, `PersistingProgress`, `Paused`, `AwaitingResume`) so a relaunch can safely continue or restart as dictated by the validation rules.

## 9. Security & safety

- Validate `destination_path` against a configured root (`downloads/`) to block path traversal or overwriting arbitrary files.
- Use `fs::rename` into the final destination only after SHA-256 verification to keep partial data isolated.
- Compare the announced `Content-Length` with a configurable maximum size to prevent disk exhaustion attacks.
- Redact URLs that contain credentials before logging errors; store secrets only in memory.
- Document that `ETag` must be immutable for the life of the file; a changing `ETag` forces a clean restart rather than deliver corrupt data.

## 10. Testing plan

- **Unit tests**
  - Serialize and deserialize `.meta.json`, including partial writes and upgrades.
  - Offset arithmetic for resume requests and range header construction.
  - SHA-256 computation over staged `.part` files.
  - Atomic rename helper ensuring final file replaces existing files safely.
- **Integration tests**
  - Simulate pause/resume across process restart by persisting metadata, then restarting the runtime.
  - Force an `ETag` change mid-download and assert the client restarts from zero with a surfaced warning.
  - Use an HTTP server that omits `Accept-Ranges` to confirm the client retries from byte `0` and informs the user.
- **Demo harness**
  - Provide `demo/http-transfer.sh` that launches Node A's HTTP server, triggers a download on Node B, pauses at 50%, restarts the Tauri backend, then resumes and finishes.
  - Record a short screen capture (≤60 s) showing start → pause → resume → finished hash to share in class.

## 11. Team boundaries

- Team Whales exposes the HTTP server and `Request` client helpers (including proper `Range` support).
- Team Pandas continues FTP client work that implements `DownloadSource` using `REST`/`RETR`.
- Team Hawks owns the resume core: metadata persistence, state machine, integrity checks, Tauri commands, and the demo harness.

## 12. Acceptance criteria

- A user can download a file over HTTP, pause, close the app, reopen it, and finish without re-downloading completed bytes.
- `.meta.json` and `.part` files are cleaned up after successful completion, and the final file matches the expected SHA-256 when provided.
- `DownloadState` values surface at least the following states: `Idle`, `PreparingHead`, `HeadBackoff`, `Restarting`, `PreflightStorage`, `ValidatingMetadata`, `Downloading`, `PersistingProgress`, `Paused`, `AwaitingResume`, `VerifyingSha`, `FinalizingIo`, `Completed`, and `Failed`, matching the ASCII diagram.
- Demo harness and automated tests described above are merged and documented.

## 13. Open questions

- Is storing the final whole-file SHA-256 sufficient, or should we also persist a rolling hash to speed future validations?
- For the baseline demo, will URLs be entered manually, or should we add a small optional field in existing metadata records to carry a plain HTTP URL?
- What maximum file size should we impose in the first release to avoid runaway disk usage during demos?

## 14. FAQ / common questions

- **Why are we standardising on HTTP first?** Plain HTTP is the simplest transport that every teammate understands, so we can demo reliability quickly while Team Whales finishes Range support. Once the core resume logic is solid we can plug in FTP or other protocols without touching this document.
- **What happens if the `.meta.json` file is lost or corrupted?** The client detects the mismatch during the `ValidatingMetadata` state, drops to `Restarting`, deletes stale artifacts, and restarts from zero with a fresh metadata file so users never get a half-trustworthy state.
- **How do we ensure the resumed data is valid?** We compare server headers (`ETag`, `Content-Length`, `Last-Modified`) before every resume and run a whole-file SHA-256 on completion, aborting if either check fails.
- **When do we clean up temporary files?** Successful completion runs through `FinalizingIo`, which renames `.part` atomically to the final destination and removes the metadata sidecar; failures leave the artifacts for inspection but label the state as `Failed`.
- **Can multiple downloads use this at the same time?** The baseline supports one active transfer per `download_id`; the API can be invoked for multiple IDs, but this proposal intentionally avoids multi-source scheduling or chunk orchestration until the professor approves the baseline.
- **What if the disk fills up mid-transfer?** `PreflightStorage` checks free space up front, and each `PersistingProgress` iteration verifies remaining space; if writes fail we emit a `Failed` state with a precise `DiskFull` error and leave the `.part` file for manual cleanup.
- **How do we handle HTTP servers that refuse range requests?** On resume we detect missing `Accept-Ranges` or `206` support, warn the user, and fall back to `Restarting` so the file downloads from byte zero without silently corrupting data.
- **When do we surface status updates to the UI?** After each loop through `PersistingProgress` we push a `DownloadStatus` snapshot via the Tauri event channel so the frontend sees fresh byte counts even if the user never pauses.
- **Where do protocol-specific concerns live?** The `DownloadSource` trait keeps this state machine agnostic; HTTP and FTP clients implement retries, header parsing, and range semantics while the Hawks core owns persistence, integrity, and resume flow.

## 15. References

- RFC 7233: Hypertext Transfer Protocol (HTTP/1.1) Range Requests.
- RFC 7232: Hypertext Transfer Protocol (HTTP/1.1) Conditional Requests (ETag/If-Range).
- MDN Web Docs: HTTP range requests and conditional requests.
- This proposal (`docs/download-restart.md`). **All implementation PRs must cite this document.**

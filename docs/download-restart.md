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
    Handshake,
    HandshakeRetry,
    LeaseRenewDue,
    PreparingHead,
    HeadBackoff,
    Restarting,
    PreflightStorage,
    ValidatingMetadata,
    Downloading,
    PersistingProgress,
    Paused,
    AwaitingResume,
    LeaseExpired,
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
    #[error("insufficient disk space")]
    DiskFull,
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

The backend also emits a `download_status` event after each persisted write so the UI can stay in sync without polling. Each payload follows:

```json
{
  "download_id": "uuid-string",
  "state": "Downloading",
  "bytes_downloaded": 5242880,
  "expected_size": 10485760,
  "etag": "\"abc123\"",
  "last_error": null
}
```

`DiskFull` is raised whenever storage preflight or a persistence loop detects insufficient free space so the UI can guide the user to free space before retrying.

### 5.2 Minimal source trait

```rust
#[async_trait]
pub trait DownloadSource: Send + Sync {
    async fn head(&self, url: &url::Url) -> Result<SourceMetadata, SourceError>;
    async fn fetch_range(
        &self,
        url: &url::Url,
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

HTTP, FTP, and any future byte-range-capable transports conform to this trait so the resume core stays protocol agnostic while still enforcing integrity and resume guarantees.

### 5.3 Expected HTTP server behaviour

- Reply to `HEAD /file` with `200 OK`, `Content-Length`, `Accept-Ranges: bytes`, a strong `ETag`, and optional `Last-Modified`. If `Content-Length` is not available, or if the server responds `405 Method Not Allowed`/`501 Not Implemented` to `HEAD`, the client sends a probe `GET` with `Range: bytes=0-0` and expects `206 Partial Content` carrying `Content-Range: bytes 0-0/total`; only when both mechanisms fail do we treat the source as unsupported.
- Reply to ranged `GET /file` requests (`Range: bytes=start-` plus `If-Range: <etag>`) with `206 Partial Content`, `Content-Range: bytes start-end/total`, and the same strong `ETag`. Servers that fall back to `200 OK` without `Content-Range` mark the file as non-resumable; the client restarts from byte `0` and surfaces a warning.
- Emit `416 Range Not Satisfiable` when the requested range exceeds the file size. The client re-probes the total size via `HEAD` or `bytes=0-0`. If the persisted offset is larger than the reported size we discard the partial data, restart from zero, and notify the user.
- Preserve a stable strong `ETag` for the lifetime of the file. Weak ETags (`W/`) signal that safe resume is impossible; the client restarts from zero. When only `Last-Modified` is present we allow resume but log the higher risk because coarse timestamps can miss mid-second mutations.
- Serve plain `GET` responses for initial downloads; range support is mandatory only for resume operations.
- Respond with `3xx` redirects only if future work explicitly enables mirrors; today we treat any redirect as an error, surface a warning, and restart from zero.

### 5.4 Seeder handshake and lease (DHT control plane)

- The downloader sends a `HandshakeRequest` DHT message to the seeder with `{file_id, download_id, epoch, requester_peer_id}`.
- The seeder validates that the file is still hosted, confirms a strong `ETag`, and answers with a signed `HandshakeAck` (`HandshakeAck{ file_id, download_id, etag, size, epoch, lease_exp, lease_issued_at, resume_token }`).
- `resume_token` is a JWS (Ed25519) containing: `sub=file_id`, `aud=seeder_peer_id`, `download_id`, `etag`, `epoch`, `iat`, `nbf`, `exp`, `scp:"resume"`, `kid`.
- Default lease window: 4 hours (`exp - iat = 14,400 s`). Minimum 5 minutes, maximum 24 hours. Downloader renews when `exp - now <= max(60 s, 10% of lease)` with jitter.
- Allowed clock skew between client and seeder `Date` header: ±5 minutes. Tokens outside this tolerance are rejected with `DownloadError::Invalid`.
- `lease_issued_at` provides the canonical server clock; clients never rely on HTTP `Date` headers for timing.
- Weak ETags are rejected; Last-Modified-only sources trigger a clean restart request instead of issuing a token.
- Redirects/CDN mirrors are not supported; the token’s `aud` must match the seeder peer id that serves HTTP.
- Renewal uses the same DHT channel. On success the seeder returns a fresh `resume_token` with a new `exp`.
- JWKS endpoint: `/.well-known/chiral/jwks.json` served by the seeder. Keys are rotated with a 48-hour overlap window; clients cache by `ETag` and `Cache-Control`.
- Failure handling:
  - `401` / invalid token -> move to `HandshakeRetry`, keep bytes, redo handshake.
  - `403` (aud/scope/kid mismatch) -> fail closed, surface error.
  - `429` / `5xx` -> exponential backoff with jitter before `HandshakeRetry`.

## 6. On-disk format

- `<destination_path>.part`: binary data stream written sequentially from byte `0`.
- `<destination_path>.meta.json`: UTF-8 JSON record with the following schema:
  ```json
  {
    "version": 1,
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
  - `version`: schema version for forward compatibility; the client rejects unknown versions with a clear error.
  - `download_id`: stable identifier stored with the metadata.
  - `url`, `etag`, `expected_size`, `last_modified`: copied from `HEAD`.
  - `bytes_downloaded`: last durable write offset flushed to disk.
  - `sha256_final`: populated after final hash verification succeeds even if `expected_sha256` was absent.

Generation and validation rules:
- If the caller omits `download_id`, the backend issues a UUID v4 and persists it before any network request.
- During resume the client compares `bytes_downloaded` with the actual `.part` length. If they differ we assume possible corruption and restart from zero rather than trimming; the user is warned via `last_error`.
- Metadata writes append to a temp file followed by `fsync` and atomic rename so partially written JSON never persists.
- If `version` is greater than the current schema (`1`), return `DownloadError::Invalid("unsupported metadata version")` and leave on-disk state untouched.

## 7. Client algorithm

- **Handshake / `Handshake`:** send a `HandshakeRequest` via the DHT, verify the signed `HandshakeAck`, cache `resume_token`, `lease_exp`, and server clock (`Date`). Enter `HandshakeRetry` with exponential backoff on timeout, malformed signatures, or 4xx/5xx errors.
- **Lease renew / `LeaseRenewDue`:** when `lease_exp - now <= max(60 s, 10% of lease)` and the download is still active, send a renewal DHT message; upon success swap in the new token, otherwise continue streaming until expiry and transition to `LeaseExpired`.
- **Start / `PreparingHead`:** issue `HEAD`, record strong `ETag`, `Last-Modified`, and `Content-Length` when present. If length is missing, immediately probe with `GET Range: bytes=0-0` and parse `Content-Range` to establish the total; fail only when both responses omit the size.
- **Retry / `HeadBackoff`:** on transient network or 5xx responses, retry `HEAD` (and the optional probe) with bounded exponential backoff (base 1 s, max 30 s) while updating telemetry so operators can see the retry count.
- **Reset / `Restarting`:** when headers disagree with stored metadata, when the server returns `200 OK` without `Content-Range` to a resume request, when a weak `ETag` (`W/`) is observed, or when a `416 Range Not Satisfiable` indicates our offset is beyond the probed size. In the `416` path we re-run the size probe to confirm the reported total before deleting partial artifacts, logging the reason, zeroing the offset, and returning to `PreparingHead`.
- **Storage preflight / `PreflightStorage`:** ensure the destination directory exists, confirm free disk space is at least `expected_size - bytes_downloaded`, and open the `.part` file with read/write/create flags while taking a per-path mutex and an advisory OS lock (`fs2::FileExt::try_lock_exclusive`) so only one process can write the destination. Seek to `bytes_downloaded` before writing; on restarts we truncate to zero. Disk exhaustion raises `DownloadError::DiskFull` and halts safely.
- **Metadata validation / `ValidatingMetadata`:** load the JSON, confirm `version == 1`, match `download_id`, and verify the `.part` length equals `bytes_downloaded`. Mismatches trigger a full restart to avoid silent corruption.
- **Streaming / `Downloading`:** the first transfer uses `GET` without `Range` for compatibility; resumes use `Range: bytes={bytes_downloaded}-` with `If-Range` containing the strong `ETag`. If the header set downgrades (for example only weak ETag is returned) we abort and restart.
- **Persistence loop / `PersistingProgress`:** write incoming chunks to disk, flush the write buffer, and `fsync` every 8 MiB (configurable) to balance safety and throughput. After each durable write, update `.meta.json` via atomic replace, emit a `download_status` event, and keep `last_error` clear.
- **Pause & await resume / `Paused` + `AwaitingResume`:** cancelling the stream or detecting shutdown flushes buffers, persists metadata, and moves to `Paused`. On relaunch, we load metadata into `AwaitingResume` where user action or auto-resume policy issues `resume_download`.
- **Resume path:** re-run `HEAD` (plus the fallback probe if necessary), confirm headers, and if safe, re-enter `Downloading`. When only `Last-Modified` is available we proceed but annotate `last_error` with a warning about potential staleness. Range requests that get `200 OK` without `Content-Range`, explicit `SourceError::RangeUnsupported`, or `416` responses trigger a size re-probe and then send the transfer back to `Restarting` after warning the user.
- **Finish / `VerifyingSha` + `FinalizingIo`:** when `bytes_downloaded == expected_size`, compute the SHA-256 of the `.part` file regardless of whether `expected_sha256` was supplied, store it in `sha256_final`, then `fsync`, perform an atomic rename into place, and remove metadata. If the destination is on another volume we stream-copy, fsync the copy, and replace the target to maintain integrity.
- **Failure handling / `Failed`:** non-recoverable errors (persistent HTTP rejections, disk full, hash mismatch, permission failure, unreachable source) mark the download as `Failed` with a structured `last_error`. Users can retry via `resume_download`, which re-enters validation, or restart from scratch.

Error code mapping:
- `NotFound` -> app code `DOWNLOAD_NOT_FOUND`
- `Invalid` -> app code `DOWNLOAD_INVALID_REQUEST`
- `Source` -> app code `DOWNLOAD_SOURCE_ERROR`
- `Io` -> system code `IO_ERROR`
- `DiskFull` -> system code `STORAGE_EXHAUSTED`
- `AlreadyCompleted` -> app code `DOWNLOAD_ALREADY_COMPLETE`

## 8. State machine

```
Control plane and setup

[Idle]
  └─ start_download -> [Handshake]
        ├─ success -> [PreparingHead]
        ├─ transient_error -> [HandshakeRetry] -> retry -> [Handshake]
        └─ lease_expired -> [Handshake] (request fresh token)

[PreparingHead]
  ├─ transient_error -> [HeadBackoff] -> retry -> [PreparingHead]
  ├─ weak_etag/size_unknown -> [Restarting] -> cleanup -> [PreparingHead]
  └─ headers_ok -> [PreflightStorage]

[PreflightStorage]
  ├─ disk_full/open_fail -> [Restarting] -> cleanup -> [PreparingHead]
  └─ space_ok -> [ValidatingMetadata]

[ValidatingMetadata]
  ├─ version or length mismatch -> [Restarting] -> cleanup -> [PreparingHead]
  └─ metadata_ok -> [Downloading]

Streaming and resume

[Downloading]
  ├─ chunk flushed -> [PersistingProgress] -> fsync ok -> [Downloading]
  ├─ lease_renew_due -> [LeaseRenewDue] -> renew_ok -> [Downloading]
  ├─ lease_expired -> [LeaseExpired] -> [Handshake]
  ├─ range 200 without Content-Range / weak ETag / 416 w/ offset > size / RangeUnsupported -> [Restarting] -> cleanup -> [PreparingHead]
  └─ pause or shutdown -> [Paused] -> relaunch -> [AwaitingResume] -> resume -> [PreparingHead]

Finish and failure

[Downloading]
  └─ bytes == expected -> [VerifyingSha]
         ├─ hash mismatch -> [Failed] -> user retry -> [AwaitingResume]
         └─ hash_ok -> [FinalizingIo]
                ├─ rename/copy ok -> [Completed]
                └─ rename fail -> [Failed] -> user retry -> [AwaitingResume]

[PersistingProgress] can also enter `[Failed]` when disk full or IO errors occur; users retry via `[AwaitingResume]`.
```

Key transitions and safeguards:
- `start_download` moves the downloader from `Idle` into `Handshake` where we obtain a signed resume token, then into `PreparingHead` to gather validators and probe for size.
- Weak ETags, missing size data after probing, a `200 OK` response to a range resume, or a `416` with an offset beyond the reported total all redirect to `Restarting`, deleting partial data and starting from byte zero.
- Disk checks in `PreflightStorage` and `PersistingProgress` raise `DownloadError::DiskFull`, leave artifacts untouched, and surface guidance to free space; advisory OS locks prevent concurrent writers.
- Lease renewal happens in `LeaseRenewDue`, refreshing the token before expiry. An expired lease moves to `LeaseExpired`, triggering `Handshake` while keeping previously downloaded bytes.
- `Paused` persists metadata on every transition so relaunching into `AwaitingResume` is deterministic. Auto-resume or UI actions run back through validation before re-entering `Downloading`.
- The integrity path keeps resuming safe: we only enter `VerifyingSha` when the byte count matches the expected size, we store the computed hash regardless of caller input, and any mismatch moves to `Failed` with instructions to restart.
- `FinalizingIo` performs an atomic rename when the destination is on the same volume; if not, we copy the temp file, `fsync`, and replace the target, ensuring no torn writes reach the user.

App crashes or manual exits persist `.meta.json` at every state transition (`Handshake`, `PreparingHead`, `PreflightStorage`, `ValidatingMetadata`, `Downloading`, `LeaseRenewDue`, `PersistingProgress`, `Paused`, `AwaitingResume`) so a relaunch can safely continue or restart according to the rules above.

## 9. Security & safety

- Validate `destination_path` against a configured root (`downloads/`) to block path traversal or overwriting arbitrary files.
- Check available disk space before writing and on every persistence loop; emit `DownloadError::DiskFull` and preserve artifacts when space is exhausted.
- Use `fs::rename` into the final destination only after SHA-256 verification when the target shares a volume with the temp file; otherwise stream-copy, `fsync`, and replace so cross-volume moves stay atomic from the user’s perspective.
- Compare the announced `Content-Length` with a configurable maximum size to prevent disk exhaustion attacks.
- Redact URLs that contain credentials before logging errors; store secrets only in memory.
- Treat weak or changing `ETag` values as unsafe and restart from byte zero to avoid serving stale bytes.

## 10. Testing plan

- **Unit tests**
  - Serialize and deserialize `.meta.json`, including partial writes and upgrades.
  - Offset arithmetic for resume requests and range header construction.
  - SHA-256 computation over staged `.part` files.
  - Metadata version negotiation (`version == 1`) and enforcement of UUID generation.
  - Atomic rename helper ensuring final file replaces existing files safely.
- **Integration tests**
  - Simulate pause/resume across process restart by persisting metadata, then restarting the runtime.
  - Force an `ETag` change mid-download and assert the client restarts from zero with a surfaced warning.
  - Use an HTTP server that omits `Accept-Ranges` to confirm the client retries from byte `0` and informs the user.
  - Serve only weak ETags and verify the client forces a restart before resuming.
  - Return `200 OK` to a range request and assert the restart-and-warn path triggers.
  - Return `416 Range Not Satisfiable`, verify the size probe, and confirm restart with cleanup when the offset is too large.
  - Crash during the fsync window, restart the process, and ensure metadata survives with a consistent resume.
  - Simulate power loss mid-chunk, causing the `.part` file to be longer than metadata, and confirm we restart safely.
  - Fill the disk mid-download and check that `DownloadError::DiskFull` reaches the UI and that partial data remains intact for inspection.
  - Validate DHT handshake success, rejection of weak ETags, and renewal just before expiry with ±5 minute clock skew.
  - Let a lease expire mid-stream, ensure the client enters `LeaseExpired`, re-handshakes, and resumes without losing data.
- **Demo harness**
  - Provide `demo/http-transfer.sh` that launches Node A's HTTP server, triggers a download on Node B, pauses at 50%, restarts the Tauri backend, then resumes and finishes.
  - Record a short screen capture (≤60 s) showing start -> pause -> resume -> finished hash to share in class.

## 11. Team boundaries

- Team Whales exposes the HTTP server and `Request` client helpers (including proper `Range` support).
- Team Pandas continues FTP client work that implements `DownloadSource` using `REST`/`RETR`.
- Team Hawks owns the resume core: metadata persistence, state machine, integrity checks, Tauri commands, and the demo harness.
- Baseline scope is one active transfer per `download_id`; multi-source coordination, BitTorrent variants, and DHT search remain out of scope until this path ships.
- HTTP resume rides on Team Whales’ implementation today, and Team Pandas can plug FTP sources into the same state machine later without new docs.

## 12. Acceptance criteria

- A user can download a file over HTTP, pause, close the app, reopen it, and finish without re-downloading completed bytes.
- `.meta.json` and `.part` files are cleaned up after successful completion, and the final file matches the expected SHA-256 when provided.
- Resume logic handles `200` without `Content-Range`, weak ETags, and `416` responses by restarting from zero with clear user messaging.
- Metadata versioning (`version == 1`) is enforced during resume and unknown versions fail fast.
- `DownloadState` values surface at least the following states: `Idle`, `Handshake`, `HandshakeRetry`, `LeaseRenewDue`, `PreparingHead`, `HeadBackoff`, `Restarting`, `PreflightStorage`, `ValidatingMetadata`, `Downloading`, `PersistingProgress`, `Paused`, `AwaitingResume`, `LeaseExpired`, `VerifyingSha`, `FinalizingIo`, `Completed`, and `Failed`, matching the state diagram.
- `.part` and `.meta.json` are removed on success and preserved on failure for post-mortem analysis.
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
- **What if the server only exposes `Last-Modified`?** We proceed with resume but surface a warning because timestamp granularity is weaker than a strong `ETag`, and any mismatch falls back to a full restart.
- **How do we choose the fsync window?** We default to syncing every 8 MiB, which balances crash safety and throughput; operators can shrink the window for extra durability or widen it for higher throughput.
- **What happens when users reuse a `download_id`?** The backend rejects collisions with `DownloadError::Invalid`, ensuring each active file keeps isolated metadata and state so accidental cross-file resumes never happen.
- **Can users cancel and immediately resume?** `pause_download` followed by `resume_download` stays within the same metadata record, but invoking `start_download` again wipes prior artifacts to guarantee a clean restart.
- **How are cross-volume destinations handled?** `FinalizingIo` detects when the destination lives on another device, stream-copies the `.part` file, `fsync`s the copy, and swaps it into place so users never see partial files.
- **What audit trails exist for errors?** Every transition that lands in `Failed` records a structured `last_error`, and the backend logs a concise line per failure so operators can correlate UI warnings with backend telemetry.
- **How does the lease handshake work without HTTP POST?** We send a signed token over the DHT, renew it before expiry, and re-handshake if we ever see `401/403` or `LeaseExpired`, keeping the data plane on plain HTTP.

## 15. References

- RFC 7233: Hypertext Transfer Protocol (HTTP/1.1) Range Requests.
- RFC 7232: Hypertext Transfer Protocol (HTTP/1.1) Conditional Requests (ETag/If-Range).
- MDN Web Docs: HTTP range requests and conditional requests.
- This proposal (`docs/download-restart.md`). **All implementation PRs must cite this document.**

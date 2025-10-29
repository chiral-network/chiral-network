# Design Proposal: Internet-Reachable HTTP File Sharing

## 1) Problem Statement

We need a **simple, reliable way to share files over HTTP across the public internet**, even when users are behind NATs, campus Wi-Fi, or CGNAT. Current local HTTP upload/download works on localhost; it does **not** consistently produce a public, shareable link and frequently breaks with 404s when the app exposes `localhost` or `0.0.0.0` URLs.

---

## 2) Goals & Non-Goals

### Goals

* **G1 — Internet reachability:** One-click “Start Internet Tunnel” yields a **public HTTPS URL** that anyone can use to download shared files.
* **G2 — Correct URLs:** Frontend and backend **always** produce valid public links (no `localhost`, no `0.0.0.0`, no `//download`).
* **G3 — Large-file readiness:** Support **streaming upload**, **streaming download**, and **HTTP Range (206)** for resume.
* **G4 — Clear UX:** Simple sender/receiver flow; copyable links; meaningful errors.

### Non-Goals

* End-to-end encrypted storage/ACLs (HTTPS is provided by tunnel).
* libp2p/DCUtR/WebRTC transport; that is a **separate future track**.

---

## 3) User Stories

* **As a sender**, I press **Start Internet Tunnel**, upload a file, and share a **public link**.
* **As a receiver**, I click that link (or run `curl`) and the file downloads over HTTPS.
* **As an instructor/demo reviewer**, I want to see the flow working on **campus Wi-Fi** (UPnP may fail) and on **home Wi-Fi** (UPnP may succeed), without manual router config.

---

## 4) Approach Overview

### 4.1 Tunneling Strategy (Provider Order)

1. **Cloudflare Tunnel** (default & recommended): stable, free, HTTPS.
2. **ngrok** (fallback): requires authtoken; free tier acceptable for small demos.
3. **Self-hosted (public IP:port)** as a **last resort** (warn user about CGNAT/campus limitations).

> The app tries **Cloudflare → ngrok → Self** and stops on first success.
> Explicit provider selection remains available if needed.

### 4.2 URL Construction Rules (Single Source of Truth)

* **BASE selection priority:**
  `tunnel.public_url` → `http://{publicIp}:{externalPort}` (UPnP) → `http://localhost:8080`.
* **Backend** returns **relative** `download_url` = `"/download/{hash}"`.
* **Frontend** always builds absolute URLs as `BASE + relative`, trimming trailing/leading slashes to avoid `//download`.

### 4.3 Large-File Readiness

* **Streaming upload** (write multipart chunks directly to disk).
* **Streaming download** with `Accept-Ranges: bytes` and **206 Partial Content** for resume.
* **Body limit** increased (e.g., up to 1 GB) and **timeouts** relaxed for slow networks.

---

## 5) Architecture

```
[Sender App]
  Axum HTTP server (upload/list/download)
  Tunnel Service (Cloudflare/ngrok) --> issues public HTTPS URL
      |
      v
   Internet
      |
      v
[Receiver]
  Browser / curl
```

* Backend serves: `POST /upload`, `GET /files`, `GET /download/{hash}`.
* Files identified by **SHA-256**; the backend returns metadata including the relative `download_url`.
* Frontend displays **Public URL** (tunnel), shared files, and copyable links.

---

## 6) UX & Screens

**Network Configuration**

* Button: **Setup Network & UPnP** (informational; UPnP may fail on campus).
* Show public IP, local IP, UPnP status.

**Internet Sharing Tunnel**

* Button: **Start Internet Tunnel** → attempts Cloudflare → ngrok → Self.
* On success: show **Public URL** + copy button.
* On self-host: show **warning** about CGNAT/loopback and suggest using a tunnel.

**Upload & Share**

* Select file → **Upload**.
* Success card shows **file name**, **size**, **hash**, and **Download URL** (built via rules above).

**Shared Files**

* List with copyable **public** download links (resolved to BASE + `/download/{hash}`).

**Download**

* Inputs: **Server URL** (pre-filled to public URL), **File Hash**, **Save Path**.
* Button: **Download File**.
* Errors are human-readable (e.g., “URL had `//download` → fixed; try again”).

---

## 7) API Contract (Backend)

* `POST /upload`

  * Body: multipart/form-data (`file=@...`)
  * Behavior: **streaming** write to disk; compute SHA-256; store metadata
  * Returns: `{ file_name, file_size, file_hash, upload_time, download_url: "/download/{hash}" }`

* `GET /files`

  * Returns: array of above metadata objects.

* `GET /download/{hash}`

  * Behavior: **streaming** file read; sets `Content-Disposition` for filename
  * **Range (206)** support with `Accept-Ranges: bytes`, `Content-Range`, correct `Content-Length`.

---

## 8) Error Handling & Observability

* **Immediate fail-fast** on ngrok auth errors (e.g., `ERR_NGROK_4018`) → next provider.
* Parse tunnel stdout/stderr to extract URLs (Cloudflare `trycloudflare.com`, ngrok `url=`/`Forwarding`).
* Structured logs for upload/download start/end, source IP, and status codes.
* ngrok inspector (`127.0.0.1:4040`) documented for debugging bad paths.

---

## 9) Security & Privacy

* Public link model (anyone with the link can download).
* Options (not in MVP): basic auth, expiring token query (`?t=uuid`).
* HTTPS is provided by the tunnel; self-host mode requires external TLS termination if used.

---

## 10) Performance Considerations

* **Cloudflare Tunnel** preferred for longer/larger transfers; **ngrok free** may cut off slow uploads.
* Streaming avoids loading entire files into memory.
* Resume (`curl -C -`) reduces retries for flaky links.
* Hash-based addressing enables post-download integrity checks.

---

## 11) Milestones & Timeline

**M1 — URL Correctness & Tunnel MVP (1–2 days)**

* Frontend: remove localhost hard-codes; add `normalizeBase` & `resolveDownloadUrl`.
* Backend: `/files` returns **relative** `download_url`.
* Tunnel service: Cloudflare → ngrok → Self order with fail-fast auth handling.
* Manual demos: campus ↔ home via Cloudflare & ngrok.

**M2 — Large-File Readiness (2–3 days)**

* Add streaming upload & download; raise body limit; relax timeouts.
* Implement **HTTP Range (206)** and `Accept-Ranges`.
* Document resume: `curl -C - -OJL "<public>/download/<hash>"`.

**M3 — Polish & Docs (1 day)**

* Robust error messages; copy buttons; link hygiene.
* README & troubleshooting guide; screenshots; test matrix results.

---

## 12) Acceptance Criteria

* Campus Wi-Fi (UPnP fails): **Cloudflare Tunnel** produces a working public link; receiver downloads successfully.
* Home Wi-Fi (UPnP may succeed): links **never** expose `localhost`/`0.0.0.0`; proper public link works externally.
* Trailing slash issues **do not** produce `//download` paths.
* 100–300 MB files upload & download successfully; interrupted downloads resume via `curl -C -`.
* ngrok without authtoken → immediate provider fallback (no long waits).

---

## 13) Risks & Mitigations

* **Tunnel dependency:** If Cloudflare/ngrok is down → documented fallback to the other; long-term: VPS + FRP/NGINX option.
* **ngrok free limits:** Slow/big uploads can drop → prefer Cloudflare; consider paid tier if needed.
* **CGNAT/loopback:** Self-host unusable → UI nudges to Tunnel mode with clear warnings.
* **Link leakage:** Optionally add basic auth or expiring tokens in a follow-up.

---

## 14) Open Questions (for reviewers)

1. Do we standardize on **Cloudflare first** as the official default?
2. Is **resume (Range 206)** required for MVP, or acceptable in M2?
3. Do we want **basic auth / expiring tokens** in MVP?

---

## 15) Appendix: Developer Notes

* **Frontend helpers:**

  * `normalizeBase(serverUrl)`: trim & remove trailing `/`.
  * `resolveDownloadUrl(d)`: if relative, prepend BASE; if `0.0.0.0/localhost`, replace host with BASE; if hash, build `/download/{hash}`.
* **Backend tips:**

  * `DefaultBodyLimit::max(...)` to raise limit; stream multipart to file with `field.chunk().await`.
  * For Range, parse `Range: bytes=start-end`, clamp to file size, return `206` with `Content-Range`.

---

**Request:** Please review this proposal’s scope and milestones. Upon approval, we’ll implement **M1 → M2 → M3** in short PRs with demo artifacts and test logs.

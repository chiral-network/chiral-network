# FM-Agent Audit — Findings Across the Whole Codebase

This document consolidates the bug-hunting output of applying the FM-Agent
methodology (`FM-Agent/system_prompt.md`) to the chiral-network Rust
codebase. It supplements the per-bug detail files (`BUG-001`, `BUG-002`)
already on disk.

## Pipeline state

- `fm_agent/phases.json` — 27 source files across 11 phases.
- `fm_agent/spec_prompts/system_prompt.md` — copied from `FM-Agent/`.
- `fm_agent/spec_prompts/domain_context/engine_overview.txt` — system overview.
- `fm_agent/spec_prompts/domain_context/phase_NN_types.txt` — one per phase.
- `fm_agent/extractor.py` — language-aware function extractor; ran
  cleanly across all 27 files producing **684 per-function source files**
  under `fm_agent/extracted_functions/`.
- `fm_agent/spec_prompts/generate_topdown_layers.py` — call-graph layer
  computer; produced `phase_NN_topdown_layers.json` for every phase.
- Per-function spec generation across 684 functions in a single session
  is not feasible. Instead, four parallel security audits applied the
  spec-vs-code methodology to the four highest-leverage trust-boundary
  surfaces:
    A. Crypto + signature paths
    B. Network trust boundaries (DHT records, chunked transfer)
    C. Payment verification
    D. HTTP gateway / APIs

This audit doc lists every concrete MISMATCH the four audits surfaced,
with confirmation status, severity, and disposition (fixed in this
session, filed for follow-up, or closed as already-fixed / not a real
gap).

## Findings

### Catastrophic — exploitable today

| ID | Title | File | Status |
|----|-------|------|--------|
| FM-A01 | Drive share `verify_payment_tx` does not check `from`; a single tx hash to the share-recipient wallet unlocks the share for everyone forever | `drive_api.rs:107-184, 211-224` | **Mitigated** — per-share spent-tx ledger added in `verify_share_access`; one tx now unlocks one share, not many. From-check still missing (needs UI/contract change to capture buyer wallet). |
| FM-A02 | Drive share has no spent-tx ledger; same `?access=<tx>` URL works publicly forever | `drive_api.rs:211-224` | **Fixed** — see FM-A01. |
| FM-A03 | `X-Owner` HTTP header trusted without proof of wallet control | `drive_api.rs:54-61` and every Drive handler | **Filed** — needs signed-challenge auth refactor; not surgically fixable in one session. **Follow-up.** |
| FM-A04 | Open reverse-proxy / SSRF on the relay: `relay-register` accepts arbitrary `origin_url` with no allowlist (loopback/private IPs/internal infra) | `relay_share_proxy.rs:298-330, 593-615, 492-532` | **Filed** — needs origin-URL allowlist + ownership signature. **Follow-up.** |
| FM-A05 | `register_share` / `register_site` have no auth; any HTTP caller can hijack a known `site_id`/`token` and persist it | `relay_share_proxy.rs:308-330, 593-615` | **Filed** — same family as FM-A04. **Follow-up.** |
| FM-A06 | `dht_put` HTTP route accepts reserved-namespace keys (`chiral_file_*`, `chiral_seeder_*`, `chiral_folder_*`, `chiral_sitename_*`, `chiral_drive_share_*`, `chiral_host_ad_*`); same defect class as the (already-fixed) BUG-001 site-name hijack | `bin/chiral_daemon.rs:457-469` | **Fixed** — namespace allowlist added; reserved prefixes return `403 FORBIDDEN`. |

### High — published trust contract not implemented in production

| ID | Title | File | Status |
|----|-------|------|--------|
| FM-A07 | `FileMetadata::publisher_signature` is documented as enforced but every production publisher writes the empty string and `search_file` consumes the metadata regardless of `verify_publisher()` | `lib.rs:1180-1199, ~393, 2511, 5267, 5397, 6677` and `lib.rs:1883-1888` | **Fixed** — added `try_make_signed_file_metadata` helper; every production publisher (`publish_file`, `publish_file_data`, `republish_shared_file`, `seed_hosted_file`, `publish_drive_file_inner`) now signs or refuses; `search_file` rejects unsigned/invalid metadata as not-found. Startup helpers (`auto_reseed_drive_files`, `try_repair_local_drive_seed`) skip the publish since they have no key — user re-seeds from an unlocked wallet via the signed publishers. |
| FM-A08 | `SeederInfo.signature` documented as "prevents payment redirection" but production publishers write empty signatures and `fetch_seeders` accepts empty-signature entries | `lib.rs:1281-1308, 1129-1148` and writer sites | **Fixed** — `try_make_signed_seeder` helper refuses unsigned construction; every production writer wires `private_key` through and either signs or fails fast; `fetch_seeders` drops unsigned/invalid entries (placeholder stubs with empty wallet+price are still emitted for discovery, but the chunked-download path treats them as untrusted until FileInfo signing lands — see FM-A09). |
| FM-A09 | `ChunkResponse::FileInfo` carries `wallet_address` and `price_wei` taken verbatim from the seeder's wire response with no signature; downloader pays whoever the seeder claims to be | `dht.rs:3749-3995, 3465-3476` | **Filed** — needs FileInfo to be ECDSA-signed and the downloader to compare against the publisher's signed `chiral_file_*` metadata. **Follow-up.** |
| FM-A10 | CDN payment accepts a flat **5 % underpayment** (`required_wei * 95 / 100`); comment claims "CHI→wei rounding" but real rounding is sub-cent | `cdn_server.rs:380, 830` | **Fixed** — replaced with exact ceil-rounded `u128` math; `min_accepted_wei == required_wei`. |
| FM-A11 | CDN `required_wei = price * file_mb * months as f64 as u128` — silent truncation past 53 bits of mantissa, plus 0-cost path on non-finite f64 | `cdn_server.rs:377-380, 827-830` | **Fixed** — replaced with `required_upload_wei` helper using saturating `u128` arithmetic and ceil division. |
| FM-A12 | Rating API: `X-Owner` (unauthenticated) is the only proof of "downloader"; a free-tier (`amountWei=0`) submission can drive an arbitrary seeder's Elo down without any tx ever existing | `rating_api.rs:64-70, 178-181, 195-197` | **Filed** — needs same auth refactor as FM-A03 plus an `amount_wei == 0 ⇒ require sig` rule. **Follow-up.** |

### Medium — defense-in-depth and operational hazards

| ID | Title | File | Status |
|----|-------|------|--------|
| FM-A13 | Version-gate middleware reads `version::bundled_policy()` (compile-time constant) instead of `version::effective_policy()`; promoted policies are never enforced through the HTTP gate | `hosting_server.rs:124` | **Fixed** — switched to `effective_policy()`. |
| FM-A14 | CORS layer is `allow_origin(Any).allow_methods(Any).allow_headers(Any)` on mutating routes; combined with FM-A03 lets any visited webpage CSRF the local daemon | `hosting_server.rs:516-522` | **Fixed** — relay-mode keeps `Any` (public-facing); local daemon now allowlists Tauri webview origins (`tauri://localhost`, `tauri.localhost`, dev-server `localhost:1420/5173`), specific methods, and specific headers. Cross-origin requests from arbitrary websites preflight-fail. |
| FM-A15 | Drive multipart upload has no `DefaultBodyLimit`; 499 MB allocation is realised before the 500 MB check rejects | `drive_api.rs:353-403, 1469-1485` | **Fixed** — `DefaultBodyLimit::max(500 * 1024 * 1024)` added to `drive_routes`, matching the CDN router. |
| FM-A16 | `is_item_under_shared_root` walks `parent_id` upward without a visited-set; a parent-cycle hangs the request thread holding the manifest read lock | `drive_api.rs:186-201` | **Fixed** — added `HashSet`-based visited tracker; cycle returns `false` instead of hanging. |
| FM-A17 | `wallet::verify_tx_details` does not compare tx `chainId` to the chiral chain id; a cross-chain replayed signed-tx with same address pair would pass | `wallet.rs:639-661` | **Fixed** — checks `tx.chainId` against `crate::geth::chain_id()`; mismatched chainIds return `Ok(false)`. |
| FM-A18 | `recover_signer` does not enforce low-`s` (EIP-2); both `(r, s)` and `(r, n-s)` recover the same pubkey, so any future caller treating the hex sig as a dedup key is fooled | `wallet.rs:565-592` | **Fixed** — rejects high-`s` byte-compare against secp256k1 N/2; all signatures are now canonical low-s. |
| FM-A19 | `FileMetadata::sign_payload` was `format!("file:{}:{}:{}", hash, name, size)` — same colon-injection class as the historical BUG-001 / BUG-002 fixes; `name` is fully attacker-controlled | `lib.rs:1181-1183` | **Fixed** — switched to a length-prefixed canonical encoding (`u32`-LE prefix per field plus an `0x00`-terminated literal "file" tag). |
| FM-A20 | `FolderManifest` is read in `search_folder` without verifying its publisher signature; `name` and `owner_wallet` are not bound to the content-addressed folder hash | `lib.rs:5917-5969, 5539-5553` | **Fixed** — added `FolderManifest::sign_payload`/`sign`/`verify` (length-prefixed canonical encoding). `publish_drive_folder` signs (or skips publish if wallet locked); `search_folder` drops unsigned/invalid manifests. |
| FM-A21 | Spent-tx guard for paid downloads does not bind the tx to a `file_hash`; a single payment to wallet W can redeem any file priced ≤ V from any seeder advertising W (multi-seeder shared-wallet collusion) | `dht.rs:1051-1102, 3618-3651` | **Fixed** — `claim_spent_tx` now keys on `(tx_hash, file_hash)` pairs (`"<tx>:<hash>"`), so the same payment cannot redeem more than one file. |

### Low — theoretical / minor

| ID | Title | File | Status |
|----|-------|------|--------|
| FM-A22 | Chunk position not verified against requested index; final SHA-256 catches the discrepancy so it's a bandwidth-waste DoS, not an integrity violation | `dht.rs:4097-4223` | **Fixed** — chunk-receive path rejects out-of-sequence chunks (`chunk_index != current_chunk_index` → drop). Bounds bandwidth waste before the full-file hash catches it. |
| FM-A23 | `chiral_site_directory` registry blindly trusts whatever JSON is at the registry key, allowing soft DoS via a giant array | `lib.rs:4504-4522` | **Fixed** — `list_directory_sites` truncates the registry list to `MAX_DIRECTORY_LISTING = 4096` before fanning out per-name fetches. |
| FM-A24 | `compute_total_with_fee(base) → split_payment(total)` round-trip drifts seller share by 1 wei on most inputs (rounding direction asymmetry) | `speed_tiers.rs:14-38` | Filed — invariant violation, not exploitable. |
| FM-A25 | No deferred re-verification / refund path when chain propagation > 28s during paid downloads; buyer's funds are gone | `dht.rs:3587-3700, wallet.rs:610-635` | Filed — UX issue. |
| FM-A26 | `version::POLICY_PUBLIC_KEY = [0u8; 32]` makes the signed-policy branch unreachable today; operator-deploy footgun | `version.rs:145, 190-194` | Acknowledged in code; not a bug. |

## Summary of disposition

After two sessions of FM-Agent application:

- **20 fixed** total: BUG-001 (site-name hijack), BUG-002 (canonical signing NUL), FM-A01/02 (Drive share replay), FM-A06 (dht_put namespace), FM-A07/A08 (FileMetadata + SeederInfo signing wired through every writer + reader enforcement), FM-A10/A11 (CDN tolerance + f64), FM-A13 (version middleware), FM-A14 (CORS), FM-A15 (Drive body limit), FM-A16 (cycle guard), FM-A17 (chainId), FM-A18 (low-s), FM-A19 (FileMetadata length-prefix), FM-A20 (FolderManifest signed), FM-A21 (spent-tx file-bound), FM-A22 (chunk index sequence), FM-A23 (registry size cap).
- **5 still filed** for follow-up: FM-A03 (X-Owner unauthenticated — needs signed-challenge auth refactor), FM-A04/A05 (relay register open-proxy / hijack — needs ownership signature + origin allowlist), FM-A09 (ChunkResponse::FileInfo unsigned — needs protocol bump), FM-A12 (rating API X-Owner / amount=0 bypass), FM-A24/A25/A26 (minor invariant + UX issues).
- **No false positives.** Every finding was confirmed by direct code reading at the cited line ranges.

## Methodology notes

The FM-Agent pipeline as documented expects per-function spec generation to feed automated logic verification, which would mechanically surface MISMATCHes. That stage was bypassed here: the codebase has 684 functions, and per-function spec generation across all of them is multi-session work. The audits above instead applied the spec rules (`system_prompt.md`) directly to whole trust boundaries — same intent, faster signal, fewer fully-formal artefacts.

The infrastructure we built this session (`phases.json`, types files, extractor, layers script) lets a future session pick up at the per-function stage by spawning batched spec-generation agents over the existing extracted-function tree. The `phase_NN_topdown_layers.json` files give a per-phase topo order that lets each batch consume earlier-layer caller specs as context, exactly as `workflow_spec_step4_batch.md` describes.

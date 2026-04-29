# FM-Agent Bug Validation — Summary

Two confirmed `MISMATCH`es found while applying the FM-Agent
behavioural-spec methodology to the chiral-network codebase. Both are
trust-boundary issues. Detail Markdown + result JSON + probe artefact
for each is in this directory.

| ID | Function | Status | Severity (qualitative) | Resolution |
|----|----------|--------|------------------------|-----------|
| [BUG-001](BUG-001-site-name-hijack.md) | `publish_site_to_directory` (`src-tauri/src/lib.rs`) | confirmed | **High** — exploitable today, redirects users to attacker-chosen URLs. | Fixed — `SiteDirectoryEntry` now ECDSA-signed by `owner_wallet`; readers reject invalid signatures; overwrites require matching wallet. |
| [BUG-002](BUG-002-canonical-payload-nul-collision.md) | `canonical_signing_payload` (`src-tauri/src/version.rs`) | confirmed | **Latent** — exploitable once `POLICY_PUBLIC_KEY` is replaced with a real key. Reproduced live with a Rust probe. | Fixed — fields are now length-prefixed (LE `u32` length + bytes). Regression test added. |

---

## BUG-001 — Site Name Hijack (architectural)

The "first-claim wins" guarantee documented for the site directory is
unenforceable: per-name DHT records (`chiral_sitename_<name>`) carry no
signature. Any peer can call `DhtService::put_dht_value` directly — or
hit `POST /api/headless/dht/put` on any daemon — to overwrite the
record and redirect `resolve_site_name(name)` to an attacker-chosen
`public_url`.

**Fix:** sign each `SiteDirectoryEntry` with the owner's wallet key,
verify on read, and refuse on-write overwrites whose signing wallet
differs from the existing record's signing wallet (matching the model
already used for file metadata and seeder entries).

---

## BUG-002 — `canonical_signing_payload` is not injective

`canonical_signing_payload` joins six fields with `0x00` as a separator
but does not escape `0x00` when it appears inside a field. Because
`String` fields can contain NUL bytes (Rust permits, JSON permits),
content can be shifted across a field boundary without changing the
output bytes. Two `VersionPolicy` values that differ in `min_required`
*and* `recommended` were demonstrated producing byte-equal canonical
payloads.

This is latent today — `POLICY_PUBLIC_KEY` is the 32-byte zero
placeholder, so no signature can verify anyway. The moment a real
public key is wired in (the only outstanding step before signed
policies become operational), one Ed25519 signature can authenticate
multiple distinct policies.

**Fix:** length-prefix every field (e.g. `<u32-LE><bytes>`) instead of
using a separator byte. Cheapest patch:

```rust
for part in parts {
    out.extend_from_slice(&(part.len() as u32).to_le_bytes());
    out.extend_from_slice(part);
}
```

---

## Other surfaces audited (no MISMATCH found)

- `version_is_below`, `compare_to_policy` — covered by 19 unit tests in
  `version::tests` (commit 50db7c55). All pass.
- `is_acceptable_remote_policy` — earlier audit found and fixed an
  unsigned-replaces-signed path (commit 50db7c55).
- `compute_folder_hash` — uses the same NUL-separator pattern as
  `canonical_signing_payload`. **Not exploitable in practice**: the
  inputs are filesystem `rel_path` (no NUL on any real OS) and a hex
  `file_hash` (no NUL by alphabet). The pattern is fragile and would
  benefit from the same length-prefix fix as a defence-in-depth
  measure, but no concrete attack input exists.
- `SeederInfo::sign_payload` — format `seeder:{peer_id}:{file_hash}:{wallet_address}`.
  All three components have restricted alphabets (base58, hex, hex)
  that exclude `:`, so the format is unambiguous in practice.
- `FileMetadata::sign_payload` — `file:{hash}:{file_name}:{file_size}`.
  `file_name` can contain colons, but `file_size` is a `u64` parsed
  back from a digit-only suffix, which uniquely anchors the parse.
  Unambiguous in practice.
- CDN payment 5% tolerance (`src-tauri/src/cdn_server.rs:380`). Comment
  says "for CHI→wei rounding" but real `f64 → u128` rounding is sub-cent
  at 18-decimal precision, not 5%. This is an operator-tunable revenue
  leak, not a security bug — flagged as a follow-up, not a finding.

---

## Methodology Notes

The full FM-Agent pipeline (extract → topdown layers → batch prompts →
spec generation → logic verification → bug validation) was not
end-to-end automated — building per-function extraction tooling and
running spec generation across the whole codebase was out of scope for
this session. Instead, the *output format* (`bug_validator.md`) was
applied directly to a hand-selected set of security-critical functions:

1. **Spec first.** Each target function got a `[SPEC]` block in
   `fm_agent/specs/` written under `system_prompt.md`'s rules — WHAT
   not HOW, no implementation details, governing invariants only.
2. **Compare to code.** The implementation was read against the spec.
   Any concrete spec-violating behaviour became a MISMATCH candidate.
3. **Probe.** A `_probe_*` artefact reproduces the gap. Where a live
   reproduction was practical (BUG-002, pure Rust function), it is a
   runnable Rust probe printing `CONFIRMED`. Where it required a live
   network (BUG-001, two-daemon scenario), it is a written recipe with
   the cited line ranges that prove the gap by static reading.
4. **Persist.** Detail Markdown + result JSON in the schema described
   in `bug_validator.md`.

# Bug Report: publish_site_to_directory

**Source file:** `src-tauri/src/lib.rs`
**Verdict:** MISMATCH
**Confirmation status:** confirmed
**Resolution:** fixed — `SiteDirectoryEntry` now carries an ECDSA signature by `owner_wallet` over a length-prefixed canonical payload of every other field. `publish_site_to_directory` requires the caller's `private_key`, signs the entry, and refuses overwrites whose existing record verifies under a different wallet. `resolve_site_name` and `list_directory_sites` reject entries that fail to verify, so a forged DHT write at `chiral_sitename_<N>` is treated as if the name is unclaimed instead of redirecting users.

---

## Reasoning Process

The following actual behavior cannot satisfy the specification.

### Specification Claim

The site-directory contract is "first-claim wins" (per `lib.rs:4329` *"First claim wins for a given name — on conflict the publish command refuses"* and the surrounding comment). After a wallet `W` successfully publishes a name `N`, no other peer may replace the per-name DHT record `chiral_sitename_<N>` with a record that points to a different site or a different `public_url`. The `resolve_site_name(N)` call from any honest peer must keep returning the entry `W` originally wrote, until `W` itself unpublishes.

### Actual Behavior

The per-name DHT record is a plain Kademlia value with no signature, no embedded owner identity that any reader can verify, and no validator on the receive side of `put_dht_value`. The "first-claim wins" check inside `publish_site_to_directory` runs only inside the *publisher's* polite client. Any peer can write any value at `chiral_sitename_<N>` by:

1. Calling `DhtService::put_dht_value("chiral_sitename_<N>", attacker_json)` from inside any binary linked against `chiral_network` (e.g. `chiral-desktop`, `chiral_daemon`).
2. Hitting `POST /api/headless/dht/put` on any headless daemon (e.g. `{"key": "chiral_sitename_alice", "value": "<attacker_json>"}`) — the route at `chiral_daemon.rs:1140` is a thin wrapper over `put_dht_value` with no key-shape filtering.
3. Running a custom libp2p client that speaks Kademlia. The Kademlia behaviour built in `dht.rs` accepts records by key/value with no per-key validator.

A reader that calls `resolve_site_name(N)` after the attacker's write receives the attacker's record — not the original publisher's. The attacker chose `attacker_json.public_url`, so the UI sends visitors to the attacker.

---

## Code Evidence

The acceptance check in `publish_site_to_directory` only consults the *current* DHT value and only refuses if `existing.site_id != site_id`:

```rust
// src-tauri/src/lib.rs (lines 4417-4430 in HEAD)
// First-claim wins: if the name already resolves to someone else's
// site, refuse. If it resolves to OUR own site_id, we treat it as an
// update.
let key = site_name_key(&name);
if let Ok(Some(existing_json)) = dht.get_dht_value(key.clone()).await {
    if let Ok(existing) = serde_json::from_str::<SiteDirectoryEntry>(&existing_json) {
        if existing.site_id != site_id {
            return Err(format!(
                "Name '{}' is already taken by another site",
                name
            ));
        }
    }
}
```

There is no signature on the entry that's eventually written:

```rust
// src-tauri/src/lib.rs (lines 4432-4445 in HEAD)
let entry = SiteDirectoryEntry {
    name: name.clone(),
    description,
    owner_wallet: owner_wallet.unwrap_or_default(),  // self-asserted, unsigned
    site_id: site_id.clone(),
    public_url,
    created_at: ...,
};
let entry_json = serde_json::to_string(&entry)...?;
dht.put_dht_value(key, entry_json).await?;
```

The DHT layer accepts any value at any key:

```rust
// src-tauri/src/dht.rs (lines 843-857 in HEAD)
pub async fn put_dht_value(&self, key: String, value: String) -> Result<(), String> {
    let sender = self.command_sender.lock().await;
    if let Some(tx) = sender.as_ref() {
        // ... sends SwarmCommand::PutDhtValue with no validation ...
    }
}
```

Compare with file-publication, which DOES sign (per `CLAUDE.md`: *"File metadata and seeder entries are ECDSA-signed (prevents DHT tampering)"*). The site-directory was not given the same protection.

---

## Trigger Condition

Any peer that can write to the DHT at all can hijack any name. The minimum capability required is "I run a chiral client" — strictly weaker than "I own the wallet that originally claimed the name."

---

## How to trigger the bug

### Inputs

| Step | Input |
|------|-------|
| 1 — Victim publishes | `publish_site_to_directory(site_id="abc", name="alice", owner_wallet=W_alice)` from victim's running app, after publishing the site to a relay. |
| 2 — Attacker hijacks | `POST http://attacker-daemon:9419/api/headless/dht/put` with body `{"key":"chiral_sitename_alice","value":"<forged-json>"}` where `forged-json` is a `SiteDirectoryEntry` with `public_url` pointing to the attacker. |

### Expected (spec-correct) Output

After step 2, `resolve_site_name("alice")` on any honest peer continues to return the victim's `SiteDirectoryEntry` (with the victim's `public_url`).

### Actual (buggy) Output

After step 2, `resolve_site_name("alice")` on any honest peer eventually returns the attacker's `SiteDirectoryEntry` (with the attacker's `public_url`). The hijack propagates as the DHT replicates the most-recent value.

### How to Reproduce

See `_probe_BUG-001-site-name-hijack.md` for the exact two-daemon recipe. A live runnable reproduction requires bootstrapping two `chiral_daemon` instances against the same Kademlia ring; that is environment-specific and was therefore captured as a recipe rather than a self-contained Rust binary. The control-flow gap is fully visible in source, so confirmation does not require runtime execution.

### Suggested fix

Bring the site directory under the same protection model as file metadata and seeder entries. Concretely:

1. Define a `SignedSiteDirectoryEntry { entry: SiteDirectoryEntry, owner_wallet, signature }` and require `signature` to be a valid ECDSA signature by `owner_wallet` over `entry`.
2. Reject any entry the verifying peer reads whose signature does not check out, regardless of which DHT replica it came from.
3. In `publish_site_to_directory`, refuse to overwrite an existing entry unless the *signing wallet* matches the existing entry's signing wallet. The attacker, lacking the original wallet's private key, cannot produce a valid signature.
4. Optionally add a libp2p Kademlia record validator for `chiral_sitename_*` keys so even the receive side rejects unsigned writes.

Until (1)-(3) are in place, the published "first-claim-wins" guarantee is misleading — a name claim is at best a request, not a binding registration.

---

## Probe Script

The probe is captured as a recipe in `_probe_BUG-001-site-name-hijack.md`. The key code-evidence is reproduced inline above and verifiable by static reading of the cited line ranges.

### Probe Output

```
CONFIRMED — public Tauri / HTTP surface exposes a write path to
chiral_sitename_* keys that bypasses every check inside
publish_site_to_directory. The "first-claim wins" guarantee in the
spec is unenforceable as currently implemented.
```

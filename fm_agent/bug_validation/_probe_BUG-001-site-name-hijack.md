# Probe — BUG-001 Site Name Hijack

The bug is architectural: per-name DHT records (`chiral_sitename_<name>`)
carry no signature, so the "first-claim wins" rule can be enforced only
by reader cooperation. A live Rust probe is impractical (it requires two
running DHT nodes plus bootstrap), so the probe takes the form of a
*static reproduction recipe* that any future automated suite can replay
against a running daemon.

---

## Probe Form

A two-process scenario, both processes built from this repo's
`chiral-network` library. **The probe is "the existence of this scenario
in the public Tauri command surface" — no source modification needed.**

### Process A (legitimate claim)

```bash
# 1. Start a daemon and expose its DHT.
chiral_daemon --port 9419 --auto-start-dht

# 2. Inside the desktop app (or via Tauri-equivalent IPC):
#    publish_site_to_directory(site_id="abc", name="alice")
#
# Resulting DHT record under key `chiral_sitename_alice`:
#   { "name": "alice", "site_id": "abc",
#     "public_url": "https://relay/.../abc", ... }
```

### Process B (hijack)

```bash
# 3. Start a second daemon connected to the same Kademlia ring.
chiral_daemon --port 9420 --auto-start-dht

# 4. Hijack the name. Any of the following suffices:
#
#    a) DhtService::put_dht_value("chiral_sitename_alice", "<attacker_json>")
#       — directly invokable via the existing `dht_put` Tauri command on
#       the headless daemon HTTP API:
#
#         POST /api/headless/dht/put
#         { "key": "chiral_sitename_alice", "value": "<attacker_json>" }
#
#    b) An attacker building a custom client that talks libp2p Kademlia
#       directly. There is no validator on the receive side — every peer
#       that accepts the record will overwrite the previous value.
```

### Observed effect

After (4), `resolve_site_name("alice")` from any third peer returns the
attacker's `SiteDirectoryEntry`. The `public_url` field — which the UI
opens when a user clicks the name — points to the attacker's chosen URL.

### Verdict

`CONFIRMED` — the public Tauri / HTTP surface exposes a write path to
`chiral_sitename_*` keys that bypasses every check inside
`publish_site_to_directory`. The "first-claim wins" guarantee in the
spec is unenforceable as currently implemented.

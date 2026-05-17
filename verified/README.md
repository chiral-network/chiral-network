# Verified core

Formal-verification companion to a small set of security-critical
pure functions in `src-tauri/src/`. Files in this directory are
checked by [Verus](https://verus-lang.github.io), **not** by `cargo`.

The aim is narrow: take properties that today are example-based
unit tests (`#[test] fn allows_…`, `#[test] fn rejects_…`) and
upgrade them to universal statements (`forall|m: Seq<char>|
… ==> …`). The Rust impl stays where it lives; this directory
just adds an SMT-checked statement about it.

## What's verified

| File | Backs | Property |
|---|---|---|
| `method_allowlist.rs` | `src-tauri/src/chain_rpc_api.rs::is_allowed_method` | For all method names starting with `miner_`/`personal_`/`debug_`/`admin_`, the allowlist returns false. (Closes the bug class behind the 2026-05-10 `.173` incident.) |

## Running the proofs

The toolchain isn't on `cargo`'s path; it's a separate driver.

```bash
# One-time setup (Linux x86_64 — adjust URL for other platforms)
mkdir -p ~/.verus && cd ~/.verus
curl -sSL -o verus.zip \
  https://github.com/verus-lang/verus/releases/latest/download/verus-x86-linux.zip
unzip -q -o verus.zip
rustup toolchain install 1.95.0-x86_64-unknown-linux-gnu --profile minimal

# Check a proof file
~/.verus/verus-x86-linux/verus verified/method_allowlist.rs
```

Expected output:

```
verification results:: 8 verified, 0 errors
```

If a proof fails to verify, Verus prints the failing `requires`
clause and the line where the assert blew up. Reading those is
roughly like reading a typechecker error — the chain of obligations
points at what to fix.

## When to add a proof here

Good candidates:

- The function is **pure** (no async, no I/O, no external crates
  beyond `std`'s data types).
- A bug in the function is **security-relevant or money-relevant**
  (auth payload encoding, allowlists, wei↔CHI math, signing payloads).
- Example-based tests already exist but feel incomplete — you've
  been writing test cases by hand and worrying about the ones you
  forgot.

Bad candidates:

- Anything `async`, anything touching `tokio::*`, anything that
  reads a file or talks to a socket.
- Anything in `dht.rs` / `lib.rs` / `cdn_server.rs` /
  `hosting_server.rs` — these are integration code; the bugs there
  aren't algorithmic.
- Anything imported from a non-verified third-party crate (Verus
  can't see inside `secp256k1`, `tiny-keccak`, `reqwest`, etc.).

## Why this isn't in CI yet

Verus needs its own toolchain (~30 MB binary + a pinned rustc
version) that the project's existing `cargo check` / `cargo test`
infrastructure doesn't share. Running the proofs in CI is doable —
add a separate job that installs Verus and calls it — but worth
delaying until the verified surface is large enough to justify the
build-time tax. For now: run locally before touching anything in
this directory, and ensure the verified result is `0 errors`.

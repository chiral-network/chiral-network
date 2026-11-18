## Why

The read-only whitepaper (`docs/chiral-book.md`) says on line 644 that "The standalone RPC gateway has its own inbound bearer and persistent journal, distinct from engine/admin credentials". Line 656 places "Caddy TLS ingress in front of … the bounded authenticated chain RPC gateway (`127.0.0.1:8547`)", and line 667 says it "exposes an exact `eth_*` method allowlist, not arbitrary node administration". Line 474 requires RPC credentials to be separate protected files and never command-line values.

The code did not follow these lines. `chiral_rpc_gateway` required `--bearer-file` only for a non-loopback bind and authorized every request when no bearer was given (`docs/apas-worker/comparison-2026-11-18.md` item 7). Yet the generated Caddyfile forwards the public RPC domain's `/api/chain/rpc` to exactly that loopback port, so a gateway started without the flag served the chain RPC to anyone through TLS ingress. The allowlist also contained `net_version`, which is not an `eth_*` method, and the provider preflight checked only the gateway's health probes, so it would have passed such a gateway.

No specification covers the gateway. Its transport bounds are specified in the pending `specify-funding-settlement`, but its credential, `Origin` refusal, allowlist, journal and probes exist only as prose and code, so a later change could weaken them without a failing requirement.

## What Changes

- Add the `chain-rpc-gateway` capability with 8 requirements and 25 scenarios. Each requirement cites its whitepaper lines. Numeric values that the book does not state are recorded in `design.md` as current implementation values; the 32-character bearer minimum and the method table are normative.
- Specify that the gateway requires its own protected inbound bearer file on every bind address, loopback included, with no development exception. The file is opened without following a symbolic link, must be owner-only, and must hold at least 32 visible ASCII characters that differ from the upstream RPC bearer. Every refusal happens before the gateway listens or contacts the chain, and stderr never shows a bearer.
- Specify that each `POST /api/chain/rpc` is authenticated from its headers before the body is read, that refusals are HTTP 401, consume no admission budget and never reach the chain, and that the inbound bearer is never forwarded upstream. Every response of that route, refusals included, carries `Cache-Control: no-store`.
- Specify that a request carrying an `Origin` header is refused even with the valid bearer.
- Specify the exact allowlist of ten `eth_*` methods with fixed parameter shapes, without `net_version`, and the envelope, batch and chain-ID rules for submissions.
- Specify that the gateway's transport follows `inference-funding-settlement` *Chain RPC transport is bounded*, with whole-request refusal for batch, aggregate-size and admission limits.
- Specify network-identity checks before listening and before each forwarded call, the persistent network-scoped journal under `--journal-dir`, and that the gateway holds no signing key.
- Specify the unauthenticated `/healthz` and `/readyz` probes and that the reference ingress forwards only `/api/chain/rpc`.
- Specify that provisioning keeps the inbound and upstream bearers separate protected files and that preflight fails unless an unauthenticated RPC request answers 401.
- Fix the code (tasks 2.1–2.5):
  - `chiral_rpc_gateway` requires `--bearer-file` on every bind address, validates it as a protected file of at least 32 visible ASCII characters, refuses one equal to the upstream bearer, and adds `no-store` to its 401;
  - the allowlist drops `net_version`, and the Verus companion is aligned;
  - `scripts/provider-preflight.py` posts an unauthenticated JSON-RPC request to the gateway and requires 401;
  - `scripts/provision-provider.py` refuses an inbound RPC bearer equal to the upstream bearer before creating the deployment.
- Record a coverage table, seven code differences (all fixed), two open questions for the whitepaper author and the risks in `design.md`.

## Capabilities

### New Capabilities

- `chain-rpc-gateway`: The standalone `chiral_rpc_gateway`: its protected inbound bearer on every bind address, header-only authentication and `Origin` refusal, the exact `eth_*` allowlist with fixed parameter shapes, bounded transport and admission, network-identity checks and persistent journal, unauthenticated local probes, and how provisioning and preflight keep it authenticated.

### Modified Capabilities

None. The spec references the pending `inference-funding-settlement` (*Chain RPC transport is bounded*) by name and adds no obligation to it. The design relates it to the pending `profile-path-resolution`, which owns where the gateway's configuration paths resolve.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code:
  - `src-tauri/src/bin/chiral_rpc_gateway.rs`: `--bearer-file` is required; `inbound_bearer`, `upstream_bearer` and `no_store` are new;
  - `src-tauri/src/chain_rpc_api.rs`: `allowed_method` drops `net_version`;
  - `scripts/provider-preflight.py`: `check_rpc_gateway_bearer`;
  - `scripts/provision-provider.py`: the upstream bearer is read before the destination is created and must differ from the inbound bearer;
  - `verified/method_allowlist.rs` and `verified/README.md`: the companion's method set;
  - `docs/provider-qualification.md`: the preflight description.
- Unchanged, because they already pass `--bearer-file`: `docker-compose.yml`, the `Dockerfile` and the guide's native gateway command.
- Tests:
  - `src-tauri/tests/chain_rpc_gateway_test.rs`: three process-level tests of the gateway binary against a loopback upstream fixture;
  - `chain_rpc_api::tests::allowlist_is_exactly_ten_eth_methods`;
  - `scripts/test_provider_preflight.py`: three tests;
  - `scripts/test_provision_provider.py`: three tests.
  Each new test fails when its check is removed.
- Missing evidence is tracked as unchecked tasks. Deployment evidence through a real TLS ingress is task 4.1.
- An operator who started the gateway on loopback without `--bearer-file`, against every documented command, now gets a startup refusal.
- No wire format, ledger schema, deployment or whitepaper change is part of this step. It grants no live-chain, public-testnet or release qualification.

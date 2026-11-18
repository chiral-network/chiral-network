# Verification companions

This directory contains a narrow [Verus](https://verus-lang.github.io) specification companion to the LLM runtime's security-critical pure logic. It is **not compiled or proved by Cargo**, and its presence does not mean the Rust implementation, chain protocol, billing ledger, or running provider has been formally verified.

## Current scope

| File | Runtime correspondence | Stated properties |
|---|---|---|
| `method_allowlist.rs` | `src-tauri/src/chain_rpc_api.rs::allowed_method` | Exact membership in an explicit set of settlement RPC methods; unlisted methods rejected; selected chain-operator/admin/unsigned-send methods excluded |

The specification lists `eth_chainId`, `eth_blockNumber`, `eth_gasPrice`, `eth_getBalance`, `eth_getTransactionCount`, `eth_getTransactionReceipt`, `eth_sendRawTransaction`, `eth_syncing`, `eth_getBlockByNumber` and `eth_estimateGas`. This is **not** a wildcard permission for `eth_*`; `net_version` and the obsolete chiral-coin methods are not allowed. The runtime additionally validates parameters and applies transport/body/admission boundaries; those are outside this companion's proof scope.

The companion must remain aligned with the runtime manually. A proof of its specification alone does not establish equivalence to the deployed Rust function or validate a public RPC ingress. The former README's expected `8 verified` result referred to older proof contents and is not evidence for the current file.

## Running the companion

Install a pinned, mutually compatible Verus release and its required Rust toolchain following upstream instructions, record their versions, then run from the repository root:

```bash
/path/to/verus verified/method_allowlist.rs
```

No current Verus execution or verification count is recorded by this migration documentation update. Capture the actual tool output and versions when qualifying it; do not copy a historical expected count into a release report. Cargo tests and the application CI's Rust checks do not run this command.

## Appropriate future proof scope

Pure canonical-encoding, exact CFX/wei arithmetic, and allowlist predicates are useful candidates when their specification and runtime correspondence can be maintained. Async engine/network code, filesystem persistence, SQLite transactions, and third-party cryptography require other evidence; a companion cannot see through `reqwest`, `secp256k1`, or a live chain node.

The migration plan's [release checklist](../docs/llm-serving-migration-plan.md#14-execution-evidence-and-open-release-checklist) remains authoritative for exercised integration/security requirements. A proof of a small allowlist property does not close those gates.

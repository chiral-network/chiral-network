# Test Suite

This repository uses Vitest for frontend/unit tests and Rust `cargo test` for backend tests.

## Run Frontend Tests

From repo root:

```bash
npm test
npm run test:watch
npm run test:coverage
```

## Run Backend Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

## Run Live DHT/Relay Tests

The relay circuit integration tests are opt-in because they start live libp2p
swarms and exercise relay reservations/circuits. Normal backend test runs keep
these tests deterministic by skipping the live portions unless this flag is set:

```bash
CHIRAL_RUN_LIVE_DHT_TESTS=1 cargo test --manifest-path src-tauri/Cargo.toml --test relay_circuit_test -- --nocapture
```

## Run Live Load Tests

Live relay/chain load suites are opt-in because they require reachable relay,
gateway, chain, or local cluster services. Normal CI keeps these deterministic
by skipping live suites unless the explicit flag is set:

```bash
CHIRAL_RUN_LIVE_LOAD_TESTS=1 npx vitest run tests/load/relay-server.test.ts
CHIRAL_RUN_LIVE_LOAD_TESTS=1 npx vitest run tests/load/gateway-drive.test.ts
CHIRAL_RUN_LIVE_LOAD_TESTS=1 ./scripts/full-feature-test.sh
```

The scaled large-file transfer phase is also opt-in because it allocates and
transfers a large test file. Run it through the scaled-test harness so the
phase has the `/tests` container paths and node list it expects:

```bash
CHIRAL_RUN_LARGE_FILE_TESTS=1 ./scripts/scaled-test.sh 10
```

## Run Everything

```bash
npm test && cargo test --manifest-path src-tauri/Cargo.toml
```

## Notes

- Frontend setup/mocks are in `tests/setup.ts`.
- Path alias `$lib` is configured in `vitest.config.ts`.
- Rust tests are colocated in source modules under `src-tauri/src/` and integration tests under `src-tauri/tests/`.

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

## Run Everything

```bash
npm test && cargo test --manifest-path src-tauri/Cargo.toml
```

## Notes

- Frontend setup/mocks are in `tests/setup.ts`.
- Path alias `$lib` is configured in `vitest.config.ts`.
- Rust tests are colocated in source modules under `src-tauri/src/` and integration tests under `src-tauri/tests/`.

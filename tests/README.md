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

## Run Everything

```bash
npm test && cargo test --manifest-path src-tauri/Cargo.toml
```

## Notes

- Frontend setup/mocks are in `tests/setup.ts`.
- Path alias `$lib` is configured in `vitest.config.ts`.
- Rust tests are colocated in source modules under `src-tauri/src/` and integration tests under `src-tauri/tests/`.

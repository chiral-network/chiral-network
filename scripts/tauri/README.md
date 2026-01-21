# tauri webdriver

this uses tauri-driver to drive the desktop app

## install tauri-driver

```
cargo install tauri-driver --locked
```

## run driver

```
tauri-driver --port 4444
```

## run pool flow

```
TAURI_APP_PATH=src-tauri/target/debug/chiral-network \
TAURI_DRIVER_URL=http://127.0.0.1:4444 \
npm run test:tauri:pool
```

screenshots are saved in `artifacts/`

## pool url override

by default the script uses the driver port as a reachable tcp endpoint
you can override it with:

```
CHIRAL_POOL_URL=stratum+tcp://127.0.0.1:4444 npm run test:tauri:pool
```

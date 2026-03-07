# Headless Mode Roadmap

## Goal
Run Chiral Network as a command-line-only system with feature parity to the desktop app.

## Current Milestone (M1)
This branch introduces the first working headless foundation:

- `chiral` CLI command tree for all feature groups.
- `chiral_daemon` long-running process.
- Headless gateway startup (Drive API + Rating API + Hosting routes).
- Implemented CLI commands:
  - `chiral daemon start|stop|status`
  - `chiral settings get|set|reset|path`
  - `chiral network bootstrap|status`
  - `chiral reputation show|batch`
  - `chiral drive ls|mkdir|star|unstar|delete`
  - `chiral diagnostics report`
- Remaining command groups are scaffolded and return explicit "not implemented in milestone 1".

## Planned Parity Map

| Feature Area | CLI Group | Status |
|---|---|---|
| Wallet lifecycle | `chiral wallet ...` | Scaffolded |
| Account / transactions | `chiral account ...` | Scaffolded |
| Network / DHT operations | `chiral network ...`, `chiral dht ...` | Partial |
| Download flows | `chiral download ...` | Scaffolded |
| Drive storage + sharing | `chiral drive ...` | Partial |
| ChiralDrop transfers | `chiral drop ...` | Scaffolded |
| Hosting server/site relay | `chiral hosting ...` | Scaffolded |
| Hosting marketplace | `chiral market ...` | Scaffolded |
| Mining controls | `chiral mining ...` | Scaffolded |
| Geth operations | `chiral geth ...` | Scaffolded |
| Reputation (Elo) | `chiral reputation ...` | Partial |
| Diagnostics | `chiral diagnostics ...` | Partial |
| Runtime process management | `chiral daemon ...` | Implemented |

## Next Milestones

### M2: Core Runtime Extraction
- Extract Tauri-independent runtime state builder from `src-tauri/src/lib.rs`.
- Add headless event bus and replace direct Tauri-only emits where needed.
- Expose DHT start/stop and peer events from daemon.

### M3: Feature Implementations
- Implement `wallet`, `account`, `geth`, `mining`, `dht`, `download`.
- Implement `drive`, `hosting`, `market`, `drop` command handlers against daemon API.

### M4: Stability and Ops
- PID/lock management hardening.
- Structured JSON output mode for all commands.
- Integration tests for end-to-end headless workflows.
- Documentation and migration guide from GUI workflows to CLI workflows.

# Proposal: Transfer Log for Sent and Received Files

## Overview
- Add a transfer log that records all files sent and received across Chiral Network features (uploads, downloads, P2P, WebRTC, relay/proxy).
- Provide a user-facing toggle to show/hide the log on the Network page without changing underlying collection.
- Design for minimal performance impact and clear privacy boundaries.

## Goals
- Track file transfers with essential metadata for troubleshooting, auditing, and user transparency.
- Support filtering, search, and simple summaries (counts, totals) in the UI.
- Persist log entries locally with safe bounded retention.
- Provide a settings flag `showTransferLogOnNetwork` for display control on the Network page.

## Data Model
TransferLogEntry
- id: string (UUID)
- direction: 'sent' | 'received'
- protocol: 'http' | 'p2p' | 'webrtc' | 'relay' | 'unknown'
- fileId?: string
- name?: string
- size?: number
- hash?: string | string[] (CID or content hash)
- peer?: string (peerId or address)
- startedAt: number (epoch ms)
- completedAt?: number (epoch ms)
- status: 'in_progress' | 'completed' | 'failed' | 'canceled'
- error?: string

Indexing
- Time-based index for range queries
- Optional secondary index by `direction` for quick filters

## Collection Points
- Upload path: when initiating and completing uploads (src/pages/Upload.svelte and related helpers/services)
- Download path: when initiating searches/downloads and when completing (src/pages/Download.svelte, TorrentDownload.svelte, download helpers/services)
- P2P/WebRTC transfers: session setup/teardown and on message/file-channel completion (src/lib/services/webrtcService.ts, dht/proxy paths)
- Relay/Proxy: when proxying a file transfer both for send/receive (src/lib/proxy.ts, proxyAuth.ts)

Each collection point emits an event (e.g., `logTransfer({ ...entry })`) that appends to a central store.

## Persistence
- Local persistence (Tauri: file-based JSONL or SQLite via plugin; Web: IndexedDB or localStorage fallback)
- Bounded retention by time (e.g., 30/90 days) and size (max entries or MiB)
- On startup: load persisted entries into memory window (e.g., last 1000 entries) for fast UI
- Background compaction/rotation to control size

## Privacy & Security
- Avoid sensitive content by default (no file contents; hash/name optional based on privacy mode)
- Respect `anonymousMode` and `ipPrivacyMode`; redact peer identifiers when strict
- Allow a “Clear Log” action with confirmation; support selective deletion in future
- Do not transmit logs externally unless the user opts into analytics (when enabled, only send aggregated non-PII metrics)

## UI/UX
- Network page: optional “Transfer Log” card
  - Columns: Time, Direction, Name/ID, Size, Peer, Protocol, Status
  - Filters: Direction, Protocol, Status; free-text search on name/hash/peer
  - Actions: Clear log, export CSV (optional follow-up)
- Empty state: helpful text + link to enable collection points or simulate activity

## Settings
- Add `showTransferLogOnNetwork: boolean` to `AppSettings` in `src/lib/stores.ts` (default: false)
- Toggle checkbox in Network page controls to show/hide the card
- Persist as part of existing settings model

## Performance
- Use append-only buffered writes; batch flush to disk
- Render virtualized list for large data sets
- Truncate in-memory view (e.g., latest N) to keep UI responsive

## Rollout Plan
Milestone 0 — Design (current)
- Land proposal and interfaces in docs.

Milestone 1 — Core Log API
- Implement in-memory store and `logTransfer(entry)` append API.
- Add basic TypeScript types and unit tests for the store behavior.

Milestone 2 — Persistence + Retention
- Add persistence (Tauri: file-based JSONL or SQLite; Web: IndexedDB fallback).
- Implement bounded retention (time- and size-based) and background compaction.

Milestone 3 — Collection Wiring
- Emit log events from upload, download, WebRTC, and proxy paths.
- Verify end-to-end entries with a small QA checklist.

Milestone 4 — UI (Network Page)
- Add an optional Transfer Log card to the Network page.
- Provide a settings flag to show/hide the log (defaults to hidden) and a Clear Log action.

Milestone 5 — Enhancements (optional)
- Filters/search, virtualization for large lists, CSV export.
- Privacy controls (redaction in strict modes) and opt-in aggregated analytics.

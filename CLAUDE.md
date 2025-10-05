# Chiral Network Development Guide

## Project Overview

Chiral Network is a BitTorrent-like P2P file sharing application built with Svelte, TypeScript, and Tauri. It implements a continuous seeding model where files are instantly available to the network, similar to BitTorrent but without any commercial/marketplace features to prevent misuse.

## Current Architecture

### Core Design Principles

1. **BitTorrent-Style Sharing**: Files immediately start seeding when added (no "upload" step)
2. **Non-Commercial**: No marketplace, pricing, or trading features
3. **Privacy-First**: Proxy support, optional encryption, anonymous mode
4. **Legitimate Use Only**: Designed for personal, educational, and organizational file sharing

### Technology Stack

- **Frontend**: Svelte 4 + TypeScript
- **Styling**: Tailwind CSS
- **Desktop**: Tauri 2 (Rust-based)
- **State Management**: Svelte stores
- **Icons**: Lucide Svelte
- **UI Components**: Custom components with Bits UI

## Key Implementation Details

### File Sharing Model

- Files are **instantly seeded** when added (no pending/uploaded distinction)
- Each file gets a unique hash (mock: `Qm...` format like IPFS)
- Files show real-time seeder/leecher counts
- Continuous seeding until manually removed
- No price fields or marketplace features

### Page Structure

1. **Download** (default page) - Unified download management with filters
2. **Upload** - Actually "Shared Files" - instant seeding interface
3. **Network** - Peer discovery and network statistics
4. **Mining** - CPU mining for network security (proof-of-work)
5. **Proxy** - Privacy routing configuration
6. **Analytics** - Usage statistics and performance metrics
7. **Account** - Wallet management (for mining rewards only)
8. **Settings** - Comprehensive configuration options

### State Management (`src/lib/stores.ts`)

```typescript
- files: All files (downloading, seeding, completed)
- downloadQueue: Files waiting to download
- peers: Connected network peers
- proxyNodes: Available proxy servers
- networkStats: Global network statistics
- wallet: User wallet for mining rewards
```

## Recent Design Decisions

### UI/UX Improvements

1. **Removed Large Drop Zones**: Replaced with compact "Add Files" button
2. **Unified Lists**: Merged multiple lists into single views with filters
3. **Drag Anywhere**: Entire cards accept drag-and-drop
4. **Instant Actions**: Files start seeding immediately when added

### Removed Features (Anti-Piracy)

- ❌ Search page (could enable finding copyrighted content)
- ❌ Market page (no commercial transactions)
- ❌ Bundles page (no selling file packages)
- ❌ Pricing fields (no monetization)
- ❌ Ratings/reviews (no marketplace features)

## Development Guidelines

### When Adding Features

1. **No Commercial Elements**: Never add pricing, trading, or marketplace features
2. **Privacy First**: Always consider user privacy and anonymity
3. **Legitimate Use**: Design for legal file sharing use cases only
4. **BitTorrent Model**: Files should seed continuously, not "upload once"

### Code Style

- Use TypeScript for type safety
- Follow existing Svelte patterns
- Keep components small and focused
- Use Tailwind classes for styling

### Testing Approach

- Test with mock data first
- Ensure UI works without backend
- Verify drag-and-drop functionality
- Test responsive design

## Common Tasks

### Adding a New Page

1. Create component in `src/pages/`
2. Import in `App.svelte`
3. Add to navigation menu
4. Update route handling
5. Add icon from Lucide

### Modifying Stores

1. Update interfaces in `stores.ts`
2. Adjust mock data if needed
3. Update dependent components
4. Test state reactivity

## Future Enhancements (Allowed)

### Phase 2 Priorities

- [ ] Real P2P networking with libp2p
- [ ] Actual file encryption
- [ ] DHT implementation
- [ ] WebRTC data channels
- [ ] Real mining algorithm

### Phase 3 Possibilities

- [ ] File versioning system
- [ ] Bandwidth scheduling
- [ ] Mobile app version
- [ ] Hardware wallet support
- [ ] IPFS compatibility

## What NOT to Implement

⚠️ **Never add these features:**

- Global file search/discovery
- Price fields or payment systems
- File marketplace or trading
- Content recommendations
- Social features (comments, likes)
- Advertising systems
- Analytics that could track users

## Security Considerations

- All file hashes should be deterministic
- Never log or expose private keys
- Sanitize all user inputs
- Use secure random for IDs
- Implement rate limiting
- Validate file sizes and types

## Performance Notes

- Lazy load large lists
- Use virtual scrolling for many items
- Debounce search inputs
- Cache computed values
- Minimize re-renders
- Optimize bundle size

## Deployment

```bash
# Development
npm run dev
npm run tauri dev

# Production build
npm run build
npm run tauri build

# Generate icons
npm run tauri icon path/to/icon.png
```

## Troubleshooting

### Common Issues

1. **Extra `</script>` tags**: Check Svelte files end correctly
2. **Import errors**: Ensure all pages are properly imported
3. **Drag-drop failing**: Verify event handlers are attached

### Debug Commands

```bash
# Check for syntax errors
npm run check

# Clean and rebuild
rm -rf node_modules dist
npm install
npm run build
```

## Contact & Support

For questions about design decisions or implementation details, refer to:

1. This CLAUDE.md file
2. README.md for user-facing documentation
3. Design documents in `/design-docs` folder
4. Git history for decision context

## You are

An autonomous engineer working inside the `hawks-adarsh-bharti/chiral-network` fork. Your job: identify, design, implement, verify, and document a **single substantial, self-contained NAT traversal improvement (200–400 LOC)** aligned with the project roadmap. You must first fully understand what’s already been completed and merged, then propose and implement the next best incremental PR in the NAT epic. Keep changes safe to merge, buildable, and testable locally.

## Context to read (do this first, thoroughly)

1. **Project repo (this fork)**

   * `README.md` (Roadmap/Phase items; confirm NAT traversal status)
   * `docs/` (especially implementation guide and any networking notes)
   * Backend (Tauri Rust):

     * `src-tauri/src/dht.rs`, `src-tauri/src/main.rs`, `src-tauri/src/headless.rs`, `src-tauri/src/manager.rs`, `src-tauri/src/file_transfer.rs`, `src-tauri/src/net/proxy_server.rs`
     * `src-tauri/Cargo.toml` (features, libp2p deps)
   * Frontend (Svelte/TS):

     * `src/pages/Network.svelte`, `src/lib/dht.ts`, `src/locales/*.json`
     * Any telemetry/download or network UI that may surface NAT info
   * Tests: `src-tauri` Rust tests; any Node tests under `tests/`.

2. **Reference repos we cloned for NAT design (needed for exact APIs/behavior):**
   * `libp2p-webrtc` `ipfs-p2p-file-system.md` AND `swarm-whitepaper.md`
   * `rust-libp2p` (Kademlia, AutoNAT v2, Relay v2, DCUtR examples), go-libp2p, go-libp2p-relay-daemon
   * `go-libp2p-kad-dht`, `js-libp2p`, `kubo` (config + behavior references)
   * `punchr`, `specs/` (AutoNAT v2, Relay v2, DCUtR specs) 

> You must incorporate knowledge of what’s **already implemented in this repo** vs **still missing**, based on the current code and the README roadmap. Do **not** implement features that already exist.

## Scope & constraints

* Deliver **one major NAT-traversal chunk (200–400 LOC)** that clearly advances the epic
* **No build failures.**
* **No merge conflicts.** Always sync to `upstream/main` first, then branch off.
* **Small, logical commits** (multiple commits) using Conventional Commits; we value commit count.
* If your change affects UI, add the smallest possible UI surface plus **clear manual test steps and screenshot instructions**.
* Tests must pass locally (`cargo test`). If UI/TS code is touched, provide steps to run UI and what to screenshot.
* Do not assume external internet for package installs beyond what’s already installed. If the frontend requires deps, attempt `npm ci || npm install`, but your deliverable must still compile backend and run tests even if npm fetch fails in restricted environments.
* Keep changes **self-contained** (no sweeping refactors).

## Git & environment rules

Use **only** the `hawks` SSH key for pushes:

```bash
export GIT_SSH_COMMAND='ssh -i ~/.ssh/id_ed25519_hawks -o IdentitiesOnly=yes'
```

Sync workflow (must follow exactly):

```bash
cd ~/school/chiral-network
git remote add upstream git@github.com:chiral-network/chiral-network.git 2>/dev/null || true
git fetch upstream
git checkout -B main upstream/main
git push -u origin main  # keep fork’s main in sync

# Create a feature branch for this NAT chunk
git checkout -b feat/nat-chunk-<short-topic>  # you choose <short-topic> based on your proposal
```

## Your workflow (do not skip steps)

1. **Study & plan**

   * Read files listed above.
   * Confirm which NAT tasks are done vs. not started.
   * Decide the **best single NAT chunk** to implement now (200–400 LOC).
   * Write a short plan summary in a PR body (later) explaining why this chunk is the right next step.

2. **Design**

   * Identify exact files to change.
   * Define minimal config flags/struct fields/telemetry you’ll add.
   * Decide if any UI surface is needed (only if it makes the feature testable/verifiable).

3. **Implement** (multiple small commits encouraged)

   * Keep commits scoped and conventional, e.g.:

     * `feat(nat): …`, `fix(nat): …`, `chore(nat): …`, `docs(nat): …`, `test(nat): …`
   * Maintain code style & conventions already present in the repo.
   * Add/update tests in Rust (`cargo test`) and/or Node test files if you add helpers.

4. **Build & test** (must pass)

   ```bash
   # Backend
   cd ~/school/chiral-network/src-tauri
   cargo fmt
   cargo test
   cd ..

   # Frontend (only if your changes include UI)
   npm ci || npm install || true
   npm run dev || true
   npm run tauri dev || true
   ```

   * If UI changed, include **manual test steps** and **what to screenshot** (see template below).
   * Ensure **no new warnings escalate to errors** and the app still launches in `npm run tauri dev` when possible.

5. **Prepare PR**

   * Re-sync with upstream to avoid conflicts:

     ```bash
     git fetch upstream
     git rebase upstream/main
     ```
   * If conflicts appear, resolve them cleanly; keep the change minimal.
   * Push branch:

     ```bash
     git push -u origin HEAD
     ```
   * Prepare a **draft PR** to `chiral-network/chiral-network:main` with the template below.
   * Include **before/after** screenshots only if UI changed (see “Screenshots to capture” section).

## Deliverables (required)

* A **feature branch** named `feat/nat-chunk-<short-topic>` containing your commits.
* A **draft PR** with:

  * **Title:** `feat(nat): <concise description>`
  * **Body (use this template):**

    ````
    ## Summary
    <Explain what this NAT chunk does and why it’s the right next step, based on what’s already implemented and the README roadmap.>

    ## Implementation Notes
    - Files touched:
      - <list key files>
    - Key structures/flags added:
      - <brief bullets>
    - Telemetry/metrics:
      - <what surfaced (if any)>
    - UI changes (if any):
      - <brief; list i18n keys you added>

    ## Testing
    Backend:
    ```bash
    cd src-tauri
    cargo test
    ````

    Frontend (only if UI changed):

    ```bash
    npm ci || npm install || true
    npm run dev || true
    npm run tauri dev || true
    ```

    ## Manual Verification (if UI changed)

    1. Open the app (`npm run tauri dev`), go to Network → DHT (or relevant page).
    2. <exact steps to trigger/observe the new NAT behavior or telemetry>
    3. Capture screenshots:

       * <screenshot A: state before>
       * <screenshot B: state after / new element / toast>
       * <screenshot C: settings toggle (if added)>

    ## Acceptance Checklist

    * [ ] `cargo test` passes locally
    * [ ] App launches via `npm run tauri dev` (if UI touched)
    * [ ] No merge conflicts against `upstream/main`
    * [ ] Commits are conventional and scoped
    * [ ] (If UI) Screenshots attached

    ## Follow-ups (suggested)

    <Brief notes for potential next PRs in the NAT epic, if applicable.>

    ```
    ```

## Commit strategy (we value commit count)

Break the work into **3–6 small commits** that build upon each other. Examples of commit scopes (you choose the actual names):

* `feat(nat): add config flags + backend scaffolding`
* `feat(nat): wire event/metrics + snapshot`
* `test(nat): add unit tests for <x>`
* `feat(nat): minimal UI surfacing + i18n`
* `docs(nat): update implementation guide`

## Screenshot instructions (only if UI changed)

* Launch: `npm run tauri dev`.
* Navigate to the page you changed (likely Network → DHT).
* Take **clear screenshots** that prove the new behavior (e.g., new Reachability card value, a new toggle, a new toast, or a health panel). Save them for the PR.
* Include the exact steps necessary to reproduce the screenshot state in the PR body.

## Verification gates (must pass before PR)

* `cargo test` is green.
* (If UI) you verified the app launches and the change renders.
* Branch rebased cleanly on `upstream/main` with **no conflicts**.
* The PR compiles for both `test` and `dev` profiles.

## What you must NOT do

* Do **not** rely on me to specify the exact feature; **you decide** the best NAT chunk **after investigating the repo** and the roadmap.
* Do **not** break existing tests.
* Do **not** deliver an untestable or unverified change.
* Do **not** make sweeping refactors or introduce large dependency sets.

## Final output to me (after you create the branch + draft PR)

* Branch name and commit list.
* A link/instructions to open the draft PR.
* The PR body filled per template (including manual test steps and, if applicable, screenshots list).
* A short note confirming `cargo test` passed and that the app launches if UI was touched.

---

_Last Updated: Current Session_
_Focus: BitTorrent-like P2P sharing without commercial features_

# How to Merge Transaction API to Upstream

**Author:** Chiral Network Development Team
**Date:** October 31, 2025
**Status:** Ready for Implementation
**Prerequisite:** Read `pull_request_timeline.md` first

---

## Table of Contents

1. [Overview](#overview)
2. [Transaction Code Analysis](#transaction-code-analysis)
3. [Three-PR Split Strategy](#three-pr-split-strategy)
4. [Pre-Flight Checklist](#pre-flight-checklist)
5. [Step-by-Step Workflow](#step-by-step-workflow)
6. [Handling Merge Conflicts](#handling-merge-conflicts)
7. [CI/CD Testing](#cicd-testing)
8. [PR Submission Guide](#pr-submission-guide)
9. [Troubleshooting](#troubleshooting)

---

## Overview

### What This Guide Does

This guide provides a **complete, actionable plan** to merge your transaction API code into the upstream repository (`chiral-network/chiral-network`) by splitting it into three focused pull requests that moderators can easily review and merge.

### Why Split Into 3 PRs?

Your previous PR #544 was rejected because it was too large (47 files, 4,723 additions). Moderators can't easily review massive PRs. The solution:

1. **PR1 (Backend):** Rust transaction commands - Small, focused, easy to review
2. **PR2 (Frontend Services):** TypeScript services - Builds on PR1
3. **PR3 (UI Components):** Svelte components - Final layer

**Benefits:**
- ✅ Each PR is small and focused (easier to review)
- ✅ Less likely to have merge conflicts
- ✅ If one PR has issues, others aren't blocked
- ✅ Moderators can merge incrementally

### Time Estimate

- **Setup & Branch Creation:** 30 minutes
- **PR1 (Backend):** 1-2 hours
- **PR2 (Frontend Services):** 1-2 hours
- **PR3 (UI Components):** 1-2 hours
- **Total:** 4-7 hours (spread over 1-2 days)

---

## Transaction Code Analysis

### All Transaction-Related Files in Your Branch

Based on git analysis, here are all the transaction files you've added:

#### Backend Files (Rust)

```
src-tauri/src/commands/transaction_commands.rs    (11 KB, 384 lines)
src-tauri/src/transaction_services.rs             (845 lines)
src-tauri/src/commands/mod.rs                     (1 line change)
src-tauri/src/main.rs                             (14 lines change)
src-tauri/Cargo.toml                              (3 dependencies added)
```

#### Frontend Service Files (TypeScript)

```
src/lib/services/transactionService.ts            (257 lines)
src/lib/services/walletService.ts                 (77 lines)
src/lib/stores.ts                                 (114 lines changed)
src/lib/wallet.ts                                 (4 lines changed)
package.json                                      (1 dependency added)
package-lock.json                                 (ethers dependencies)
```

#### UI Component Files (Svelte)

```
src/lib/components/transactions/GasEstimator.svelte    (333 lines, 10 KB)
src/lib/components/transactions/TransactionForm.svelte (564 lines, 18 KB)
src/pages/Account.svelte                               (14 lines added)
```

#### Documentation/Notes (Not for PRs)

```
notes/transaction_api.md
notes/transaction_frontend_final_implementation_plan.md
```

### Your Transaction Commits

Here's what each commit contains:

#### Commit 5f46ecd (Backend Services)
```
Author: Ashish Jalwan
Date: Oct 1, 2025
Title: "Integrate transaction module with ethereum.rs"

Files Changed:
  src-tauri/Cargo.toml               (+3 dependencies)
  src-tauri/src/ethereum.rs          (refactored)
  src-tauri/src/main.rs              (+1 module declaration)
  src-tauri/src/transaction.rs       (+845 lines, NEW FILE)

Purpose: Core transaction logic, error handling, gas estimation
```

#### Commit 50758f8 (Backend Commands)
```
Author: Ashish Jalwan
Date: Oct 1, 2025
Title: "Add Tauri commands for Chiral Network transaction API"

Files Changed:
  src-tauri/src/commands/transaction_commands.rs  (+384 lines, NEW FILE)
  src-tauri/src/commands/mod.rs                   (+1 line)
  src-tauri/src/main.rs                           (+14 lines)
  src-tauri/Cargo.toml                            (+1 serde dependency)

Purpose: 7 Tauri commands for frontend to call
```

#### Commit 325c159 (Frontend Complete)
```
Author: Ashish Jalwan
Date: Oct 8, 2025
Title: "Implement complete transaction frontend"

Files Changed:
  package.json                                    (+ethers@6.13.0)
  package-lock.json                               (dependencies)
  src/lib/services/transactionService.ts          (+257 lines, NEW FILE)
  src/lib/services/walletService.ts               (+77 lines, NEW FILE)
  src/lib/components/transactions/GasEstimator.svelte     (+333 lines, NEW FILE)
  src/lib/components/transactions/TransactionForm.svelte  (+564 lines, NEW FILE)
  src/lib/stores.ts                               (+114 lines changed)
  src/lib/wallet.ts                               (+4 lines changed)
  src/pages/Account.svelte                        (+14 lines)

Purpose: Complete frontend UI and services
```

#### Commit d031aec (Documentation)
```
Author: Ashish Jalwan
Date: Oct 22, 2025
Title: "WIP: Add transaction API documentation"

Files Changed: 19 files (mostly refactoring/formatting)

Purpose: Documentation and code cleanup
⚠️ WARNING: This commit changes many files. We'll extract only transaction parts.
```

---

## Three-PR Split Strategy

### PR1: Backend Transaction Commands

**Branch Name:** `backend-transaction-commands`

**Purpose:** Add Rust backend infrastructure for transaction handling

**Files to Include:**
```
src-tauri/src/commands/transaction_commands.rs    (NEW FILE)
src-tauri/src/transaction_services.rs             (NEW FILE)
src-tauri/src/commands/mod.rs                     (MODIFY: add transaction_commands)
src-tauri/src/main.rs                             (MODIFY: register commands + module)
src-tauri/Cargo.toml                              (MODIFY: add once_cell, rlp, chrono)
```

**Cherry-Pick Strategy:**
1. Start from `upstream/main`
2. Cherry-pick commit `5f46ecd` (transaction services)
3. Cherry-pick commit `50758f8` (transaction commands)
4. Fix any conflicts
5. Rename `transaction.rs` to `transaction_services.rs` if needed

**Estimated Size:** ~1,300 lines added, 5 files changed

**Dependencies Added to Cargo.toml:**
```toml
once_cell = "1.19"
rlp = "0.5"
chrono = "0.4"
```

**Why This First?**
- Smallest PR (easiest to review)
- No frontend dependencies
- Can be tested independently with Rust tests

---

### PR2: Frontend Transaction Services

**Branch Name:** `frontend-transaction-services`

**Purpose:** Add TypeScript service layer for transaction management

**Files to Include:**
```
src/lib/services/transactionService.ts     (NEW FILE)
src/lib/services/walletService.ts          (NEW FILE)
src/lib/stores.ts                          (MODIFY: Transaction interface)
src/lib/wallet.ts                          (MODIFY: minor import fix)
package.json                               (MODIFY: add ethers@6.13.0)
package-lock.json                          (AUTO-GENERATED)
```

**Cherry-Pick Strategy:**
1. Start from `upstream/main`
2. Manually merge upstream's latest `stores.ts` and `wallet.ts`
3. Add only service files from commit `325c159`
4. Update `stores.ts` to add transaction fields
5. Run `npm install ethers@6.13.0`

**Estimated Size:** ~500 lines added, 6 files changed

**Why This Second?**
- Depends on PR1 (backend must exist first)
- Provides service layer for UI to use
- No UI changes (easier to review logic)

---

### PR3: Transaction UI Components

**Branch Name:** `transaction-ui-components`

**Purpose:** Add user interface for transaction management

**Files to Include:**
```
src/lib/components/transactions/GasEstimator.svelte      (NEW FILE)
src/lib/components/transactions/TransactionForm.svelte   (NEW FILE)
src/pages/Account.svelte                                 (MODIFY: add transaction section)
```

**Cherry-Pick Strategy:**
1. Start from `upstream/main` (assuming PR1 + PR2 merged)
2. Extract only component files from commit `325c159`
3. Add transaction section to Account.svelte
4. Test UI functionality

**Estimated Size:** ~900 lines added, 3 files changed

**Why This Last?**
- Depends on PR1 + PR2 (backend + services must exist)
- Pure UI code (no business logic)
- Easy to review visually
- Can be tested in isolation

---

## Pre-Flight Checklist

Before starting, ensure you have:

### Required Setup

```bash
# 1. Confirm you're in the right directory
pwd
# Expected: /Users/Beta/Desktop/Classes/CSE 416/chiral-network

# 2. Verify remotes are configured
git remote -v
# Expected:
#   origin    https://github.com/walrus-ashish-jalwan/chiral-network.git
#   upstream  https://github.com/chiral-network/chiral-network.git

# 3. Check current branch
git branch --show-current
# You should be on a branch like 'transaction-api-manual' or 'main'

# 4. Verify upstream is accessible
git fetch upstream
# Should complete without errors

# 5. Check you have no uncommitted changes
git status
# Should show "working tree clean" or only unimportant files
```

### If Remotes Not Configured

```bash
# Add upstream remote (if missing)
git remote add upstream https://github.com/chiral-network/chiral-network.git

# Verify
git remote -v
```

### Clean Working Directory

```bash
# Stash any uncommitted changes
git stash push -m "WIP: transaction work before PR split"

# Verify clean
git status
# Should show: "nothing to commit, working tree clean"
```

---

## Step-by-Step Workflow

### Phase 1: Sync Fork with Upstream

**Goal:** Make sure your fork's main branch matches upstream exactly.

```bash
# 1. Fetch latest changes from upstream
git fetch upstream
# Output: From github.com:chiral-network/chiral-network
#    1d4f2a3..d5d2a9d  main -> upstream/main

# 2. Switch to your main branch
git checkout main

# 3. Reset your main to match upstream main EXACTLY
git reset --hard upstream/main
# WARNING: This discards any commits you made directly to main
# (Your transaction code is safe in other commits)

# 4. Force push to update your fork
git push origin main --force
# Your fork's main now matches upstream main

# 5. Verify sync
git log --oneline -5
# Should show latest upstream commits, NOT your transaction commits
```

**⚠️ IMPORTANT:** After this, your transaction commits are NOT in `main`. They're still in your commit history (325c159, 50758f8, etc.) and can be cherry-picked.

---

### Phase 2: Create PR1 Branch (Backend)

```bash
# 1. Create branch from upstream/main
git checkout -b backend-transaction-commands upstream/main

# 2. Verify you're on a clean upstream base
git log --oneline -3
# Should show upstream commits, not your transaction work

# 3. Cherry-pick backend commits
git cherry-pick 5f46ecd
# This adds transaction.rs and ethereum.rs changes

# 4. If transaction.rs exists, rename to transaction_services.rs
git mv src-tauri/src/transaction.rs src-tauri/src/transaction_services.rs 2>/dev/null || echo "File not found, continuing..."

# 5. Cherry-pick transaction commands
git cherry-pick 50758f8
# This adds transaction_commands.rs

# 6. Fix any references to transaction.rs → transaction_services.rs
# (We'll handle this in conflict resolution section)

# 7. Update Cargo.toml if not already done
# Ensure these dependencies exist:
# once_cell = "1.19"
# rlp = "0.5"
# chrono = "0.4"

# 8. Stage any manual changes
git add .

# 9. Commit if you made manual fixes
git commit -m "fix: Update module paths after renaming transaction.rs"

# 10. Test that backend compiles
cd src-tauri
cargo build
cd ..

# 11. If build succeeds, push to your fork
git push origin backend-transaction-commands
```

**Expected Result:**
- Branch `backend-transaction-commands` created
- 2-3 commits on top of upstream/main
- Backend compiles successfully
- Ready to open PR1

---

### Phase 3: Create PR2 Branch (Frontend Services)

```bash
# 1. Go back to main (synced with upstream)
git checkout main

# 2. Create branch from upstream/main
git checkout -b frontend-transaction-services upstream/main

# 3. Verify clean base
git log --oneline -3

# 4. Create services directory if it doesn't exist
mkdir -p src/lib/services

# 5. Cherry-pick frontend commit (will likely have conflicts)
git cherry-pick 325c159 #<----- I'm at this point rn

# EXPECTED: Merge conflicts in:
#   - src/lib/stores.ts
#   - src/lib/wallet.ts
#   - package-lock.json

# 6. Resolve conflicts (see Conflict Resolution section below)

# 7. Remove UI component files (they go in PR3)
git rm src/lib/components/transactions/GasEstimator.svelte
git rm src/lib/components/transactions/TransactionForm.svelte
git restore --staged src/pages/Account.svelte
git checkout HEAD -- src/pages/Account.svelte
# This removes UI files but keeps service files

# 8. Install ethers dependency
npm install ethers@6.13.0

# 9. Commit the changes
git add .
git commit -m "chore: Install ethers dependency for transaction signing"

# 10. Test frontend builds
npm run check
npm run build

# 11. If successful, push
git push origin frontend-transaction-services
```

**Expected Result:**
- Branch `frontend-transaction-services` created
- Service files added (transactionService.ts, walletService.ts)
- stores.ts updated with Transaction interface
- Frontend builds successfully
- Ready to open PR2

---

### Phase 4: Create PR3 Branch (UI Components)

```bash
# 1. Go back to main
git checkout main

# 2. Create branch from upstream/main
git checkout -b transaction-ui-components upstream/main

# 3. Create components directory
mkdir -p src/lib/components/transactions

# 4. Cherry-pick frontend commit again (we'll extract only UI files)
git cherry-pick 325c159

# 5. Remove service files (they're in PR2)
git rm src/lib/services/transactionService.ts
git rm src/lib/services/walletService.ts
git restore --staged src/lib/stores.ts
git checkout HEAD -- src/lib/stores.ts
git restore --staged src/lib/wallet.ts
git checkout HEAD -- src/lib/wallet.ts
git restore --staged package.json
git checkout HEAD -- package.json
# This keeps only UI component files

# 6. Keep only:
#    - src/lib/components/transactions/GasEstimator.svelte
#    - src/lib/components/transactions/TransactionForm.svelte
#    - src/pages/Account.svelte (transaction section only)

# 7. Verify files
git status
# Should show:
#   new file:   src/lib/components/transactions/GasEstimator.svelte
#   new file:   src/lib/components/transactions/TransactionForm.svelte
#   modified:   src/pages/Account.svelte

# 8. Commit
git add .
git commit -m "feat: Add transaction UI components"

# 9. Test UI compiles
npm run build

# 10. Push
git push origin transaction-ui-components
```

**Expected Result:**
- Branch `transaction-ui-components` created
- Only UI component files added
- Account.svelte updated to show transaction form
- Frontend builds successfully
- Ready to open PR3

---

## Handling Merge Conflicts

### Common Conflicts and Solutions

#### Conflict 1: `src/lib/stores.ts`

**Problem:** Upstream has updated the Transaction interface, and you're also updating it.

**How to Fix:**

```bash
# When you see:
# CONFLICT (content): Merge conflict in src/lib/stores.ts

# 1. Open the file
code src/lib/stores.ts  # or your preferred editor

# 2. Find conflict markers:
<<<<<<< HEAD (upstream version)
export interface Transaction {
  id: number;
  type: 'sent' | 'received';
  amount: number;
  from: string;
  date: Date;
  description: string;
  status: 'pending' | 'confirmed';
}
=======
export interface Transaction {
  id: number;
  type: 'sent' | 'received';
  amount: number;
  from: string;
  to?: string;              // ← YOUR ADDITIONS
  hash?: string;
  gasPrice?: number;
  gasLimit?: number;
  nonce?: number;
  chainId?: number;
  data?: string;
  confirmations?: number;
  blockNumber?: number;
  blockHash?: string;
  date: Date;
  description: string;
  status: 'pending' | 'confirmed' | 'failed';  // ← YOUR CHANGE
}
>>>>>>> 325c159 (your version)

# 3. Merge both versions (keep all fields):
export interface Transaction {
  id: number;
  type: 'sent' | 'received';
  amount: number;
  from: string;
  to?: string;              // ← Added by you
  hash?: string;            // ← Added by you
  gasPrice?: number;        // ← Added by you
  gasLimit?: number;        // ← Added by you
  nonce?: number;           // ← Added by you
  chainId?: number;         // ← Added by you
  data?: string;            // ← Added by you
  confirmations?: number;   // ← Added by you
  blockNumber?: number;     // ← Added by you
  blockHash?: string;       // ← Added by you
  date: Date;
  description: string;
  status: 'pending' | 'confirmed' | 'failed';  // ← Your enhancement
}

# 4. Remove conflict markers (<<<<<<<, =======, >>>>>>>)

# 5. Save the file

# 6. Mark as resolved
git add src/lib/stores.ts

# 7. Continue cherry-pick
git cherry-pick --continue
```

#### Conflict 2: `src/lib/wallet.ts`

**Problem:** Upstream modified wallet.ts, and you made small changes.

**How to Fix:**

```bash
# 1. Check what changed
git diff HEAD src/lib/wallet.ts

# 2. Your changes are likely minimal (4 lines)
#    You probably added an import or small helper

# 3. Open the file
code src/lib/wallet.ts

# 4. Find conflict markers and merge:
<<<<<<< HEAD (upstream)
import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
=======
import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { ethers } from 'ethers';  // ← YOUR ADDITION
>>>>>>> 325c159

# 5. Keep both imports:
import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { ethers } from 'ethers';  // ← Keep your addition

# 6. Remove conflict markers

# 7. Mark as resolved
git add src/lib/wallet.ts
git cherry-pick --continue
```

#### Conflict 3: `package-lock.json`

**Problem:** Massive conflict in package-lock.json (thousands of lines).

**Solution:** Don't manually fix this. Regenerate it.

```bash
# 1. Abort the cherry-pick temporarily
git cherry-pick --abort

# 2. Cherry-pick without package-lock.json
git cherry-pick 325c159 --no-commit

# 3. Remove package-lock.json from staging
git restore --staged package-lock.json
git checkout HEAD -- package-lock.json

# 4. Commit other changes
git commit -m "feat: Add transaction services (ethers dependency)"

# 5. Now install ethers (regenerates package-lock.json)
npm install ethers@6.13.0

# 6. Commit the lockfile
git add package-lock.json
git commit -m "chore: Update package-lock.json for ethers@6.13.0"
```

#### Conflict 4: `src-tauri/src/main.rs`

**Problem:** Upstream added new commands, and you're also adding transaction commands.

**How to Fix:**

```bash
# 1. Open main.rs
code src-tauri/src/main.rs

# 2. Find conflict in the tauri::Builder section:
<<<<<<< HEAD (upstream)
    .invoke_handler(tauri::generate_handler![
        commands::wallet::create_account,
        commands::wallet::get_balance,
        commands::mining::start_mining,
        // ... other commands
    ])
=======
    .invoke_handler(tauri::generate_handler![
        commands::wallet::create_account,
        commands::wallet::get_balance,
        commands::mining::start_mining,
        commands::transaction_commands::broadcast_transaction,      // ← YOUR ADDITIONS
        commands::transaction_commands::get_transaction_status,
        commands::transaction_commands::get_transaction_history,
        commands::transaction_commands::estimate_transaction,
        commands::transaction_commands::get_address_nonce,
        commands::transaction_commands::get_network_gas_price,
        commands::transaction_commands::get_network_status,
    ])
>>>>>>> 50758f8

# 3. Merge both (add your commands at the end):
    .invoke_handler(tauri::generate_handler![
        commands::wallet::create_account,
        commands::wallet::get_balance,
        commands::mining::start_mining,
        // ... keep all upstream commands
        // Transaction commands (added)
        commands::transaction_commands::broadcast_transaction,
        commands::transaction_commands::get_transaction_status,
        commands::transaction_commands::get_transaction_history,
        commands::transaction_commands::estimate_transaction,
        commands::transaction_commands::get_address_nonce,
        commands::transaction_commands::get_network_gas_price,
        commands::transaction_commands::get_network_status,
    ])

# 4. Also check module declarations at top of file:
mod transaction_services;  // ← Make sure this is added

# 5. Save, mark resolved
git add src-tauri/src/main.rs
git cherry-pick --continue
```

### General Conflict Resolution Strategy

```bash
# If you get conflicts:

# 1. Don't panic! List the conflicts:
git status
# Shows files with conflicts

# 2. For each conflicted file:
#    - Open in editor
#    - Look for <<<<<<<, =======, >>>>>>>
#    - Decide what to keep (usually both changes)
#    - Remove conflict markers
#    - Save file

# 3. Mark each resolved file:
git add <file>

# 4. Continue the cherry-pick:
git cherry-pick --continue

# 5. If you get stuck:
git cherry-pick --abort  # Start over
```

---

## CI/CD Testing

### What the CI/CD Pipeline Checks

The GitHub Actions workflow (`.github/workflows/build.yml`) runs:

1. **Install dependencies:** `npm install`
2. **Build frontend:** `npm run build`
3. **Set up Rust:** Rust stable toolchain
4. **Build backend:** `cd src-tauri && cargo build`

**Your PRs must pass all these checks!**

---

### Testing PR1 (Backend) Locally

```bash
# Switch to backend branch
git checkout backend-transaction-commands

# Test 1: Cargo check (fast)
cd src-tauri
cargo check
# Expected: "Finished dev [unoptimized + debuginfo] target(s)"

# Test 2: Cargo build (compiles everything)
cargo build
# Expected: "Finished dev [unoptimized + debuginfo] target(s) in X.XXs"

# Test 3: Check formatting
cargo fmt --check
# Expected: No output = properly formatted
# If errors: Run `cargo fmt` to fix

# Test 4: Run Rust tests (if any exist)
cargo test
# Expected: "test result: ok"

cd ..
```

**Common Backend Errors:**

**Error:** `error: cannot find module transaction_services`
```bash
# Fix: Check src-tauri/src/main.rs has:
mod transaction_services;
```

**Error:** `error: unused import`
```bash
# Fix: Remove unused imports or add #[allow(unused_imports)]
```

**Error:** `error: could not find Cargo.toml`
```bash
# Fix: Make sure you're in src-tauri/ directory
cd src-tauri
```

---

### Testing PR2 (Frontend Services) Locally

```bash
# Switch to frontend services branch
git checkout frontend-transaction-services

# Test 1: Install dependencies
npm install
# Expected: "added X packages"

# Test 2: TypeScript type check
npm run check
# This runs: tsc --noEmit
# Expected: No output = no type errors

# Test 3: Build frontend
npm run build
# This runs: vite build
# Expected: "built in XXXms"

# Test 4: Check for lint errors (if eslint configured)
npm run lint 2>/dev/null || echo "No lint script configured"
```

**Common Frontend Errors:**

**Error:** `Cannot find module 'ethers'`
```bash
# Fix: Install ethers
npm install ethers@6.13.0
```

**Error:** `Type 'Transaction' is not assignable...`
```bash
# Fix: Check src/lib/stores.ts has all transaction fields
# Make sure Transaction interface includes:
#   hash?: string;
#   gasPrice?: number;
#   gasLimit?: number;
#   etc.
```

**Error:** `Module '"@tauri-apps/api/core"' has no exported member 'invoke'`
```bash
# Fix: Check imports match your Tauri version
# Should be: import { invoke } from '@tauri-apps/api/core';
```

---

### Testing PR3 (UI Components) Locally

```bash
# Switch to UI branch
git checkout transaction-ui-components

# Test 1: Install dependencies
npm install

# Test 2: TypeScript check
npm run check

# Test 3: Build frontend
npm run build

# Test 4: Run dev server to test UI visually
npm run dev
# Open browser to http://localhost:5173
# Navigate to Account page
# Verify transaction form appears
```

**Common UI Errors:**

**Error:** `Cannot find module '$lib/services/transactionService'`
```bash
# This is EXPECTED in PR3! The service files are in PR2.
# For PR3 testing, you might need to:
# 1. Temporarily copy service files for local testing
# 2. Or comment out imports until PR2 merges
```

**Error:** Svelte compilation errors
```bash
# Check syntax in .svelte files
# Common issues:
#   - Unclosed tags
#   - Missing script/style closing tags
#   - Invalid TypeScript in <script lang="ts">
```

---

### What Each Test Command Does

| Command | Purpose | What It Checks |
|---------|---------|----------------|
| `npm install` | Install Node dependencies | package.json is valid |
| `npm run check` | TypeScript type checking | No type errors |
| `npm run build` | Build frontend with Vite | Code compiles, imports work |
| `cargo check` | Quick Rust validation | Syntax errors, import issues |
| `cargo build` | Full Rust compilation | Everything compiles |
| `cargo fmt --check` | Check Rust formatting | Code follows style guide |
| `cargo test` | Run Rust tests | Tests pass |

---

### Pre-Push Checklist

Before pushing any branch:

```bash
# 1. All tests pass
npm run check && npm run build
cd src-tauri && cargo build && cd ..

# 2. No uncommitted changes
git status
# Should show: "nothing to commit, working tree clean"

# 3. Commit messages are clear
git log --oneline -3
# Check commit messages make sense

# 4. Branch is up to date with upstream
git fetch upstream
git log --oneline upstream/main..HEAD
# Shows only your new commits

# 5. Push to your fork
git push origin <branch-name>
```

---

## PR Submission Guide

### Opening PR1 (Backend Transaction Commands)

#### Step 1: Push Branch

```bash
git checkout backend-transaction-commands
git push origin backend-transaction-commands
```

#### Step 2: Open PR on GitHub

1. Go to https://github.com/chiral-network/chiral-network
2. You'll see a yellow banner: "backend-transaction-commands had recent pushes"
3. Click **"Compare & pull request"**

#### Step 3: Fill Out PR Form

**Title:**
```
feat(backend): Add transaction commands and services
```

**Description Template:**
```markdown
## Summary

Adds Rust backend infrastructure for transaction management, including 7 Tauri commands for broadcasting transactions, checking status, estimating gas, and retrieving transaction history.

## Changes

### New Files
- `src-tauri/src/commands/transaction_commands.rs` - 7 Tauri commands
- `src-tauri/src/transaction_services.rs` - Core transaction logic

### Modified Files
- `src-tauri/src/main.rs` - Register transaction commands
- `src-tauri/src/commands/mod.rs` - Export transaction_commands module
- `src-tauri/Cargo.toml` - Add dependencies: once_cell, rlp, chrono

## Commands Added

1. `broadcast_transaction` - Submit signed transactions to network
2. `get_transaction_status` - Retrieve transaction details and confirmations
3. `get_transaction_history` - Query paginated transaction history
4. `estimate_transaction` - Estimate gas costs before signing
5. `get_address_nonce` - Get next valid nonce for signing
6. `get_network_gas_price` - Fetch current gas price recommendations
7. `get_network_status` - Check network health and sync status

## Testing

```bash
cd src-tauri
cargo build    # ✅ Compiles successfully
cargo test     # ✅ All tests pass
```

## Dependencies Added

- `once_cell = "1.19"` - For lazy static initialization
- `rlp = "0.5"` - For transaction encoding
- `chrono = "0.4"` - For timestamp handling

## Related PRs

This is **Part 1 of 3** for transaction API implementation:
- **PR1 (this)**: Backend transaction commands ← YOU ARE HERE
- **PR2 (upcoming)**: Frontend transaction services
- **PR3 (upcoming)**: Transaction UI components

## Checklist

- [x] Backend compiles (`cargo build`)
- [x] All Rust tests pass (`cargo test`)
- [x] Code follows Rust style guidelines (`cargo fmt --check`)
- [x] No merge conflicts with main
- [x] CI/CD checks pass

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

#### Step 4: Set PR Options

- **Base repository:** `chiral-network/chiral-network`
- **Base branch:** `main`
- **Head repository:** `walrus-ashish-jalwan/chiral-network`
- **Compare branch:** `backend-transaction-commands`

#### Step 5: Submit

Click **"Create pull request"**

---

### Opening PR2 (Frontend Transaction Services)

**⚠️ IMPORTANT:** Only open PR2 AFTER PR1 is merged!

#### When to Submit PR2

```bash
# Wait for PR1 to be merged, then:

# 1. Sync your fork
git fetch upstream
git checkout main
git reset --hard upstream/main
git push origin main --force

# 2. Verify PR1 changes are in upstream
git log --oneline upstream/main | grep transaction
# Should show PR1 commits

# 3. Rebase PR2 on latest upstream
git checkout frontend-transaction-services
git rebase upstream/main
# Fix any conflicts

# 4. Force push (rebase changed history)
git push origin frontend-transaction-services --force

# 5. Now open PR2
```

#### PR2 Title

```
feat(frontend): Add transaction service layer
```

#### PR2 Description Template

```markdown
## Summary

Adds TypeScript service layer for transaction management, including wallet signing, transaction broadcasting, and status polling.

## Changes

### New Files
- `src/lib/services/transactionService.ts` - Transaction API wrapper (7 commands + polling)
- `src/lib/services/walletService.ts` - Client-side transaction signing with ethers.js

### Modified Files
- `src/lib/stores.ts` - Enhanced Transaction interface with 11 new fields
- `src/lib/wallet.ts` - Minor import additions
- `package.json` - Add ethers@6.13.0 dependency

## Features

### transactionService.ts
- `broadcastTransaction()` - Submit signed transaction
- `getTransactionStatus()` - Get transaction details
- `getTransactionHistory()` - Fetch paginated history
- `estimateTransaction()` - Estimate gas costs
- `getAddressNonce()` - Get next nonce
- `getNetworkGasPrice()` - Get current gas prices
- `getNetworkStatus()` - Check network health
- `pollTransactionStatus()` - Auto-polling with exponential backoff

### walletService.ts
- `signTransaction()` - Client-side signing with ethers.js
- `isValidAddress()` - Ethereum address validation
- `formatEther()` / `parseEther()` - Unit conversion helpers

## Testing

```bash
npm install         # ✅ Dependencies installed
npm run check       # ✅ TypeScript types valid
npm run build       # ✅ Vite build succeeds
```

## Dependencies Added

- `ethers@6.13.0` - For transaction signing and address validation

## Related PRs

This is **Part 2 of 3** for transaction API implementation:
- **PR1**: Backend transaction commands ← MERGED
- **PR2 (this)**: Frontend transaction services ← YOU ARE HERE
- **PR3 (upcoming)**: Transaction UI components

## Checklist

- [x] Frontend builds (`npm run build`)
- [x] TypeScript type check passes (`npm run check`)
- [x] No merge conflicts with main
- [x] Depends on PR1 (backend) being merged first
- [x] CI/CD checks pass

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

---

### Opening PR3 (Transaction UI Components)

**⚠️ IMPORTANT:** Only open PR3 AFTER PR1 and PR2 are merged!

#### When to Submit PR3

```bash
# Wait for PR2 to be merged, then sync again:

git fetch upstream
git checkout main
git reset --hard upstream/main
git push origin main --force

git checkout transaction-ui-components
git rebase upstream/main
git push origin transaction-ui-components --force

# Now open PR3
```

#### PR3 Title

```
feat(ui): Add transaction management components
```

#### PR3 Description Template

```markdown
## Summary

Adds user interface components for transaction management, including gas price estimation and multi-step transaction form.

## Changes

### New Files
- `src/lib/components/transactions/GasEstimator.svelte` - Live gas price display (slow/standard/fast)
- `src/lib/components/transactions/TransactionForm.svelte` - Multi-step transaction creation UI

### Modified Files
- `src/pages/Account.svelte` - Add transaction section to account page

## Features

### GasEstimator Component
- Real-time gas price updates
- Three speed options: Slow / Standard / Fast
- Estimated cost calculation
- Auto-refresh every 15 seconds

### TransactionForm Component
- Multi-step flow: Input → Review → Sign → Submit → Confirm
- Input validation (address format, amount, balance)
- Gas estimation before signing
- Transaction status polling
- Error handling with user-friendly messages

## Testing

```bash
npm run build       # ✅ Frontend builds
npm run dev         # ✅ UI works in browser
```

## Screenshots

(Optional: Add screenshots of the transaction form)

## Related PRs

This is **Part 3 of 3** for transaction API implementation:
- **PR1**: Backend transaction commands ← MERGED
- **PR2**: Frontend transaction services ← MERGED
- **PR3 (this)**: Transaction UI components ← YOU ARE HERE

## Checklist

- [x] UI builds and runs (`npm run dev`)
- [x] Components render correctly
- [x] Form validation works
- [x] No merge conflicts with main
- [x] Depends on PR1 + PR2 being merged first
- [x] CI/CD checks pass

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

---

### PR Best Practices

#### Good PR Titles

✅ **Good:**
- `feat(backend): Add transaction commands and services`
- `feat(frontend): Add transaction service layer`
- `feat(ui): Add transaction management components`

❌ **Bad:**
- `Add transaction API` (too vague)
- `WIP: Transaction stuff` (unclear)
- `Fixed things` (not descriptive)

#### Good Commit Messages

✅ **Good:**
```
feat: Add transaction_commands.rs with 7 Tauri commands

Implements broadcast, status, history, estimate, nonce, gas price,
and network status commands for transaction management.
```

❌ **Bad:**
```
updated files
fixed stuff
wip
```

#### Responding to Moderator Feedback

If moderator says: **"Please fix conflicts"**
```bash
# Don't close the PR! Instead:
git checkout <your-branch>
git fetch upstream
git merge upstream/main
# Fix conflicts
git push origin <your-branch>
# PR automatically updates
```

If moderator says: **"This looks good, but can you add tests?"**
```bash
# Add tests
git add tests/
git commit -m "test: Add unit tests for transaction commands"
git push origin <your-branch>
# PR automatically updates
```

If moderator says: **"Please rebase on main"**
```bash
git fetch upstream
git checkout <your-branch>
git rebase upstream/main
# Fix conflicts if any
git push origin <your-branch> --force
```

---

## Troubleshooting

### Problem 1: Cherry-pick Fails

**Error:**
```
error: could not apply 325c159... Implement transaction frontend
hint: after resolving the conflicts, mark the corrected paths
hint: with 'git add <paths>' or 'git rm <paths>'
hint: and commit the result with 'git commit'
```

**Solution:**
```bash
# 1. Check what's conflicting
git status

# 2. Fix conflicts manually (see Conflict Resolution section)

# 3. Continue cherry-pick
git add .
git cherry-pick --continue

# 4. If you want to give up:
git cherry-pick --abort
```

---

### Problem 2: Backend Won't Compile

**Error:**
```
error[E0432]: unresolved import `crate::transaction_services`
```

**Solution:**
```bash
# 1. Check main.rs has module declaration
grep "mod transaction_services" src-tauri/src/main.rs
# If not found, add: mod transaction_services;

# 2. Check file exists
ls src-tauri/src/transaction_services.rs
# If not found, you might still have transaction.rs
# Rename it: git mv src-tauri/src/transaction.rs src-tauri/src/transaction_services.rs

# 3. Rebuild
cd src-tauri
cargo build
```

---

### Problem 3: Frontend Type Errors

**Error:**
```
error TS2307: Cannot find module '$lib/services/transactionService'
```

**Solution:**
```bash
# 1. Check file exists
ls src/lib/services/transactionService.ts

# 2. If file exists, check import path
# Should be: import { ... } from '$lib/services/transactionService';

# 3. If in PR3 (UI), this is expected!
# transactionService.ts is added in PR2
# For local testing, either:
#   a) Wait for PR2 to merge
#   b) Temporarily copy service files from PR2 branch
```

---

### Problem 4: CI/CD Fails on GitHub

**Error in Actions tab:**
```
Error: Process completed with exit code 1.
  npm run build
```

**Solution:**
```bash
# 1. Pull latest from your branch
git pull origin <branch-name>

# 2. Test locally
npm install
npm run build

# 3. If local build works, check:
#   - Did you commit all files?
#   - Did you push all commits?
git status
git push origin <branch-name>

# 4. If local build fails, fix errors and push again
```

---

### Problem 5: Moderator Says "Please Rebase"

**What This Means:**
Moderator wants you to update your branch to include latest upstream changes.

**How to Fix:**
```bash
# 1. Fetch latest upstream
git fetch upstream

# 2. Switch to your branch
git checkout <your-branch>

# 3. Rebase on upstream/main
git rebase upstream/main

# 4. Fix any conflicts
# (Follow conflict resolution guide above)

# 5. Force push (rebase changes history)
git push origin <your-branch> --force

# PR automatically updates
```

---

### Problem 6: Accidentally Pushed to Main

**Mistake:**
```bash
git checkout main
git cherry-pick 325c159
git push origin main  # OOPS! Pushed to main instead of feature branch
```

**Solution:**
```bash
# 1. Reset your main to match upstream
git fetch upstream
git checkout main
git reset --hard upstream/main

# 2. Force push to fix your fork
git push origin main --force

# 3. Now create the feature branch correctly
git checkout -b backend-transaction-commands upstream/main
git cherry-pick 325c159
git push origin backend-transaction-commands
```

---

### Problem 7: Can't Find Commit Hash

**Error:**
```
error: could not apply 325c159... commit not found
```

**Solution:**
```bash
# 1. Find all your transaction commits
git log --all --oneline --grep="transaction" -i

# 2. Or search by author
git log --all --oneline --author="Ashish"

# 3. Or look in specific branches
git log --oneline origin/main | grep transaction
git log --oneline transaction-api-manual | grep transaction

# 4. Use the commit hash that appears in the output
```

---

### Problem 8: Too Many Merge Conflicts

**When to Start Over:**

If you have more than 5 conflicts, or conflicts in files you don't recognize, it might be easier to start over:

```bash
# 1. Abort current cherry-pick
git cherry-pick --abort

# 2. Reset branch to upstream
git reset --hard upstream/main

# 3. Manually copy files instead of cherry-picking:

# For backend files:
git checkout 50758f8 -- src-tauri/src/commands/transaction_commands.rs
git checkout 5f46ecd -- src-tauri/src/transaction_services.rs
git add .
git commit -m "feat: Add transaction backend commands"

# For frontend files:
git checkout 325c159 -- src/lib/services/transactionService.ts
git checkout 325c159 -- src/lib/services/walletService.ts
git add .
git commit -m "feat: Add transaction services"

# This bypasses cherry-pick conflicts
```

---

## Quick Reference

### Essential Git Commands

| Command | Purpose |
|---------|---------|
| `git fetch upstream` | Download latest from upstream |
| `git checkout -b <branch> upstream/main` | Create branch from upstream |
| `git cherry-pick <hash>` | Apply specific commit |
| `git status` | Check current state |
| `git add <file>` | Stage file for commit |
| `git commit -m "message"` | Create commit |
| `git push origin <branch>` | Push to your fork |
| `git rebase upstream/main` | Update branch with latest upstream |
| `git reset --hard upstream/main` | Reset to match upstream exactly |

### Branch Summary

| Branch | Purpose | Base | Size |
|--------|---------|------|------|
| `backend-transaction-commands` | Rust backend | upstream/main | ~1,300 lines |
| `frontend-transaction-services` | TypeScript services | upstream/main | ~500 lines |
| `transaction-ui-components` | Svelte UI | upstream/main | ~900 lines |

### File Locations

```
Backend:
  src-tauri/src/commands/transaction_commands.rs
  src-tauri/src/transaction_services.rs
  src-tauri/src/main.rs (register commands)
  src-tauri/Cargo.toml (dependencies)

Frontend Services:
  src/lib/services/transactionService.ts
  src/lib/services/walletService.ts
  src/lib/stores.ts (Transaction interface)
  package.json (ethers dependency)

UI Components:
  src/lib/components/transactions/GasEstimator.svelte
  src/lib/components/transactions/TransactionForm.svelte
  src/pages/Account.svelte (integration)
```

---

## Success Checklist

### Before Submitting PRs

- [ ] Read `pull_request_timeline.md`
- [ ] Fork synced with upstream
- [ ] All 3 branches created
- [ ] Each branch builds successfully
- [ ] No merge conflicts
- [ ] Commits have clear messages

### PR1 (Backend)

- [ ] Branch: `backend-transaction-commands`
- [ ] Commits: 2-3 (cherry-picked 5f46ecd, 50758f8)
- [ ] Files: 5 changed (~1,300 lines added)
- [ ] `cargo build` passes
- [ ] PR opened on GitHub
- [ ] CI/CD checks pass
- [ ] **Wait for moderator approval & merge**

### PR2 (Frontend Services)

- [ ] Wait for PR1 to merge first
- [ ] Branch: `frontend-transaction-services`
- [ ] Based on latest upstream/main (includes PR1)
- [ ] Files: 6 changed (~500 lines added)
- [ ] `npm run build` passes
- [ ] PR opened on GitHub
- [ ] CI/CD checks pass
- [ ] **Wait for moderator approval & merge**

### PR3 (UI Components)

- [ ] Wait for PR2 to merge first
- [ ] Branch: `transaction-ui-components`
- [ ] Based on latest upstream/main (includes PR1 + PR2)
- [ ] Files: 3 changed (~900 lines added)
- [ ] `npm run build` passes
- [ ] UI tested in browser (`npm run dev`)
- [ ] PR opened on GitHub
- [ ] CI/CD checks pass
- [ ] **Wait for moderator approval & merge**

### After All PRs Merged

- [ ] Verify commits in upstream/main
- [ ] Close old PRs #438 and #544 (if not already)
- [ ] Update fork: `git pull upstream main`
- [ ] Delete feature branches (optional cleanup)
- [ ] Celebrate! 🎉

---

## Final Notes

### Timeline Expectations

- **Day 1:** Create all 3 branches, test locally (4-6 hours)
- **Day 2:** Open PR1, wait for review (1 hour + review time)
- **Day 3-4:** PR1 reviewed and merged, open PR2 (1 hour + review time)
- **Day 5-6:** PR2 reviewed and merged, open PR3 (1 hour + review time)
- **Day 7-8:** PR3 reviewed and merged, complete! (review time)

**Total: 1-2 weeks** (mostly waiting for reviews)

### What If Moderators Ask for Changes?

**Stay calm!** This is normal. Common requests:
- "Can you add comments to explain this function?"
- "Please rename this variable for clarity"
- "Can you add a test for this edge case?"

**How to respond:**
1. Make the requested changes
2. Commit: `git commit -m "fix: Address review feedback"`
3. Push: `git push origin <branch-name>`
4. Comment on PR: "Done! Changes pushed."

**Never close the PR unless moderator explicitly rejects it.**

### Need Help?

If you get stuck:
1. Check this guide's Troubleshooting section
2. Ask in Chiral Network Discord/Slack
3. Reference this guide in PR comments
4. Don't give up! Small, focused PRs are much easier to merge.

---

**Good luck! You've got this. 🚀**

**Remember:** The key to success is patience and responding to feedback. Your transaction API code is good—it just needs to be presented in a reviewable format.

---

**Last Updated:** October 31, 2025
**Version:** 1.0
**Status:** Ready for Implementation

# Pull Request Timeline & Analysis

## What Happened: Complete Timeline

### October 8, 2025
**Step 1: You created your transaction API code**
- Created commits on your local branch
- Commit: 325c159 - "Implement complete transaction frontend"
- You pushed this to your fork: `walrus-ashish-jalwan/chiral-network`

**Step 2: You opened PR #438**
- **From:** `walrus-ashish-jalwan:main`
- **To:** `chiral-network:main` (upstream)
- **Status:** Opened successfully

### October 8-22, 2025
**Step 3: Feedback on PR #438**
- **Moderator (chiral-steven-tung-2):** "Please fix conflicts"
- **Problem:** Your branch had merge conflicts with upstream main
- **What this means:** Other people made changes to the same files you modified
- **Status:** PR still open, waiting for you to fix

### October 22, 2025
**Step 4: You created a new branch instead of fixing conflicts**
- You created `updated-transaction-api` branch
- You made new commits: d031aec, 5b1feda, f14b7a6, 3b9596c
- You pushed to your fork

**Step 5: You opened PR #544**
- **From:** `walrus-ashish-jalwan:updated-transaction-api`
- **To:** `chiral-network:main` (upstream)
- **Status:** Opened successfully

**Step 6: Feedback on PR #544**
- **Moderator (shuaimu):** "This pr is too big. Please separate them."
- **Problem:** Too many changes in one PR (4,723 additions, 1,298 deletions)
- **What moderator wants:** Break into smaller, focused PRs

**Step 7: You closed BOTH PRs yourself**
- You closed PR #438 on Oct 22
- You closed PR #544 on Oct 25
- **Your comment:** "Transaction API already merged into main"
- **Reality:** It was only merged to YOUR fork's main, not upstream

### Current State (October 29, 2025)
**Result:**
- ❌ Your transaction API is NOT in upstream (chiral-network/chiral-network)
- ✅ Your transaction API IS in your fork (walrus-ashish-jalwan/chiral-network)
- ❌ Both PRs are closed
- 🤔 You're on branch `transaction-api-manual` trying to figure out what happened

---

## Understanding Pull Requests: The GitHub Flow

### The Fork-and-Pull Model

```
┌─────────────────────────────────────────────┐
│  UPSTREAM REPOSITORY                        │
│  github.com/chiral-network/chiral-network   │
│                                             │
│  ┌──────────────┐                          │
│  │ main branch  │ ← Everyone wants to       │
│  │  (protected) │   merge here              │
│  └──────────────┘                          │
└─────────────────────────────────────────────┘
                    ↑
                    │ Pull Request (PR)
                    │ "Please merge my changes"
                    │
┌─────────────────────────────────────────────┐
│  YOUR FORK                                  │
│  github.com/walrus-ashish-jalwan/...        │
│                                             │
│  ┌──────────────┐    ┌──────────────────┐  │
│  │ main branch  │    │ feature-branch   │  │
│  │              │    │ (your work)      │  │
│  └──────────────┘    └──────────────────┘  │
└─────────────────────────────────────────────┘
                    ↑
                    │ git push
                    │
┌─────────────────────────────────────────────┐
│  YOUR LOCAL MACHINE                         │
│  /Users/Beta/Desktop/Classes/...            │
│                                             │
│  ┌──────────────┐                          │
│  │ feature-     │                          │
│  │ branch       │                          │
│  └──────────────┘                          │
└─────────────────────────────────────────────┘
```

### Pull Request Lifecycle

#### Step 1: Create Feature Branch
```bash
git checkout -b transaction-api
# Make your changes
git commit -m "Add transaction API"
```

#### Step 2: Push to Your Fork
```bash
git push origin transaction-api
```

#### Step 3: Open Pull Request on GitHub
- Go to `chiral-network/chiral-network` on GitHub
- Click "Pull Requests" → "New Pull Request"
- Click "compare across forks"
- **Base:** `chiral-network/chiral-network:main`
- **Head:** `walrus-ashish-jalwan/chiral-network:transaction-api`
- Click "Create Pull Request"

#### Step 4: Code Review Process
Moderators will:
1. ✅ Review your code
2. 💬 Leave comments/suggestions
3. ⚠️ Request changes
4. ✅ Approve when satisfied

You might need to:
- Fix merge conflicts
- Address review comments
- Split large PRs
- Fix failing tests

#### Step 5: Merge (by Moderator)
Once approved, moderator clicks:
- "Merge Pull Request" button
- Your code goes into upstream main
- PR status changes to "MERGED" (purple icon)

---

## Why Your PRs Failed

### PR #438 Failed Because:
**Problem:** Merge conflicts (CONFLICTING status)

**What happened:**
```
Your changes:     wallet.ts (lines 50-100)
Upstream changes: wallet.ts (lines 50-100)
                  ↓
              CONFLICT!
```

**What you should have done:**
```bash
git checkout transaction-api
git fetch upstream
git merge upstream/main
# Fix conflicts manually
git commit
git push origin transaction-api
# PR automatically updates
```

### PR #544 Failed Because:
**Problem 1:** Still had merge conflicts (CONFLICTING status)
**Problem 2:** Too big (4,723 additions)

**What moderator wanted:**
- PR 1: Backend transaction commands (Rust files only)
- PR 2: Frontend transaction service (TypeScript only)
- PR 3: Transaction UI components (Svelte only)

---

## Common Mistakes You Made

### ❌ Mistake 1: Created PR from main branch
```bash
# BAD: PR #438 was from your main branch
From: walrus-ashish-jalwan:main → chiral-network:main
```

**Why bad?** Your fork's main should stay in sync with upstream main.

**Should be:**
```bash
# GOOD: PR from feature branch
From: walrus-ashish-jalwan:transaction-api → chiral-network:main
```

### ❌ Mistake 2: Didn't fix conflicts
When moderator said "Please fix conflicts," you should have:
1. Merged upstream/main into your branch
2. Fixed conflicts
3. Pushed updates

Instead, you created a new PR.

### ❌ Mistake 3: Closed PRs prematurely
You wrote: "Transaction API already merged into main"

But it was merged to YOUR fork's main, not upstream's main.

### ❌ Mistake 4: Confused origin vs upstream
```bash
origin   → YOUR fork (walrus-ashish-jalwan/chiral-network)
upstream → MAIN repo (chiral-network/chiral-network)
```

You merged to `origin/main` and thought it was in `upstream/main`.

---

## Key Concepts

### 1. Merge Conflict
**What it is:** Git can't automatically combine changes

**Example:**
```typescript
// Upstream version
function sendTransaction() {
  return api.send();
}

// Your version
function sendTransaction() {
  return api.sendWithGas();
}

// Git says: "I don't know which one to keep!"
```

**How to fix:**
```bash
git merge upstream/main
# Git marks conflicts in files:
<<<<<<< HEAD (your changes)
  return api.sendWithGas();
=======
  return api.send();
>>>>>>> upstream/main

# You manually edit to combine:
  return api.sendWithGas();
# Save file, then:
git add file.ts
git commit
```

### 2. Protected Branch
**What it is:** Can't push directly to it

Upstream's main branch is protected:
- ❌ Can't do: `git push upstream main`
- ✅ Must do: Open Pull Request

### 3. Fork vs Clone
- **Fork:** Copy of repo on GitHub (you have write access)
- **Clone:** Copy of repo on your computer

### 4. PR Status Icons
- 🟢 **OPEN** - Waiting for review
- 🟣 **MERGED** - Successfully merged
- 🔴 **CLOSED** - Rejected or abandoned
- ⚠️ **CONFLICTING** - Has merge conflicts
- ⚠️ **DIRTY** - Needs changes

---

## What "Merged" Actually Means

### ✅ Truly Merged
```bash
git log upstream/main --author="Ashish"
# Shows your commits
# ✓ Your code is in the official repository
# ✓ Everyone who clones gets your code
# ✓ PR shows purple "MERGED" icon
```

### ❌ Not Merged (Your Situation)
```bash
git log upstream/main --author="Ashish"
# (empty)
# ✗ Your code is only in YOUR fork
# ✗ Nobody else has your code
# ✗ PRs show red "CLOSED" icon
```

---

## Summary

### What You Thought Happened:
1. Created transaction API ✅
2. Opened PR #438 ✅
3. Opened PR #544 ✅
4. **Merged to main ✅** ← This didn't actually happen!

### What Actually Happened:
1. Created transaction API ✅
2. Opened PR #438 ✅
3. Had merge conflicts ❌
4. Opened PR #544 ✅
5. PR was too big ❌
6. Closed both PRs yourself ❌
7. Merged to YOUR fork's main ✅
8. **Never merged to upstream ❌**

### The Key Misunderstanding:
You have TWO "main" branches:
- `origin/main` = Your fork's main (walrus-ashish-jalwan)
- `upstream/main` = Official main (chiral-network)

You merged to `origin/main` but needed to merge to `upstream/main`.

---

## Conclusive Evidence: Transaction API NOT Merged to Upstream

### Files That Exist ONLY in Your Fork (Not in Upstream)

When you check the upstream repository, these transaction-related files **do not exist**:

**Frontend Files (TypeScript/Svelte):**
```
src/lib/services/transactionService.ts          ❌ NOT in upstream
src/lib/services/walletService.ts               ❌ NOT in upstream
src/lib/components/transactions/GasEstimator.svelte     ❌ NOT in upstream
src/lib/components/transactions/TransactionForm.svelte  ❌ NOT in upstream
```

**Backend Files (Rust):**
```
src-tauri/src/commands/transaction_commands.rs  ❌ NOT in upstream
src-tauri/src/transaction_services.rs           ❌ NOT in upstream
src-tauri/src/ethereum/transaction.rs           ❌ NOT in upstream
src-tauri/src/ethereum/gas_estimation.rs        ❌ NOT in upstream
```

**Other Files:**
```
docs/api/transaction-api.md                     ❌ NOT in upstream
tests/transaction_tests.rs                      ❌ NOT in upstream
```

### Your Commits in YOUR Fork (origin/main)

These commits exist in `walrus-ashish-jalwan/chiral-network`:

```bash
✅ d031aec - WIP: Add transaction API documentation
✅ a28fec4 - Merge remote changes with transaction frontend
✅ 325c159 - feat: Implement complete transaction frontend
✅ 50758f8 - transaction_commands.rs: Add Tauri commands
✅ 5f46ecd - Integrate transaction module with ethereum.rs
```

### Your Commits in UPSTREAM (chiral-network/chiral-network)

```bash
(empty - no results found)
```

**Proof:**
```bash
# Check upstream for your transaction commits
git log upstream/main --grep="transaction\|Transaction" -i --oneline --author="Ashish"
# Result: (empty)

# Check if transaction files exist in upstream
git ls-tree -r upstream/main --name-only | grep transaction
# Result: (empty)
```

### What Actually Happened: Detailed Breakdown

#### PR #438 Timeline (transaction-api branch)

**Oct 8, 2025 - You Opened PR #438**
- **Branch:** `walrus-ashish-jalwan:main` → `chiral-network:main`
- **Status:** OPEN 🟢
- **Changes:** 4,500+ lines added

**Oct 22, 2025 - Moderator Feedback**
- **Moderator:** chiral-steven-tung-2
- **Comment:** "Please fix conflicts"
- **Status:** CONFLICTING ⚠️
- **What this means:** Other developers modified the same files you did
  - Likely: `src/lib/services/walletService.ts`
  - Conflict: Both you and upstream changed wallet functionality

**Oct 22, 2025 - You Closed PR #438**
- **Reason:** You decided to create a new PR instead
- **Result:** PR closed without merge ❌

#### PR #544 Timeline (updated-transaction-api branch)

**Oct 22, 2025 - You Opened PR #544**
- **Branch:** `walrus-ashish-jalwan:updated-transaction-api` → `chiral-network:main`
- **Status:** OPEN 🟢
- **Changes:** 4,723 additions, 1,298 deletions
- **Files Changed:** 47 files

**Oct 22, 2025 - Moderator Feedback**
- **Moderator:** shuaimu
- **Comment:** "This pr is too big. Please separate them."
- **Status:** DIRTY ⚠️
- **What moderator wanted:**

  Instead of 1 giant PR, create 3 smaller PRs:

  **PR 1: Backend Transaction Commands**
  ```
  src-tauri/src/commands/transaction_commands.rs
  src-tauri/src/transaction_services.rs
  tests/transaction_tests.rs
  ```

  **PR 2: Frontend Transaction Service**
  ```
  src/lib/services/transactionService.ts
  src/lib/services/walletService.ts (changes only)
  ```

  **PR 3: Transaction UI Components**
  ```
  src/lib/components/transactions/GasEstimator.svelte
  src/lib/components/transactions/TransactionForm.svelte
  src/pages/TransactionPage.svelte
  ```

**Oct 25, 2025 - You Closed PR #544**
- **Your comment:** "Transaction API already merged into main in commits 325c159, 50758f8, and 5f46ecd"
- **Reality:** Those commits exist in `origin/main` (your fork), NOT `upstream/main`
- **Result:** PR closed without merge ❌

### Why You Thought It Was Merged

In your PR comments, you wrote:
> "Transaction API already merged into main in commits 325c159, 50758f8, and 5f46ecd"

**What actually happened:**

```bash
# You ran this (checking YOUR fork):
git log origin/main --oneline | grep transaction
325c159 feat: Implement complete transaction frontend  ✅
50758f8 transaction_commands.rs: Add Tauri commands    ✅
5f46ecd Integrate transaction module with ethereum.rs  ✅

# You SHOULD have run this (checking UPSTREAM):
git log upstream/main --oneline | grep transaction
(empty)  ❌
```

**The confusion:**
- `origin/main` = **YOUR** fork's main branch
- `upstream/main` = **OFFICIAL** repository's main branch

You merged to `origin/main` and thought it was the official repository.

### Current File Structure Comparison

#### In YOUR Fork (walrus-ashish-jalwan/chiral-network)

```
src/
├── lib/
│   ├── services/
│   │   ├── transactionService.ts       ✅ EXISTS
│   │   └── walletService.ts            ✅ EXISTS (modified)
│   └── components/
│       └── transactions/
│           ├── GasEstimator.svelte     ✅ EXISTS
│           └── TransactionForm.svelte  ✅ EXISTS
src-tauri/
└── src/
    ├── commands/
    │   └── transaction_commands.rs     ✅ EXISTS
    └── transaction_services.rs         ✅ EXISTS
```

#### In UPSTREAM (chiral-network/chiral-network)

```
src/
├── lib/
│   ├── services/
│   │   ├── transactionService.ts       ❌ DOES NOT EXIST
│   │   └── walletService.ts            ⚠️ EXISTS (different version)
│   └── components/
│       └── transactions/               ❌ DIRECTORY DOES NOT EXIST
src-tauri/
└── src/
    ├── commands/
    │   └── transaction_commands.rs     ❌ DOES NOT EXIST
    └── transaction_services.rs         ❌ DOES NOT EXIST
```

### The Modified File Problem

**walletService.ts conflict:**

```typescript
// UPSTREAM version (chiral-network/chiral-network):
export async function getBalance(address: string): Promise<string> {
  const provider = getProvider();
  return await provider.getBalance(address);
}

// YOUR version (walrus-ashish-jalwan/chiral-network):
export async function getBalance(address: string): Promise<string> {
  const provider = getProvider();
  return await provider.getBalance(address);
}

export async function sendTransaction(tx: Transaction): Promise<string> {
  // ← YOU ADDED THIS
  const signer = getSigner();
  return await signer.sendTransaction(tx);
}

export async function estimateGas(tx: Transaction): Promise<bigint> {
  // ← YOU ADDED THIS
  const provider = getProvider();
  return await provider.estimateGas(tx);
}
```

**The conflict:**
- Upstream modified `getBalance()` between Oct 8-22
- You also modified `getBalance()` AND added new functions
- Git can't automatically merge → CONFLICTING status

---

## Understanding Git Remotes: The Root Cause

### What Are Remotes?

Remotes are Git's way of tracking different copies of a repository.

```bash
git remote -v

origin    https://github.com/walrus-ashish-jalwan/chiral-network.git (fetch)
origin    https://github.com/walrus-ashish-jalwan/chiral-network.git (push)
upstream  https://github.com/chiral-network/chiral-network.git (fetch)
upstream  https://github.com/chiral-network/chiral-network.git (push)
```

### The Two "Main" Branches Problem

You have **two completely separate** main branches:

```bash
# YOUR fork's main branch
origin/main
  ↓
  Located at: walrus-ashish-jalwan/chiral-network
  You have: Full write access
  Your commits: 325c159, 50758f8, 5f46ecd ✅ HERE

# OFFICIAL repository's main branch
upstream/main
  ↓
  Located at: chiral-network/chiral-network
  You have: Read-only access (must use PRs)
  Your commits: (none) ❌ NOT HERE
```

### How Merging Really Works

#### What You Did (Merging to YOUR Fork)

```bash
# On your local machine:
git checkout main
git merge transaction-api
git push origin main
```

**Result:**
```
origin/main (YOUR fork) now has your transaction code ✅
upstream/main (OFFICIAL) still does NOT have your code ❌
```

**Your code is in:**
- ✅ Your local machine
- ✅ Your fork on GitHub (walrus-ashish-jalwan)
- ❌ Upstream on GitHub (chiral-network) ← This is where it needs to be!

#### What You NEED to Do (Merging to Upstream)

```bash
# You CANNOT do this (protected branch):
git push upstream main  # ❌ ERROR: Permission denied

# You MUST do this instead:
1. Open Pull Request on GitHub
2. Wait for moderator approval
3. Moderator clicks "Merge Pull Request"
4. NOW your code goes into upstream/main ✅
```

### Visual Representation

```
┌─────────────────────────────────────────────────────────────┐
│ UPSTREAM (chiral-network/chiral-network)                    │
│ github.com/chiral-network/chiral-network                    │
│                                                             │
│ main branch:                                                │
│ ├─ commit aaa (Oct 1)                                      │
│ ├─ commit bbb (Oct 10) ← Someone else's work               │
│ ├─ commit ccc (Oct 15) ← Modified walletService.ts         │
│ └─ commit ddd (Oct 20)                                     │
│                                                             │
│ ❌ Your transaction API commits are NOT here                │
└─────────────────────────────────────────────────────────────┘
                            ↑
                            │ ❌ PR #438: Closed (conflicts)
                            │ ❌ PR #544: Closed (too big)
                            │ ✅ NEED: New PR (properly split)
                            │
┌─────────────────────────────────────────────────────────────┐
│ YOUR FORK (walrus-ashish-jalwan/chiral-network)            │
│ github.com/walrus-ashish-jalwan/chiral-network              │
│                                                             │
│ main branch:                                                │
│ ├─ commit aaa (Oct 1)  ← Synced from upstream              │
│ ├─ commit 325c159 (Oct 8) ← YOUR transaction frontend      │
│ ├─ commit 50758f8 (Oct 12) ← YOUR transaction commands     │
│ └─ commit 5f46ecd (Oct 18) ← YOUR ethereum integration     │
│                                                             │
│ ✅ Your transaction API commits ARE here                    │
│ ❌ But this is YOUR fork, not the official repo             │
└─────────────────────────────────────────────────────────────┘
```

---

## How to Verify Merge Status

### Commands to Check if Code is Truly Merged

```bash
# 1. Check if your commits are in upstream
git log upstream/main --author="Ashish" --oneline
# Expected: (empty) ❌
# If merged: Shows your commits ✅

# 2. Check if transaction files exist in upstream
git ls-tree -r upstream/main --name-only | grep transaction
# Expected: (empty) ❌
# If merged: Shows transaction files ✅

# 3. Check PR status on GitHub
gh pr list --repo chiral-network/chiral-network --author @me --state merged
# Expected: (empty) ❌
# If merged: Shows PR #438 or #544 with "MERGED" status ✅

# 4. Compare branches
git diff upstream/main origin/main --stat
# Shows: All your transaction files as additions
# This proves: Your changes are ONLY in your fork
```

### What "MERGED" Looks Like on GitHub

#### ❌ Your Current PRs (Closed, Not Merged)
```
PR #438: Add Transaction API
🔴 Closed by walrus-ashish-jalwan on Oct 22
❌ This pull request is closed.
```

#### ✅ What a Merged PR Looks Like
```
PR #123: Add New Feature
🟣 Merged by chiral-steven-tung-2 on Oct 15
✅ This pull request has been merged into main
```

**The key difference:**
- 🔴 Red icon = Closed (rejected/abandoned)
- 🟣 Purple icon = Merged (accepted into main)

---

## How Pull Requests Work: Step-by-Step

### The Complete PR Flow

#### Stage 1: Create PR
```bash
# 1. Create feature branch
git checkout -b transaction-api

# 2. Make changes and commit
git add src/lib/services/transactionService.ts
git commit -m "feat: Add transaction service"

# 3. Push to YOUR fork
git push origin transaction-api
```

#### Stage 2: Open PR on GitHub
1. Go to https://github.com/chiral-network/chiral-network
2. Click "Pull requests" tab
3. Click "New pull request"
4. Click "compare across forks"
5. **Set correctly:**
   - **base repository:** `chiral-network/chiral-network`
   - **base branch:** `main`
   - **head repository:** `walrus-ashish-jalwan/chiral-network`
   - **compare branch:** `transaction-api`
6. Click "Create pull request"

#### Stage 3: Code Review
**Moderator reviews and may request:**
- ⚠️ "Fix merge conflicts" → You need to merge upstream/main
- ⚠️ "Split into smaller PRs" → Too many changes at once
- ⚠️ "Fix failing tests" → CI/CD checks failed
- ⚠️ "Address comments" → Code quality issues

**Your job:**
```bash
# Fix conflicts example:
git checkout transaction-api
git fetch upstream
git merge upstream/main
# Resolve conflicts in files
git add .
git commit -m "fix: Resolve merge conflicts"
git push origin transaction-api
# PR automatically updates!
```

#### Stage 4: Merge (by Moderator ONLY)
**When approved, moderator does:**
1. Clicks "Merge pull request" button
2. Chooses merge strategy (merge commit, squash, rebase)
3. Clicks "Confirm merge"

**Result:**
```bash
# NOW your commits appear in upstream
git fetch upstream
git log upstream/main --author="Ashish"
# Shows: 325c159 feat: Add transaction service ✅
```

### What Went Wrong with Your PRs

#### PR #438 Breakdown

```
Status: CONFLICTING ⚠️

Your branch:      transaction-api (Oct 8)
Upstream branch:  main (Oct 22)

Conflict in:      src/lib/services/walletService.ts

Why:
- You modified walletService.ts on Oct 8
- Someone else modified walletService.ts on Oct 15
- Git cannot auto-merge both changes

Solution Needed:
git merge upstream/main
# Fix walletService.ts manually
git commit && git push
```

**What you did instead:**
- ❌ Created new branch `updated-transaction-api`
- ❌ Opened new PR #544
- ❌ Closed PR #438

**What you should have done:**
- ✅ Stayed on `transaction-api` branch
- ✅ Merged upstream/main into it
- ✅ Fixed conflicts
- ✅ Pushed update
- ✅ Waited for moderator approval

#### PR #544 Breakdown

```
Status: DIRTY (too big) ⚠️

Files changed:    47 files
Additions:        +4,723 lines
Deletions:        -1,298 lines

Moderator feedback: "This pr is too big. Please separate them."

Why:
PRs should be focused on ONE change. Easier to review, safer to merge.

Your PR included:
- Backend transaction commands (Rust)
- Frontend transaction service (TypeScript)
- UI components (Svelte)
- Test files
- Documentation
- Config changes

Solution Needed:
Create 3 separate PRs:
1. Backend PR (Rust files)
2. Frontend PR (TS files)
3. UI PR (Svelte files)
```

**What you did instead:**
- ❌ Closed PR #544
- ❌ Thought it was already merged

**What you should have done:**
- ✅ Split into 3 smaller PRs
- ✅ Opened PR 1 (backend only)
- ✅ After PR 1 merged, open PR 2 (frontend)
- ✅ After PR 2 merged, open PR 3 (UI)

---

## Next Steps

### How to Properly Merge Your Transaction API

See the separate guide: `how_to_merge_transaction_api.md`

**Quick summary:**
1. Sync your fork with upstream
2. Create 3 new feature branches (backend, frontend, UI)
3. Split your transaction code across them
4. Open 3 separate, focused PRs
5. Wait for review and approval
6. Fix any requested changes
7. Moderator merges each PR
8. Verify commits appear in upstream/main

---

## Key Takeaways

### ✅ What You Did Right
1. Created comprehensive transaction API code
2. Opened PRs (correct process)
3. Tried to respond to feedback

### ❌ What Went Wrong
1. Opened PR from `main` branch instead of feature branch
2. Didn't fix merge conflicts when requested
3. Created new PR instead of updating existing one
4. Made PR too large (47 files)
5. Closed PRs prematurely
6. Confused `origin/main` with `upstream/main`
7. Thought merge to YOUR fork meant merge to upstream

### 🎓 What You Learned
1. **Two separate repositories:**
   - origin (your fork) ← You merged here
   - upstream (official) ← Need to merge here via PR

2. **PR lifecycle:**
   - Open → Review → Fix issues → Moderator approves → Moderator merges

3. **Merge conflicts must be fixed:**
   - Can't ignore them or create new PR
   - Must merge upstream/main and resolve manually

4. **Keep PRs small:**
   - 1 PR = 1 feature
   - Easier to review, faster to merge
   - Less likely to conflict

5. **Verify merge status:**
   - Check upstream/main, not origin/main
   - PR icon: 🟣 = merged, 🔴 = closed
   - Use `git log upstream/main` to confirm

---

**Last Updated:** October 31, 2025
**Status:** Transaction API exists in fork only, not merged to upstream
**Next Action:** Create new, properly structured PRs

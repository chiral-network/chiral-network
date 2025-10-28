# **Chiral Network – Wallet & Blockchain Design**

## **1. Overview**

The **Wallet & Blockchain Layer** provides a minimal **payment and identity system** for the Chiral Network.
It enables users to:

- **Earn** cryptocurrency for seeding files
- **Pay** for file downloads
- **Verify** payments and balances on-chain
- **Authenticate** nodes cryptographically

This layer is **Ethereum-compatible**, meaning all accounts and transactions follow Ethereum standards and can be used with supported Ethereum libraries.

Our EVM-compatible blockchain is hosted on a dedicated Geth node, which processes transactions, maintains the ledger, and provides a JSON-RPC interface for wallet interactions.

### Wallet Description

> **Note:** This description describes the current wallet system in the application, but for maximum simplicity we can consider integrating Alloy instead.

The project implements a **single-account, local wallet** managed by the backend (`src-tauri`):

- **State management:** Wallet state is held in `AppState` (`active_account` and `active_account_private_key`) and exposed to the UI via Tauri commands.
- **Persistence:** Accounts are saved and loaded using a password-protected Keystore module (`src-tauri/src/keystore.rs`).
- **Blockchain interaction:** Transactions are signed locally using the in-memory private key and submitted to the blockchain via the `ethereum` module (`src-tauri/src/ethereum.rs`).
- **UI / CLI integration:** Wallet operations such as creating/importing accounts, checking balances, sending transactions, and processing download payments are exposed through Tauri commands.
- **Payment & file flows:** Download and upload payments are checked, recorded, and triggered using hooks (`process_download_payment` → `ethereum::send_transaction`) with notifications to seeders via DHT events.
- **Security features:** Private keys are kept in memory during sessions and cleared on logout. Keystore is encrypted and optional 2FA/TOTP support (`totp_rs`) is available for sensitive actions.

This setup provides a **lightweight, secure, and Rust-native wallet** fully integrated with the Chiral Network file-sharing and payment system, while relying on a **local Geth node** for Ethereum-compatible blockchain operations.

## **2. Design Goals**

| Goal              | Description                                                               |
| ----------------- | ------------------------------------------------------------------------- |
| **Simplicity**    | Minimal logic — single-account wallet, no mining or full node integration |
| **Compatibility** | Works with any Ethereum-compatible chain or testnet                       |
| **Separation**    | Decoupled from file transfer layer                                        |
| **Transparency**  | All payments verifiable on-chain                                          |

---

## **3. System Components**

### 3.1 Wallet Service

Lightweight client-side key manager for generating, storing, and using Ethereum-compatible wallets.

**Responsibilities:**

- Create a new wallet or import from mnemonic (single account)
- Sign and send blockchain transactions (offline signing supported)
- Query account balances
- Expose wallet functions to the app (UI & API)

**Key Functions:**

- `createWallet()` – Generate BIP39 mnemonic, derive first account (`m/44'/60'/0'/0/0`)
- `importWallet(mnemonic)` – Restore wallet from mnemonic
- `getAddress()` – Return wallet address
- `getBalance()` – Query account balance
- `signMessage(message)` – Offline message signing
- `sendTransaction(to, amount, data?)` – Submit transaction via RPC

> **Note:** This is a general guideline. The actual implementation can differ.

---

### 3.2 Blockchain Service

Handles communication with the Ethereum-compatible blockchain via **JSON-RPC** by interacting with a **Geth node**.

**Responsibilities:**

- Connect to a configurable **Geth RPC endpoint** (HTTP or WebSocket)
- Submit **locally signed transactions** to the Geth node
- Fetch blockchain data from Geth (balances, receipts, confirmations)
- Handle gas estimation (optional; can use default values or query Geth)
- Track transaction status (`pending` / `confirmed`) via Geth

**Core APIs:**

- `connect(gethRpcUrl)` — Connects to the Geth node’s RPC endpoint
- `getBalance(address)` — Queries wallet balance from Geth
- `sendRawTransaction(txSigned)` — Submits signed transaction via Geth
- `getTransactionStatus(txHash)` — Checks transaction status through Geth

> **Note:** This is a general guideline. The actual implementation can differ.

> **Note:** The Rust Blockchain Service does not run an Ethereum node itself; it relies on a local or remote **Geth node** for all blockchain interactions.

## **4. Wallet Architecture**

```text
┌──────────────────────────────────────────┐
│               Wallet Layer               │
│ ┌──────────────────────────────────────┐ │
│ │ Mnemonic / Private Key Management    │ │
│ │ - BIP39 Mnemonic                     │ │
│ │ - BIP32 Derivation                   │ │
│ │ - Single account derivation          │ │
│ │ - secp256k1 Signatures               │ │
│ └──────────────────────────────────────┘ │
│ ┌──────────────────────────────────────┐ │
│ │ Transaction Signing                  │ │
│ │ - EIP-155 Transaction Format         │ │
│ │ - Local / Offline Signing            │ │
│ └──────────────────────────────────────┘ │
│ ┌──────────────────────────────────────┐ │
│ │ RPC Interface                        │ │
│ │ - Send Transactions via RPC          │ │
│ │ - Query Balances & Receipts          │ │
│ └──────────────────────────────────────┘ │
└──────────────────────────────────────────┘
```

## 5.1 Network Setup

- **Type:** Ethereum-compatible
  - Wallet and transactions follow Ethereum standards (addresses, accounts, transactions).

- **Blockchain Node:** Local Geth node
  - Communicates over a single **Geth node** running the Chiral Network blockchain.
  - This node handles transaction submission, balance queries, and block information.

- **RPC Endpoint:** Configurable
  - The blockchain service connects to Geth via **JSON-RPC** over HTTP or WebSocket.
  - Example: `http://localhost:8545` or `ws://localhost:8546`

- **Notes:**
  - For simplicity, only one node is required per client.
  - No need for testnets, mainnet, or external providers in the minimal implementation.
  - The RPC endpoint can be changed in app settings if the user wants to connect to another local or remote Geth node.

### 5.2 Transaction Flow

1. Seeder or leecher triggers a payment event.
2. Wallet signs transaction locally.
3. Blockchain Service submits transaction via RPC.
4. UI reflects transaction status: **pending** → **confirmed**.

## **6. User Flow (File-Sharing Context)**

### 6.1 Create Wallet

1. On first launch, the node prompts the user to **create a Chiral wallet**.
2. System generates a **BIP39 mnemonic** and derives the first Ethereum account (`m/44'/60'/0'/0/0`).
3. The address becomes the node’s **on-chain identity** for earning or paying tokens.
4. User is prompted to **securely back up the mnemonic** (never stored remotely).

> This wallet represents the node for both authentication and payment settlement.

---

### 6.2 Import Wallet

1. Existing users can **import a wallet** using a mnemonic phrase.
2. Wallet restores locally, loading balance, transaction history, and **previous upload/download logs**.
3. Node resumes previous seeding/downloading state with the same identity.

---

### 6.3 Pay for Download (Leecher Flow)

1. A **leecher** requests a file or chunk.
2. System determines the **price** (from metadata or smart contract).
3. Wallet **signs and sends a transaction** via `ethereum::send_transaction()`.
4. The node logs the **download event** with:
   - File hash / name
   - Payment transaction hash
   - Amount
   - Timestamp
   - Seeder address

5. Seeder receives proof of payment before transfer begins.
6. After confirmation, file data is streamed.
7. **Transaction + download logs** are stored locally for the user to view.

---

### 6.4 Earn for Seeding (Seeder Flow)

1. **Seeder node** advertises available files with its wallet address attached.
2. When a leecher initiates a download and pays:
   - Seeder’s address is the **payment recipient**
   - Seeder logs the **upload event**:
     - File hash / name
     - Payment transaction hash
     - Amount received
     - Timestamp
     - Leecher address

3. After verification (hash match / receipt proof), the seeder account **receives payment**.
4. Seeder can review **payment + upload logs** in the wallet UI/CLI.

---

### 6.5 View Transactions and File History

- Wallet UI or CLI displays **financial and file-level history**, including all uploads and downloads:
  - `Received from` / `Sent to` addresses
  - File hash / name
  - Amount / payment transaction hash
  - Timestamp
  - Status: `pending` / `confirmed`
  - Type: `Upload` / `Download`

- Filterable by:
  - **“Payments Sent”**
  - **“Earnings Received”**
  - **“File Uploads”**
  - **“File Downloads”**

## **7. Separation of Wallet & Blockchain Layer from File-Sharing Logic**

Keeping the **Wallet & Blockchain Layer** separate from upload/download logic provides several key benefits:

### 7.1 Modularity

- The wallet and blockchain logic can evolve independently of the file-sharing features.
- You can swap blockchain implementations, upgrade dependencies, or change RPC endpoints without touching file transfer code.

### 7.2 Maintainability

- Bugs, security issues, or updates in one layer do not directly affect the other.
- Smaller, well-defined modules are easier to test and debug.

### 7.3 Security

- Sensitive information such as private keys never needs to leave the wallet layer.
- Upload/download logic only interacts with signed transactions or payment proofs, reducing the risk of exposing keys.

### 7.4 Flexibility

- The file-sharing system can integrate with other blockchains in the future.

### 7.5 Simplified Development

- Developers working on file transfer features do not need to understand Ethereum transaction mechanics.
- Blockchain developers can focus on payments, balances, and smart contracts without managing file flows.

> **Summary:** Decoupling ensures a clear separation of concerns, improving security, maintainability, and adaptability of the Chiral Network.

## **8. Summary**

The **Wallet & Blockchain Layer** in Chiral Network provides:

- ✅ Lightweight Ethereum-compatible wallet (single account)
- ✅ Offline or RPC-based transaction signing
- ✅ Simple RPC integration for payments
- ✅ Fully decoupled from file-sharing protocols
- ✅ Minimal transaction states (`pending` / `confirmed`)
- ✅ Extensible for future token and smart contract logic

---

## **See Also**

- [Security & Privacy](security-privacy.md) – Wallet security details
- [User Guide](user-guide.md) – Step-by-step wallet usage
- [API Documentation](api-documentation.md) – Wallet API reference

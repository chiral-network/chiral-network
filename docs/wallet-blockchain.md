# **Chiral Network – Wallet & Blockchain Design**

## **1. Overview**

The **Wallet & Blockchain Layer** provides a minimal **payment and identity system** for the Chiral Network.
It enables users to:

- **Earn** cryptocurrency for seeding files
- **Pay** for file downloads
- **Verify** payments and balances on-chain
- **Authenticate** nodes cryptographically

This layer is **Ethereum-compatible**, meaning all accounts and transactions follow Ethereum standards and can be used with supported Ethereum libraries.

Our **EVM-compatible blockchain** is hosted on a dedicated **Geth node**, which processes transactions, maintains the ledger, provides a JSON-RPC interface, and handles mining. **Clef** is used as an **external signer** to manage wallets and key signing securely.

### Wallet Description

- **State management:** Clef stores private keys and signs transactions; Geth interacts via JSON-RPC.
- **Persistence:** Wallets are encrypted and managed entirely by Clef; no keys are stored in the application memory.
- **Blockchain interaction:** Transactions are signed through Clef and submitted via the Geth RPC endpoint.
- **UI / CLI integration:** Wallet operations such as creating/importing accounts, checking balances, sending transactions, and processing download payments are exposed through Tauri commands that communicate with Clef + Geth.
- **Security features:** Private keys never leave Clef. Geth only handles signed transactions. Optional 2FA/TOTP can be enforced via Clef policies for sensitive actions.

This setup provides a **lightweight, secure, and standard Ethereum-compatible wallet system** fully integrated with the Chiral Network file-sharing and payment system, while relying on a **single Geth node** for all blockchain operations.

---

## **2. Design Goals**

| Goal              | Description                                                               |
| ----------------- | ------------------------------------------------------------------------- |
| **Simplicity**    | Single-account wallet handled externally by Clef; Geth manages blockchain |
| **Compatibility** | Works with any Ethereum-compatible chain or testnet                       |
| **Separation**    | Wallet signing handled by Clef, blockchain operations handled by Geth     |
| **Transparency**  | All payments verifiable on-chain via Geth                                 |

---

## **3. System Components**

### 3.1 Wallet Service (Clef)

Handles key management and signing of Ethereum-compatible transactions.

**Responsibilities:**

- Generate and import accounts (mnemonic or private key)
- Sign transactions and messages securely
- Enforce signing policies (optional 2FA or multi-sig)
- Expose wallet functions to the application via IPC/JSON-RPC

**Key Functions:**

- `createWallet()` – Clef generates a new account
- `importWallet(mnemonic)` – Restore wallet from mnemonic
- `getAddress()` – Return the wallet address
- `signMessage(message)` – Sign messages or transactions
- `sendTransaction(txUnsigned)` – Sign and return raw transaction to Geth for broadcasting

---

### 3.2 Blockchain Service (Geth)

Handles all blockchain operations via **JSON-RPC**:

**Responsibilities:**

- Connect to a local or remote **Geth node RPC endpoint** (HTTP/WebSocket)
- Submit **Clef-signed transactions** to the blockchain
- Query balances, transaction receipts, and confirmations
- Optionally handle **mining** if enabled on the node
- Track transaction status (`pending` / `confirmed`)

**Core APIs:**

- `connect(gethRpcUrl)` — Connect to the Geth node
- `getBalance(address)` — Query account balance from Geth
- `sendRawTransaction(txSigned)` — Submit signed transaction via Geth
- `getTransactionStatus(txHash)` — Check transaction status

---

## **4. Wallet Architecture**

```text
┌──────────────────────────────────────────┐
│               Wallet Layer               │
│ ┌──────────────────────────────────────┐ │
│ │ Clef Key Management                   │ │
│ │ - Mnemonic / Private Key Storage      │ │
│ │ - secp256k1 Signatures                │ │
│ │ - Signing Policies / 2FA              │ │
│ └──────────────────────────────────────┘ │
│ ┌──────────────────────────────────────┐ │
│ │ Transaction Signing                   │ │
│ │ - EIP-155 Transaction Format          │ │
│ │ - Offline / Secure Signing            │ │
│ └──────────────────────────────────────┘ │
│ ┌──────────────────────────────────────┐ │
│ │ Geth RPC Interface                    │ │
│ │ - Send Signed Transactions            │ │
│ │ - Query Balances & Receipts           │ │
│ │ - Mining (optional)                   │ │
│ └──────────────────────────────────────┘ │
└──────────────────────────────────────────┘
```

---

## **5.1 Network Setup**

- **Type:** Ethereum-compatible
  - Wallet and transactions follow Ethereum standards.

- **Blockchain Node:** Local Geth node
  - Handles transaction submission, mining (optional), balance queries, and block information.

- **Wallet & Signing:** Clef
  - Manages keys, signs transactions, and enforces signing policies.

- **RPC Endpoint:** Configurable
  - Example: `http://localhost:8545` or `ws://localhost:8546`
  - Rust app communicates with **Geth + Clef** through JSON-RPC.

---

### 5.2 Transaction Flow

1. Seeder or leecher triggers a payment event.
2. Wallet (Clef) signs the transaction securely.
3. Blockchain Service (Geth) submits the signed transaction via RPC.
4. UI reflects transaction status: **pending** → **confirmed**.

---

## **6. User Flow (File-Sharing Context)**

### 6.1 Create Wallet

1. On first launch, Clef generates a **new wallet**.
2. The address becomes the node’s **on-chain identity** for earning or paying tokens.
3. User securely backs up mnemonic/private key (never stored remotely).

### 6.2 Import Wallet

1. Existing users **import a wallet** via mnemonic/private key in Clef.
2. Wallet state is restored, including balances and transaction history.
3. Node resumes previous seeding/downloading state with the same identity.

### 6.3 Pay for Download (Leecher Flow)

1. Leecher requests a file or chunk.
2. Wallet (Clef) signs the payment transaction.
3. Geth submits the signed transaction to the blockchain.
4. Seeder receives proof of payment before transfer begins.
5. Transaction + download logs stored locally for tracking.

### 6.4 Earn for Seeding (Seeder Flow)

1. Seeder advertises files with wallet address attached.
2. Leecher initiates payment → Clef signs → Geth broadcasts.
3. Seeder account receives payment upon confirmation.
4. Transaction + upload logs tracked locally.

### 6.5 View Transactions and File History

- Wallet UI or CLI displays financial and file-level history, including uploads and downloads.
- Filterable by: Payments Sent, Earnings Received, File Uploads, File Downloads.

---

## **7. Separation of Wallet & Blockchain Layer from File-Sharing Logic**

### Key Benefits

- **Modularity:** Wallet and blockchain logic handled entirely by Clef + Geth.
- **Maintainability:** Updates in Geth or Clef do not affect file transfer code.
- **Security:** Private keys remain in Clef; file-sharing logic only sees signed transactions.
- **Flexibility:** Supports future blockchains and smart contract logic.
- **Simplified Development:** File transfer and blockchain/payment logic remain independent.

---

## **8. Summary**

The **Wallet & Blockchain Layer** in Chiral Network now provides:

- ✅ Ethereum-compatible wallet handled by **Clef**
- ✅ Blockchain operations, mining, and transaction submission handled by **Geth**
- ✅ Offline or RPC-based signing
- ✅ Fully decoupled from file-sharing protocols
- ✅ Minimal transaction states (`pending` / `confirmed`)
- ✅ Extensible for future token and smart contract logic

---

## **9. Future Exploration – Alloy**

The current design intentionally relies on **Geth** because it provides the **most stable and complete implementation** of an Ethereum-compatible client that still supports **mining** on **Ethereum Classic (ETC)** networks.

During design discussions, it was noted that:

- **Geth** remains the only reliable option for mining on legacy ETH/ETC chains.
- **Alloy** (Rust-based Ethereum SDK) does **not currently support mining**, so Geth must remain part of the architecture for now.
- Because Geth already exposes a **wallet RPC interface**, it simplifies development to handle both **wallet** and **blockchain** functions directly through Geth’s HTTP API rather than maintaining Clef separately at this stage.
- **Long-term goal:**
  - Migrate wallet operations to **Alloy** for improved modularity, performance, and Rust-native integration.
  - Retain Geth only for **mining and consensus** functions.
  - Eventually, if the project grows and mining becomes obsolete (e.g., due to network transition or impracticality of CPU mining), **Geth could be dropped entirely**.

Preliminary review of Alloy documentation suggests it **likely supports Ethereum Classic** and legacy transaction formats (see [Alloy legacy transaction example](https://alloy.rs/examples/transactions/send_legacy_transaction)), though this should be confirmed through testing.

In summary, the **current approach keeps Geth as the unified node for wallet + blockchain + mining**, while **Alloy is reserved as a future candidate** for wallet abstraction once mining support is no longer a requirement.

---

## **See Also**

- [Security & Privacy](security-privacy.md) – Wallet security details
- [User Guide](user-guide.md) – Step-by-step wallet usage
- [API Documentation](api-documentation.md) – Wallet API reference

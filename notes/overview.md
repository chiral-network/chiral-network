# Backend

Responsibilities: Communicate with Geth Node and exposing a secure API 

Core Node Logic (transaction_services.rs):
- Broadcasting TXs
- Estimating Gas
- Getting current Nonce
- Checking Network Status

Enriched Error Handling (transaction_services.rs)

API Command Layer (src-tauri/src/commands/transactions_commands.rs)
 - Each API Endpoint (/broadcast, /transaction/{tx_hash}) will be a Tauri command
 - register within main.rs 

# Front End

Responsibilities: Managing User Keys, creating and signing transactions, display

Client-Side Signing (src/lib/services/walletService.ts): 
 - Create TX
 - Sign TX

API Service Layer (src/lib/services/transactionService.ts):
- Need a service to command with Tauri Backend 

UI Commponents

Step 1: Update Your Data Model 📝
Before creating any UI, you need to ensure your application's data structure matches the API specification.
File to Modify: src/lib/stores.ts.
Action: Find the Transaction interface and enhance it by adding new fields like transaction_hash, gas_used, and confirmations. You should also expand the status field to include values like 'failed' and 'submitted'.


Step 2: Create the New UI Components 🏗️
The analysis strongly recommends creating new files for this new functionality.
Files to Create:

 - src/lib/components/transactions/TransactionForm.svelte: A form for creating and sending new transactions.
 - src/lib/components/transactions/TransactionHistory.svelte: A component to display a list of past transactions.
 - src/lib/components/transactions/GasEstimator.svelte: A component to show real-time gas fee estimates.

Action: As you build these components, remember to reference your existing UI library (e.g., input.svelte, button.svelte) to maintain a consistent style and layout.

Step 3: Integrate into Your Pages 🧩
Once the new components are ready, you need to add them to the main application view.

File to Modify: src/pages/Account.svelte.

Action: Import and place your new TransactionForm.svelte and TransactionHistory.svelte components within this page to make the transaction functionality available to the user.

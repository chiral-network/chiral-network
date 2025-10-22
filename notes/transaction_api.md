# Chiral Network Transaction API Specification
## Developer Experience Focused Design

This document outlines the complete REST API specification for the Chiral Network transaction system, incorporating feedback from security, reliability, and developer experience experts.

## Design Philosophy

The API follows a **Geth-native approach** that delegates core transaction validation to the battle-tested Geth client while providing a **developer-friendly abstraction layer** with enriched error handling and value-added endpoints.

### Key Principles
- **Security First**: Client-side signing ensures private keys never leave user devices
- **Backend Simplicity**: Leverage Geth's native validation instead of duplicating logic
- **Developer Experience**: Provide actionable error messages and helper endpoints
- **Stable Contract**: API error codes independent of underlying Geth version changes

---

## Base Configuration

### Base URL
```
https://api.chiral-network.com/v1
```

### Authentication Model
- **Client-side signing**: Private keys remain on user devices
- **Cryptographic authentication**: Transaction signatures provide authorization
- **No API keys required**: Self-authenticating through blockchain signatures

### Network Details
- **Chain ID**: 98765 (Chiral Network)
- **Native Currency**: CHR (Chiral)
- **Block Time**: ~15 seconds
- **Consensus**: Proof of Work (Ethash)

---

## Core Transaction Endpoints

### 1. Broadcast Signed Transaction

**Endpoint**: `POST /transactions/broadcast`

Broadcast a pre-signed transaction to the Chiral network.

#### Request Format
```http
POST /transactions/broadcast
Content-Type: application/json

{
  "signed_transaction_payload": "0xf86c098504a817c800825208943535353535353535353535353535353535353535880de0b6b3a76400008025a028ef61340bd939bc2195fe537567866003e1a15d3c71ff63e1590620aa636276a067cbe9d8997f761aecb703304b3800ccf555c9f3dc64214b297fb1966a3b6d83"
}
```

#### Success Response (202 Accepted)
```json
{
  "status": "success",
  "data": {
    "transaction_hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
    "status": "submitted",
    "timestamp": "2025-01-20T10:30:00Z"
  }
}
```

#### Enhanced Error Responses

**Nonce Too Low (400 Bad Request)**
```json
{
  "status": "error",
  "error": {
    "code": "NONCE_TOO_LOW",
    "message": "The transaction nonce is lower than the next valid nonce for the sender's account",
    "details": {
      "sender_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
      "submitted_nonce": 14,
      "expected_nonce": 15,
      "difference": -1
    },
    "suggestion": "Use GET /addresses/{address}/nonce to get the correct nonce, then re-sign and resubmit the transaction",
    "documentation_url": "https://api.chiral-network.com/docs/errors#nonce_too_low",
    "geth_error": "nonce too low"
  }
}
```

**Insufficient Funds (400 Bad Request)**
```json
{
  "status": "error",
  "error": {
    "code": "INSUFFICIENT_FUNDS",
    "message": "Account balance is insufficient to cover transaction value and gas costs",
    "details": {
      "sender_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
      "account_balance": "1.5002 ETH",
      "transaction_value": "1.5 ETH",
      "gas_cost": "0.0004 ETH",
      "total_required": "1.5004 ETH",
      "shortfall": "0.0002 ETH"
    },
    "suggestion": "Either reduce the transaction amount or add more ETH to the sender's account",
    "documentation_url": "https://api.chiral-network.com/docs/errors#insufficient_funds",
    "geth_error": "insufficient funds for gas * price + value"
  }
}
```

**Nonce Too High (409 Conflict)**
```json
{
  "status": "error",
  "error": {
    "code": "NONCE_TOO_HIGH",
    "message": "The transaction nonce is higher than expected, indicating a gap in the transaction sequence",
    "details": {
      "sender_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
      "submitted_nonce": 18,
      "expected_nonce": 15,
      "gap_size": 3
    },
    "suggestion": "Check for pending transactions or use GET /addresses/{address}/nonce to get the correct nonce",
    "documentation_url": "https://api.chiral-network.com/docs/errors#nonce_too_high",
    "geth_error": "nonce too high"
  }
}
```

**Gas Price Too Low (400 Bad Request)**
```json
{
  "status": "error",
  "error": {
    "code": "GAS_PRICE_TOO_LOW",
    "message": "The transaction gas price is below the network minimum",
    "details": {
      "submitted_gas_price": "10000000000",
      "minimum_gas_price": "20000000000",
      "suggested_gas_price": "25000000000",
      "price_increase_needed": "150%"
    },
    "suggestion": "Use GET /network/gas-price to get current recommended gas prices, then re-sign with higher gas price",
    "documentation_url": "https://api.chiral-network.com/docs/errors#gas_price_too_low",
    "geth_error": "transaction underpriced"
  }
}
```

**Gas Limit Exceeded (400 Bad Request)**
```json
{
  "status": "error",
  "error": {
    "code": "GAS_LIMIT_EXCEEDED",
    "message": "Transaction gas limit exceeds block gas limit",
    "details": {
      "submitted_gas_limit": 50000000,
      "block_gas_limit": 30000000,
      "max_allowed_gas": 30000000
    },
    "suggestion": "Reduce the gas limit to be within the block gas limit",
    "documentation_url": "https://api.chiral-network.com/docs/errors#gas_limit_exceeded",
    "geth_error": "exceeds block gas limit"
  }
}
```

**Replacement Transaction Underpriced (400 Bad Request)**
```json
{
  "status": "error",
  "error": {
    "code": "REPLACEMENT_UNDERPRICED",
    "message": "Replacement transaction must have higher gas price",
    "details": {
      "existing_gas_price": "20000000000",
      "submitted_gas_price": "22000000000",
      "minimum_required_gas_price": "24000000000",
      "minimum_increase_percent": 20
    },
    "suggestion": "Increase gas price by at least 20% above the existing transaction",
    "documentation_url": "https://api.chiral-network.com/docs/errors#replacement_underpriced",
    "geth_error": "replacement transaction underpriced"
  }
}
```

**Transaction Pool Full (429 Too Many Requests)**
```json
{
  "status": "error",
  "error": {
    "code": "MEMPOOL_FULL",
    "message": "Network transaction pool is full",
    "details": {
      "current_pool_size": 4096,
      "max_pool_size": 4096,
      "estimated_wait_time": "30-60 seconds"
    },
    "suggestion": "Wait and retry, or increase gas price for priority processing",
    "documentation_url": "https://api.chiral-network.com/docs/errors#mempool_full",
    "geth_error": "txpool is full"
  }
}
```

**Invalid Transaction Format (400 Bad Request)**
```json
{
  "status": "error",
  "error": {
    "code": "INVALID_TRANSACTION_FORMAT",
    "message": "Invalid transaction format or signature",
    "details": {
      "validation_failure": "invalid signature recovery",
      "field": "signature"
    },
    "suggestion": "Verify the transaction was signed with the correct private key and chain ID (98765)",
    "documentation_url": "https://api.chiral-network.com/docs/errors#invalid_transaction_format",
    "geth_error": "invalid transaction v, r, s values"
  }
}
```

---

### 2. Transaction Status

**Endpoint**: `GET /transactions/{transaction_hash}`

Retrieve detailed transaction status and information.

#### Request Format
```http
GET /transactions/0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef
```

#### Success Response (200 OK)
```json
{
  "status": "success",
  "data": {
    "transaction_hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
    "status": "success",
    "block_number": 12345,
    "block_hash": "0x9876543210fedcba9876543210fedcba9876543210fedcba9876543210fedcba",
    "transaction_index": 2,
    "gas_used": 21000,
    "effective_gas_price": "20000000000",
    "confirmations": 12,
    "from_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
    "to_address": "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
    "value": "1500000000000000000",
    "nonce": 15,
    "logs": [],
    "confirmation_time": "2025-01-20T10:31:15Z",
    "submission_time": "2025-01-20T10:30:00Z"
  }
}
```

#### Transaction Status Values
- `submitted`: Transaction accepted by Geth mempool
- `pending`: Transaction in mempool, not yet mined
- `success`: Transaction mined with status = 1
- `failed`: Transaction mined with status = 0 (reverted)
- `not_found`: Transaction hash not found in node

#### Failed Transaction Response
```json
{
  "status": "success",
  "data": {
    "transaction_hash": "0x1234...",
    "status": "failed",
    "block_number": 12345,
    "gas_used": 21000,
    "failure_reason": "execution reverted",
    "error_message": "insufficient balance for transfer",
    "logs": []
  }
}
```

---

### 3. Transaction History

**Endpoint**: `GET /transactions`

Get paginated transaction history for an address.

#### Request Format
```http
GET /transactions?address=0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266&limit=20&status=success
```

#### Query Parameters
- `address` (string, required): Ethereum address
- `limit` (integer, optional): Number of transactions (default: 20, max: 100)
- `offset` (integer, optional): Pagination offset (default: 0)
- `status` (string, optional): Filter by status
- `from_block` (integer, optional): Start block number
- `to_block` (integer, optional): End block number
- `min_amount` (string, optional): Minimum value in ETH
- `max_amount` (string, optional): Maximum value in ETH

#### Success Response (200 OK)
```json
{
  "status": "success",
  "data": {
    "transactions": [
      {
        "transaction_hash": "0x1234...",
        "from_address": "0xf39fd6e51...",
        "to_address": "0x70997970...",
        "value": "1.5",
        "status": "success",
        "block_number": 12345,
        "gas_used": 21000,
        "gas_price": "20000000000",
        "timestamp": "2025-01-20T10:31:15Z",
        "confirmations": 25
      }
    ],
    "pagination": {
      "total": 150,
      "limit": 20,
      "offset": 0,
      "has_more": true
    },
    "summary": {
      "total_sent": "45.7 ETH",
      "total_received": "23.1 ETH",
      "total_gas_spent": "0.0842 ETH",
      "transaction_count": 150
    }
  }
}
```

---

## Value-Added Helper Endpoints

### 4. Get Address Nonce

**Endpoint**: `GET /addresses/{address}/nonce`

Get the next valid nonce for transaction signing.

#### Request Format
```http
GET /addresses/0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266/nonce
```

#### Success Response (200 OK)
```json
{
  "status": "success",
  "data": {
    "address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
    "next_nonce": 15,
    "pending_count": 2,
    "confirmed_count": 13
  }
}
```

---

### 5. Transaction Estimation

**Endpoint**: `POST /transactions/estimate`

Estimate gas costs and validate transaction parameters before signing.

#### Request Format
```http
POST /transactions/estimate
Content-Type: application/json

{
  "from": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
  "to": "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
  "value": "1500000000000000000",
  "data": "0x"
}
```

#### Success Response (200 OK)
```json
{
  "status": "success",
  "data": {
    "gas_estimate": 21000,
    "gas_price": {
      "slow": "20000000000",
      "standard": "25000000000",
      "fast": "30000000000"
    },
    "total_cost": {
      "slow": "1.0004 ETH",
      "standard": "1.000525 ETH",
      "fast": "1.00063 ETH"
    },
    "validation": {
      "sufficient_balance": true,
      "valid_recipient": true,
      "account_balance": "2.5 ETH"
    },
    "recommended_nonce": 15
  }
}
```

---

### 6. Network Gas Prices

**Endpoint**: `GET /network/gas-price`

Get current recommended gas prices for different confirmation speeds.

#### Request Format
```http
GET /network/gas-price
```

#### Success Response (200 OK)
```json
{
  "status": "success",
  "data": {
    "timestamp": "2025-01-20T10:30:00Z",
    "prices": {
      "slow": {
        "gas_price": "20000000000",
        "estimated_time": "~2 minutes"
      },
      "standard": {
        "gas_price": "25000000000",
        "estimated_time": "~1 minute"
      },
      "fast": {
        "gas_price": "30000000000",
        "estimated_time": "~30 seconds"
      }
    },
    "network_congestion": "moderate",
    "base_fee": "18000000000",
    "priority_fee": {
      "slow": "2000000000",
      "standard": "7000000000",
      "fast": "12000000000"
    }
  }
}
```

---

### 7. Network Status

**Endpoint**: `GET /network/status`

Get current network and node health information.

#### Request Format
```http
GET /network/status
```

#### Success Response (200 OK)
```json
{
  "status": "success",
  "data": {
    "network_id": 98765,
    "latest_block": 12345,
    "peer_count": 8,
    "is_syncing": false,
    "sync_progress": null,
    "node_version": "geth/v1.10.0",
    "network_hashrate": "156.7 KH/s",
    "difficulty": "2.5K",
    "average_block_time": 15,
    "mempool_size": 42,
    "suggested_gas_price": "20000000000",
    "chain_id": 98765
  }
}
```

---

## Implementation Architecture

### Backend Implementation (Rust)

```rust
use serde_json::json;
use web3::types::Transaction;
use reqwest::Client;

#[derive(Debug)]
pub struct EnrichedApiError {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
    pub suggestion: String,
    pub documentation_url: String,
    pub geth_error: String,
}

// Main broadcast function - delegates to Geth with error enrichment
async fn broadcast_transaction(req: BroadcastRequest) -> Result<BroadcastResponse, ApiError> {
    let client = Client::new();

    // Direct delegation to Geth's eth_sendRawTransaction
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [req.signed_transaction_payload],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
        .json(&payload)
        .send()
        .await
        .map_err(|_| ApiError::ServiceUnavailable("NODE_UNAVAILABLE", "Cannot connect to Geth node"))?;

    let json_response: serde_json::Value = response.json().await?;

    // Handle Geth errors with enrichment
    if let Some(error) = json_response.get("error") {
        let geth_message = error["message"].as_str().unwrap_or("unknown error");
        let enriched_error = enrich_geth_error(geth_message, &req.signed_transaction_payload).await;
        return Err(enriched_error);
    }

    let tx_hash = json_response["result"].as_str()
        .ok_or(ApiError::InternalServerError("INVALID_RESPONSE", "Missing transaction hash"))?;

    Ok(BroadcastResponse {
        transaction_hash: tx_hash.to_string(),
        status: "submitted".to_string(),
        timestamp: chrono::Utc::now(),
    })
}

// Error enrichment with stable API contract
async fn enrich_geth_error(geth_message: &str, signed_tx: &str) -> ApiError {
    match geth_message {
        msg if msg.contains("nonce too low") => {
            if let Ok(decoded_tx) = decode_transaction(signed_tx) {
                if let Ok(expected_nonce) = get_transaction_count(&decoded_tx.sender).await {
                    return ApiError::BadRequest(EnrichedApiError {
                        code: "NONCE_TOO_LOW".to_string(),
                        message: "The transaction nonce is lower than the next valid nonce for the sender's account".to_string(),
                        details: json!({
                            "sender_address": decoded_tx.sender,
                            "submitted_nonce": decoded_tx.nonce,
                            "expected_nonce": expected_nonce,
                            "difference": decoded_tx.nonce as i64 - expected_nonce as i64
                        }),
                        suggestion: "Use GET /addresses/{address}/nonce to get the correct nonce, then re-sign and resubmit the transaction".to_string(),
                        documentation_url: "https://api.chiral-network.com/docs/errors#nonce_too_low".to_string(),
                        geth_error: msg.to_string(),
                    });
                }
            }
            ApiError::Conflict("NONCE_TOO_LOW", msg)
        },

        msg if msg.contains("insufficient funds") => {
            if let Ok(decoded_tx) = decode_transaction(signed_tx) {
                if let Ok(balance) = get_balance(&decoded_tx.sender).await {
                    let gas_cost = decoded_tx.gas_limit * decoded_tx.gas_price;
                    let total_required = decoded_tx.value + gas_cost;
                    let shortfall = if total_required > balance { total_required - balance } else { 0 };

                    return ApiError::BadRequest(EnrichedApiError {
                        code: "INSUFFICIENT_FUNDS".to_string(),
                        message: "Account balance is insufficient to cover transaction value and gas costs".to_string(),
                        details: json!({
                            "sender_address": decoded_tx.sender,
                            "account_balance": format!("{:.6} ETH", balance as f64 / 1e18),
                            "transaction_value": format!("{:.6} ETH", decoded_tx.value as f64 / 1e18),
                            "gas_cost": format!("{:.6} ETH", gas_cost as f64 / 1e18),
                            "total_required": format!("{:.6} ETH", total_required as f64 / 1e18),
                            "shortfall": format!("{:.6} ETH", shortfall as f64 / 1e18)
                        }),
                        suggestion: "Either reduce the transaction amount or add more ETH to the sender's account".to_string(),
                        documentation_url: "https://api.chiral-network.com/docs/errors#insufficient_funds".to_string(),
                        geth_error: msg.to_string(),
                    });
                }
            }
            ApiError::BadRequest("INSUFFICIENT_FUNDS", msg)
        },

        // Additional error mappings...
        msg => ApiError::InternalServerError("NETWORK_ERROR", msg)
    }
}

// Helper endpoints
async fn get_address_nonce(address: String) -> Result<NonceResponse, ApiError> {
    let pending_nonce = get_transaction_count(&address).await?;
    let confirmed_nonce = get_transaction_count_at_block(&address, "latest").await?;

    Ok(NonceResponse {
        address,
        next_nonce: pending_nonce,
        pending_count: pending_nonce - confirmed_nonce,
        confirmed_count: confirmed_nonce,
    })
}

async fn estimate_transaction(req: EstimateRequest) -> Result<EstimateResponse, ApiError> {
    let gas_estimate = estimate_gas(&req).await?;
    let gas_prices = get_recommended_gas_prices().await?;
    let balance = get_balance(&req.from).await?;
    let next_nonce = get_transaction_count(&req.from).await?;

    Ok(EstimateResponse {
        gas_estimate,
        gas_price: gas_prices,
        validation: ValidationResult {
            sufficient_balance: balance >= (req.value + gas_estimate * gas_prices.standard),
            valid_recipient: is_valid_address(&req.to),
            account_balance: format!("{:.6} ETH", balance as f64 / 1e18),
        },
        recommended_nonce: next_nonce,
    })
}
```

### Integration with Existing Chiral Functions

The implementation maintains compatibility with existing functions:

```rust
// Existing functions used
use crate::ethereum::{
    send_chiral_transaction,         // Replaced by direct Geth calls
    get_transaction_receipt,         // Used for status endpoint
    get_balance,                     // Used for balance checks
    get_peer_count,                  // Used for network status
    get_block_number,               // Used for confirmations
    get_network_difficulty,         // Used for network status
    get_network_hashrate,           // Used for network status
};

// Transaction status using existing function
async fn get_enhanced_transaction_status(tx_hash: String) -> Result<TransactionStatusResponse, ApiError> {
    match get_transaction_receipt(&tx_hash).await {
        Ok(receipt) => {
            let confirmations = if let Some(block_num) = receipt.block_number {
                get_block_number().await.unwrap_or(0).saturating_sub(block_num)
            } else {
                0
            };

            Ok(TransactionStatusResponse {
                transaction_hash: tx_hash,
                status: receipt.status,
                block_number: receipt.block_number,
                gas_used: receipt.gas_used,
                confirmations,
            })
        },
        Err(e) if e.contains("not found") => {
            Ok(TransactionStatusResponse {
                transaction_hash: tx_hash,
                status: "not_found".to_string(),
                block_number: None,
                gas_used: None,
                confirmations: 0,
            })
        },
        Err(e) => Err(ApiError::InternalServerError("RECEIPT_ERROR", &e))
    }
}
```

---

## Client Integration Examples

### JavaScript/TypeScript Client

```typescript
import { ethers } from 'ethers';

class ChiralTransactionClient {
  private apiUrl: string;

  constructor(apiUrl: string = 'https://api.chiral-network.com/v1') {
    this.apiUrl = apiUrl;
  }

  // Get recommended nonce
  async getNonce(address: string): Promise<number> {
    const response = await fetch(`${this.apiUrl}/addresses/${address}/nonce`);
    const data = await response.json();
    return data.data.next_nonce;
  }

  // Estimate transaction costs
  async estimateTransaction(from: string, to: string, value: string): Promise<any> {
    const response = await fetch(`${this.apiUrl}/transactions/estimate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ from, to, value, data: '0x' })
    });
    return response.json();
  }

  // Send transaction
  async sendTransaction(wallet: ethers.Wallet, to: string, value: string): Promise<string> {
    // Get current gas prices and nonce
    const [gasData, nonce] = await Promise.all([
      this.estimateTransaction(wallet.address, to, value),
      this.getNonce(wallet.address)
    ]);

    // Create transaction
    const tx = {
      to,
      value: ethers.parseEther(value),
      gasLimit: gasData.data.gas_estimate,
      gasPrice: gasData.data.gas_price.standard,
      nonce,
      chainId: 98765
    };

    // Sign transaction
    const signedTx = await wallet.signTransaction(tx);

    // Broadcast via API
    const response = await fetch(`${this.apiUrl}/transactions/broadcast`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ signed_transaction_payload: signedTx })
    });

    if (!response.ok) {
      const error = await response.json();
      throw new Error(`Transaction failed: ${error.error.message}`);
    }

    const result = await response.json();
    return result.data.transaction_hash;
  }

  // Monitor transaction status
  async waitForTransaction(txHash: string, confirmations: number = 1): Promise<any> {
    while (true) {
      const response = await fetch(`${this.apiUrl}/transactions/${txHash}`);
      const data = await response.json();

      if (data.data.status === 'success' && data.data.confirmations >= confirmations) {
        return data.data;
      } else if (data.data.status === 'failed') {
        throw new Error('Transaction failed');
      }

      // Wait before checking again
      await new Promise(resolve => setTimeout(resolve, 5000));
    }
  }
}
```

### Python Client

```python
import requests
from eth_account import Account
from web3 import Web3

class ChiralTransactionClient:
    def __init__(self, api_url='https://api.chiral-network.com/v1'):
        self.api_url = api_url
        self.w3 = Web3()

    def get_nonce(self, address: str) -> int:
        response = requests.get(f'{self.api_url}/addresses/{address}/nonce')
        return response.json()['data']['next_nonce']

    def estimate_transaction(self, from_addr: str, to_addr: str, value: str) -> dict:
        response = requests.post(f'{self.api_url}/transactions/estimate', json={
            'from': from_addr,
            'to': to_addr,
            'value': str(int(float(value) * 1e18)),
            'data': '0x'
        })
        return response.json()

    def send_transaction(self, private_key: str, to_addr: str, value: str) -> str:
        account = Account.from_key(private_key)

        # Get gas data and nonce
        gas_data = self.estimate_transaction(account.address, to_addr, value)
        nonce = self.get_nonce(account.address)

        # Create transaction
        tx = {
            'to': to_addr,
            'value': int(float(value) * 1e18),
            'gas': gas_data['data']['gas_estimate'],
            'gasPrice': int(gas_data['data']['gas_price']['standard']),
            'nonce': nonce,
            'chainId': 98765
        }

        # Sign transaction
        signed = self.w3.eth.account.sign_transaction(tx, private_key)

        # Broadcast
        response = requests.post(f'{self.api_url}/transactions/broadcast', json={
            'signed_transaction_payload': signed.rawTransaction.hex()
        })

        if response.status_code != 202:
            error = response.json()
            raise Exception(f"Transaction failed: {error['error']['message']}")

        return response.json()['data']['transaction_hash']
```

---

## Error Handling Best Practices

### Client-Side Error Handling

```typescript
interface ApiError {
  code: string;
  message: string;
  details: any;
  suggestion: string;
  documentation_url: string;
  geth_error: string;
}

class TransactionError extends Error {
  public code: string;
  public details: any;
  public suggestion: string;

  constructor(apiError: ApiError) {
    super(apiError.message);
    this.code = apiError.code;
    this.details = apiError.details;
    this.suggestion = apiError.suggestion;
  }

  // Helper methods for common error types
  isNonceTooLow(): boolean {
    return this.code === 'NONCE_TOO_LOW';
  }

  isInsufficientFunds(): boolean {
    return this.code === 'INSUFFICIENT_FUNDS';
  }

  isGasPriceTooLow(): boolean {
    return this.code === 'GAS_PRICE_TOO_LOW';
  }

  // Get suggested remediation
  getFixSuggestion(): string {
    return this.suggestion;
  }
}

// Usage example with automatic retry
async function sendTransactionWithRetry(
  client: ChiralTransactionClient,
  wallet: ethers.Wallet,
  to: string,
  value: string,
  maxRetries: number = 3
): Promise<string> {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      return await client.sendTransaction(wallet, to, value);
    } catch (error) {
      if (error instanceof TransactionError) {
        if (error.isNonceTooLow() && attempt < maxRetries - 1) {
          // Nonce issue - wait and retry (nonce will be refreshed)
          await new Promise(resolve => setTimeout(resolve, 1000));
          continue;
        } else if (error.isGasPriceTooLow() && attempt < maxRetries - 1) {
          // Gas price too low - could implement automatic gas price increase
          console.warn(`Gas price too low, suggested: ${error.details.suggested_gas_price}`);
          throw error; // For now, let client handle
        }
      }
      throw error;
    }
  }
  throw new Error(`Transaction failed after ${maxRetries} attempts`);
}
```

---

## Security Considerations

### Client-Side Security

1. **Private Key Protection**
   - Never transmit private keys over network
   - Use secure key storage (hardware wallets, encrypted stores)
   - Implement proper key derivation for hierarchical deterministic wallets

2. **Transaction Signing**
   - Always verify transaction details before signing
   - Use deterministic signing (RFC 6979) to prevent replay attacks
   - Validate chain ID to prevent cross-chain replay

3. **Network Security**
   - Always use HTTPS for API communications
   - Implement certificate pinning for mobile applications
   - Validate API responses to prevent man-in-the-middle attacks

### Server-Side Security

1. **Input Validation**
   - Validate all transaction payloads for proper RLP encoding
   - Check signature validity and chain ID
   - Sanitize all user inputs

2. **Rate Limiting**
   - Implement per-IP rate limits
   - Add per-address transaction limits
   - Use exponential backoff for repeated failures

3. **Monitoring and Logging**
   - Log all transaction broadcast attempts
   - Monitor for unusual patterns or attacks
   - Never log sensitive data (private keys, raw transactions)

---

## Performance and Scalability

### Caching Strategy

1. **Gas Price Caching**
   - Cache gas price recommendations for 30 seconds
   - Update based on network congestion

2. **Nonce Caching**
   - Short-term nonce caching (5 seconds) to reduce node load
   - Invalidate on transaction broadcast

3. **Transaction Status Caching**
   - Cache confirmed transaction data indefinitely
   - Cache pending transactions for 30 seconds

### Load Balancing

1. **Multiple Geth Nodes**
   - Load balance across multiple Geth instances
   - Health check nodes before routing requests
   - Failover to backup nodes on errors

2. **API Scaling**
   - Horizontal scaling with load balancers
   - Connection pooling for Geth JSON-RPC
   - Async processing for non-critical operations

---

## Monitoring and Observability

### Key Metrics

1. **Transaction Metrics**
   - Transaction broadcast success rate
   - Average confirmation time
   - Gas price accuracy

2. **API Performance**
   - Request latency (p50, p95, p99)
   - Error rates by endpoint
   - Throughput (requests per second)

3. **Network Health**
   - Geth node connectivity
   - Peer count and sync status
   - Block processing times

### Alerting

1. **Critical Alerts**
   - Geth node disconnection
   - High error rates (>5%)
   - Transaction broadcast failures

2. **Warning Alerts**
   - High latency (>2s for broadcasts)
   - Low peer count (<3 peers)
   - Mempool congestion

---

## Testing Strategy

### Unit Tests

1. **Error Mapping Tests**
   - Test all Geth error message mappings
   - Verify error enrichment logic
   - Test edge cases and malformed inputs

2. **Helper Endpoint Tests**
   - Nonce calculation accuracy
   - Gas estimation validation
   - Network status reporting

### Integration Tests

1. **End-to-End Transaction Flow**
   - Complete transaction lifecycle testing
   - Error handling validation
   - Performance benchmarking

2. **Network Interaction Tests**
   - Geth node compatibility testing
   - Network congestion handling
   - Failover scenarios

### Load Testing

1. **Concurrent Transaction Broadcasting**
   - Test with high transaction volumes
   - Validate rate limiting
   - Monitor resource usage

2. **API Scalability Testing**
   - Test horizontal scaling
   - Database performance under load
   - Cache effectiveness

---

## Deployment and Operations

### Environment Configuration

1. **Development Environment**
   - Local Geth node with test data
   - Mock transaction responses
   - Debug logging enabled

2. **Staging Environment**
   - Testnet Geth connectivity
   - Production-like configuration
   - Performance monitoring

3. **Production Environment**
   - Mainnet Geth cluster
   - Full monitoring and alerting
   - Automated failover

### Deployment Process

1. **Blue-Green Deployment**
   - Zero-downtime deployments
   - Automatic rollback on failure
   - Health checks during deployment

2. **Database Migrations**
   - Backward-compatible schema changes
   - Data migration validation
   - Rollback procedures

3. **Configuration Management**
   - Environment-specific configs
   - Secret management
   - Feature flags for gradual rollouts

---

## Conclusion

This API specification provides a robust, secure, and developer-friendly interface for Chiral Network transactions. By combining the reliability of Geth's native validation with enriched error handling and value-added endpoints, it delivers an excellent developer experience while maintaining backend simplicity and security.

The design successfully addresses the key requirements:
- **Security**: Client-side signing protects private keys
- **Reliability**: Delegates validation to battle-tested Geth
- **Usability**: Provides actionable errors and helpful utilities
- **Maintainability**: Stable error contract independent of Geth versions

This specification serves as the foundation for building production-ready transaction systems on the Chiral Network.
use serde::{Deserialize, Serialize};

// ============================================================================
// Response/Error Wrapper Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T> {
    pub status: ResponseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            status: ResponseStatus::Success,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(code: String, message: String, details: Option<serde_json::Value>) -> Self {
        Self {
            status: ResponseStatus::Error,
            data: None,
            error: Some(ApiError {
                code,
                message,
                details,
            }),
        }
    }
}

// ============================================================================
// 1. Broadcast Transaction Types
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct BroadcastTransactionRequest {
    pub signed_transaction_payload: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BroadcastTransactionData {
    pub transaction_hash: String,
    pub status: String,
    pub timestamp: String,
}

// ============================================================================
// 2. Transaction Status Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct TransactionStatusData {
    pub transaction_hash: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_gas_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u32>,
    pub from_address: String,
    pub to_address: String,
    pub value: String,
    pub nonce: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation_time: Option<String>,
    pub submission_time: String,
}

// ============================================================================
// 3. Transaction History Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct TransactionHistoryItem {
    pub transaction_hash: String,
    pub from_address: String,
    pub to_address: String,
    pub value: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionHistoryPagination {
    pub total: u64,
    pub offset: u32,
    pub limit: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionHistorySummary {
    pub total_sent: String,
    pub total_received: String,
    pub total_fees: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionHistoryData {
    pub transactions: Vec<TransactionHistoryItem>,
    pub pagination: TransactionHistoryPagination,
    pub summary: TransactionHistorySummary,
}

// ============================================================================
// 4. Address Nonce Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct AddressNonceData {
    pub address: String,
    pub next_nonce: u64,
    pub pending_count: u32,
    pub confirmed_count: u64,
}

// ============================================================================
// 5. Transaction Estimation Types
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionEstimateRequest {
    pub from: String,
    pub to: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GasPriceInfo {
    pub slow: String,
    pub standard: String,
    pub fast: String,
    pub instant: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TotalCostInfo {
    pub min: String,
    pub standard: String,
    pub max: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationInfo {
    pub is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionEstimateData {
    pub gas_estimate: u64,
    pub gas_price: GasPriceInfo,
    pub total_cost: TotalCostInfo,
    pub validation: ValidationInfo,
    pub recommended_nonce: u64,
}

// ============================================================================
// 6. Network Gas Price Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct GasPriceData {
    pub timestamp: String,
    pub prices: GasPriceInfo,
}

// ============================================================================
// 7. Network Status Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct NetworkStatusData {
    pub network_id: u64,
    pub latest_block: u64,
    pub peer_count: u32,
    pub is_syncing: bool,
}

// ============================================================================
// Tauri Command Implementations
// ============================================================================

/// 1. Broadcast a pre-signed transaction to the Chiral network
#[tauri::command]
pub async fn broadcast_transaction(
    signed_transaction_payload: String,
) -> Result<ApiResponse<BroadcastTransactionData>, String> {
    // Mock implementation - In production, this would call the Geth RPC
    let transaction_hash = format!(
        "0x{}",
        format!("{:x}", md5::compute(&signed_transaction_payload))
    );

    let data = BroadcastTransactionData {
        transaction_hash,
        status: "submitted".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    Ok(ApiResponse::success(data))
}

/// 2. Get detailed transaction status and information
#[tauri::command]
pub async fn get_transaction_status(
    transaction_hash: String,
) -> Result<ApiResponse<TransactionStatusData>, String> {
    // Mock implementation
    let data = TransactionStatusData {
        transaction_hash: transaction_hash.clone(),
        status: "success".to_string(),
        block_number: Some(12345),
        block_hash: Some("0x9876543210abcdef9876543210abcdef9876543210abcdef9876543210abcdef".to_string()),
        transaction_index: Some(2),
        gas_used: Some(21000),
        effective_gas_price: Some("20000000000".to_string()),
        confirmations: Some(12),
        from_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
        to_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8".to_string(),
        value: "1500000000000000000".to_string(),
        nonce: 15,
        logs: Some(vec![]),
        confirmation_time: Some(chrono::Utc::now().to_rfc3339()),
        submission_time: chrono::Utc::now().to_rfc3339(),
    };

    Ok(ApiResponse::success(data))
}

/// 3. Get paginated transaction history for an address
#[tauri::command]
pub async fn get_transaction_history(
    address: String,
    limit: Option<u32>,
    offset: Option<u32>,
    status: Option<String>,
) -> Result<ApiResponse<TransactionHistoryData>, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);

    // Mock implementation
    let transactions = vec![
        TransactionHistoryItem {
            transaction_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            from_address: address.clone(),
            to_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8".to_string(),
            value: "1.5".to_string(),
            status: status.clone().unwrap_or_else(|| "success".to_string()),
            block_number: Some(12345),
            timestamp: chrono::Utc::now().to_rfc3339(),
        },
    ];

    let data = TransactionHistoryData {
        transactions,
        pagination: TransactionHistoryPagination {
            total: 1,
            offset,
            limit,
            has_more: false,
        },
        summary: TransactionHistorySummary {
            total_sent: "1.5".to_string(),
            total_received: "0".to_string(),
            total_fees: "0.00042".to_string(),
        },
    };

    Ok(ApiResponse::success(data))
}

/// 4. Get the next valid nonce for transaction signing
#[tauri::command]
pub async fn get_address_nonce(address: String) -> Result<ApiResponse<AddressNonceData>, String> {
    // Mock implementation
    let data = AddressNonceData {
        address: address.clone(),
        next_nonce: 15,
        pending_count: 2,
        confirmed_count: 13,
    };

    Ok(ApiResponse::success(data))
}

/// 5. Estimate gas costs and validate transaction parameters
#[tauri::command]
pub async fn estimate_transaction(
    _from: String,
    _to: String,
    _value: String,
    _data: Option<String>,
) -> Result<ApiResponse<TransactionEstimateData>, String> {
    // Mock implementation
    let estimate = TransactionEstimateData {
        gas_estimate: 21000,
        gas_price: GasPriceInfo {
            slow: "10000000000".to_string(),
            standard: "20000000000".to_string(),
            fast: "30000000000".to_string(),
            instant: "40000000000".to_string(),
        },
        total_cost: TotalCostInfo {
            min: "0.00021".to_string(),
            standard: "0.00042".to_string(),
            max: "0.00084".to_string(),
        },
        validation: ValidationInfo {
            is_valid: true,
            warnings: None,
        },
        recommended_nonce: 15,
    };

    Ok(ApiResponse::success(estimate))
}

/// 6. Get current recommended gas prices
#[tauri::command]
pub async fn get_network_gas_price() -> Result<ApiResponse<GasPriceData>, String> {
    // Mock implementation
    let data = GasPriceData {
        timestamp: chrono::Utc::now().to_rfc3339(),
        prices: GasPriceInfo {
            slow: "10000000000".to_string(),
            standard: "20000000000".to_string(),
            fast: "30000000000".to_string(),
            instant: "40000000000".to_string(),
        },
    };

    Ok(ApiResponse::success(data))
}

/// 7. Get current network and node health information
#[tauri::command]
pub async fn get_network_status() -> Result<ApiResponse<NetworkStatusData>, String> {
    // Mock implementation
    let data = NetworkStatusData {
        network_id: 98765,
        latest_block: 12345,
        peer_count: 8,
        is_syncing: false,
    };

    Ok(ApiResponse::success(data))
}

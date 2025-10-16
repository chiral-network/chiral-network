use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumJob {
    pub job_id: String,
    pub prevhash: String,
    pub coinb1: String,
    pub coinb2: String,
    pub merkle_branch: Vec<String>,
    pub version: String,
    pub nbits: String,
    pub ntime: String,
    pub clean_jobs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumResponse {
    pub id: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<StratumError>,
    pub method: Option<String>,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumError {
    pub code: i32,
    pub message: String,
    pub traceback: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub connected_miners: u32,
    pub pool_hashrate: String,
    pub your_hashrate: String,
    pub your_share_percentage: f64,
    pub shares_submitted: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub estimated_payout_24h: f64,
    pub last_share_time: u64,
}

pub struct StratumClient {
    pool_url: String,
    pool_port: u16,
    worker_name: String,
    password: String,
    stream: Option<Arc<Mutex<TcpStream>>>,
    subscription_id: Option<String>,
    extranonce1: Option<String>,
    extranonce2_size: Option<usize>,
    current_job: Option<StratumJob>,
    authorized: bool,
    request_id: u64,
    stats: Arc<Mutex<PoolStats>>,
}

impl StratumClient {
    pub fn new(pool_url: String, pool_port: u16, worker_name: String, password: String) -> Self {
        Self {
            pool_url,
            pool_port,
            worker_name,
            password,
            stream: None,
            subscription_id: None,
            extranonce1: None,
            extranonce2_size: None,
            current_job: None,
            authorized: false,
            request_id: 0,
            stats: Arc::new(Mutex::new(PoolStats {
                connected_miners: 0,
                pool_hashrate: "0 H/s".to_string(),
                your_hashrate: "0 H/s".to_string(),
                your_share_percentage: 0.0,
                shares_submitted: 0,
                shares_accepted: 0,
                shares_rejected: 0,
                estimated_payout_24h: 0.0,
                last_share_time: 0,
            })),
        }
    }

    pub async fn connect(&mut self) -> Result<(), String> {
        let addr = format!("{}:{}", self.pool_url, self.pool_port);
        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| format!("Failed to connect to pool: {}", e))?;

        self.stream = Some(Arc::new(Mutex::new(stream)));

        // Subscribe to mining
        self.subscribe().await?;

        // Authorize worker
        self.authorize().await?;

        Ok(())
    }

    async fn subscribe(&mut self) -> Result<(), String> {
        let request = json!({
            "id": self.next_request_id(),
            "method": "mining.subscribe",
            "params": ["chiral-miner/1.0.0"]
        });

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(format!("Subscribe error: {}", error.message));
        }

        if let Some(result) = response.result {
            if let Some(arr) = result.as_array() {
                if arr.len() >= 3 {
                    self.subscription_id = arr[1].as_str().map(|s| s.to_string());
                    self.extranonce1 = arr[1].as_str().map(|s| s.to_string());
                    self.extranonce2_size = arr[2].as_u64().map(|n| n as usize);
                }
            }
        }

        Ok(())
    }

    async fn authorize(&mut self) -> Result<(), String> {
        let request = json!({
            "id": self.next_request_id(),
            "method": "mining.authorize",
            "params": [self.worker_name.clone(), self.password.clone()]
        });

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(format!("Authorization error: {}", error.message));
        }

        if let Some(result) = response.result {
            self.authorized = result.as_bool().unwrap_or(false);
            if !self.authorized {
                return Err("Authorization failed".to_string());
            }
        }

        Ok(())
    }

    pub async fn submit_work(
        &mut self,
        job_id: &str,
        extranonce2: &str,
        ntime: &str,
        nonce: &str,
    ) -> Result<bool, String> {
        if !self.authorized {
            return Err("Not authorized".to_string());
        }

        let request = json!({
            "id": self.next_request_id(),
            "method": "mining.submit",
            "params": [
                self.worker_name.clone(),
                job_id,
                extranonce2,
                ntime,
                nonce
            ]
        });

        let response = self.send_request(request).await?;

        let mut stats = self.stats.lock().await;
        stats.shares_submitted += 1;
        stats.last_share_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(error) = response.error {
            stats.shares_rejected += 1;
            return Err(format!("Submit error: {}", error.message));
        }

        if let Some(result) = response.result {
            let accepted = result.as_bool().unwrap_or(false);
            if accepted {
                stats.shares_accepted += 1;
            } else {
                stats.shares_rejected += 1;
            }
            return Ok(accepted);
        }

        Ok(false)
    }

    pub async fn listen_for_jobs(&mut self) -> Result<(), String> {
        let stream = self
            .stream
            .as_ref()
            .ok_or("Not connected")?
            .clone();

        let stream_locked = stream.lock().await;
        let (reader, _writer) = tokio::io::split(&*stream_locked);
        drop(stream_locked);

        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    return Err("Connection closed by pool".to_string());
                }
                Ok(_) => {
                    if let Ok(response) = serde_json::from_str::<StratumResponse>(&line) {
                        self.handle_message(response).await?;
                    }
                }
                Err(e) => {
                    return Err(format!("Read error: {}", e));
                }
            }
        }
    }

    async fn handle_message(&mut self, message: StratumResponse) -> Result<(), String> {
        if let Some(method) = message.method {
            match method.as_str() {
                "mining.notify" => {
                    if let Some(params) = message.params {
                        self.handle_mining_notify(params).await?;
                    }
                }
                "mining.set_difficulty" => {
                    if let Some(params) = message.params {
                        self.handle_set_difficulty(params).await?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn handle_mining_notify(&mut self, params: serde_json::Value) -> Result<(), String> {
        if let Some(arr) = params.as_array() {
            if arr.len() >= 9 {
                let job = StratumJob {
                    job_id: arr[0].as_str().unwrap_or("").to_string(),
                    prevhash: arr[1].as_str().unwrap_or("").to_string(),
                    coinb1: arr[2].as_str().unwrap_or("").to_string(),
                    coinb2: arr[3].as_str().unwrap_or("").to_string(),
                    merkle_branch: arr[4]
                        .as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                    version: arr[5].as_str().unwrap_or("").to_string(),
                    nbits: arr[6].as_str().unwrap_or("").to_string(),
                    ntime: arr[7].as_str().unwrap_or("").to_string(),
                    clean_jobs: arr[8].as_bool().unwrap_or(false),
                };
                self.current_job = Some(job);
            }
        }
        Ok(())
    }

    async fn handle_set_difficulty(&mut self, _params: serde_json::Value) -> Result<(), String> {
        // Handle difficulty change
        // Update local difficulty for share calculation
        Ok(())
    }

    async fn send_request(&mut self, request: serde_json::Value) -> Result<StratumResponse, String> {
        let stream = self
            .stream
            .as_ref()
            .ok_or("Not connected")?
            .clone();

        let mut stream_locked = stream.lock().await;
        let request_str = format!("{}\n", request.to_string());

        stream_locked
            .write_all(request_str.as_bytes())
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        drop(stream_locked);

        // Read response
        let stream_locked = stream.lock().await;
        let (reader, _writer) = tokio::io::split(&*stream_locked);
        drop(stream_locked);

        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Read error: {}", e))?;

        serde_json::from_str(&line).map_err(|e| format!("Parse error: {}", e))
    }

    fn next_request_id(&mut self) -> u64 {
        self.request_id += 1;
        self.request_id
    }

    pub async fn disconnect(&mut self) -> Result<(), String> {
        if let Some(stream) = self.stream.take() {
            let stream_locked = stream.lock().await;
            drop(stream_locked);
        }
        self.authorized = false;
        self.current_job = None;
        Ok(())
    }

    pub async fn get_stats(&self) -> PoolStats {
        self.stats.lock().await.clone()
    }

    pub fn get_current_job(&self) -> Option<StratumJob> {
        self.current_job.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some() && self.authorized
    }

    pub async fn update_hashrate(&self, hashrate: String) {
        let mut stats = self.stats.lock().await;
        stats.your_hashrate = hashrate;
    }

    pub async fn calculate_payout_estimate(&self, block_reward: f64) {
        let stats = self.stats.lock().await;
        if stats.shares_submitted > 0 {
            let acceptance_rate = stats.shares_accepted as f64 / stats.shares_submitted as f64;
            // Estimate based on share percentage and acceptance rate
            let estimated_daily_blocks = 24.0 * 60.0 / 15.0; // Assuming 15s block time
            let estimated_payout = stats.your_share_percentage * block_reward * estimated_daily_blocks * acceptance_rate;
            drop(stats);
            let mut stats = self.stats.lock().await;
            stats.estimated_payout_24h = estimated_payout;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stratum_client_creation() {
        let client = StratumClient::new(
            "pool.example.com".to_string(),
            3333,
            "worker1".to_string(),
            "password".to_string(),
        );

        assert_eq!(client.pool_url, "pool.example.com");
        assert_eq!(client.pool_port, 3333);
        assert_eq!(client.worker_name, "worker1");
        assert!(!client.is_connected());
    }
}

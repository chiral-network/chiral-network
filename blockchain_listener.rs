use ethers::{
    prelude::*,
    providers::{Provider, Ws},
};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

use crate::dht;

// Define the Challenge event from your smart contract
// event Challenge(bytes32 indexed fileRoot, uint256 chunkIndex, address indexed challenger);
#[derive(Debug, Clone, EthEvent)]
pub struct ChallengeEvent {
    #[ethevent(name = "fileRoot", indexed)]
    pub file_root: [u8; 32],
    #[ethevent(name = "chunkIndex")]
    pub chunk_index: U256,
    #[ethevent(name = "challenger", indexed)]
    pub challenger: Address,
}

/// Listens for blockchain challenge events and triggers proof generation.
pub async fn run_blockchain_listener(
    ws_url: String,
    contract_address: String,
    dht_service: Arc<dht::DhtService>, // Assuming DhtService is accessible
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to blockchain node at {}...", ws_url);
    let provider = Provider::<Ws>::connect(&ws_url).await?;
    let client = Arc::new(provider);

    let contract_addr: Address = contract_address.parse()?;
    let contract = Contract::new(contract_addr, client.clone());

    println!(
        "Subscribing to Challenge events from contract: {}",
        contract_address
    );
    let events = contract.event::<ChallengeEvent>();
    let mut stream = events.subscribe().await?;

    // Main event loop
    while let Some(Ok(log)) = stream.next().await {
        println!("Received Challenge event: {:?}", log);

        // Spawn a new task to handle the challenge without blocking the listener
        let dht_clone = dht_service.clone();
        tokio::spawn(async move {
            handle_challenge(log, dht_clone).await;
        });
    }

    eprintln!("Blockchain listener stream ended.");
    Ok(())
}

/// Handles a single challenge event, with a timeout.
async fn handle_challenge(event: ChallengeEvent, dht_service: Arc<dht::DhtService>) {
    const RESPONSE_TIMEOUT_SECONDS: u64 = 120; // 2-minute timeout to respond

    println!(
        "Handling challenge for file root: 0x{}",
        hex::encode(event.file_root)
    );

    let response_future = file_handler::generate_and_submit_proof(
        hex::encode(event.file_root),
        event.chunk_index.as_u64(),
        dht_service,
    );

    match timeout(
        Duration::from_secs(RESPONSE_TIMEOUT_SECONDS),
        response_future,
    )
    .await
    {
        Ok(Ok(_)) => {
            println!(
                "Successfully submitted proof for file root: 0x{}",
                hex::encode(event.file_root)
            );
        }
        Ok(Err(e)) => {
            eprintln!(
                "Error handling challenge for file root 0x{}: {}",
                hex::encode(event.file_root),
                e
            );
            // Here you could implement logic to penalize the node for failing to respond.
        }
        Err(_) => {
            eprintln!(
                "Timeout: Failed to respond to challenge for file root 0x{} within {} seconds.",
                hex::encode(event.file_root),
                RESPONSE_TIMEOUT_SECONDS
            );
            // Penalize for missed response.
        }
    }
}
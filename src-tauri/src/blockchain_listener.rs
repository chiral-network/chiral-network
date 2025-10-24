use ethers::{
    prelude::*,
    // No longer need Abigen
    providers::{Provider, Ws},
};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

use crate::dht;

// The event name in Solidity is "ChallengeIssued"
#[derive(Debug, Clone, EthEvent)]
#[ethevent(name = "ChallengeIssued")]
pub struct ChallengeIssuedEvent {
    #[ethevent(name = "fileRoot", indexed)]
    pub file_root: [u8; 32],
    #[ethevent(name = "chunkIndex")]
    pub chunk_index: U256,
}

/// Listens for blockchain challenge events and triggers proof generation.
pub async fn run_blockchain_listener(
    ws_url: String,
    contract_address: String,
    dht_service: Arc<dht::DhtService>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to blockchain node at {}...", ws_url);
    let provider: Provider<Ws> = Provider::<Ws>::connect(&ws_url).await?;
    let client = Arc::new(provider);

    // Create a filter for the "ChallengeIssued" event.
    // This is the correct way to listen for a specific event without the full contract ABI.
    let filter = Filter::new()
        .address(contract_address.parse::<Address>()?)
        .topic0(ChallengeIssuedEvent::signature());

    println!(
        "Subscribing to ChallengeIssued events from contract: {}",
        contract_address
    );
    let mut stream = client.subscribe_logs(&filter).await?;

    // Main event loop
    while let Some(log) = stream.next().await {
        // The stream provides raw logs; we need to decode them into our event struct.
        let event = <ChallengeIssuedEvent as EthLogDecode>::decode_log(&log.into())?;
        println!("Received Challenge event: {:?}", event);

        // Spawn a new task to handle the challenge without blocking the listener
        let dht_clone = dht_service.clone();
        tokio::spawn(async move { handle_challenge(event, dht_clone).await });
    }

    eprintln!("Blockchain listener stream ended.");
    Ok(())
}

/// Handles a single challenge event, with a timeout.
async fn handle_challenge(event: ChallengeIssuedEvent, dht_service: Arc<dht::DhtService>) {
    const RESPONSE_TIMEOUT_SECONDS: u64 = 120; // 2-minute timeout to respond

    println!(
        "Handling challenge for file root: 0x{}",
        hex::encode(event.file_root)
    );

    let response_future = dht_service
        .generate_and_submit_proof(hex::encode(event.file_root), event.chunk_index.as_u64());

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

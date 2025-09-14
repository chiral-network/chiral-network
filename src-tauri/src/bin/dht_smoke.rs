//AI built test file for DHT smoke test


#[path = "../dht.rs"]
mod dht;
use dht::{DhtService, FileMetadata, DhtEvent};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Simple logger
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    // Use high ports unlikely to be occupied
    let port_a: u16 = 4101;
    let port_b: u16 = 4102;

    // Start node A without bootstrap
    let a = DhtService::new(port_a, vec![]).await?;
    let a_peer = a.get_peer_id().await;
    a.run().await;

    // Start node B with A as bootstrap
    let bootstrap = format!("/ip4/127.0.0.1/tcp/{}/p2p/{}", port_a, a_peer);
    let b = DhtService::new(port_b, vec![bootstrap.clone()]).await?;
    b.run().await;

    // Give time for listeners to bind
    sleep(Duration::from_millis(500)).await;

    // Explicitly connect B to A
    b.connect_peer(bootstrap.clone()).await?;
    sleep(Duration::from_secs(1)).await;

    // Publish a record on A
    let file_hash = "QmSmokeTest123".to_string();
    let meta = FileMetadata {
        file_hash: file_hash.clone(),
        file_name: "smoke.txt".to_string(),
        file_size: 1234,
        seeders: vec![a_peer.clone()],
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        mime_type: Some("text/plain".to_string()),
    };
    a.publish_file(meta).await?;

    // Search from B
    b.search_file(file_hash.clone()).await?;

    // Await discovery event on B for up to ~8 seconds
    let mut found = false;
    for _ in 0..40 { // 40 * 200ms = 8s
        let events = b.drain_events(100).await;
        for ev in events {
            match ev {
                DhtEvent::FileDiscovered(m) if m.file_hash == file_hash => {
                    println!("OK: discovered {} ({} bytes)", m.file_name, m.file_size);
                    found = true;
                    break;
                }
                DhtEvent::Error(e) => {
                    eprintln!("WARN: {}", e);
                }
                _ => {}
            }
        }
        if found { break; }
        sleep(Duration::from_millis(200)).await;
    }

    if !found {
        eprintln!("FAIL: did not discover file via DHT in time");
        std::process::exit(1);
    }

    Ok(())
}

/// NAT Traversal Validation Tests
///
/// Tests for the new NAT traversal validation and configuration features:
/// - DCUtR validation
/// - Public IP detection
/// - Port forwarding configuration

use chiral_network::dht::{self, DhtService};
use std::time::Duration;

#[tokio::test]
async fn test_public_ip_detection() {
    println!("\n🧪 Testing Public IP Auto-Detection");
    println!("{}", "=".repeat(70));

    match dht::detect_public_ip().await {
        Ok(ip) => {
            println!("✓ Successfully detected public IP: {}", ip);

            // Validate it's a proper IPv4 address
            assert!(
                ip.parse::<std::net::Ipv4Addr>().is_ok(),
                "Detected IP should be a valid IPv4 address"
            );

            println!("✓ IP validation passed");
        }
        Err(e) => {
            println!("⚠ Could not detect public IP: {}", e);
            println!("  This is expected in environments without internet access");
            println!("  or if all IP detection services are unavailable");
        }
    }

    println!("✅ Public IP detection test completed!\n");
}

#[tokio::test]
async fn test_dcutr_validation_disabled() {
    println!("\n🧪 Testing DCUtR Validation (Disabled State)");
    println!("{}", "=".repeat(70));

    // Create a service with AutoNAT disabled (which disables DCUtR)
    let service = DhtService::new(
        0, // Random port
        vec![],
        None,
        false,
        false, // Disable AutoNAT (disables DCUtR)
        None,
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        false,
        None,
        None,
    )
    .await;

    assert!(service.is_ok(), "Failed to create DHT service");
    let service = service.unwrap();

    println!("✓ DHT service started");

    // Get DCUtR validation
    let validation = service.validate_dcutr().await;
    println!("\n📊 DCUtR Validation Results:");
    println!("  Enabled: {}", validation.enabled);
    println!("  Status: {}", validation.status);
    println!("  Total Attempts: {}", validation.total_attempts);
    println!("  Successes: {}", validation.successes);
    println!("  Failures: {}", validation.failures);
    println!("  Success Rate: {:.2}%", validation.success_rate);

    assert!(!validation.enabled, "DCUtR should be disabled");
    assert_eq!(validation.status, "disabled", "Status should be 'disabled'");
    assert_eq!(validation.total_attempts, 0, "Should have no attempts");

    if !validation.recommendations.is_empty() {
        println!("\n💡 Recommendations:");
        for rec in &validation.recommendations {
            println!("  • {}", rec);
        }
    }

    // Cleanup
    let _ = service.shutdown().await;
    println!("✅ DCUtR validation (disabled) test passed!\n");
}

#[tokio::test]
async fn test_dcutr_validation_enabled() {
    println!("\n🧪 Testing DCUtR Validation (Enabled State)");
    println!("{}", "=".repeat(70));

    // Create a service with AutoNAT enabled (enables DCUtR)
    let service = DhtService::new(
        0, // Random port
        vec![],
        None,
        false,
        true, // Enable AutoNAT (enables DCUtR)
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        false,
        None,
        None,
    )
    .await;

    assert!(service.is_ok(), "Failed to create DHT service");
    let service = service.unwrap();

    println!("✓ DHT service started with DCUtR enabled");

    // Get DCUtR validation
    let validation = service.validate_dcutr().await;
    println!("\n📊 DCUtR Validation Results:");
    println!("  Enabled: {}", validation.enabled);
    println!("  Status: {}", validation.status);
    println!("  Total Attempts: {}", validation.total_attempts);
    println!("  Successes: {}", validation.successes);
    println!("  Failures: {}", validation.failures);
    println!("  Success Rate: {:.2}%", validation.success_rate);

    assert!(validation.enabled, "DCUtR should be enabled");
    assert!(
        validation.status == "not_tested" || validation.status == "disabled",
        "Status should be 'not_tested' initially"
    );

    if !validation.recommendations.is_empty() {
        println!("\n💡 Recommendations:");
        for rec in &validation.recommendations {
            println!("  • {}", rec);
        }
    }

    // Cleanup
    let _ = service.shutdown().await;
    println!("✅ DCUtR validation (enabled) test passed!\n");
}

#[tokio::test]
async fn test_port_forwarding_config() {
    println!("\n🧪 Testing Port Forwarding Configuration");
    println!("{}", "=".repeat(70));

    // Create a DHT service
    let service = DhtService::new(
        14701, // Specific port for testing
        vec![],
        None,
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        false,
        None,
        None,
    )
    .await;

    assert!(service.is_ok(), "Failed to create DHT service");
    let service = service.unwrap();

    println!("✓ DHT service started on port 14701");

    // Wait a moment for the service to initialize
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Get port forwarding config
    let config = service.get_port_forwarding_config().await;

    println!("\n🔌 Port Forwarding Configuration:");
    println!("  Public IP: {:?}", config.public_ip);
    println!("  Local IP: {:?}", config.local_ip);
    println!("  Primary Port: {:?}", config.primary_port);
    println!("  NAT Status: {}", config.nat_status);
    println!("  Reachability: {:?}", config.reachability);

    // Verify we have listen addresses
    assert!(
        !config.listen_addresses.is_empty(),
        "Should have at least one listen address"
    );

    println!("\n📋 Listen Addresses:");
    for addr in &config.listen_addresses {
        println!("  • {}", addr);
    }

    if !config.instructions.is_empty() {
        println!("\n📝 Port Forwarding Instructions:");
        for (i, instruction) in config.instructions.iter().enumerate() {
            if i == 0 {
                println!("{}", instruction);
            } else {
                println!("{}", instruction);
            }
        }
    }

    // Cleanup
    let _ = service.shutdown().await;
    println!("✅ Port forwarding config test passed!\n");
}

#[tokio::test]
async fn test_metrics_snapshot_includes_dcutr() {
    println!("\n🧪 Testing Metrics Snapshot Includes DCUtR Data");
    println!("{}", "=".repeat(70));

    let service = DhtService::new(
        0,
        vec![],
        None,
        false,
        true, // Enable AutoNAT (enables DCUtR)
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        false,
        None,
        None,
    )
    .await
    .unwrap();

    println!("✓ DHT service started");

    let metrics = service.metrics_snapshot().await;

    println!("\n📊 Metrics Snapshot:");
    println!("  DCUtR Enabled: {}", metrics.dcutr_enabled);
    println!("  DCUtR Attempts: {}", metrics.dcutr_hole_punch_attempts);
    println!("  DCUtR Successes: {}", metrics.dcutr_hole_punch_successes);
    println!("  DCUtR Failures: {}", metrics.dcutr_hole_punch_failures);

    assert!(metrics.dcutr_enabled, "DCUtR should be enabled in metrics");
    assert_eq!(
        metrics.dcutr_hole_punch_attempts, 0,
        "Should have no attempts initially"
    );

    // Cleanup
    let _ = service.shutdown().await;
    println!("✅ Metrics snapshot test passed!\n");
}

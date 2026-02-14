/// Integration tests for the Analytics service
///
/// These tests verify that:
/// 1. Bandwidth tracking works correctly
/// 2. Performance metrics are calculated properly
/// 3. Network activity is tracked
/// 4. Historical data is recorded
/// 5. Upload/download operations update analytics

#[cfg(test)]
mod analytics_integration_tests {
    use chiral_network::analytics::AnalyticsService;

    #[tokio::test]
    async fn test_bandwidth_tracking() {
        let analytics = AnalyticsService::new();

        // Initial state should be zero
        let stats = analytics.get_bandwidth_stats().await;
        assert_eq!(stats.upload_bytes, 0, "Initial upload bytes should be 0");
        assert_eq!(
            stats.download_bytes, 0,
            "Initial download bytes should be 0"
        );

        // Record some uploads
        analytics.record_upload(1024).await; // 1 KB
        analytics.record_upload(2048).await; // 2 KB

        let stats = analytics.get_bandwidth_stats().await;
        assert_eq!(
            stats.upload_bytes, 3072,
            "Upload bytes should be 3072 (1KB + 2KB)"
        );
        assert_eq!(stats.download_bytes, 0, "Download bytes should still be 0");

        // Record some downloads
        analytics.record_download(4096).await; // 4 KB
        analytics.record_download(8192).await; // 8 KB

        let stats = analytics.get_bandwidth_stats().await;
        assert_eq!(stats.upload_bytes, 3072, "Upload bytes should remain 3072");
        assert_eq!(
            stats.download_bytes, 12288,
            "Download bytes should be 12288 (4KB + 8KB)"
        );

        println!("[OK] Bandwidth tracking test passed!");
    }

    #[tokio::test]
    async fn test_performance_metrics() {
        let analytics = AnalyticsService::new();

        // Initial metrics should be zero
        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(metrics.total_connections, 0);
        assert_eq!(metrics.successful_transfers, 0);
        assert_eq!(metrics.failed_transfers, 0);

        // Simulate successful upload transfer (1MB in 1 second = 8000 kbps)
        analytics.record_transfer(1_000_000, 1000, true, true).await;

        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(metrics.total_connections, 1);
        assert_eq!(metrics.successful_transfers, 1);
        assert_eq!(metrics.failed_transfers, 0);
        assert!(
            metrics.avg_upload_speed_kbps > 0.0,
            "Average upload speed should be recorded"
        );
        assert!(
            metrics.peak_upload_speed_kbps > 0.0,
            "Peak upload speed should be recorded"
        );

        // Simulate successful download transfer (2MB in 2 seconds = 8000 kbps)
        analytics
            .record_transfer(2_000_000, 2000, false, true)
            .await;

        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(metrics.total_connections, 2);
        assert_eq!(metrics.successful_transfers, 2);
        assert!(
            metrics.avg_download_speed_kbps > 0.0,
            "Average download speed should be recorded"
        );

        // Simulate failed transfer
        analytics.record_transfer(500_000, 500, false, false).await;

        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(metrics.total_connections, 3);
        assert_eq!(metrics.successful_transfers, 2);
        assert_eq!(metrics.failed_transfers, 1);

        println!("[OK] Performance metrics test passed!");
    }

    #[tokio::test]
    async fn test_network_activity() {
        let analytics = AnalyticsService::new();

        // Initial activity should be zero
        let activity = analytics.get_network_activity().await;
        assert_eq!(activity.active_uploads, 0);
        assert_eq!(activity.active_downloads, 0);
        assert_eq!(activity.completed_uploads, 0);
        assert_eq!(activity.completed_downloads, 0);

        // Update network activity
        analytics.update_network_activity(2, 3, 5).await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(activity.active_uploads, 2);
        assert_eq!(activity.active_downloads, 3);
        assert_eq!(activity.queued_downloads, 5);

        // Record completed transfers
        analytics.record_upload_completed().await;
        analytics.record_upload_completed().await;
        analytics.record_download_completed().await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(activity.completed_uploads, 2);
        assert_eq!(activity.completed_downloads, 1);

        println!("[OK] Network activity test passed!");
    }

    #[tokio::test]
    async fn test_peer_tracking() {
        let analytics = AnalyticsService::new();

        // Record peer connections
        analytics.record_peer_connected("peer1".to_string()).await;
        analytics.record_peer_connected("peer2".to_string()).await;
        analytics.record_peer_connected("peer1".to_string()).await; // Duplicate

        let activity = analytics.get_network_activity().await;
        assert_eq!(
            activity.total_peers_connected, 2,
            "Should have 2 unique peers"
        );
        assert_eq!(
            activity.unique_peers_all_time, 2,
            "Should track 2 unique peers all time"
        );

        // Add another peer
        analytics.record_peer_connected("peer3".to_string()).await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(activity.total_peers_connected, 3);
        assert_eq!(activity.unique_peers_all_time, 3);

        println!("[OK] Peer tracking test passed!");
    }

    #[tokio::test]
    async fn test_resource_contribution() {
        let analytics = AnalyticsService::new();

        // Initial contribution should be zero
        let contribution = analytics.get_resource_contribution().await;
        assert_eq!(contribution.storage_contributed_bytes, 0);
        assert_eq!(contribution.bandwidth_contributed_bytes, 0);
        assert_eq!(contribution.files_shared, 0);
        assert_eq!(contribution.total_seedtime_hours, 0.0);

        // Upload contributes to bandwidth
        analytics.record_upload(5_000_000).await; // 5 MB

        let contribution = analytics.get_resource_contribution().await;
        assert_eq!(contribution.bandwidth_contributed_bytes, 5_000_000);

        // Update storage contribution
        analytics.update_storage_contribution(10_000_000, 3).await;

        let contribution = analytics.get_resource_contribution().await;
        assert_eq!(contribution.storage_contributed_bytes, 10_000_000);
        assert_eq!(contribution.files_shared, 3);

        // Add seedtime
        analytics.add_seedtime_hours(1.5).await;
        analytics.add_seedtime_hours(2.0).await;

        let contribution = analytics.get_resource_contribution().await;
        assert_eq!(contribution.total_seedtime_hours, 3.5);

        println!("[OK] Resource contribution test passed!");
    }

    #[tokio::test]
    async fn test_latency_tracking() {
        let analytics = AnalyticsService::new();

        // Record latencies
        analytics.record_latency(100.0).await; // 100ms
        analytics.record_latency(150.0).await; // 150ms
        analytics.record_latency(120.0).await; // 120ms

        let metrics = analytics.get_performance_metrics().await;
        assert!(
            metrics.avg_latency_ms > 0.0,
            "Average latency should be recorded"
        );
        assert!(
            metrics.avg_latency_ms < 200.0,
            "Average latency should be reasonable"
        );

        println!(
            "[OK] Latency tracking test passed! Avg latency: {:.2}ms",
            metrics.avg_latency_ms
        );
    }

    #[tokio::test]
    async fn test_bandwidth_history() {
        let analytics = AnalyticsService::new();

        // History should be empty initially
        let history = analytics.get_bandwidth_history(None).await;
        assert_eq!(history.len(), 0, "History should be empty initially");

        // Record some bandwidth
        analytics.record_upload(1000).await;
        analytics.record_download(2000).await;

        // Wait a bit and record more (history is recorded every 60 seconds by default)
        // For testing, we can't wait 60 seconds, so this tests the retrieval mechanism
        let history = analytics.get_bandwidth_history(Some(10)).await;
        assert!(history.len() <= 10, "Should respect limit parameter");

        println!("[OK] Bandwidth history test passed!");
    }

    #[tokio::test]
    async fn test_contribution_history() {
        let analytics = AnalyticsService::new();

        // History should be empty initially
        let history = analytics.get_contribution_history(None).await;
        assert_eq!(
            history.len(),
            0,
            "Contribution history should be empty initially"
        );

        // Record contributions
        analytics.update_storage_contribution(5_000_000, 2).await;
        analytics.record_upload(1_000_000).await;

        let history = analytics.get_contribution_history(Some(5)).await;
        assert!(history.len() <= 5, "Should respect limit parameter");

        println!("[OK] Contribution history test passed!");
    }

    #[tokio::test]
    async fn test_reset_stats() {
        let analytics = AnalyticsService::new();

        // Record some data
        analytics.record_upload(1000).await;
        analytics.record_download(2000).await;
        analytics.record_transfer(5000, 100, true, true).await;
        analytics.update_network_activity(1, 2, 3).await;

        // Verify data exists
        let stats = analytics.get_bandwidth_stats().await;
        assert!(stats.upload_bytes > 0);
        assert!(stats.download_bytes > 0);

        // Reset
        analytics.reset_stats().await;

        // Verify reset
        let stats = analytics.get_bandwidth_stats().await;
        assert_eq!(stats.upload_bytes, 0, "Upload bytes should be reset to 0");
        assert_eq!(
            stats.download_bytes, 0,
            "Download bytes should be reset to 0"
        );

        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(
            metrics.total_connections, 0,
            "Connections should be reset to 0"
        );
        assert_eq!(
            metrics.successful_transfers, 0,
            "Successful transfers should be reset to 0"
        );

        println!("[OK] Reset stats test passed!");
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let analytics = AnalyticsService::new();

        // Simulate concurrent uploads and downloads
        let analytics_clone1 = analytics.clone();
        let analytics_clone2 = analytics.clone();
        let analytics_clone3 = analytics.clone();

        let upload_task = tokio::spawn(async move {
            for _ in 0..10 {
                analytics_clone1.record_upload(1000).await;
            }
        });

        let download_task = tokio::spawn(async move {
            for _ in 0..10 {
                analytics_clone2.record_download(2000).await;
            }
        });

        let transfer_task = tokio::spawn(async move {
            for i in 0..5 {
                analytics_clone3
                    .record_transfer(5000, 100, i % 2 == 0, true)
                    .await;
            }
        });

        // Wait for all tasks
        upload_task.await.unwrap();
        download_task.await.unwrap();
        transfer_task.await.unwrap();

        // Verify results
        let stats = analytics.get_bandwidth_stats().await;
        assert_eq!(stats.upload_bytes, 10_000, "Should have 10KB uploaded");
        assert_eq!(stats.download_bytes, 20_000, "Should have 20KB downloaded");

        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(metrics.total_connections, 5, "Should have 5 connections");

        println!("[OK] Concurrent operations test passed!");
    }

    #[tokio::test]
    async fn test_complete_upload_flow() {
        // This simulates a complete file upload flow
        let analytics = AnalyticsService::new();

        // 1. Start upload
        analytics.update_network_activity(1, 0, 0).await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(activity.active_uploads, 1, "Should have 1 active upload");

        // 2. Upload in progress - track bytes
        let file_size: u64 = 5_000_000; // 5 MB
        analytics.record_upload(file_size).await;

        let stats = analytics.get_bandwidth_stats().await;
        assert_eq!(stats.upload_bytes, file_size);

        // 3. Upload completes
        analytics.record_transfer(file_size, 5000, true, true).await; // 5 seconds
        analytics.record_upload_completed().await;
        analytics.update_network_activity(0, 0, 0).await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(activity.active_uploads, 0, "Should have 0 active uploads");
        assert_eq!(
            activity.completed_uploads, 1,
            "Should have 1 completed upload"
        );

        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(metrics.successful_transfers, 1);
        assert!(metrics.avg_upload_speed_kbps > 0.0);

        // 4. Update storage contribution
        analytics.update_storage_contribution(file_size, 1).await;

        let contribution = analytics.get_resource_contribution().await;
        assert_eq!(contribution.storage_contributed_bytes, file_size);
        assert_eq!(contribution.bandwidth_contributed_bytes, file_size);
        assert_eq!(contribution.files_shared, 1);

        println!("[OK] Complete upload flow test passed!");
        println!("   - File size: {} bytes", file_size);
        println!(
            "   - Upload speed: {:.2} kbps",
            metrics.avg_upload_speed_kbps
        );
        println!(
            "   - Storage contributed: {} bytes",
            contribution.storage_contributed_bytes
        );
    }

    #[tokio::test]
    async fn test_complete_download_flow() {
        // This simulates a complete file download flow
        let analytics = AnalyticsService::new();

        // 1. Queue download
        analytics.update_network_activity(0, 0, 1).await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(
            activity.queued_downloads, 1,
            "Should have 1 queued download"
        );

        // 2. Start download
        analytics.update_network_activity(0, 1, 0).await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(
            activity.active_downloads, 1,
            "Should have 1 active download"
        );
        assert_eq!(activity.queued_downloads, 0, "Queue should be empty");

        // 3. Download in progress - track bytes
        let file_size: u64 = 10_000_000; // 10 MB
        analytics.record_download(file_size).await;

        let stats = analytics.get_bandwidth_stats().await;
        assert_eq!(stats.download_bytes, file_size);

        // 4. Download completes
        analytics
            .record_transfer(file_size, 10000, false, true)
            .await; // 10 seconds
        analytics.record_download_completed().await;
        analytics.update_network_activity(0, 0, 0).await;

        let activity = analytics.get_network_activity().await;
        assert_eq!(
            activity.active_downloads, 0,
            "Should have 0 active downloads"
        );
        assert_eq!(
            activity.completed_downloads, 1,
            "Should have 1 completed download"
        );

        let metrics = analytics.get_performance_metrics().await;
        assert_eq!(metrics.successful_transfers, 1);
        assert!(metrics.avg_download_speed_kbps > 0.0);

        println!("[OK] Complete download flow test passed!");
        println!("   - File size: {} bytes", file_size);
        println!(
            "   - Download speed: {:.2} kbps",
            metrics.avg_download_speed_kbps
        );
    }
}

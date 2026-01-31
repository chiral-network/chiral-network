import { chromium } from 'playwright';

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1400, height: 1200 } });

await page.addInitScript(() => {
  let callbackId = 0;
  const callbacks = new Map();

  function transformCallback(callback, once) {
    const id = callbackId++;
    callbacks.set(id, { callback, once });
    return id;
  }

  // mock recovery data for chunk recovery panel
  const mockRecoveries = [
    {
      merkle_root: 'a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6',
      file_name: 'project-assets.zip',
      file_size: 52428800,
      tmp_path: '/tmp/chiral/project-assets.zip.part',
      dst_path: '/downloads/project-assets.zip',
      total_chunks: 200,
      pending: 40,
      downloaded: 120,
      verified: 35,
      failed: 5,
      providers: ['peer1', 'peer2', 'peer3'],
      encrypted: false
    },
    {
      merkle_root: 'b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7',
      file_name: 'dataset-2025.tar.gz',
      file_size: 104857600,
      tmp_path: '/tmp/chiral/dataset-2025.tar.gz.part',
      dst_path: '/downloads/dataset-2025.tar.gz',
      total_chunks: 400,
      pending: 0,
      downloaded: 0,
      verified: 400,
      failed: 0,
      providers: ['peer4'],
      encrypted: true
    },
    {
      merkle_root: 'c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8',
      file_name: 'video-lecture.mp4',
      file_size: 209715200,
      tmp_path: '/tmp/chiral/video-lecture.mp4.part',
      dst_path: '/downloads/video-lecture.mp4',
      total_chunks: 800,
      pending: 300,
      downloaded: 200,
      verified: 280,
      failed: 20,
      providers: ['peer1', 'peer5'],
      encrypted: false
    }
  ];

  const mockCorruption = {
    merkle_root: 'c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8',
    corrupted_chunks: [45, 102, 203],
    missing_chunks: [500, 501, 502, 503, 504],
    verified_chunks: Array.from({ length: 280 }, (_, i) => i),
    total_chunks: 800,
    corruption_detected: true
  };

  window.__TAURI_INTERNALS__ = {
    transformCallback,
    invoke: async (cmd, args) => {
      switch (cmd) {
        case 'has_active_account': return true;
        case 'detect_locale': return 'en';
        case 'get_dht_peer_id': return '12D3KooWMyNodeId123456789abcdef';
        case 'get_dht_connected_peers': return [];
        case 'get_connected_peer_metrics': return [];
        case 'get_dht_health': return { is_healthy: true, peer_count: 5 };
        case 'is_dht_running': return true;
        case 'get_dht_peer_count': return 5;
        case 'start_dht_node': return '12D3KooWMyNodeId123456789abcdef';
        case 'get_bootstrap_nodes_command': return [];
        case 'connect_to_peer': return null;
        case 'get_relay_reputation_stats': return { total_relays: 0, top_relays: [] };
        case 'is_geth_running': return false;
        case 'check_geth_binary': return false;
        case 'get_block_reward': return '0';
        case 'get_blockchain_sync_status': return { synced: false, currentBlock: 0, highestBlock: 0 };
        case 'plugin:store|load': return {};
        case 'plugin:store|get':
          if (args?.key === 'locale') return 'en';
          return null;
        case 'plugin:store|set': return null;
        case 'plugin:store|save': return null;
        case 'set_bandwidth_limits': return null;
        case 'start_file_transfer_service': return null;
        case 'get_download_directory': return '/home/user/Downloads';
        case 'p2p_chunk_startup_recovery': return mockRecoveries;
        case 'p2p_chunk_scan': return mockRecoveries;
        case 'p2p_chunk_check_corruption': return mockCorruption;
        case 'p2p_chunk_verify': return { merkle_root: '', valid: true, total_chunks: 0, verified_chunks: 0, failed_chunks: [] };
        case 'p2p_chunk_get_state': return mockRecoveries[0];
        case 'p2p_chunk_create_coordinator': return {
          merkle_root: mockRecoveries[0].merkle_root,
          file_name: mockRecoveries[0].file_name,
          total_chunks: 200, pending: 40, downloaded: 120, verified: 35, failed: 5,
          active_peers: 3, avg_speed: 2097152, eta_secs: 45
        };
        case 'p2p_chunk_get_progress': return null;
        default: return null;
      }
    },
    metadata: { tauriVersion: '2.0.0', version: '0.1.0' }
  };

  window.__TAURI_INVOKE__ = window.__TAURI_INTERNALS__.invoke;
  Object.defineProperty(navigator, 'language', { value: 'en', writable: false });
  Object.defineProperty(navigator, 'languages', { value: ['en'], writable: false });
});

page.on('pageerror', () => {});

console.log('navigating to download page...');
await page.goto('http://127.0.0.1:5173/', { waitUntil: 'networkidle', timeout: 30000 });
await page.waitForTimeout(3000);

// take full page screenshot
await page.screenshot({ path: 'artifacts/download-page-full.png', fullPage: true });
console.log('saved download-page-full.png');

// scroll to chunk recovery section
await page.evaluate(() => {
  const panel = document.querySelector('.chunk-recovery-panel');
  if (panel) panel.scrollIntoView({ behavior: 'instant', block: 'start' });
});
await page.waitForTimeout(1000);
await page.screenshot({ path: 'artifacts/download-chunk-recovery.png', fullPage: false });
console.log('saved download-chunk-recovery.png');

await browser.close();
console.log('done');

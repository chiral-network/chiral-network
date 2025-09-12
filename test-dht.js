// Test script for DHT functionality
// Run this in the browser console after the app starts

async function testDHT() {
    const { invoke } = window.__TAURI__.core;
    
    console.log("Starting DHT test...");
    
    try {
        // Start DHT node on port 4001 with optional bootstrap nodes
        console.log("1. Starting DHT node on port 4001...");
        const peerId = await invoke('start_dht_node', { 
            port: 4001, 
            bootstrapNodes: [
                // Add bootstrap nodes here if you have them
                // "/ip4/192.168.1.100/tcp/4001/p2p/QmPeerId"
            ] 
        });
        console.log("✅ DHT node started successfully");
        console.log("📍 Local Peer ID:", peerId);
        
        // Wait a moment for the node to initialize
        await new Promise(resolve => setTimeout(resolve, 2000));
        
        // Publish some file metadata
        console.log("2. Publishing file metadata...");
        await invoke('publish_file_metadata', {
            fileHash: "QmTest123FileHash",
            fileName: "test-document.pdf",
            fileSize: 1024 * 1024 * 5, // 5MB
            mimeType: "application/pdf"
        });
        console.log("✅ File metadata published");
        
        // Search for the file
        console.log("3. Searching for file metadata...");
        await invoke('search_file_metadata', {
            fileHash: "QmTest123FileHash"
        });
        console.log("✅ Search initiated");
        
        // Get DHT events to see what happened
        console.log("4. Getting DHT events...");
        const events = await invoke('get_dht_events');
        console.log("DHT Events:", events);
        
        // Connect to a peer (if you have another node running)
        // console.log("5. Connecting to peer...");
        // await invoke('connect_to_peer', {
        //     peerAddress: "/ip4/192.168.1.100/tcp/4001/p2p/QmPeerId"
        // });
        // console.log("✅ Peer connection initiated");
        
        console.log("\n✅ DHT test completed successfully!");
        console.log("The DHT is now running and ready for P2P file metadata exchange.");
        console.log("\nTo test with another node:");
        console.log("1. Run another instance of the app");
        console.log("2. Start DHT on a different port (e.g., 4002)");
        console.log("3. Connect nodes using connect_to_peer with the multiaddr");
        
    } catch (error) {
        console.error("❌ DHT test failed:", error);
    }
}

// Run the test
testDHT();

// Helper function to stop DHT when done
async function stopDHT() {
    const { invoke } = window.__TAURI__.core;
    try {
        await invoke('stop_dht_node');
        console.log("✅ DHT node stopped");
    } catch (error) {
        console.error("❌ Failed to stop DHT:", error);
    }
}

console.log("Test script loaded. The test will run automatically.");
console.log("To stop the DHT, run: stopDHT()");
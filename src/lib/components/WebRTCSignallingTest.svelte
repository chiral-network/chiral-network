<script lang="ts">
    import { onMount, onDestroy } from 'svelte';
    import { SignalingService } from '$lib/services/signallingService';

    console.log("[WebRTCTest] Component initializing");
    
    let signaling: SignalingService;
    let connected = false;
    let peers: string[] = [];
    let messages: string[] = [];

    onMount(async () => {
        try {
            console.log("[WebRTCTest] Component mounted");
            signaling = new SignalingService("ws://localhost:3000");
            
            // Subscribe to peers updates before connecting
            signaling.peers.subscribe(value => {
                console.log("[WebRTCTest] Peers updated:", value);
                peers = value;
            });

            signaling.connected.subscribe(value => {
                console.log("[WebRTCTest] Connection state:", value);
                connected = value;
            });
            
            console.log("[WebRTCTest] Connecting to signaling server...");
            await signaling.connect();
        } catch (err) {
            console.error("[WebRTCTest] Connection failed:", err);
            connected = false;
        }
    });
    
    onDestroy(() => {
        console.log("[WebRTCTest] Component destroying");
        signaling?.disconnect();
    });

    function connectToPeer(peerId: string) {
        console.log("[WebRTCTest] Attempting to connect to peer:", peerId);
        // TODO: Implement peer connection
    }
</script>

<div class="p-4">
    <h1 class="text-xl mb-4">WebRTC Test</h1>
    
    <div class="mb-4 p-2 border rounded">
        Connection Status: {connected ? 'Connected' : 'Disconnected'}
    </div>

    <div class="mb-4">
        <h2 class="text-lg mb-2">Connected Peers:</h2>
        <ul class="list-disc pl-5">
            {#each peers as peer}
                <li class="mb-2">
                    {peer}
                    <button 
                        class="ml-2 px-2 py-1 bg-blue-500 text-white rounded"
                        on:click={() => connectToPeer(peer)}>
                        Connect
                    </button>
                </li>
            {/each}
        </ul>
    </div>

    <div>
        <h2 class="text-lg mb-2">Messages:</h2>
        <div class="border p-2 h-40 overflow-y-auto">
            {#each messages as message}
                <div class="mb-1">{message}</div>
            {/each}
        </div>
    </div>
</div>
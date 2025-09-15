<script lang="ts">
  import { onDestroy } from 'svelte';
  import { createWebRTCSession, type IceServer, type WebRTCSession } from '$lib/services/webrtcService';

  // Configurable ICE servers (edit in UI or pass as props)
  export let iceServers: IceServer[] = [
    { urls: 'stun:stun.l.google.com:19302' },
  ];

  // Simple manual signaling demo (paste JSON SDP/candidates)
  // SDP/ICE textareas
  let localSDP = '';
  let remoteSDP = '';
  let newRemoteCandidate = '';
  let log: string[] = [];

  // Add loading state
  let isCreatingOffer = false;
  let isProcessingAnswer = false;
  let isAddingCandidate = false;

  // Role: 'initiator' or 'responder'
  let role: 'initiator' | 'responder' = 'initiator';

  // Active WebRTC session and stores
  let session: WebRTCSession;
  let connectionState;
  let channelState;

  function resetUiState() {
    localSDP = '';
    remoteSDP = '';
    newRemoteCandidate = '';
    isCreatingOffer = false;
    isProcessingAnswer = false;
    isAddingCandidate = false;
    log = [];
  }

  function createSession() {
    // Close any existing session first
    try { session?.close(); } catch {}

    session = createWebRTCSession({
      iceServers,
      isInitiator: role === 'initiator',
      onLocalDescription: (sdp) => {
        localSDP = JSON.stringify(sdp, null, 2);
        log = [`Local description set. Share this with remote.`, ...log];
      },
      onLocalIceCandidate: (cand) => {
        log = [`Local ICE candidate: ${JSON.stringify(cand)}`, ...log];
      },
      onMessage: (data) => {
        log = [`recv: ${typeof data === 'string' ? data : '[binary]'}`, ...log];
      },
      onConnectionStateChange: (state) => {
        log = [`connection: ${state}`, ...log];
      },
      onDataChannelOpen: () => {
        log = ['DataChannel open', ...log];
      },
      onDataChannelClose: () => {
        log = ['DataChannel closed', ...log];
      },
      onError: (e) => {
        log = [`error: ${String(e)}`, ...log];
        isCreatingOffer = false;
        isProcessingAnswer = false;
        isAddingCandidate = false;
      },
    });

    // expose stores for template auto-subscription
    connectionState = session.connectionState;
    channelState = session.channelState;
  }

  // Initialize session on mount
  createSession();

  function switchRole(newRole: 'initiator' | 'responder') {
    if (role === newRole) return;
    role = newRole;
    resetUiState();
    createSession();
  }

  async function startOffer() {
    if (isCreatingOffer) return;
    
    try {
      isCreatingOffer = true;
      await session.createOffer();
      log = ['Offer created successfully', ...log];
    } catch (error) {
      log = [`Failed to create offer: ${String(error)}`, ...log];
    } finally {
      isCreatingOffer = false;
    }
  }

  async function acceptRemoteAnswer() {
    if (!remoteSDP.trim()) {
      log = ['Error: No remote SDP provided', ...log];
      return;
    }
    
    if (isProcessingAnswer) return;

    try {
      isProcessingAnswer = true;
      const parsedSDP = JSON.parse(remoteSDP);
      
      // Validate SDP structure
      if (!parsedSDP.type || !parsedSDP.sdp) {
        throw new Error('Invalid SDP format - missing type or sdp field');
      }
      
      await session.acceptAnswer(parsedSDP);
      log = ['Remote answer accepted', ...log];
    } catch (error) {
      log = [`Failed to accept remote answer: ${String(error)}`, ...log];
    } finally {
      isProcessingAnswer = false;
    }
  }

  async function acceptOfferCreateAnswer() {
    if (!remoteSDP.trim()) {
      log = ['Error: No remote SDP (offer) provided', ...log];
      return;
    }

    if (isProcessingAnswer) return;

    try {
      isProcessingAnswer = true;
      const parsedSDP = JSON.parse(remoteSDP);

      if (!parsedSDP.type || !parsedSDP.sdp || parsedSDP.type !== 'offer') {
        throw new Error('Invalid SDP: expected an offer with type and sdp');
      }

      const answer = await session.acceptOfferCreateAnswer(parsedSDP);
      // localSDP is already set by onLocalDescription, but ensure it's available
      localSDP = JSON.stringify(answer, null, 2);
      log = ['Offer accepted. Created answer. Share with remote.', ...log];
    } catch (error) {
      log = [`Failed to accept offer: ${String(error)}`, ...log];
    } finally {
      isProcessingAnswer = false;
    }
  }

  async function addCandidate() {
    if (!newRemoteCandidate.trim()) {
      log = ['Error: No remote candidate provided', ...log];
      return;
    }
    
    if (isAddingCandidate) return;

    try {
      isAddingCandidate = true;
      const parsedCandidate = JSON.parse(newRemoteCandidate);
      
      // Validate candidate structure
      if (!parsedCandidate.candidate && parsedCandidate.candidate !== '') {
        throw new Error('Invalid ICE candidate format');
      }
      
      await session.addRemoteIceCandidate(parsedCandidate);
      log = ['Remote ICE candidate added', ...log];
      newRemoteCandidate = '';
    } catch (error) {
      log = [`Failed to add remote candidate: ${String(error)}`, ...log];
    } finally {
      isAddingCandidate = false;
    }
  }

  function sendMessage() {
    const message = 'hello from initiator';
    try {
      session.send(message);
      log = [`sent: ${message}`, ...log];
    } catch (error) {
      log = [`Failed to send message: ${String(error)}`, ...log];
    }
  }

  function clearLog() {
    log = [];
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text).then(() => {
      log = ['Copied to clipboard', ...log];
    }).catch(() => {
      log = ['Failed to copy to clipboard', ...log];
    });
  }

  onDestroy(() => {
    try {
      session.close();
    } catch (error) {
      console.error('Error closing WebRTC session:', error);
    }
  });
</script>

<div class="space-y-3 p-3 border rounded-md max-w-4xl">
  <div class="flex items-center justify-between">
    <strong>WebRTC Test (manual signaling)</strong>
    <div class="flex items-center gap-2 text-xs">
      <span class="text-gray-500">connection: {$connectionState}</span>
      <span class="text-gray-500">channel: {$channelState}</span>
    </div>
  </div>

  <div class="flex items-center gap-3">
    <label class="text-sm">Role:</label>
    <div class="flex items-center gap-2">
      <label class="text-sm flex items-center gap-1">
        <input type="radio" name="role" value="initiator" checked={role === 'initiator'} on:change={() => switchRole('initiator')} />
        Initiator
      </label>
      <label class="text-sm flex items-center gap-1">
        <input type="radio" name="role" value="responder" checked={role === 'responder'} on:change={() => switchRole('responder')} />
        Responder
      </label>
    </div>
  </div>

  <div class="flex gap-2 flex-wrap">
    {#if role === 'initiator'}
      <button
        class="px-2 py-1 border rounded disabled:opacity-50 disabled:cursor-not-allowed"
        on:click={startOffer}
        disabled={isCreatingOffer}
      >
        {isCreatingOffer ? 'Creating...' : 'Create Offer'}
      </button>
    {:else}
      <button
        class="px-2 py-1 border rounded disabled:opacity-50 disabled:cursor-not-allowed"
        on:click={acceptOfferCreateAnswer}
        disabled={isProcessingAnswer || !remoteSDP.trim()}
      >
        {isProcessingAnswer ? 'Processing...' : 'Accept Offer & Create Answer'}
      </button>
    {/if}

    <button
      class="px-2 py-1 border rounded disabled:opacity-50 disabled:cursor-not-allowed"
      on:click={sendMessage}
      disabled={$channelState !== 'open'}
    >
      Send Message
    </button>

    <button class="px-2 py-1 border rounded" on:click={clearLog}>
      Clear Log
    </button>
  </div>

  <div class="grid md:grid-cols-2 gap-3">
    {#if role === 'initiator'}
      <div>
        <div class="flex items-center justify-between mb-1">
          <label class="text-sm font-medium">Local SDP (Offer) — send to remote</label>
          {#if localSDP}
            <button
              class="text-xs px-2 py-1 border rounded hover:bg-gray-50"
              on:click={() => copyToClipboard(localSDP)}
            >
              Copy
            </button>
          {/if}
        </div>
        <textarea
          class="w-full h-32 p-2 border rounded text-xs font-mono"
          readonly
          bind:value={localSDP}
          placeholder="Local offer SDP will appear here after creating offer"
        ></textarea>
      </div>

      <div>
        <label class="block text-sm font-medium mb-1">Remote SDP (paste Answer here)</label>
        <textarea
          class="w-full h-32 p-2 border rounded text-xs font-mono"
          bind:value={remoteSDP}
          placeholder="Paste the remote peer's answer SDP here"
        ></textarea>
        <div class="mt-2">
          <button
            class="px-2 py-1 border rounded disabled:opacity-50 disabled:cursor-not-allowed"
            on:click={acceptRemoteAnswer}
            disabled={isProcessingAnswer || !remoteSDP.trim()}
          >
            {isProcessingAnswer ? 'Processing...' : 'Accept Answer'}
          </button>
        </div>
      </div>
    {:else}
      <div>
        <label class="block text-sm font-medium mb-1">Remote SDP (paste Offer here)</label>
        <textarea
          class="w-full h-32 p-2 border rounded text-xs font-mono"
          bind:value={remoteSDP}
          placeholder="Paste the remote peer's offer SDP here"
        ></textarea>
        <div class="mt-2 text-xs text-gray-600">
          After pasting the offer, click "Accept Offer & Create Answer" above.
        </div>
      </div>

      <div>
        <div class="flex items-center justify-between mb-1">
          <label class="text-sm font-medium">Local SDP (Answer) — send to remote</label>
          {#if localSDP}
            <button
              class="text-xs px-2 py-1 border rounded hover:bg-gray-50"
              on:click={() => copyToClipboard(localSDP)}
            >
              Copy
            </button>
          {/if}
        </div>
        <textarea
          class="w-full h-32 p-2 border rounded text-xs font-mono"
          readonly
          bind:value={localSDP}
          placeholder="Your generated answer SDP will appear here"
        ></textarea>
      </div>
    {/if}
  </div>

  <div>
    <label class="block text-sm font-medium mb-1">Remote ICE Candidate (JSON)</label>
    <textarea
      class="w-full h-20 p-2 border rounded text-xs font-mono"
      bind:value={newRemoteCandidate}
      placeholder={`{"candidate": "...", "sdpMLineIndex": 0, "sdpMid": "..."}`}
    ></textarea>
    <div class="mt-2 flex items-center gap-2 text-xs text-gray-600">
      Paste candidates received from the other peer and click Add Candidate.
    </div>
    <div class="mt-2">
      <button
        class="px-2 py-1 border rounded disabled:opacity-50 disabled:cursor-not-allowed"
        on:click={addCandidate}
        disabled={isAddingCandidate || !newRemoteCandidate.trim()}
      >
        {isAddingCandidate ? 'Adding...' : 'Add Candidate'}
      </button>
    </div>
  </div>

  <div>
    <div class="flex items-center justify-between mb-1">
      <label class="text-sm font-medium">Log ({log.length} entries)</label>
      <button class="text-xs px-2 py-1 border rounded hover:bg-gray-50" on:click={clearLog}>
        Clear
      </button>
    </div>
    <div class="h-32 overflow-auto border rounded p-2 text-xs bg-gray-50 font-mono">
      {#each log as line, i}
        <div class="py-0.5 {i === 0 ? 'font-semibold' : ''}">[{new Date().toLocaleTimeString()}] {line}</div>
      {/each}
      {#if log.length === 0}
        <div class="text-gray-400 italic">No log entries yet...</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .space-y-3 > * + * { 
    margin-top: 0.75rem; 
  }
</style>

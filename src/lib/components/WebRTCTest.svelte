<script lang="ts">
  import { onDestroy } from 'svelte';
  import { createWebRTCSession, type IceServer } from '$lib/services/webrtcService';

  // Configurable ICE servers (edit in UI or pass as props)
  export let iceServers: IceServer[] = [
    { urls: 'stun:stun.l.google.com:19302' },
  ];

  // Simple manual signaling demo (paste JSON SDP/candidates)
  let localSDP = '';
  let remoteSDP = '';
  let newRemoteCandidate = '';
  let log: string[] = [];

  const session = createWebRTCSession({
    iceServers,
    isInitiator: true,
    onLocalDescription: (sdp) => {
      localSDP = JSON.stringify(sdp);
      log.unshift('Local description set. Share this with remote.');
    },
    onLocalIceCandidate: (cand) => {
      log.unshift('Local ICE candidate generated. Share with remote.');
    },
    onMessage: (data) => {
      log.unshift(`recv: ${typeof data === 'string' ? data : '[binary]'}`);
    },
    onConnectionStateChange: (state) => {
      log.unshift(`connection: ${state}`);
    },
    onDataChannelOpen: () => log.unshift('DataChannel open'),
    onDataChannelClose: () => log.unshift('DataChannel closed'),
    onError: (e) => log.unshift(`error: ${String(e)}`),
  });

  // expose stores for template auto-subscription
  const connectionState = session.connectionState;
  const channelState = session.channelState;

  async function startOffer() {
    await session.createOffer();
  }

  async function acceptRemoteAnswer() {
    if (!remoteSDP) return;
    await session.acceptAnswer(JSON.parse(remoteSDP));
  }

  async function addCandidate() {
    if (!newRemoteCandidate) return;
    await session.addRemoteIceCandidate(JSON.parse(newRemoteCandidate));
    newRemoteCandidate = '';
  }

  function sendMessage() {
    session.send('hello from initiator');
  }

  onDestroy(() => session.close());
</script>

<div class="space-y-3 p-3 border rounded-md">
  <div class="flex items-center justify-between">
    <strong>WebRTC Test (manual signaling)</strong>
    <span class="text-xs text-gray-500">state: {$connectionState}</span>
  </div>

  <div class="flex gap-2 flex-wrap">
    <button class="px-2 py-1 border rounded" on:click={startOffer}>Create Offer</button>
    <button class="px-2 py-1 border rounded" on:click={sendMessage} disabled={$channelState !== 'open'}>Send</button>
  </div>

  <div class="grid md:grid-cols-2 gap-3">
    <div>
      <label class="block text-sm font-medium mb-1">Local SDP (send to remote)</label>
      <textarea class="w-full h-32 p-2 border rounded" readonly bind:value={localSDP}></textarea>
    </div>
    <div>
      <label class="block text-sm font-medium mb-1">Remote SDP (paste answer here)</label>
      <textarea class="w-full h-32 p-2 border rounded" bind:value={remoteSDP}></textarea>
      <div class="mt-2">
        <button class="px-2 py-1 border rounded" on:click={acceptRemoteAnswer}>Accept Answer</button>
      </div>
    </div>
  </div>

  <div>
    <label class="block text-sm font-medium mb-1">Remote ICE Candidate (JSON)</label>
    <textarea class="w-full h-20 p-2 border rounded" bind:value={newRemoteCandidate}></textarea>
    <div class="mt-2">
      <button class="px-2 py-1 border rounded" on:click={addCandidate}>Add Candidate</button>
    </div>
  </div>

  <div>
    <label class="block text-sm font-medium mb-1">Log</label>
    <div class="h-28 overflow-auto border rounded p-2 text-xs bg-gray-50">
      {#each log as line, i}
        <div>{line}</div>
      {/each}
    </div>
  </div>
</div>

<style>
  .space-y-3 > * + * { margin-top: 0.75rem; }
</style>

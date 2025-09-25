<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    proxyNodes,
    initProxyEvents,
    disposeProxyEvents,
    connectProxy,
    disconnectProxy,
    listProxies,
  } from '$lib/proxy';
  import { invoke } from '@tauri-apps/api/core';

  // Connect form
  let url = 'ws://127.0.0.1:4001';
  let token = 'test-token';

  // Echo form
  let selectedPeerId = '';
  let peerId = '';            // final target (defaults from selection)
  let echoPayload = 'hello';
  let echoResult = '';
  let echoBusy = false;

  // Keep peerId in sync with dropdown selection
  $: if (selectedPeerId) peerId = selectedPeerId;

  async function doEcho() {
    try {
      // String -> bytes
      const payload = Array.from(new TextEncoder().encode(echoPayload));
      // Rust returns Vec<u8> -> number[]
      const out = await invoke<number[]>('proxy_echo', { peerId, payload });
      // bytes -> String
      echoResult = new TextDecoder().decode(new Uint8Array(out));
    } catch (e) {
      echoResult = String(e);
    }
  }

  onMount(async () => {
    await initProxyEvents();
    await listProxies();
  });

  onDestroy(() => {
    disposeProxyEvents();
  });
</script>

<div class="p-4 space-y-6">
  <h1 class="text-2xl font-bold">Proxy Self-Test</h1>

  <!-- Connect / Disconnect -->
  <section class="space-y-3">
    <div class="flex gap-4">
      <input type="text" bind:value={url} placeholder="Proxy URL" class="input" />
      <input type="text" bind:value={token} placeholder="Auth Token" class="input" />
    </div>
    <div class="flex gap-4">
      <button on:click={() => connectProxy(url, token)} class="btn btn-primary">Connect</button>
      <button on:click={() => disconnectProxy(url)} class="btn">Disconnect</button>
      <button on:click={listProxies} class="btn">Refresh List</button>
    </div>
  </section>

  <!-- Connected Proxies -->
  <section class="space-y-2">
    <h2 class="text-xl font-bold">Connected Proxies</h2>
    {#if $proxyNodes.length === 0}
      <div class="text-sm text-gray-500">No proxies yet. Connect above.</div>
    {/if}
    <ul class="space-y-1">
      {#each $proxyNodes as node}
        <li class="flex items-center gap-2">
          <strong>{node.address}</strong>
          <span>— {node.status} ({node.latency}ms)</span>
          {#if node.error}
            <span class="text-red-500">· {node.error}</span>
          {/if}
          <!-- Quick set as Echo target if node.id is present -->
          {#if node.id}
            <button
              class="btn btn-xs ml-2"
              on:click={() => { selectedPeerId = node.id; }}
              title="Use this peer for Echo"
            >
              Use for Echo
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  </section>

  <!-- Echo Test -->
  <section class="space-y-2">
    <h2 class="text-xl font-bold">Echo Test</h2>

    <!-- Peer picker populated from proxies -->
    <div class="flex gap-2 items-center">
      <label class="text-sm" for="target-peer-select">Target Peer</label>
      <select id="target-peer-select" class="input" bind:value={selectedPeerId}>
        <option value="">— Select from connected proxies —</option>
        {#each $proxyNodes as node}
          {#if node.id}
            <option value={node.id}>
              {node.id} ({node.latency}ms)
            </option>
          {/if}
        {/each}
      </select>
    </div>

    <!-- Or paste PeerId manually -->
    <input class="input" placeholder="Or paste PeerId" bind:value={peerId} />

    <div class="flex gap-2">
      <input class="input flex-1" placeholder="Payload" bind:value={echoPayload} />
      <button class="btn btn-primary" on:click={doEcho} disabled={echoBusy}>
        {echoBusy ? 'Sending…' : 'Echo'}
      </button>
    </div>

    {#if echoResult}
      <div class="mt-1 text-sm">
        <span class="font-semibold">Result:</span> {echoResult}
      </div>
    {/if}
  </section>
</div>

<style>
  .input { @apply border rounded px-3 py-2 w-full; }
  .btn { @apply border rounded px-3 py-2; }
  .btn-primary { @apply bg-black text-white; }
  .btn-xs { @apply text-xs px-2 py-1; }
</style>

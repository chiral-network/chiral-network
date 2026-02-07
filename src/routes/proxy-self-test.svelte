<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import DropDown from '$lib/components/ui/dropDown.svelte';
  import {
    proxyNodes,
    echoInbox,              
    initProxyEvents,
    disposeProxyEvents,
    connectProxy,
    disconnectProxy,
    listProxies,
  } from '$lib/proxy';

  // Connect form
  let url = 'ws://127.0.0.1:4001';
  let token = 'test-token';

  // Echo form
  let selectedPeerId = '';
  let peerId = '';            // final target (default to dropdown selection)
  let echoPayload = 'hello';
  let echoResult = '';
  let echoBusy = false;

  // Synchronize selectedPeerId to peerId
  $: if (selectedPeerId) peerId = selectedPeerId;

  $: peerOptions = [
    { value: '', label: '— Select from connected proxies —' },
    ...$proxyNodes
      .filter(node => !!node.id)
      .map(node => ({
        value: node.id,
        label: `${node.id}${node.latency !== undefined ? ` (${node.latency}ms)` : ''}`
      }))
  ];

  async function doEcho() {
    if (!peerId) {
      echoResult = 'Pick a target peer first';
      return;
    }
    try {
      echoBusy = true;
      // String -> bytes (number[])
      const payload = Array.from(new TextEncoder().encode(echoPayload));
      // Rust returns Vec<u8> -> number[]
      const out = await invoke<number[]>('proxy_echo', { peerId, payload });
      // bytes -> String
      echoResult = new TextDecoder().decode(new Uint8Array(out));
    } catch (e) {
      echoResult = String(e);
    } finally {
      echoBusy = false;
    }
  }

  async function disconnectByPeer(_id: string, address?: string) {
    try {
      if (!address) return; // no-op
      await disconnectProxy(address);
    } catch (e) {
      console.error(e);
    }
  }

  async function removeOffline() {
    await listProxies(); // we don't have forget, so just refresh
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
      <button on:click={() => listProxies()} class="btn">Refresh List</button>
    </div>
  </section>

  <!-- Connected Proxies -->
  <section class="space-y-2">
    <div class="flex gap-2">
      <button class="btn" on:click={listProxies}>Refresh</button>
      <button class="btn" on:click={removeOffline}>Remove offline</button>
    </div>

    <h2 class="text-xl font-bold">Connected Proxies</h2>
    {#if $proxyNodes.length === 0}
      <div class="text-sm text-gray-500">No proxies yet. Connect above.</div>
    {/if}

    <ul class="space-y-1">
      {#each $proxyNodes as node}
        <li class="flex flex-wrap items-center gap-2">
          <!-- ID/Address/Status display -->
          <code class="font-mono text-xs">{node.id}</code>
          {#if node.address}
            <span class="text-xs opacity-60">· {node.address}</span>
          {/if}
          <span>— {node.status}{#if node.latency !== undefined} ({node.latency}ms){/if}</span>
          {#if node.error}
            <span class="text-red-500">· {node.error}</span>
          {/if}

          <!-- Echo target selection -->
          {#if node.id}
            <button
              class="btn btn-xs ml-2"
              on:click={() => { selectedPeerId = node.id; }}
              title="Use this peer for Echo">
              Use for Echo
            </button>
          {/if}


          <!-- Disconnect button -->
          <button
            class="btn btn-xs ml-auto"
            title="Disconnect"
            on:click={() => disconnectByPeer(node.id, node.address)}
            disabled={!node.address}
          >
            Disconnect
          </button>
        </li>
      {/each}
    </ul>
  </section>

  <!-- Echo Test -->
  <section class="space-y-2">
    <h2 class="text-xl font-bold">Echo Test</h2>

    <div class="flex gap-2 items-center">
      <label class="text-sm" for="target-peer-select">Target Peer</label>
      <DropDown
        id="target-peer-select"
        options={peerOptions}
        bind:value={selectedPeerId}
      />
    </div>

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

  <!-- Echo Inbox (Checking for incoming messages) -->
  <section class="space-y-2">
    <h2 class="text-xl font-bold">Echo Inbox</h2>
    {#if $echoInbox.length === 0}
      <div class="text-sm text-gray-500">No incoming echo messages yet.</div>
    {/if}
    <ul class="space-y-1">
      {#each $echoInbox as msg, i}
        <li class="text-sm">
          <span class="font-mono text-gray-600">[{i}]</span>
          <span class="font-mono">{msg.from}</span> ->
          <span>{msg.text}</span>
          <span class="text-gray-500 ml-2">({new Date(msg.ts).toLocaleTimeString()})</span>
        </li>
      {/each}
    </ul>
  </section>
</div>

<style>
  .input { border: 1px solid #d1d5db; border-radius: 0.25rem; padding: 0.5rem 0.75rem; width: 100%; }
  .btn { border: 1px solid #d1d5db; border-radius: 0.25rem; padding: 0.5rem 0.75rem; }
  .btn-primary { background-color: #000000; color: #ffffff; }
  .btn-xs { font-size: 0.75rem; padding: 0.25rem 0.5rem; }
</style>

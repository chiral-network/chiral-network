
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { proxyNodes, initProxyEvents, disposeProxyEvents, connectProxy, disconnectProxy, listProxies } from '$lib/proxy';

  let url = 'ws://127.0.0.1:4001';
  let token = 'test-token';

  onMount(async () => {
    await initProxyEvents();
    await listProxies();
  });

  onDestroy(() => {
    disposeProxyEvents();
  });
</script>

<div class="p-4">
  <h1 class="text-2xl font-bold mb-4">Proxy Self-Test</h1>

  <div class="flex gap-4 mb-4">
    <input type="text" bind:value={url} placeholder="Proxy URL" class="input" />
    <input type="text" bind:value={token} placeholder="Auth Token" class="input" />
  </div>

  <div class="flex gap-4 mb-4">
    <button on:click={() => connectProxy(url, token)} class="btn btn-primary">Connect</button>
    <button on:click={() => disconnectProxy(url)} class="btn">Disconnect</button>
    <button on:click={listProxies} class="btn">Refresh List</button>
  </div>

  <h2 class="text-xl font-bold mb-2">Connected Proxies</h2>
  <ul>
    {#each $proxyNodes as node}
      <li>
        <strong>{node.address}</strong> - {node.status} ({node.latency}ms)
        {#if node.error}
          <span class="text-red-500">{node.error}</span>
        {/if}
      </li>
    {/each}
  </ul>
</div>

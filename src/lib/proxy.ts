import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface ProxyNode {
  id: string;
  address: string;
  status: 'online' | 'offline' | 'connecting' | 'error';
  latency: number;
  error?: string;
}

export const proxyNodes = writable<ProxyNode[]>([]);

let unlisten: UnlistenFn | null = null;

export async function initProxyEvents() {
  if (typeof window === 'undefined') return; // SSR guard
  if (unlisten) return; // already initialized

  unlisten = await listen('proxy_status_update', (event) => {
    const updated = event.payload as ProxyNode;
    console.log('[proxy_status_update]', updated);

    proxyNodes.update((nodes) => {
      const idx = nodes.findIndex((n) => n.id === updated.id);
      if (idx !== -1) {
        // return a NEW array (don’t mutate)
        const next = nodes.slice();
        next[idx] = updated;
        return next;
      }
      return [...nodes, updated];
    });
  });
}

export function disposeProxyEvents() {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
}

export async function connectProxy(url: string, token: string) {
  try {
    console.log(`Connecting to proxy: ${url}`);
    await invoke('proxy_connect', { url, token });
  } catch (e) {
    console.error('proxy_connect failed:', e);
  }
}

export async function disconnectProxy(url: string) {
  try {
    console.log(`Disconnecting from proxy: ${url}`);
    await invoke('proxy_disconnect', { url });
  } catch (e) {
    console.error('proxy_disconnect failed:', e);
  }
}

export async function listProxies() {
  try {
    console.log('Refreshing proxy list');
    const nodes = await invoke<ProxyNode[]>('list_proxies');
    proxyNodes.set(nodes ?? []);
  } catch (e) {
    console.error('list_proxies failed:', e);
    proxyNodes.set([]);
  }
}
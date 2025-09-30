import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ProxyNode {
  id: string;
  address?: string;
  status?: "online" | "offline" | "connecting" | "error";
  latency?: number;
  error?: string;
}

type ProxyUpdate = Partial<ProxyNode> & { id: string };

export const proxyNodes = writable<ProxyNode[]>([]);
export const echoInbox = writable<
  { from: string; text?: string; bytes: number; ts: number }[]
>([]);

let unlisten: UnlistenFn | null = null;
let unlistenEcho: (() => void) | undefined;

function mergeNode(prev: ProxyNode | undefined, upd: ProxyUpdate): ProxyNode {
  const base: ProxyNode = prev ?? { id: upd.id };
  return {
    ...base,
    // update only if there's a new value
    // address: upd.address ?? base.address,
    address:
      upd.address !== undefined && upd.address !== ""
        ? upd.address
        : base.address,
    status: upd.status ?? base.status,
    latency: upd.latency ?? base.latency,
    error: upd.error ?? base.error,
  };
}

function sortNodes(xs: ProxyNode[]) {
  const order = { online: 0, connecting: 1, offline: 2, error: 3 } as const;
  return xs.slice().sort((a, b) => {
    const sa = a.status ?? "offline";
    const sb = b.status ?? "offline";
    const oa = (order as any)[sa] ?? 9;
    const ob = (order as any)[sb] ?? 9;
    if (oa !== ob) return oa - ob;
    const la = a.latency ?? Infinity;
    const lb = b.latency ?? Infinity;
    return la - lb;
  });
}

export async function initProxyEvents() {
  if (typeof window === "undefined") return;
  if (unlisten) return;

  // State update (ProxyStatus) - merge by PeerId
  unlisten = await listen("proxy_status_update", (event) => {
    const updated = event.payload as ProxyUpdate;

    proxyNodes.update((nodes) => {
      const i = nodes.findIndex((n) => n.id === updated.id);
      const next = nodes.slice();
      if (i >= 0) {
        next[i] = mergeNode(next[i], updated);
      } else {
        next.push(mergeNode(undefined, updated));
      }
      // Duplicates should not happen, but just in case
      const seen = new Set<string>();
      const dedup = next.filter((n) =>
        seen.has(n.id) ? false : (seen.add(n.id), true)
      );
      return sortNodes(dedup);
    });
  });

  // Echo inbox
  const un = await listen("proxy_echo_rx", (e) => {
    const m = e.payload as { from: string; text?: string; bytes?: number };
    echoInbox.update((xs) =>
      [
        { from: m.from, text: m.text, bytes: m.bytes ?? 0, ts: Date.now() },
        ...xs,
      ].slice(0, 100)
    );
  });
  unlistenEcho = un;
}

export function disposeProxyEvents() {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
  unlistenEcho?.();
}

export async function connectProxy(url: string, token: string) {
  try {
    await invoke("proxy_connect", { url, token });
    // if needed: optimistic placeholder if PeerId is unknown
  } catch (e) {
    console.error("proxy_connect failed:", e);
  }
}

export async function disconnectProxy(url: string) {
  try {
    await invoke("proxy_disconnect", { url });
  } catch (e) {
    console.error("proxy_disconnect failed:", e);
  }
}

export async function listProxies() {
  try {
    const incoming = (await invoke<ProxyNode[]>("list_proxies")) ?? [];
    // merge with existing nodes to preserve status/latency if possible
    proxyNodes.update((nodes) => {
      const map = new Map<string, ProxyNode>();
      for (const n of nodes) map.set(n.id, n);
      for (const u of incoming) map.set(u.id, mergeNode(map.get(u.id), u));
      return sortNodes([...map.values()]);
    });
  } catch (e) {
    console.error("list_proxies failed:", e);
    proxyNodes.set([]);
  }
}

import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ProxyNode {
  id: string;
  address?: string;
  status?: "online" | "offline" | "connecting" | "error";
  latency?: number;
  error?: string | null;
}

type ProxyUpdate = Partial<ProxyNode> & { id: string };

export const proxyNodes = writable<ProxyNode[]>([]);
export const echoInbox = writable<
  { from: string; text?: string | null; bytes: number; ts: number }[]
>([]);

let unlistenStatus: UnlistenFn | null = null;
let unlistenEcho: UnlistenFn | null = null;
let unlistenReset: UnlistenFn | null = null;

function mergeNode(prev: ProxyNode | undefined, upd: ProxyUpdate): ProxyNode {
  const base: ProxyNode = prev ?? { id: upd.id };
  const addr =
    upd.address !== undefined && upd.address !== ""
      ? upd.address
      : base.address ?? upd.id;
  return {
    ...base,
    id: upd.id ?? base.id,
    address: addr,
    status: upd.status ?? base.status ?? "offline",
    latency:
      typeof upd.latency === "number"
        ? upd.latency
        : typeof base.latency === "number"
        ? base.latency
        : undefined,
    error: typeof upd.error !== "undefined" ? upd.error : base.error ?? null,
  };
}

function sortNodes(xs: ProxyNode[]) {
  const order: Record<string, number> = {
    online: 0,
    connecting: 1,
    offline: 2,
    error: 3,
  };
  return xs
    .slice()
    .sort((a, b) => {
      const sa = a.status ?? "offline";
      const sb = b.status ?? "offline";
      const oa = order[sa] ?? 9;
      const ob = order[sb] ?? 9;
      if (oa !== ob) return oa - ob;
      const la = a.latency ?? Infinity;
      const lb = b.latency ?? Infinity;
      return la - lb;
    });
}

export async function initProxyEvents() {
  if (typeof window === "undefined") return;
  if (unlistenStatus || unlistenEcho || unlistenReset) return;

  unlistenStatus = await listen<ProxyUpdate>("proxy_status_update", (event) => {
    const updated = event.payload;
    proxyNodes.update((nodes) => {
      const i = nodes.findIndex((n) => n.id === updated.id);
      const next = nodes.slice();
      if (i >= 0) {
        next[i] = mergeNode(next[i], updated);
      } else {
        next.push(mergeNode(undefined, updated));
      }
      const seen = new Set<string>();
      const dedup = next.filter((n) =>
        seen.has(n.id) ? false : (seen.add(n.id), true)
      );
      return sortNodes(dedup);
    });
  });

  unlistenEcho = await listen<{ from: string; text?: string | null; bytes?: number }>(
    "proxy_echo_rx",
    (e) => {
      const m = e.payload;
      echoInbox.update((xs) =>
        [{ from: m.from, text: m.text ?? null, bytes: m.bytes ?? 0, ts: Date.now() }, ...xs].slice(
          0,
          100
        )
      );
      proxyNodes.update((nodes) => {
        const i = nodes.findIndex((n) => n.id === m.from);
        if (i < 0) {
          return sortNodes([
            ...nodes,
            {
              id: m.from,
              address: m.from,
              status: "online",
              latency: nodes.find((n) => n.id === m.from)?.latency ?? 0,
              error: null,
            },
          ]);
        }
        const next = nodes.slice();
        next[i] = {
          ...next[i],
          status: "online",
        };
        return sortNodes(next);
      });
    }
  );

  unlistenReset = await listen("proxy_reset", () => {
    proxyNodes.set([]);
    echoInbox.set([]);
  });
}

export function disposeProxyEvents() {
  if (unlistenStatus) {
    unlistenStatus();
    unlistenStatus = null;
  }
  if (unlistenEcho) {
    unlistenEcho();
    unlistenEcho = null;
  }
  if (unlistenReset) {
    unlistenReset();
    unlistenReset = null;
  }
}

export async function connectProxy(url: string, token: string) {
  await initProxyEvents();
  try {
    await invoke("proxy_connect", { url, token });
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

export async function removeProxy(url: string) {
  try {
    await invoke("proxy_remove", { url });
    proxyNodes.update((nodes) => nodes.filter((n) => n.address !== url));
  } catch (e) {
    console.error("proxy_remove failed:", e);
  }
}

export async function listProxies() {
  try {
    const incoming = (await invoke<ProxyNode[]>("list_proxies")) ?? [];
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

export async function getProxyOptimizationStatus(): Promise<string> {
  try {
    const status = await invoke<any>("get_proxy_optimization_status");
    if (typeof status === 'object' && status !== null) {
      return JSON.stringify(status, null, 2);
    }
    return String(status);
  } catch (e) {
    console.error("get_proxy_optimization_status failed:", e);
    return "[X] Failed to get optimization status";
  }
}
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writable } from "svelte/store";
import type { PeerInfo } from "$lib/stores";
import { peers } from "$lib/stores";

export type PeerDiscovery = {
  peerId: string;
  addresses: string[];
  lastSeen: number;
};

type PeerDiscoveredPayload = {
  peerId: string;
  addresses?: string[] | null;
};

type PeerConnectedPayload = {
  peerId: string;
  address?: string | null;
};

type PeerDisconnectedPayload = {
  peerId: string;
};

const discoveredPeersStore = writable<PeerDiscovery[]>([]);

function sortDiscoveries(entries: PeerDiscovery[]): PeerDiscovery[] {
  return entries
    .slice()
    .sort((a, b) => b.lastSeen - a.lastSeen)
    .slice(0, 200);
}

function mergeDiscovery(peerId: string, addresses: string[]) {
  const now = Date.now();
  const normalized = addresses
    .filter((addr) => typeof addr === "string")
    .map((addr) => addr.trim())
    .filter((addr) => addr.length > 0);

  discoveredPeersStore.update((entries) => {
    const idx = entries.findIndex((entry) => entry.peerId === peerId);
    if (idx >= 0) {
      const current = entries[idx];
      const mergedAddresses =
        normalized.length > 0
          ? Array.from(new Set([...current.addresses, ...normalized]))
          : current.addresses;
      const next = entries.slice();
      next[idx] = {
        peerId,
        addresses: mergedAddresses,
        lastSeen: now,
      };
      return sortDiscoveries(next);
    }

    const entry: PeerDiscovery = {
      peerId,
      addresses: normalized,
      lastSeen: now,
    };
    return sortDiscoveries([entry, ...entries]);
  });
}

function upsertPeerRecord(peerId: string, address?: string | null) {
  const now = new Date();
  const normalizedAddress = address?.trim();

  peers.update((list) => {
    const idx = list.findIndex(
      (peer) =>
        peer.id === peerId ||
        peer.address === peerId ||
        (normalizedAddress &&
          (peer.address === normalizedAddress || peer.id === normalizedAddress))
    );

    if (idx >= 0) {
      const existing = list[idx];
      const resolvedAddress = normalizedAddress?.length
        ? normalizedAddress
        : (existing.address ?? peerId);
      const updated: PeerInfo = {
        ...existing,
        id: peerId,
        address: resolvedAddress,
        status: "online",
        lastSeen: now,
      };
      const next = list.slice();
      next[idx] = updated;
      return next;
    }

    const resolvedAddress = normalizedAddress?.length
      ? normalizedAddress
      : peerId;
    const newPeer: PeerInfo = {
      id: peerId,
      address: resolvedAddress,
      nickname: undefined,
      status: "online",
      reputation: 0,
      sharedFiles: 0,
      totalSize: 0,
      joinDate: now,
      lastSeen: now,
      location: undefined,
    };
    return [newPeer, ...list];
  });
}

function markPeerOffline(peerId: string) {
  const now = new Date();
  peers.update((list) => {
    const idx = list.findIndex(
      (peer) => peer.id === peerId || peer.address === peerId
    );
    if (idx < 0) {
      return list;
    }
    const next = list.slice();
    next[idx] = {
      ...next[idx],
      status: "offline",
      lastSeen: now,
    };
    return next;
  });
}

export const peerDiscoveryStore = {
  subscribe: discoveredPeersStore.subscribe,
};

export async function startPeerEventStream(): Promise<() => void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return () => {};
  }

  const unlistenFns: UnlistenFn[] = [];

  try {
    unlistenFns.push(
      await listen<PeerDiscoveredPayload>("dht_peer_discovered", (event) => {
        const payload = event.payload;
        if (!payload || !payload.peerId) return;
        mergeDiscovery(payload.peerId, payload.addresses ?? []);
      })
    );

    unlistenFns.push(
      await listen<PeerConnectedPayload>("dht_peer_connected", (event) => {
        const payload = event.payload;
        if (!payload || !payload.peerId) return;
        const addresses = payload.address ? [payload.address] : [];
        mergeDiscovery(payload.peerId, addresses);
        upsertPeerRecord(payload.peerId, payload.address ?? null);
      })
    );

    unlistenFns.push(
      await listen<PeerDisconnectedPayload>(
        "dht_peer_disconnected",
        (event) => {
          const payload = event.payload;
          if (!payload || !payload.peerId) return;
          mergeDiscovery(payload.peerId, []);
          markPeerOffline(payload.peerId);
        }
      )
    );
  } catch (error) {
    console.error("Failed to register peer event listeners:", error);
    unlistenFns.forEach((fn) => fn());
    throw error;
  }

  return () => {
    unlistenFns.forEach((fn) => fn());
  };
}

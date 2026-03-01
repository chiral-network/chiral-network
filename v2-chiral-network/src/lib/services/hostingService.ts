import { invoke } from '@tauri-apps/api/core';
import type {
  HostAdvertisement,
  HostingAgreement,
  HostEntry,
  HostingConfig,
} from '$lib/types/hosting';
import { ratingApi } from '$lib/services/ratingApiService';
import { get } from 'svelte/store';
import { peers } from '$lib/stores';

const AGREEMENT_INDEX_KEY = 'chiral-my-agreement-ids';

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function nowSecs(): number {
  return Math.floor(Date.now() / 1000);
}

function generateAgreementId(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** Load local agreement ID index from localStorage */
function loadAgreementIndex(): string[] {
  try {
    const raw = localStorage.getItem(AGREEMENT_INDEX_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

/** Save local agreement ID index to localStorage */
function saveAgreementIndex(ids: string[]): void {
  localStorage.setItem(AGREEMENT_INDEX_KEY, JSON.stringify(ids));
}

/** Add an agreement ID to the local index */
function addToIndex(id: string): void {
  const ids = loadAgreementIndex();
  if (!ids.includes(id)) {
    ids.push(id);
    saveAgreementIndex(ids);
  }
}

class HostingService {
  /** Publish this node's host advertisement to DHT */
  async publishHostAdvertisement(config: HostingConfig, walletAddress: string): Promise<void> {
    if (!isTauri()) return;

    const ad: HostAdvertisement = {
      peerId: '', // filled by backend
      walletAddress,
      maxStorageBytes: config.maxStorageBytes,
      usedStorageBytes: 0,
      pricePerMbPerDayWei: config.pricePerMbPerDayWei,
      minDepositWei: config.minDepositWei,
      uptimePercent: 100,
      publishedAt: nowSecs(),
      lastHeartbeatAt: nowSecs(),
    };

    await invoke('publish_host_advertisement', {
      advertisementJson: JSON.stringify(ad),
    });
  }

  /** Remove this node's host advertisement from DHT */
  async unpublishHostAdvertisement(): Promise<void> {
    if (!isTauri()) return;
    await invoke('unpublish_host_advertisement');
  }

  /** Discover available hosts from the DHT registry */
  async discoverHosts(): Promise<HostEntry[]> {
    if (!isTauri()) return [];

    const registryJson = await invoke<string>('get_host_registry');
    let registry: { peerId: string; walletAddress: string; updatedAt: number }[];
    try {
      registry = JSON.parse(registryJson);
    } catch {
      return [];
    }

    // Filter out stale entries (no heartbeat in last hour)
    const oneHourAgo = nowSecs() - 3600;

    const connectedPeers = get(peers);
    const connectedIds = new Set(connectedPeers.map((p) => p.id));

    // Get own peer ID to exclude self from the list
    let myPeerId: string | null = null;
    try {
      myPeerId = await invoke<string | null>('get_peer_id');
    } catch {}

    // Fetch advertisements
    const ads: { entry: typeof registry[0]; ad: HostAdvertisement }[] = [];
    for (const entry of registry) {
      // Skip self
      if (entry.peerId === myPeerId) continue;
      // Skip offline peers
      if (!connectedIds.has(entry.peerId)) continue;
      try {
        const adJson = await invoke<string | null>('get_host_advertisement', {
          peerId: entry.peerId,
        });
        if (!adJson) continue;

        const ad: HostAdvertisement = JSON.parse(adJson);
        if (ad.lastHeartbeatAt < oneHourAgo) continue;

        ads.push({ entry, ad });
      } catch {
        continue;
      }
    }

    // Batch fetch reputation scores via wallet addresses
    const wallets = ads.map((a) => a.ad.walletAddress).filter(Boolean);
    let ratings: Record<string, { average: number; count: number }> = {};
    if (wallets.length > 0) {
      try {
        ratings = await ratingApi.getBatchRatings(wallets);
      } catch {
        // Ratings unavailable — continue with default scores
      }
    }

    return ads.map(({ entry, ad }) => {
      const rating = ratings[ad.walletAddress];
      // Normalize 0-5 star rating to 0-1 score; default to 0.5 if no ratings
      const reputationScore = rating && rating.count > 0 ? rating.average / 5 : 0.5;
      const availableStorageBytes = ad.maxStorageBytes - ad.usedStorageBytes;
      const isOnline = connectedIds.has(entry.peerId);

      return { advertisement: ad, reputationScore, availableStorageBytes, isOnline };
    });
  }

  /** Propose a hosting agreement to a host */
  async proposeAgreement(
    clientPeerId: string,
    clientWalletAddress: string,
    hostPeerId: string,
    hostWalletAddress: string,
    fileHashes: string[],
    totalSizeBytes: number,
    durationSecs: number,
    pricePerMbPerDayWei: string,
    depositWei: string,
  ): Promise<HostingAgreement> {
    const agreementId = generateAgreementId();

    // Calculate total cost: (totalSizeBytes / 1MB) * pricePerMbPerDay * (durationSecs / 86400)
    const sizeMb = totalSizeBytes / (1024 * 1024);
    const days = durationSecs / 86400;
    const pricePerMbPerDay = BigInt(pricePerMbPerDayWei);
    const totalCostWei = (pricePerMbPerDay * BigInt(Math.ceil(sizeMb * days * 1000)) / 1000n).toString();

    const agreement: HostingAgreement = {
      agreementId,
      clientPeerId,
      clientWalletAddress,
      hostPeerId,
      hostWalletAddress,
      fileHashes,
      totalSizeBytes,
      durationSecs,
      pricePerMbPerDayWei,
      totalCostWei,
      depositWei,
      status: 'proposed',
      proposedAt: nowSecs(),
    };

    if (isTauri()) {
      await invoke('store_hosting_agreement', {
        agreementId,
        agreementJson: JSON.stringify(agreement),
      });

    }

    addToIndex(agreementId);

    // Send the proposal directly to the host peer via echo protocol
    if (isTauri()) {
      const message = JSON.stringify({
        type: 'hosting_proposal',
        agreement,
      });
      await invoke('echo_peer', {
        peerId: hostPeerId,
        payload: Array.from(new TextEncoder().encode(message)),
      });
    }

    return agreement;
  }

  /** Accept or reject a hosting agreement (called by host) */
  async respondToAgreement(agreementId: string, accept: boolean): Promise<HostingAgreement> {
    const agreement = await this.getAgreement(agreementId);
    if (!agreement) throw new Error('Agreement not found');

    agreement.status = accept ? 'accepted' : 'rejected';
    agreement.respondedAt = nowSecs();

    if (isTauri()) {
      await invoke('store_hosting_agreement', {
        agreementId,
        agreementJson: JSON.stringify(agreement),
      });

      // Notify the proposer of the response via echo protocol
      const message = JSON.stringify({
        type: 'hosting_response',
        agreementId,
        status: agreement.status,
      });
      await invoke('echo_peer', {
        peerId: agreement.clientPeerId,
        payload: Array.from(new TextEncoder().encode(message)),
      });
    }

    return agreement;
  }

  /** Download files from proposer after accepting an agreement (called by host) */
  async fulfillAgreement(agreement: HostingAgreement): Promise<void> {
    for (const fileHash of agreement.fileHashes) {
      await invoke('start_download', {
        fileHash,
        fileName: fileHash,
        seeders: [agreement.clientPeerId],
        speedTier: 'standard',
        fileSize: 0,
        walletAddress: null,
        privateKey: null,
        seederPriceWei: null,
        _seederWalletAddress: null,
      });
    }
  }

  /** Get an agreement by ID (local disk first, DHT fallback) */
  async getAgreement(agreementId: string): Promise<HostingAgreement | null> {
    if (!isTauri()) return null;

    const json = await invoke<string | null>('get_hosting_agreement', { agreementId });
    if (!json) return null;

    try {
      return JSON.parse(json) as HostingAgreement;
    } catch {
      return null;
    }
  }

  /** Get all agreements for this peer (as client or host) */
  async getMyAgreements(): Promise<HostingAgreement[]> {
    // Merge any agreements saved to disk (e.g. received via echo while on another page)
    if (isTauri()) {
      try {
        const diskIds = await invoke<string[]>('list_hosting_agreements');
        for (const id of diskIds) {
          addToIndex(id);
        }
      } catch {}
    }

    const ids = loadAgreementIndex();
    const agreements: HostingAgreement[] = [];

    for (const id of ids) {
      const agreement = await this.getAgreement(id);
      if (agreement) {
        // Update expired status
        if (
          agreement.status === 'active' &&
          agreement.expiresAt &&
          nowSecs() > agreement.expiresAt
        ) {
          agreement.status = 'expired';
        }
        agreements.push(agreement);
      }
    }

    return agreements;
  }

  /** Record deposit transaction hash for an agreement and activate it */
  async recordDeposit(agreementId: string, txHash: string): Promise<void> {
    const agreement = await this.getAgreement(agreementId);
    if (!agreement) throw new Error('Agreement not found');

    agreement.depositTxHash = txHash;
    agreement.status = 'active';
    agreement.activatedAt = nowSecs();
    agreement.expiresAt = nowSecs() + agreement.durationSecs;

    if (isTauri()) {
      await invoke('store_hosting_agreement', {
        agreementId,
        agreementJson: JSON.stringify(agreement),
      });
    }
  }

  /** Request early cancellation (requires other party's consent) */
  async requestCancellation(agreementId: string, myPeerId: string): Promise<void> {
    const agreement = await this.getAgreement(agreementId);
    if (!agreement) throw new Error('Agreement not found');

    agreement.cancelRequestedBy = myPeerId;

    if (isTauri()) {
      await invoke('store_hosting_agreement', {
        agreementId,
        agreementJson: JSON.stringify(agreement),
      });

      // Notify the other party via echo
      const otherPeerId = myPeerId === agreement.clientPeerId
        ? agreement.hostPeerId
        : agreement.clientPeerId;
      const message = JSON.stringify({
        type: 'hosting_cancel_request',
        agreementId,
      });
      await invoke('echo_peer', {
        peerId: otherPeerId,
        payload: Array.from(new TextEncoder().encode(message)),
      });
    }
  }

  /** Approve or deny a cancellation request from the other party */
  async respondToCancellation(agreementId: string, approve: boolean, myPeerId: string): Promise<void> {
    const agreement = await this.getAgreement(agreementId);
    if (!agreement) throw new Error('Agreement not found');

    if (approve) {
      agreement.status = 'cancelled';
    }
    delete agreement.cancelRequestedBy;

    if (isTauri()) {
      await invoke('store_hosting_agreement', {
        agreementId,
        agreementJson: JSON.stringify(agreement),
      });

      // Notify the requester of the decision
      const otherPeerId = myPeerId === agreement.clientPeerId
        ? agreement.hostPeerId
        : agreement.clientPeerId;
      const message = JSON.stringify({
        type: 'hosting_cancel_response',
        agreementId,
        approved: approve,
      });
      await invoke('echo_peer', {
        peerId: otherPeerId,
        payload: Array.from(new TextEncoder().encode(message)),
      });
    }
  }

  /** Store a received agreement in DHT and add to local index */
  async storeAndIndex(agreement: HostingAgreement): Promise<void> {
    if (isTauri()) {
      await invoke('store_hosting_agreement', {
        agreementId: agreement.agreementId,
        agreementJson: JSON.stringify(agreement),
      });
    }
    addToIndex(agreement.agreementId);
  }

  /** Calculate the total cost in wei for a hosting agreement */
  calculateTotalCostWei(
    totalSizeBytes: number,
    durationSecs: number,
    pricePerMbPerDayWei: string,
  ): string {
    const sizeMb = totalSizeBytes / (1024 * 1024);
    const days = durationSecs / 86400;
    const pricePerMbPerDay = BigInt(pricePerMbPerDayWei);
    return (pricePerMbPerDay * BigInt(Math.ceil(sizeMb * days * 1000)) / 1000n).toString();
  }
}

export const hostingService = new HostingService();

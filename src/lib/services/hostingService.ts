import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
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

/** Send an echo message with retry (best-effort, 2 attempts) */
async function echoWithRetry(peerId: string, message: string): Promise<void> {
  const payload = Array.from(new TextEncoder().encode(message));
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      await invoke('echo_peer', { peerId, payload });
      return;
    } catch {
      if (attempt === 0) {
        await new Promise((r) => setTimeout(r, 2000));
      }
    }
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
    } catch { /* peer ID unavailable — don't filter self */ }

    // Fetch advertisements concurrently (not sequentially)
    const candidates = registry.filter(
      (entry) => entry.peerId !== myPeerId && connectedIds.has(entry.peerId)
    );

    const adResults = await Promise.allSettled(
      candidates.map(async (entry) => {
        const adJson = await invoke<string | null>('get_host_advertisement', {
          peerId: entry.peerId,
        });
        if (!adJson) return null;
        const ad: HostAdvertisement = JSON.parse(adJson);
        if (ad.lastHeartbeatAt < oneHourAgo) return null;
        return { entry, ad };
      })
    );

    const ads = adResults
      .filter((r): r is PromiseFulfilledResult<{ entry: typeof registry[0]; ad: HostAdvertisement } | null> =>
        r.status === 'fulfilled' && r.value !== null
      )
      .map((r) => r.value!);

    // Batch fetch Elo reputation scores via wallet addresses
    const wallets = ads.map((a) => a.ad.walletAddress).filter(Boolean);
    let reputations: Record<string, { elo: number }> = {};
    if (wallets.length > 0) {
      try {
        reputations = await ratingApi.getBatchReputation(wallets);
      } catch {
        // Reputation service unavailable — continue with base Elo.
      }
    }

    return ads.map(({ entry, ad }) => {
      const rep = reputations[ad.walletAddress];
      // Elo score is 0-100, base 50.
      const reputationScore = rep?.elo ?? 50;
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

    // Send the proposal directly to the host peer via echo protocol (with retry)
    if (isTauri()) {
      await echoWithRetry(hostPeerId, JSON.stringify({
        type: 'hosting_proposal',
        agreement,
      }));
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

      // Notify the proposer of the response via echo protocol (with retry)
      await echoWithRetry(agreement.clientPeerId, JSON.stringify({
        type: 'hosting_response',
        agreementId,
        status: agreement.status,
      }));
    }

    return agreement;
  }

  /** Download files from proposer after accepting, then auto-seed each one.
   *  The host receives files for free, adds them to Drive, and starts seeding. */
  async fulfillAgreement(agreement: HostingAgreement): Promise<void> {
    if (!isTauri()) return;

    const pendingHashes = new Set(agreement.fileHashes);

    // Listen for download completions to auto-seed each file
    const unlisten = await listen<{
      fileHash: string;
      fileName: string;
      filePath: string;
      fileSize: number;
    }>('file-download-complete', async (event) => {
      const { fileHash, filePath } = event.payload;
      if (!pendingHashes.has(fileHash)) return;
      pendingHashes.delete(fileHash);

      try {
        // Add to Drive
        await invoke('drive_upload_file', {
          owner: agreement.hostWalletAddress,
          filePath,
          parentId: null,
          merkleRoot: fileHash,
        });

        // Register as seeder in DHT so downloaders can find us
        await invoke('seed_hosted_file', {
          fileHash,
          priceChi: null, // host can change price later from Drive page
          walletAddress: agreement.hostWalletAddress,
        });
      } catch (err) {
        console.error(`Failed to seed hosted file ${fileHash}:`, err);
      }

      // Clean up listener when all files are done
      if (pendingHashes.size === 0) {
        unlisten();
      }
    });

    // Start downloading all files from the client (free transfer)
    for (const fileHash of agreement.fileHashes) {
      await invoke('start_download', {
        fileHash,
        fileName: fileHash,
        seeders: [agreement.clientPeerId],
        fileSize: 0,
        walletAddress: null,
        privateKey: null,
        seederPriceWei: null,
        _seederWalletAddress: null,
      });
    }

    // Safety: clean up listener after 10 minutes if some downloads never complete
    setTimeout(() => {
      if (pendingHashes.size > 0) {
        unlisten();
      }
    }, 10 * 60 * 1000);
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
      } catch { /* disk index sync is best-effort */ }
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

  /** Request early cancellation.
   *  - Proposed agreements: cancelled directly (no consent needed — client is withdrawing).
   *  - Accepted/active agreements: sets cancelRequestedBy, other party must approve.
   */
  async requestCancellation(agreementId: string, myPeerId: string): Promise<'cancelled' | 'pending'> {
    const agreement = await this.getAgreement(agreementId);
    if (!agreement) throw new Error('Agreement not found');

    const isProposed = agreement.status === 'proposed';

    if (isProposed) {
      // Direct cancel — no mutual consent needed for withdrawing a proposal
      agreement.status = 'cancelled';
    } else {
      // Mutual consent needed for accepted/active agreements
      agreement.cancelRequestedBy = myPeerId;
    }

    if (isTauri()) {
      await invoke('store_hosting_agreement', {
        agreementId,
        agreementJson: JSON.stringify(agreement),
      });

      // Notify the other party via echo (with retry)
      const otherPeerId = myPeerId === agreement.clientPeerId
        ? agreement.hostPeerId
        : agreement.clientPeerId;
      await echoWithRetry(otherPeerId, JSON.stringify({
        type: 'hosting_cancel_request',
        agreementId,
      }));
    }

    return isProposed ? 'cancelled' : 'pending';
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

      // Notify the requester of the decision (with retry)
      const otherPeerId = myPeerId === agreement.clientPeerId
        ? agreement.hostPeerId
        : agreement.clientPeerId;
      await echoWithRetry(otherPeerId, JSON.stringify({
        type: 'hosting_cancel_response',
        agreementId,
        approved: approve,
      }));
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

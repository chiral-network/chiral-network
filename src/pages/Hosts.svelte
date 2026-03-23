<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Server, Users, Shield, AlertCircle, FileText } from 'lucide-svelte';
  import { settings, walletAccount, networkConnected } from '$lib/stores';
  import { get } from 'svelte/store';
  import { toasts } from '$lib/toastStore';
  import { logger } from '$lib/logger';
  import { hostingService } from '$lib/services/hostingService';
  import { ratingApi } from '$lib/services/ratingApiService';
  import {
    buildHostedSiteUrl,
    resolveHostingPort,
  } from '$lib/utils/hostingPageUtils';
  import type { HostEntry, HostingAgreement } from '$lib/types/hosting';

  import HostingServerBar from '$lib/components/hosting/HostingServerBar.svelte';
  import HostingSiteCreator from '$lib/components/hosting/HostingSiteCreator.svelte';
  import HostingSiteList from '$lib/components/hosting/HostingSiteList.svelte';
  import HostingMarketplace from '$lib/components/hosting/HostingMarketplace.svelte';
  import HostingAgreements from '$lib/components/hosting/HostingAgreements.svelte';
  import HostingProposalModal from '$lib/components/hosting/HostingProposalModal.svelte';
  import HostingDrivePicker from '$lib/components/hosting/HostingDrivePicker.svelte';

  const log = logger('Hosting');
  const RELAY_GATEWAY = 'http://130.245.173.73:8080';

  // ---------------------------------------------------------------------------
  // Tauri check
  // ---------------------------------------------------------------------------
  let isTauri = $state(false);
  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  // ---------------------------------------------------------------------------
  // Types (site hosting)
  // ---------------------------------------------------------------------------
  interface SiteFile { path: string; size: number; }
  interface HostedSite {
    id: string; name: string; directory: string; createdAt: number;
    files: SiteFile[]; relayUrl?: string | null;
  }
  interface ServerStatus { running: boolean; address: string | null; }

  // ---------------------------------------------------------------------------
  // Tab state
  // ---------------------------------------------------------------------------
  type Tab = 'sites' | 'marketplace' | 'agreements';
  let activeTab = $state<Tab>('sites');

  // ---------------------------------------------------------------------------
  // Site hosting state
  // ---------------------------------------------------------------------------
  let serverStatus = $state<ServerStatus>({ running: false, address: null });
  let port = $state(8080);
  let sites = $state<HostedSite[]>([]);
  let newSiteName = $state('');
  let selectedFiles = $state<{ name: string; path: string; size: number }[]>([]);
  let isCreating = $state(false);
  let isStartingServer = $state(false);
  let publishingStates = $state<Record<string, boolean>>({});
  let isDragOver = $state(false);

  // Delete confirmation
  let deleteConfirm = $state<{ id: string; name: string } | null>(null);

  // Drive picker for site creation
  let showDrivePickerForSite = $state(false);
  let drivePickerFiles = $state<{ id: string; name: string; size: number }[]>([]);
  let drivePickerLoading = $state(false);

  // ---------------------------------------------------------------------------
  // Marketplace / agreements state
  // ---------------------------------------------------------------------------
  let loadingAgreements = $state(true);
  let loadingHosts = $state(true);
  let hostingPublishing = $state(false);
  let marketplaceError = $state<string | null>(null);
  let hosts = $state<HostEntry[]>([]);
  let myAgreements = $state<HostingAgreement[]>([]);
  let sortBy = $state<'reputation' | 'price' | 'storage'>('reputation');
  let myPeerId = $state<string | null>(null);

  // Proposal modal
  let proposalHost = $state<HostEntry | null>(null);
  let proposalFileHashes = $state('');
  let proposalDurationDays = $state(7);
  let isProposing = $state(false);

  // Drive picker for proposals
  let proposalDriveFiles = $state<{ id: string; name: string; size: number }[]>([]);
  let showProposalDrivePicker = $state(false);
  let publishingDriveFile = $state<string | null>(null);

  // Auto-accept state
  let autoAcceptInFlight = $state<Record<string, boolean>>({});
  const proposerEloCache = new Map<string, { elo: number; expiresAt: number }>();
  const ELO_CACHE_TTL_MS = 60000;

  // Derived
  let incomingProposals = $derived(
    myAgreements.filter((a) => a.hostPeerId === myPeerId && a.status === 'proposed')
  );
  let activeAgreements = $derived(
    myAgreements.filter((a) =>
      a.status !== 'cancelled' &&
      (a.status !== 'proposed' || a.clientPeerId === myPeerId)
    )
  );
  let hostedFiles = $derived.by(() => {
    if (!myPeerId) return [];
    const files: { fileHash: string; agreementId: string; clientPeerId: string; expiresAt?: number }[] = [];
    for (const agreement of myAgreements) {
      if (agreement.hostPeerId !== myPeerId) continue;
      if (agreement.status !== 'active' && agreement.status !== 'accepted') continue;
      for (const hash of agreement.fileHashes) {
        files.push({
          fileHash: hash,
          agreementId: agreement.agreementId,
          clientPeerId: agreement.clientPeerId,
          expiresAt: agreement.expiresAt,
        });
      }
    }
    return files;
  });

  // =========================================================================
  // Site hosting functions
  // =========================================================================

  async function loadServerStatus() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      serverStatus = await invoke<ServerStatus>('get_hosting_server_status');
    } catch (err) {
      log.error('Failed to get server status:', err);
    }
  }

  async function startServer() {
    if (!isTauri) return;
    isStartingServer = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const addr = await invoke<string>('start_hosting_server', { port });
      serverStatus = { running: true, address: addr };
      toasts.detail('Server started', `Listening on ${addr}`, 'success');
      localStorage.setItem('chiral-hosting-port', String(port));
    } catch (err: any) {
      toasts.detail('Server failed to start', String(err), 'error');
    } finally {
      isStartingServer = false;
    }
  }

  async function stopServer() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('stop_hosting_server');
      serverStatus = { running: false, address: null };
      // Silent — server status reflected in UI
    } catch (err: any) {
      toasts.detail('Failed to stop server', String(err), 'error');
    }
  }

  async function loadSites() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      sites = await invoke<HostedSite[]>('list_hosted_sites');
    } catch (err) {
      log.error('Failed to load sites:', err);
    }
  }

  async function selectFiles() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const paths = await invoke<string[]>('open_file_dialog', { multiple: true });
      if (paths && paths.length > 0) {
        for (const p of paths) {
          const name = p.split(/[\\/]/).pop() || p;
          const size = await invoke<number>('get_file_size', { filePath: p });
          if (!selectedFiles.some(f => f.path === p)) {
            selectedFiles = [...selectedFiles, { name, path: p, size }];
          }
        }
      }
    } catch (err) {
      log.error('File dialog error:', err);
    }
  }

  async function openDrivePickerForSite() {
    const wallet = get(walletAccount);
    if (!wallet?.address) {
      toasts.show('Connect your wallet first', 'warning');
      return;
    }
    drivePickerLoading = true;
    showDrivePickerForSite = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const items = await invoke<{ id: string; name: string; itemType: string; size?: number }[]>(
        'drive_list_items', { owner: wallet.address, parentId: null }
      );
      drivePickerFiles = items
        .filter((i) => i.itemType === 'file' && i.size)
        .map((i) => ({ id: i.id, name: i.name, size: i.size! }));
    } catch {
      toasts.show('Failed to load Drive files', 'error');
      showDrivePickerForSite = false;
    } finally {
      drivePickerLoading = false;
    }
  }

  async function addDriveFilesToSite(files: { id: string; name: string; size: number }[]) {
    showDrivePickerForSite = false;
    if (files.length === 0) return;
    const wallet = get(walletAccount);
    if (!wallet?.address) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      for (const file of files) {
        const filePath = await invoke<string>('get_drive_file_path', {
          owner: wallet.address,
          itemId: file.id,
        });
        if (!selectedFiles.some(f => f.path === filePath)) {
          selectedFiles = [...selectedFiles, { name: file.name, path: filePath, size: file.size }];
        }
      }
    } catch (err: any) {
      toasts.detail('Failed to load files', String(err?.message || err), 'error');
    }
  }

  function removeSelectedFile(index: number) {
    selectedFiles = selectedFiles.filter((_, i) => i !== index);
  }

  async function createSite() {
    if (!isTauri || !newSiteName.trim() || selectedFiles.length === 0) return;
    isCreating = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const filePaths = selectedFiles.map(f => f.path);
      const site = await invoke<HostedSite>('create_hosted_site', {
        name: newSiteName.trim(),
        filePaths,
      });
      sites = [...sites, site];
      toasts.show(`Site "${site.name}" created`, 'success');
      newSiteName = '';
      selectedFiles = [];
    } catch (err: any) {
      toasts.detail('Failed to create site', String(err), 'error');
    } finally {
      isCreating = false;
    }
  }

  function deleteSite(siteId: string, siteName: string) {
    deleteConfirm = { id: siteId, name: siteName };
  }

  async function confirmDeleteSite() {
    if (!deleteConfirm || !isTauri) return;
    const { id, name } = deleteConfirm;
    deleteConfirm = null;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('delete_hosted_site', { siteId: id });
      sites = sites.filter(s => s.id !== id);
      // Silent — site removed from list
    } catch (err: any) {
      toasts.detail('Failed to delete site', String(err), 'error');
    }
  }

  function copySiteUrl(site: HostedSite) {
    const url = buildHostedSiteUrl(site.id, site.relayUrl, serverStatus.address, port);
    navigator.clipboard.writeText(url);
    toasts.show('URL copied', 'success');
  }

  async function openSite(site: HostedSite) {
    const url = buildHostedSiteUrl(site.id, site.relayUrl, serverStatus.address, port);
    try {
      const { open } = await import('@tauri-apps/plugin-shell');
      await open(url);
    } catch {
      window.open(url, '_blank');
    }
  }

  async function publishToRelay(siteId: string) {
    if (!isTauri) return;
    publishingStates = { ...publishingStates, [siteId]: true };
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const relayUrl = await invoke<string>('publish_site_to_relay', { siteId, relayUrl: RELAY_GATEWAY });
      await loadSites();
      toasts.detail('Site published', relayUrl, 'success');
    } catch (err: any) {
      toasts.detail('Failed to publish', String(err), 'error');
    } finally {
      publishingStates = { ...publishingStates, [siteId]: false };
    }
  }

  async function unpublishFromRelay(siteId: string) {
    if (!isTauri) return;
    publishingStates = { ...publishingStates, [siteId]: true };
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('unpublish_site_from_relay', { siteId });
      await loadSites();
      // Silent — publish state reflected in UI
    } catch (err: any) {
      toasts.detail('Failed to unpublish', String(err), 'error');
    } finally {
      publishingStates = { ...publishingStates, [siteId]: false };
    }
  }

  // Drag and drop
  let unlistenDragDrop: (() => void) | undefined;

  async function addFilesFromPaths(paths: string[]) {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      for (const p of paths) {
        const name = p.split(/[\\/]/).pop() || p;
        if (selectedFiles.some(f => f.path === p)) continue;
        let size = 0;
        try { size = await invoke<number>('get_file_size', { filePath: p }); } catch { /* ignore */ }
        selectedFiles = [...selectedFiles, { name, path: p, size }];
      }
    } catch (err) {
      log.error('Failed to add dropped files:', err);
    }
  }

  // =========================================================================
  // Marketplace / agreements functions
  // =========================================================================

  async function loadAgreements() {
    loadingAgreements = true;
    try {
      if (isTauri) {
        const { invoke } = await import('@tauri-apps/api/core');
        const pid = await invoke<string | null>('get_peer_id');
        myPeerId = pid;
      }
      myAgreements = await hostingService.getMyAgreements().catch(() => [] as HostingAgreement[]);
    } catch (err: any) {
      marketplaceError = `Failed to load agreements: ${err.message || err}`;
    } finally {
      loadingAgreements = false;
    }
  }

  async function loadHosts() {
    loadingHosts = true;
    try {
      hosts = await hostingService.discoverHosts().catch(() => [] as HostEntry[]);
    } catch { /* show empty */ } finally {
      loadingHosts = false;
    }
  }

  async function refreshHosts() {
    loadingHosts = true;
    try {
      hosts = await hostingService.discoverHosts();
      // Silent — host list updated in UI
    } catch (err: any) {
      toasts.detail('Failed to refresh hosts', String(err.message || err), 'error');
    } finally {
      loadingHosts = false;
    }
  }

  let hostingPublished = $state(false);

  async function publishHosting() {
    if (hostingPublishing) return;
    if (!get(networkConnected)) { toasts.show('Connect to the network first', 'warning'); return; }
    const wallet = get(walletAccount);
    if (!wallet?.address) { toasts.show('Connect your wallet first', 'warning'); return; }
    hostingPublishing = true;
    try {
      await hostingService.publishHostAdvertisement($settings.hostingConfig, wallet.address);
      hostingPublished = true;
      toasts.show('Hosting published to network', 'success');
    } catch (err: any) {
      log.error('Publish hosting failed:', err);
      toasts.detail('Failed to publish hosting', String(err?.message || err), 'error');
    } finally {
      hostingPublishing = false;
    }
  }

  async function unpublishHosting() {
    if (hostingPublishing) return;
    if (!get(networkConnected)) { toasts.show('Not connected to the network', 'warning'); return; }
    hostingPublishing = true;
    try {
      await hostingService.unpublishHostAdvertisement();
      hostingPublished = false;
      toasts.show('Hosting unpublished', 'info');
    } catch (err: any) {
      log.error('Unpublish hosting failed:', err);
      toasts.detail('Failed to unpublish', String(err?.message || err), 'error');
    } finally {
      hostingPublishing = false;
    }
  }

  async function toggleHostingEnabled() {
    const nextEnabled = !$settings.hostingConfig.enabled;
    settings.update((s) => ({
      ...s,
      hostingConfig: { ...s.hostingConfig, enabled: nextEnabled },
    }));
    if (nextEnabled) {
      const wallet = get(walletAccount);
      if (wallet?.address) {
        await publishHosting();
      } else {
        toasts.detail('Hosting enabled', 'Will auto-publish when wallet is connected', 'info');
      }
      return;
    }
    await unpublishHosting();
  }

  // Proposal flow
  function openProposalModal(host: HostEntry) {
    const wallet = get(walletAccount);
    if (!wallet?.address) { toasts.show('Connect your wallet first', 'warning'); return; }
    proposalHost = host;
    proposalFileHashes = '';
    proposalDurationDays = 7;
  }

  async function loadProposalDriveFiles() {
    const wallet = get(walletAccount);
    if (!wallet?.address) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const items = await invoke<{ id: string; name: string; itemType: string; size?: number }[]>(
        'drive_list_items', { owner: wallet.address, parentId: null }
      );
      proposalDriveFiles = items
        .filter((i) => i.itemType === 'file' && i.size)
        .map((i) => ({ id: i.id, name: i.name, size: i.size! }));
      showProposalDrivePicker = true;
    } catch {
      toasts.show('Failed to load Drive files', 'error');
    }
  }

  async function addProposalDriveFile(fileId: string, fileName: string) {
    const wallet = get(walletAccount);
    if (!wallet?.address) return;
    publishingDriveFile = fileId;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const item = await invoke<{ merkleRoot?: string }>('publish_drive_file', {
        owner: wallet.address, itemId: fileId,
        protocol: null, priceChi: null, walletAddress: wallet.address,
      });
      const hash = item.merkleRoot;
      if (!hash) { toasts.show(`${fileName} has no file hash`, 'error'); return; }
      const existing = proposalFileHashes.split('\n').map((h) => h.trim()).filter(Boolean);
      if (!existing.includes(hash)) {
        proposalFileHashes = [...existing, hash].join('\n');
      }
      toasts.show(`${fileName} published to network`, 'success');
    } catch (err: any) {
      toasts.detail(`Failed to publish ${fileName}`, String(err.message || err), 'error');
    } finally {
      publishingDriveFile = null;
    }
  }

  async function sendProposal() {
    if (!proposalHost || isProposing) return;
    const wallet = get(walletAccount);
    if (!wallet?.address || !myPeerId) {
      toasts.show('Wallet or peer ID not available', 'warning');
      return;
    }
    const hashes = proposalFileHashes.split('\n').map((h) => h.trim()).filter(Boolean);
    if (hashes.length === 0) { toasts.show('Enter at least one file hash', 'warning'); return; }

    isProposing = true;
    try {
      const agreement = await hostingService.proposeAgreement(
        myPeerId, wallet.address,
        proposalHost.advertisement.peerId, proposalHost.advertisement.walletAddress,
        hashes, 0, proposalDurationDays * 86400,
        proposalHost.advertisement.pricePerMbPerDayWei,
        proposalHost.advertisement.minDepositWei,
      );
      myAgreements = [...myAgreements, agreement];
      proposalHost = null;
      toasts.show('Hosting proposal sent', 'success');
    } catch (err: any) {
      toasts.detail('Proposal failed', String(err.message || err), 'error');
    } finally {
      isProposing = false;
    }
  }

  // Agreement actions
  async function respondToAgreement(
    agreementId: string, accept: boolean,
    options?: { silent?: boolean; reason?: string },
  ) {
    try {
      const updated = await hostingService.respondToAgreement(agreementId, accept);
      myAgreements = myAgreements.map((a) =>
        a.agreementId === agreementId
          ? { ...a, status: accept ? 'accepted' : 'rejected', respondedAt: Math.floor(Date.now() / 1000) }
          : a
      );
      if (!options?.silent) {
        toasts.show(accept ? 'Agreement accepted — downloading files' : 'Agreement rejected', accept ? 'success' : 'info');
      } else if (options.reason) {
        toasts.show(options.reason, 'success');
      }
      if (accept && updated) {
        hostingService.fulfillAgreement(updated).catch((err: any) => {
          toasts.detail('File download failed', String(err.message || err), 'error');
        });
      }
    } catch (err: any) {
      toasts.detail('Action failed', String(err.message || err), 'error');
    }
  }

  async function requestCancellation(agreementId: string) {
    if (!myPeerId) return;
    try {
      const result = await hostingService.requestCancellation(agreementId, myPeerId);
      if (result === 'cancelled') {
        myAgreements = myAgreements.map((a) =>
          a.agreementId === agreementId ? { ...a, status: 'cancelled' } : a
        );
        toasts.show('Proposal withdrawn', 'info');
      } else {
        myAgreements = myAgreements.map((a) =>
          a.agreementId === agreementId ? { ...a, cancelRequestedBy: myPeerId ?? undefined } : a
        );
        toasts.show('Cancellation requested — waiting for other party', 'info');
      }
    } catch (err: any) {
      toasts.detail('Cancellation failed', String(err.message || err), 'error');
    }
  }

  async function respondToCancellation(agreementId: string, approve: boolean) {
    if (!myPeerId) return;
    try {
      await hostingService.respondToCancellation(agreementId, approve, myPeerId);
      if (approve) {
        const agreement = myAgreements.find((a) => a.agreementId === agreementId);
        if (agreement && agreement.hostPeerId === myPeerId) {
          const { invoke } = await import('@tauri-apps/api/core');
          try { await invoke('cleanup_agreement_files', { agreementId }); } catch { /* best-effort */ }
          try { await cleanupDriveSharedFiles(agreementId); } catch { /* best-effort */ }
        }
        myAgreements = myAgreements.map((a) =>
          a.agreementId === agreementId ? { ...a, status: 'cancelled', cancelRequestedBy: undefined } : a
        );
        toasts.show('Agreement cancelled', 'info');
      } else {
        myAgreements = myAgreements.map((a) =>
          a.agreementId === agreementId ? { ...a, cancelRequestedBy: undefined } : a
        );
        toasts.show('Cancellation denied', 'info');
      }
    } catch (err: any) {
      toasts.detail('Action failed', String(err.message || err), 'error');
    }
  }

  async function cleanupDriveSharedFiles(agreementId: string) {
    const wallet = get(walletAccount);
    if (!wallet?.address) return;
    const agreement = myAgreements.find((a) => a.agreementId === agreementId);
    if (!agreement?.fileHashes?.length) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const allItems = await invoke<{ id: string; name: string; itemType: string; merkleRoot?: string }[]>(
        'drive_list_all_items', { owner: wallet.address },
      );
      const hashSet = new Set(agreement.fileHashes);
      for (const item of allItems) {
        if (item.itemType === 'file' && (hashSet.has(item.name) || (item.merkleRoot && hashSet.has(item.merkleRoot)))) {
          await invoke('drive_delete_item', { owner: wallet.address, itemId: item.id });
        }
      }
    } catch (err) {
      console.warn('Failed to cleanup Drive shared files:', err);
    }
  }

  // Auto-accept
  async function getWalletElo(walletAddress: string): Promise<number> {
    const key = walletAddress.trim();
    if (!key) return 50;
    const now = Date.now();
    const cached = proposerEloCache.get(key);
    if (cached && cached.expiresAt > now) return cached.elo;
    try {
      const reputations = await ratingApi.getBatchReputation([key]);
      const elo = reputations?.[key]?.elo;
      const normalized = Number.isFinite(elo) ? Number(elo) : 50;
      proposerEloCache.set(key, { elo: normalized, expiresAt: now + ELO_CACHE_TTL_MS });
      return normalized;
    } catch { return 50; }
  }

  async function maybeAutoAcceptAgreement(agreement: HostingAgreement) {
    if (!myPeerId || !$settings.hostingConfig.autoAcceptByElo) return;
    if (agreement.hostPeerId !== myPeerId || agreement.status !== 'proposed') return;
    if (autoAcceptInFlight[agreement.agreementId]) return;
    autoAcceptInFlight = { ...autoAcceptInFlight, [agreement.agreementId]: true };
    try {
      const proposerElo = await getWalletElo(agreement.clientWalletAddress);
      const threshold = Number.isFinite($settings.hostingConfig.minAutoAcceptElo)
        ? Number($settings.hostingConfig.minAutoAcceptElo) : 60;
      if (proposerElo < threshold) return;
      await respondToAgreement(agreement.agreementId, true, {
        silent: true,
        reason: `Auto-accepted: proposer Elo ${proposerElo.toFixed(1)} >= ${threshold.toFixed(1)}`,
      });
    } finally {
      const next = { ...autoAcceptInFlight };
      delete next[agreement.agreementId];
      autoAcceptInFlight = next;
    }
  }

  async function maybeAutoAcceptIncomingProposals() {
    if (!myPeerId || !$settings.hostingConfig.autoAcceptByElo) return;
    const proposals = myAgreements.filter((a) => a.hostPeerId === myPeerId && a.status === 'proposed');
    for (const proposal of proposals) {
      await maybeAutoAcceptAgreement(proposal);
    }
  }

  // =========================================================================
  // Lifecycle
  // =========================================================================

  let unlistenDragDrop_fn: (() => void) | undefined;
  let unlistenProposal: (() => void) | null = null;
  let unlistenResponse: (() => void) | null = null;
  let unlistenDownloadComplete: (() => void) | null = null;
  let unlistenCancelRequest: (() => void) | null = null;
  let unlistenCancelResponse: (() => void) | null = null;

  onMount(async () => {
    isTauri = checkTauriAvailability();
    port = resolveHostingPort(localStorage.getItem('chiral-hosting-port'));

    // Load site hosting data
    await loadServerStatus();
    await loadSites();

    // Load marketplace data
    await loadAgreements();
    loadHosts();

    if (isTauri) {
      // Drag-drop for site creation
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const appWindow = getCurrentWindow();
        unlistenDragDrop_fn = await appWindow.onDragDropEvent((event) => {
          if (activeTab !== 'sites') return;
          if (event.payload.type === 'drop') {
            const paths = event.payload.paths;
            if (paths && paths.length > 0) addFilesFromPaths(paths);
          } else if (event.payload.type === 'enter') {
            isDragOver = true;
          } else if (event.payload.type === 'leave') {
            isDragOver = false;
          }
        });
      } catch (error) {
        log.error('Failed to setup drag-drop listener:', error);
      }

      const { listen } = await import('@tauri-apps/api/event');
      const { invoke } = await import('@tauri-apps/api/core');

      // Re-register hosted files
      try {
        const hostedEntries = await invoke<{ fileHash: string; agreementId: string; clientPeerId: string }[]>('get_active_hosted_files');
        const downloadDir = await invoke<string>('get_download_directory');
        const wallet = get(walletAccount);
        for (const entry of hostedEntries) {
          try {
            await invoke('republish_shared_file', {
              fileHash: entry.fileHash,
              filePath: `${downloadDir}/${entry.fileHash}`,
              fileName: entry.fileHash,
              fileSize: 0,
              priceChi: null,
              walletAddress: wallet?.address ?? null,
            });
          } catch { /* skip */ }
        }
      } catch { /* agreements dir may not exist */ }

      // Listen for incoming proposals
      unlistenProposal = await listen<{ fromPeer: string; agreementJson: string }>(
        'hosting_proposal_received',
        (event) => {
          try {
            const agreement: HostingAgreement = JSON.parse(event.payload.agreementJson);
            hostingService.storeAndIndex(agreement);
            const exists = myAgreements.some((a) => a.agreementId === agreement.agreementId);
            if (exists) {
              myAgreements = myAgreements.map((a) =>
                a.agreementId === agreement.agreementId ? agreement : a,
              );
            } else {
              myAgreements = [...myAgreements, agreement];
              toasts.detail('New hosting proposal', `From peer ${event.payload.fromPeer.slice(0, 8)}…`, 'info');
            }
            void maybeAutoAcceptAgreement(agreement);
          } catch { /* ignore malformed */ }
        },
      );

      // Listen for responses
      unlistenResponse = await listen<{ agreementId: string; status: string }>(
        'hosting_response_received',
        (event) => {
          const { agreementId, status } = event.payload;
          myAgreements = myAgreements.map((a): HostingAgreement =>
            a.agreementId === agreementId ? { ...a, status: status as HostingAgreement['status'] } : a
          );
          if (status === 'accepted') toasts.show('Agreement accepted by host', 'success');
          else if (status === 'rejected') toasts.show('Agreement rejected by host', 'warning');
          else if (status === 'active') toasts.show('Host is now seeding your files', 'success');
        },
      );

      // Listen for download completions
      unlistenDownloadComplete = await listen<{ fileHash: string; fileName: string; filePath: string; fileSize: number }>(
        'file-download-complete',
        async (event) => {
          const { fileHash, fileName } = event.payload;
          const agreement = myAgreements.find(
            (a) => a.hostPeerId === myPeerId && (a.status === 'accepted' || a.status === 'active') && a.fileHashes.includes(fileHash)
          );
          if (!agreement) return;
          toasts.detail('Now seeding', `${fileName} for hosting agreement`, 'success');
          myAgreements = myAgreements.map((a) =>
            a.agreementId === agreement.agreementId ? { ...a, status: 'active' } : a
          );
        },
      );

      // Listen for cancellation requests
      unlistenCancelRequest = await listen<{ agreementId: string; fromPeer: string; autoCancelled: boolean }>(
        'hosting_cancel_request_received',
        async (event) => {
          const { agreementId, fromPeer, autoCancelled } = event.payload;
          if (autoCancelled) {
            const agreement = myAgreements.find((a) => a.agreementId === agreementId);
            if (agreement && agreement.hostPeerId === myPeerId) {
              try { await invoke('cleanup_agreement_files', { agreementId }); } catch { /* best-effort */ }
              try { await cleanupDriveSharedFiles(agreementId); } catch { /* best-effort */ }
            }
            myAgreements = myAgreements.map((a) =>
              a.agreementId === agreementId ? { ...a, status: 'cancelled', cancelRequestedBy: undefined } : a
            );
            toasts.detail('Agreement cancelled', `By peer ${fromPeer.slice(0, 8)}…`, 'info');
          } else {
            myAgreements = myAgreements.map((a) =>
              a.agreementId === agreementId ? { ...a, cancelRequestedBy: fromPeer } : a
            );
            toasts.detail('Cancellation requested', `By peer ${fromPeer.slice(0, 8)}…`, 'info');
          }
        },
      );

      // Listen for cancellation responses
      unlistenCancelResponse = await listen<{ agreementId: string; approved: boolean }>(
        'hosting_cancel_response_received',
        async (event) => {
          const { agreementId, approved } = event.payload;
          if (approved) {
            const agreement = myAgreements.find((a) => a.agreementId === agreementId);
            if (agreement && agreement.hostPeerId === myPeerId) {
              try { await invoke('cleanup_agreement_files', { agreementId }); } catch { /* best-effort */ }
              try { await cleanupDriveSharedFiles(agreementId); } catch { /* best-effort */ }
            }
            myAgreements = myAgreements.map((a) =>
              a.agreementId === agreementId ? { ...a, status: 'cancelled', cancelRequestedBy: undefined } : a
            );
            toasts.show('Cancellation approved — agreement cancelled', 'info');
          } else {
            myAgreements = myAgreements.map((a) =>
              a.agreementId === agreementId ? { ...a, cancelRequestedBy: undefined } : a
            );
            toasts.show('Cancellation denied by other party', 'info');
          }
        },
      );
    }
  });

  onDestroy(() => {
    unlistenDragDrop_fn?.();
    unlistenProposal?.();
    unlistenResponse?.();
    unlistenDownloadComplete?.();
    unlistenCancelRequest?.();
    unlistenCancelResponse?.();
  });

  $effect(() => {
    if (!$settings.hostingConfig.autoAcceptByElo) return;
    if (!myPeerId) return;
    void maybeAutoAcceptIncomingProposals();
  });
</script>

<svelte:head>
  <title>Hosts - Chiral Network</title>
</svelte:head>

<div class="p-4 sm:p-6 space-y-5 max-w-6xl mx-auto">
  <!-- Header -->
  <div class="flex items-start justify-between gap-4">
    <div>
      <h1 class="text-2xl font-bold text-gray-900 dark:text-white">Hosts</h1>
      <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
        Host websites and files, find hosting providers, and manage agreements
      </p>
    </div>
    <!-- Quick stats -->
    <div class="hidden sm:flex items-center gap-3">
      {#if sites.length > 0}
        <div class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-primary-50 dark:bg-primary-900/20 text-xs font-medium text-primary-700 dark:text-primary-300">
          <Server class="w-3.5 h-3.5" />
          {sites.length} site{sites.length !== 1 ? 's' : ''}
        </div>
      {/if}
      {#if activeAgreements.filter(a => a.status === 'active').length > 0}
        <div class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-emerald-50 dark:bg-emerald-900/20 text-xs font-medium text-emerald-700 dark:text-emerald-300">
          <FileText class="w-3.5 h-3.5" />
          {activeAgreements.filter(a => a.status === 'active').length} active
        </div>
      {/if}
    </div>
  </div>

  <!-- Server Status Bar (always visible) -->
  <HostingServerBar
    serverRunning={serverStatus.running}
    serverAddress={serverStatus.address}
    {port}
    {isStartingServer}
    onStartServer={startServer}
    onStopServer={stopServer}
    onPortChange={(p) => port = p}
  />

  <!-- Tab bar -->
  <div class="flex gap-1 bg-gray-100 dark:bg-gray-800/60 rounded-xl p-1" role="tablist" aria-label="Hosting sections">
    <button
      onclick={() => activeTab = 'sites'}
      role="tab"
      aria-selected={activeTab === 'sites'}
      class="flex items-center gap-2 px-4 py-2.5 text-sm font-medium rounded-lg transition-all flex-1 justify-center
        {activeTab === 'sites'
          ? 'bg-white dark:bg-gray-700 text-gray-900 dark:text-white shadow-sm ring-1 ring-gray-200/50 dark:ring-gray-600/50'
          : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-white/50 dark:hover:bg-gray-700/30'}"
    >
      <Server class="w-4 h-4" />
      <span class="hidden sm:inline">My Sites</span>
    </button>
    <button
      onclick={() => activeTab = 'marketplace'}
      role="tab"
      aria-selected={activeTab === 'marketplace'}
      class="flex items-center gap-2 px-4 py-2.5 text-sm font-medium rounded-lg transition-all flex-1 justify-center
        {activeTab === 'marketplace'
          ? 'bg-white dark:bg-gray-700 text-gray-900 dark:text-white shadow-sm ring-1 ring-gray-200/50 dark:ring-gray-600/50'
          : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-white/50 dark:hover:bg-gray-700/30'}"
    >
      <Users class="w-4 h-4" />
      <span class="hidden sm:inline">Marketplace</span>
    </button>
    <button
      onclick={() => activeTab = 'agreements'}
      role="tab"
      aria-selected={activeTab === 'agreements'}
      class="flex items-center gap-2 px-4 py-2.5 text-sm font-medium rounded-lg transition-all flex-1 justify-center
        {activeTab === 'agreements'
          ? 'bg-white dark:bg-gray-700 text-gray-900 dark:text-white shadow-sm ring-1 ring-gray-200/50 dark:ring-gray-600/50'
          : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-white/50 dark:hover:bg-gray-700/30'}"
    >
      <Shield class="w-4 h-4" />
      <span class="hidden sm:inline">Agreements</span>
      {#if incomingProposals.length > 0}
        <span class="ml-0.5 min-w-[1.25rem] px-1.5 py-0.5 text-[10px] font-bold rounded-full bg-blue-500 text-white leading-none text-center animate-pulse">
          {incomingProposals.length}
        </span>
      {/if}
    </button>
  </div>

  <!-- Tab content -->
  <div role="tabpanel" id="tabpanel-{activeTab}" aria-labelledby="tab-{activeTab}" class="space-y-5">
  {#if activeTab === 'sites'}
    <HostingSiteCreator
      {newSiteName}
      {selectedFiles}
      {isDragOver}
      {isCreating}
      onNameChange={(name) => newSiteName = name}
      onSelectFiles={selectFiles}
      onSelectFromDrive={openDrivePickerForSite}
      onRemoveFile={removeSelectedFile}
      onCreateSite={createSite}
    />
    <HostingSiteList
      {sites}
      serverAddress={serverStatus.address}
      {port}
      {publishingStates}
      onPublish={publishToRelay}
      onUnpublish={unpublishFromRelay}
      onCopyUrl={copySiteUrl}
      onOpenSite={openSite}
      onDeleteSite={deleteSite}
    />
  {:else if activeTab === 'marketplace'}
    {#if marketplaceError}
      <div class="text-center py-20">
        <AlertCircle class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
        <p class="text-gray-500 dark:text-gray-400">{marketplaceError}</p>
      </div>
    {:else}
      <HostingMarketplace
        {hosts}
        {loadingHosts}
        {hostingPublishing}
        {hostingPublished}
        connected={$networkConnected}
        {sortBy}
        onSortChange={(s) => sortBy = s}
        onRefreshHosts={refreshHosts}
        onPropose={openProposalModal}
        onToggleEnabled={toggleHostingEnabled}
        onPublish={publishHosting}
        onUnpublish={unpublishHosting}
      />
    {/if}
  {:else if activeTab === 'agreements'}
    <HostingAgreements
      {myAgreements}
      {loadingAgreements}
      {myPeerId}
      {incomingProposals}
      {activeAgreements}
      {hostedFiles}
      onRespondToAgreement={(id, accept) => respondToAgreement(id, accept)}
      onRequestCancellation={requestCancellation}
      onRespondToCancellation={respondToCancellation}
    />
  {/if}
  </div>
</div>

<!-- Proposal Modal -->
{#if proposalHost}
  <HostingProposalModal
    {proposalHost}
    {proposalFileHashes}
    {proposalDurationDays}
    {isProposing}
    driveFiles={proposalDriveFiles}
    showDrivePicker={showProposalDrivePicker}
    {publishingDriveFile}
    onFileHashesChange={(v) => proposalFileHashes = v}
    onDurationChange={(d) => proposalDurationDays = d}
    onLoadDriveFiles={loadProposalDriveFiles}
    onAddDriveFile={addProposalDriveFile}
    onSendProposal={sendProposal}
    onClose={() => proposalHost = null}
  />
{/if}

<!-- Drive File Picker for Site Creation -->
{#if showDrivePickerForSite}
  <HostingDrivePicker
    files={drivePickerFiles}
    loading={drivePickerLoading}
    onSelect={addDriveFilesToSite}
    onClose={() => showDrivePickerForSite = false}
  />
{/if}

<!-- Delete site confirmation -->
{#if deleteConfirm}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
    onclick={() => deleteConfirm = null}
    onkeydown={(e) => { if (e.key === 'Escape') deleteConfirm = null; }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl p-6 max-w-sm w-full mx-4"
      onclick={(e) => e.stopPropagation()}
    >
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-2">Delete Site</h3>
      <p class="text-sm text-gray-600 dark:text-gray-400 mb-1">
        Are you sure you want to delete <strong class="text-gray-900 dark:text-white">"{deleteConfirm.name}"</strong>?
      </p>
      <p class="text-sm text-amber-600 dark:text-amber-400 mb-4">This cannot be undone.</p>
      <div class="flex justify-end gap-3">
        <button
          onclick={() => deleteConfirm = null}
          class="px-4 py-2 text-sm font-medium rounded-lg text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 transition"
        >Cancel</button>
        <button
          onclick={confirmDeleteSite}
          class="px-4 py-2 text-sm font-medium rounded-lg text-white bg-red-600 hover:bg-red-700 transition"
        >Delete</button>
      </div>
    </div>
  </div>
{/if}

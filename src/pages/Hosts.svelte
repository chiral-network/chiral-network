<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Server, Users, Shield, AlertCircle, FileText, Cloud, Globe, Loader2, RefreshCw, Upload, Trash2, Copy, Check } from 'lucide-svelte';
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
  type Tab = 'sites' | 'cdn' | 'marketplace' | 'agreements';
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

  // ---------------------------------------------------------------------------
  // CDN state
  // ---------------------------------------------------------------------------
  const CDN_SERVERS = [
    { url: 'http://130.245.173.73:9420', name: 'CDN Primary (US East)', region: 'New York' },
  ];

  interface CdnServerInfo {
    url: string;
    name: string;
    region: string;
    status: string;
    peerId: string;
    walletAddress: string;
    activeFiles: number;
    totalStorageBytes: number;
    uniqueOwners: number;
    myFiles: CdnFile[];
  }

  interface CdnFile {
    fileHash: string;
    fileName: string;
    fileSize: number;
    ownerWallet: string;
    downloadPriceChi?: string;
    uploadedAt: number;
    expiresAt: number;
  }

  let cdnServers = $state<CdnServerInfo[]>([]);
  let loadingCdn = $state(false);
  let copiedCdnHash = $state<string | null>(null);

  async function loadCdnServers() {
    loadingCdn = true;
    const results: CdnServerInfo[] = [];

    const myWallet = $walletAccount?.address || '';

    for (const server of CDN_SERVERS) {
      try {
        const statusResp = await fetch(`${server.url}/api/cdn/status`);
        const status = await statusResp.json();

        let myFiles: CdnFile[] = [];
        if (myWallet) {
          const filesResp = await fetch(`${server.url}/api/cdn/files?owner=${myWallet}`);
          const filesData = await filesResp.json();
          myFiles = filesData.files || [];
        }

        results.push({
          url: server.url,
          name: server.name,
          region: server.region,
          status: status.status || 'unknown',
          peerId: status.peerId || '',
          walletAddress: status.walletAddress || '',
          activeFiles: status.activeFiles || 0,
          totalStorageBytes: status.totalStorageBytes || 0,
          uniqueOwners: status.uniqueOwners || 0,
          myFiles,
        });
      } catch {
        results.push({
          url: server.url,
          name: server.name,
          region: server.region,
          status: 'offline',
          peerId: '',
          walletAddress: '',
          activeFiles: 0,
          totalStorageBytes: 0,
          uniqueOwners: 0,
          myFiles: [],
        });
      }
    }

    cdnServers = results;
    loadingCdn = false;
  }

  async function deleteCdnFile(serverUrl: string, fileHash: string) {
    const myWallet = $walletAccount?.address || '';
    if (!myWallet) { toasts.show('No wallet connected', 'error'); return; }

    try {
      const resp = await fetch(`${serverUrl}/api/cdn/files/${fileHash}?owner=${myWallet}`, { method: 'DELETE' });
      if (resp.ok) {
        toasts.show('File removed from CDN', 'success');
        await loadCdnServers();
      } else {
        const text = await resp.text();
        toasts.show(`Failed to delete: ${text}`, 'error');
      }
    } catch (err) {
      toasts.show(`Delete failed: ${err}`, 'error');
    }
  }

  async function updateCdnPrice(serverUrl: string, fileHash: string, newPrice: string) {
    const owner = $walletAccount?.address || '';
    if (!owner) return;
    try {
      const resp = await fetch(`${serverUrl}/api/cdn/files/${fileHash}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ owner, downloadPriceChi: newPrice }),
      });
      if (resp.ok) {
        toasts.show(`Price updated to ${newPrice} CHI`, 'success');
        await loadCdnServers();
      } else {
        toasts.show(`Failed to update price`, 'error');
      }
    } catch (err) {
      toasts.show(`Price update failed: ${err}`, 'error');
    }
  }

  function copyCdnHash(hash: string) {
    navigator.clipboard.writeText(hash);
    copiedCdnHash = hash;
    setTimeout(() => copiedCdnHash = null, 2000);
  }

  // CDN upload state
  let showCdnUploadPicker = $state(false);
  let cdnUploadFiles = $state<{ id: string; name: string; size: number; merkleRoot?: string }[]>([]);
  let cdnUploadLoading = $state(false);
  let cdnUploading = $state<string | null>(null);

  // CDN upload confirmation modal
  let cdnConfirmFile = $state<{ id: string; name: string; size: number } | null>(null);
  let cdnConfirmServerUrl = $state('');
  let cdnConfirmPricing = $state<{ totalCostChi: string; pricePerMbMonthChi: string } | null>(null);
  let cdnFilePrice = $state('0'); // price in CHI that downloaders will pay

  async function loadCdnDriveFiles() {
    if (!isTauri) return;
    cdnUploadLoading = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const owner = $walletAccount?.address || '';
      if (!owner) { cdnUploadLoading = false; return; }
      const items = await invoke<{ id: string; name: string; size: number; itemType: string; merkleRoot?: string }[]>(
        'drive_list_items', { owner, parentId: null }
      );
      cdnUploadFiles = items
        .filter((f) => f.itemType === 'file')
        .map((f) => ({ id: f.id, name: f.name, size: f.size || 0, merkleRoot: f.merkleRoot }));
    } catch (err) {
      toasts.show(`Failed to load Drive files: ${err}`, 'error');
    } finally {
      cdnUploadLoading = false;
    }
  }

  async function confirmCdnUpload(serverUrl: string, file: { id: string; name: string; size: number }) {
    // Fetch pricing before showing confirmation
    const sizeMb = Math.max((file.size || 1) / (1024 * 1024), 0.001);
    try {
      const resp = await fetch(`${serverUrl}/api/cdn/pricing?sizeMb=${sizeMb}&durationDays=30`);
      cdnConfirmPricing = await resp.json();
    } catch {
      cdnConfirmPricing = { totalCostChi: '?', pricePerMbMonthChi: '?' };
    }
    cdnConfirmFile = file;
    cdnConfirmServerUrl = serverUrl;
    cdnFilePrice = '0';
  }

  async function uploadToCdn(serverUrl: string, file: { id: string; name: string; size: number; merkleRoot?: string }) {
    if (!isTauri) return;
    const owner = $walletAccount?.address || '';
    const privateKey = $walletAccount?.privateKey || '';
    if (!owner || !privateKey) { toasts.show('No wallet connected', 'error'); return; }

    cdnUploading = file.id;
    try {
      const { invoke } = await import('@tauri-apps/api/core');

      // Step 1: Get pricing from CDN
      const sizeMb = (file.size || 1) / (1024 * 1024);
      const pricingResp = await fetch(`${serverUrl}/api/cdn/pricing?sizeMb=${sizeMb}&durationDays=30`);
      const pricing = await pricingResp.json();
      const totalCostChi = pricing.totalCostChi || '0';
      const totalCostWei = pricing.totalCostWei || '0';

      // Step 2: Get CDN wallet address
      const statusResp = await fetch(`${serverUrl}/api/cdn/status`);
      const cdnStatus = await statusResp.json();
      const cdnWallet = cdnStatus.walletAddress;

      if (!cdnWallet) { toasts.show('CDN wallet not available', 'error'); cdnUploading = null; return; }

      // Step 3: Send payment to CDN wallet
      toasts.show(`Sending ${totalCostChi} CHI to CDN...`, 'info', 5000);
      let paymentTx = '';
      if (parseFloat(totalCostChi) > 0) {
        const payResult = await invoke<{ hash: string }>('send_transaction', {
          fromAddress: owner,
          toAddress: cdnWallet,
          amount: totalCostChi,
          privateKey,
        });
        paymentTx = payResult.hash;
        toasts.show(`Payment sent: ${paymentTx.slice(0, 12)}... Waiting for confirmation...`, 'info', 8000);
      }

      // Step 4: Read file and upload with payment proof
      const fileData = await invoke<number[]>('drive_read_file_bytes', { owner, itemId: file.id });
      const base64 = btoa(String.fromCharCode(...new Uint8Array(fileData)));

      const resp = await fetch(`${serverUrl}/api/cdn/upload`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          fileName: file.name,
          fileData: base64,
          ownerWallet: owner,
          paymentTx,
          durationDays: 30,
          downloadPriceChi: cdnFilePrice || '0',
        }),
      });

      if (resp.ok) {
        const result = await resp.json();
        toasts.show(`Uploaded ${file.name} to CDN (hash: ${result.fileHash?.slice(0, 12)}...)`, 'success');
        showCdnUploadPicker = false;
        await loadCdnServers();
      } else {
        const text = await resp.text();
        toasts.show(`CDN upload failed: ${text}`, 'error');
      }
    } catch (err) {
      toasts.show(`Upload failed: ${err}`, 'error');
    } finally {
      cdnUploading = null;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function formatCdnDate(ts: number): string {
    return new Date(ts * 1000).toLocaleDateString();
  }

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
      toasts.show('Host list refreshed', 'success');
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
    loadCdnServers();

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
      onclick={() => { activeTab = 'cdn'; loadCdnServers(); }}
      role="tab"
      aria-selected={activeTab === 'cdn'}
      class="flex items-center gap-2 px-4 py-2.5 text-sm font-medium rounded-lg transition-all flex-1 justify-center
        {activeTab === 'cdn'
          ? 'bg-white dark:bg-gray-700 text-gray-900 dark:text-white shadow-sm ring-1 ring-gray-200/50 dark:ring-gray-600/50'
          : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-white/50 dark:hover:bg-gray-700/30'}"
    >
      <Cloud class="w-4 h-4" />
      <span class="hidden sm:inline">CDN Servers</span>
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
      <span class="hidden sm:inline">Peer Hosts</span>
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
  {:else if activeTab === 'cdn'}
    <!-- CDN Servers -->
    <div class="space-y-4">
      <div class="flex items-center justify-between">
        <div>
          <h2 class="text-lg font-semibold dark:text-white">CDN Servers</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">Always-on servers that host your files so they stay available when you go offline</p>
        </div>
        <button
          onclick={() => loadCdnServers()}
          disabled={loadingCdn}
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors disabled:opacity-50 dark:text-gray-300"
        >
          {#if loadingCdn}
            <Loader2 class="w-3.5 h-3.5 animate-spin" />
          {:else}
            <RefreshCw class="w-3.5 h-3.5" />
          {/if}
          Refresh
        </button>
      </div>

      {#each cdnServers as cdn}
        <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 overflow-hidden">
          <!-- Server header -->
          <div class="p-5 border-b border-gray-100 dark:border-gray-700">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div class="p-2.5 rounded-xl {cdn.status === 'online' ? 'bg-emerald-100 dark:bg-emerald-900/30' : 'bg-red-100 dark:bg-red-900/30'}">
                  <Globe class="w-5 h-5 {cdn.status === 'online' ? 'text-emerald-600 dark:text-emerald-400' : 'text-red-600 dark:text-red-400'}" />
                </div>
                <div>
                  <h3 class="font-semibold dark:text-white">{cdn.name}</h3>
                  <p class="text-xs text-gray-500 dark:text-gray-400">{cdn.region}</p>
                </div>
              </div>
              <span class="px-2.5 py-1 text-xs font-medium rounded-full {cdn.status === 'online' ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400' : 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'}">
                {cdn.status === 'online' ? 'Online' : 'Offline'}
              </span>
            </div>

            {#if cdn.status === 'online'}
              <div class="mt-3 grid grid-cols-3 gap-3">
                <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5 text-center">
                  <p class="text-lg font-bold dark:text-white">{cdn.activeFiles}</p>
                  <p class="text-[10px] text-gray-500 dark:text-gray-400 uppercase tracking-wide">Files Hosted</p>
                </div>
                <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5 text-center">
                  <p class="text-lg font-bold dark:text-white">{formatBytes(cdn.totalStorageBytes)}</p>
                  <p class="text-[10px] text-gray-500 dark:text-gray-400 uppercase tracking-wide">Storage Used</p>
                </div>
                <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5 text-center">
                  <p class="text-lg font-bold dark:text-white">{cdn.uniqueOwners}</p>
                  <p class="text-[10px] text-gray-500 dark:text-gray-400 uppercase tracking-wide">Users</p>
                </div>
              </div>
            {/if}
          </div>

          <!-- My files on this CDN -->
          {#if cdn.myFiles.length > 0}
            <div class="p-5">
              <div class="flex items-center justify-between mb-3">
                <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300">Your Files on This CDN</h4>
                <button
                  onclick={() => { showCdnUploadPicker = true; loadCdnDriveFiles(); }}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
                >
                  <Upload class="w-3 h-3" />
                  Upload
                </button>
              </div>
              <div class="space-y-2">
                {#each cdn.myFiles as file}
                  <div class="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-700/50 rounded-xl">
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium dark:text-white truncate">{file.fileName}</p>
                      <div class="flex items-center gap-3 mt-0.5 flex-wrap">
                        <span class="text-xs text-gray-500 dark:text-gray-400">{formatBytes(file.fileSize)}</span>
                        <span class="text-xs text-gray-400 dark:text-gray-500">Expires {formatCdnDate(file.expiresAt)}</span>
                        <span class="text-xs font-medium {file.downloadPriceChi && file.downloadPriceChi !== '0' ? 'text-emerald-600 dark:text-emerald-400' : 'text-gray-400'}">
                          {file.downloadPriceChi && file.downloadPriceChi !== '0' ? `${file.downloadPriceChi} CHI` : 'Free'}
                        </span>
                        <button
                          onclick={() => {
                            const price = prompt('Set download price (CHI):', file.downloadPriceChi || '0');
                            if (price !== null) updateCdnPrice(cdn.url, file.fileHash, price);
                          }}
                          class="text-[10px] text-blue-500 hover:text-blue-700 dark:text-blue-400"
                        >edit</button>
                      </div>
                    </div>
                    <div class="flex items-center gap-1.5 ml-3">
                      <button
                        onclick={() => copyCdnHash(file.fileHash)}
                        class="p-1.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
                        title="Copy file hash"
                      >
                        {#if copiedCdnHash === file.fileHash}
                          <Check class="w-3.5 h-3.5 text-emerald-500" />
                        {:else}
                          <Copy class="w-3.5 h-3.5" />
                        {/if}
                      </button>
                      <button
                        onclick={() => deleteCdnFile(cdn.url, file.fileHash)}
                        class="p-1.5 text-gray-400 hover:text-red-500 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
                        title="Remove from CDN"
                      >
                        <Trash2 class="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          {:else if cdn.status === 'online'}
            <div class="p-5 text-center">
              <Cloud class="w-8 h-8 text-gray-300 dark:text-gray-600 mx-auto mb-2" />
              <p class="text-sm text-gray-500 dark:text-gray-400">No files hosted on this CDN yet</p>
              <button
                onclick={() => { showCdnUploadPicker = true; loadCdnDriveFiles(); }}
                class="mt-3 px-4 py-2 text-sm bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors inline-flex items-center gap-2"
              >
                <Upload class="w-4 h-4" />
                Upload from Drive
              </button>
            </div>
          {/if}
        </div>
      {/each}

      {#if cdnServers.length === 0 && !loadingCdn}
        <div class="text-center py-16">
          <Cloud class="w-12 h-12 text-gray-300 dark:text-gray-600 mx-auto mb-3" />
          <p class="text-gray-500 dark:text-gray-400">No CDN servers configured</p>
        </div>
      {/if}
    </div>

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

<!-- CDN Upload File Picker -->
{#if showCdnUploadPicker}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 bg-black/50 flex items-center justify-center z-[9998]"
  onclick={() => showCdnUploadPicker = false}
  onkeydown={(e) => { if (e.key === 'Escape') showCdnUploadPicker = false; }}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl p-6 max-w-md w-full mx-4 max-h-[80vh] flex flex-col"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold dark:text-white">Upload to CDN</h3>
      <button onclick={() => showCdnUploadPicker = false} class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300">
        &times;
      </button>
    </div>
    <p class="text-sm text-gray-500 dark:text-gray-400 mb-4">Select a file from your Drive. You'll pay the CDN storage fee and the file stays available for 30 days even when you're offline.</p>

    {#if cdnUploadLoading}
      <div class="flex items-center justify-center py-8">
        <Loader2 class="w-6 h-6 animate-spin text-gray-400" />
        <span class="ml-2 text-sm text-gray-500">Loading Drive files...</span>
      </div>
    {:else if cdnUploadFiles.length === 0}
      <div class="text-center py-8">
        <Cloud class="w-10 h-10 text-gray-300 dark:text-gray-600 mx-auto mb-2" />
        <p class="text-sm text-gray-500 dark:text-gray-400">No files in your Drive</p>
        <p class="text-xs text-gray-400 mt-1">Upload files to Drive first, then upload them to the CDN</p>
      </div>
    {:else}
      <div class="overflow-y-auto flex-1 space-y-2">
        {#each cdnUploadFiles as file}
          <div class="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-700/50 rounded-xl hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors">
            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium dark:text-white truncate">{file.name}</p>
              <p class="text-xs text-gray-500 dark:text-gray-400">{formatBytes(file.size)}</p>
            </div>
            <button
              onclick={() => confirmCdnUpload(CDN_SERVERS[0].url, file)}
              class="ml-3 px-3 py-1.5 text-xs bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors flex items-center gap-1.5"
            >
              <Upload class="w-3 h-3" />
              Select
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
{/if}

<!-- CDN Upload Confirmation Modal -->
{#if cdnConfirmFile}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 bg-black/50 flex items-center justify-center z-[9999]"
  onclick={() => cdnConfirmFile = null}
  onkeydown={(e) => { if (e.key === 'Escape') cdnConfirmFile = null; }}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl p-6 max-w-sm w-full mx-4"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <h3 class="text-lg font-semibold dark:text-white mb-4">Confirm CDN Upload</h3>

    <div class="space-y-3 mb-5">
      <div class="bg-gray-50 dark:bg-gray-700/50 rounded-xl p-3">
        <p class="text-sm font-medium dark:text-white truncate">{cdnConfirmFile.name}</p>
        <p class="text-xs text-gray-500 dark:text-gray-400">{formatBytes(cdnConfirmFile.size)}</p>
      </div>

      <div class="bg-gray-50 dark:bg-gray-700/50 rounded-xl p-3">
        <p class="text-xs text-gray-500 dark:text-gray-400 mb-1">Hosting Cost (30 days)</p>
        <p class="text-lg font-bold text-blue-600 dark:text-blue-400">
          {cdnConfirmPricing?.totalCostChi ?? '...'} CHI
        </p>
        <p class="text-[10px] text-gray-400">{cdnConfirmPricing?.pricePerMbMonthChi ?? '?'} CHI/MB/month</p>
      </div>

      <div>
        <label class="block text-xs font-medium text-gray-600 dark:text-gray-400 mb-1">
          Download Price (CHI per download)
        </label>
        <input
          type="number"
          step="0.001"
          min="0"
          bind:value={cdnFilePrice}
          placeholder="0 = free"
          class="w-full px-3 py-2 text-sm bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg dark:text-white focus:ring-2 focus:ring-blue-500 outline-none"
        />
        <p class="text-[10px] text-gray-400 mt-1">Set to 0 for free downloads, or set a price others will pay</p>
      </div>
    </div>

    <div class="flex gap-3">
      <button
        onclick={() => cdnConfirmFile = null}
        class="flex-1 px-4 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors dark:text-gray-300"
      >
        Cancel
      </button>
      <button
        onclick={async () => {
          const file = cdnConfirmFile;
          const serverUrl = cdnConfirmServerUrl;
          cdnConfirmFile = null;
          showCdnUploadPicker = false;
          if (file) await uploadToCdn(serverUrl, file);
        }}
        disabled={cdnUploading !== null}
        class="flex-1 px-4 py-2 text-sm bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors disabled:opacity-50 flex items-center justify-center gap-2"
      >
        {#if cdnUploading}
          <Loader2 class="w-4 h-4 animate-spin" />
          Uploading...
        {:else}
          Pay & Upload
        {/if}
      </button>
    </div>
  </div>
</div>
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

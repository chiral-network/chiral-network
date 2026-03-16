export interface SiteFileLike {
  size: number;
}

export function formatHostedFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

export function formatHostedTimeAgo(unixSecs: number, nowSecs: number = Math.floor(Date.now() / 1000)): string {
  const diff = nowSecs - unixSecs;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function buildHostedSiteUrl(
  siteId: string,
  relayUrl: string | null | undefined,
  serverAddress: string | null,
  fallbackPort: number,
): string {
  if (relayUrl) return relayUrl;
  const port = serverAddress?.split(':').pop() || String(fallbackPort);
  return `http://localhost:${port}/sites/${siteId}/`;
}

export function buildHostedLocalUrl(serverAddress: string | null, fallbackPort: number): string {
  if (serverAddress) return `http://${serverAddress}`;
  return `http://localhost:${fallbackPort}`;
}

export function getTotalHostedSiteSize(files: SiteFileLike[]): number {
  return files.reduce((sum, f) => sum + f.size, 0);
}

export function resolveHostingPort(savedValue: string | null, fallbackPort: number = 8080): number {
  if (!savedValue) return fallbackPort;
  const parsed = parseInt(savedValue, 10);
  if (isNaN(parsed) || parsed <= 0 || parsed > 65535) return fallbackPort;
  return parsed;
}

export function formatPeerId(id: string): string {
  if (id.length <= 16) return id;
  return `${id.slice(0, 8)}...${id.slice(-6)}`;
}

export function formatWeiAsChi(wei: string): string {
  try {
    const value = Number(BigInt(wei)) / 1e18;
    if (value === 0) return 'Free';
    if (value < 0.000001) return '< 0.000001 CHI';
    return `${parseFloat(value.toFixed(6))} CHI`;
  } catch {
    return 'Free';
  }
}

export function weiToChiNumber(wei: string, fallback: number): number {
  try {
    const n = Number(BigInt(wei)) / 1e18;
    return Number.isFinite(n) && n >= 0 ? n : fallback;
  } catch {
    return fallback;
  }
}

export function chiToWeiString(chi: number, fallbackWei: string): string {
  if (!Number.isFinite(chi) || chi < 0) return fallbackWei;
  return BigInt(Math.round(chi * 1e18)).toString();
}

export function formatDuration(secs: number): string {
  const days = Math.floor(secs / 86400);
  if (days >= 365) return `${(days / 365).toFixed(1)} years`;
  if (days >= 30) return `${(days / 30).toFixed(1)} months`;
  return `${days} day${days !== 1 ? 's' : ''}`;
}

export function timeRemaining(expiresAt: number | undefined): string {
  if (!expiresAt) return 'N/A';
  const remaining = expiresAt - Math.floor(Date.now() / 1000);
  if (remaining <= 0) return 'Expired';
  return formatDuration(remaining);
}

export function statusColor(status: string): string {
  switch (status) {
    case 'proposed': return 'bg-violet-500/10 text-violet-600 dark:text-violet-400';
    case 'accepted': return 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-emerald-400';
    case 'active': return 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400';
    case 'rejected': return 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400';
    case 'expired': return 'bg-[var(--surface-2)] text-[var(--text-secondary)]';
    case 'cancelled': return 'bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400';
    default: return 'bg-[var(--surface-2)] text-[var(--text-secondary)]';
  }
}

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

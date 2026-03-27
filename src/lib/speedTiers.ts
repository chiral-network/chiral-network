/** Fixed download cost: 0.001 CHI per MB (unlimited speed) */
const COST_PER_MB = 0.001;

/** Calculate total download cost in CHI for a given file size */
export function calculateCost(fileSizeBytes: number): number {
  if (fileSizeBytes <= 0) return 0;
  const sizeMb = fileSizeBytes / 1_000_000;
  return sizeMb * COST_PER_MB;
}

/** Format a CHI cost for display */
export function formatCost(costChi: number): string {
  if (costChi === 0) return 'Free';
  if (costChi < 0.000001) return '< 0.000001 CHI';
  return `${costChi.toFixed(6).replace(/0+$/, '').replace(/\.$/, '')} CHI`;
}

/** Format bytes per second as a human-readable speed */
export function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec === 0) return '0 B/s';
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
}

/** Fixed download cost: 0.01 CHI per MB (unlimited speed) */
const COST_PER_MB = 0.01;

/** Platform fee: 0.5% of all transactions */
export const PLATFORM_FEE_PERCENT = 0.5;

/** Calculate total download cost in CHI for a given file size (before platform fee) */
export function calculateCost(fileSizeBytes: number): number {
  if (fileSizeBytes <= 0) return 0;
  const sizeMb = fileSizeBytes / 1_000_000;
  return sizeMb * COST_PER_MB;
}

/** Calculate the platform fee (0.5%) on a CHI amount */
export function calculatePlatformFee(amountChi: number): number {
  return amountChi * (PLATFORM_FEE_PERCENT / 100);
}

/** Calculate total cost including platform fee */
export function calculateTotalWithFee(baseCostChi: number): number {
  return baseCostChi + calculatePlatformFee(baseCostChi);
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

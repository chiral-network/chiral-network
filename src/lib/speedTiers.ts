import {
  BASIS_POINTS_DENOMINATOR,
  DECIMAL_BYTES_PER_MB,
  LAUNCH_DOWNLOAD_COST_PER_MB_WEI,
  PLATFORM_FEE_BPS,
  WEI_PER_CHI,
} from './launchFeePolicy';

export {
  LAUNCH_DOWNLOAD_COST_PER_MB_CHI,
  LAUNCH_DOWNLOAD_COST_PER_MB_WEI,
  PLATFORM_FEE_BPS,
  PLATFORM_FEE_PERCENT,
  PLATFORM_WALLET,
} from './launchFeePolicy';

/** Calculate total launch download cost in wei for a given file size. */
export function calculateCostWei(fileSizeBytes: number): bigint {
  if (!Number.isFinite(fileSizeBytes) || fileSizeBytes <= 0) return 0n;
  const bytes = BigInt(Math.ceil(fileSizeBytes));
  const costPerMbWei = BigInt(LAUNCH_DOWNLOAD_COST_PER_MB_WEI);
  return (bytes * costPerMbWei + DECIMAL_BYTES_PER_MB - 1n) / DECIMAL_BYTES_PER_MB;
}

/** Calculate total download cost in CHI for a given file size (before platform fee) */
export function calculateCost(fileSizeBytes: number): number {
  return Number(calculateCostWei(fileSizeBytes)) / Number(WEI_PER_CHI);
}

/** Calculate the platform fee (0.5%) on a wei amount, rounded up. */
export function calculatePlatformFeeWei(amountWei: bigint): bigint {
  if (amountWei <= 0n) return 0n;
  return (amountWei * BigInt(PLATFORM_FEE_BPS) + BASIS_POINTS_DENOMINATOR - 1n) / BASIS_POINTS_DENOMINATOR;
}

/** Calculate the platform fee (0.5%) on a CHI amount for display-only callers. */
export function calculatePlatformFee(amountChi: number): number {
  if (!Number.isFinite(amountChi) || amountChi <= 0) return 0;
  const amountWei = BigInt(Math.ceil(amountChi * Number(WEI_PER_CHI)));
  return Number(calculatePlatformFeeWei(amountWei)) / Number(WEI_PER_CHI);
}

/** Split a listed price into seller proceeds and platform fee. Buyer pays totalWei. */
export function splitPaymentWei(totalWei: bigint): { sellerWei: bigint; platformFeeWei: bigint } {
  if (totalWei <= 0n) {
    return { sellerWei: 0n, platformFeeWei: 0n };
  }

  const platformFeeWei = calculatePlatformFeeWei(totalWei);
  return {
    sellerWei: totalWei - platformFeeWei,
    platformFeeWei,
  };
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

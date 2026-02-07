export type SpeedTier = 'free' | 'standard' | 'premium';

export interface TierConfig {
  id: SpeedTier;
  name: string;
  speedLimit: number; // bytes per second, 0 = unlimited
  speedLabel: string;
  costPerMb: number; // in CHR
  description: string;
}

export const TIERS: TierConfig[] = [
  {
    id: 'free',
    name: 'Free',
    speedLimit: 100 * 1024, // 100 KB/s
    speedLabel: '100 KB/s',
    costPerMb: 0,
    description: 'Rate-limited, always available',
  },
  {
    id: 'standard',
    name: 'Standard',
    speedLimit: 1024 * 1024, // 1 MB/s
    speedLabel: '1 MB/s',
    costPerMb: 0.001,
    description: 'Moderate speed, affordable',
  },
  {
    id: 'premium',
    name: 'Premium',
    speedLimit: 0, // unlimited
    speedLabel: 'Unlimited',
    costPerMb: 0.005,
    description: 'Full speed, premium pricing',
  },
];

export function getTierConfig(tier: SpeedTier): TierConfig {
  return TIERS.find((t) => t.id === tier)!;
}

/** Calculate total download cost in CHR for a given tier and file size */
export function calculateCost(tier: SpeedTier, fileSizeBytes: number): number {
  const config = getTierConfig(tier);
  if (config.costPerMb === 0) return 0;
  const sizeMb = fileSizeBytes / 1_000_000;
  return sizeMb * config.costPerMb;
}

/** Format a CHR cost for display */
export function formatCost(costChr: number): string {
  if (costChr === 0) return 'Free';
  if (costChr < 0.000001) return '< 0.000001 CHR';
  return `${costChr.toFixed(6).replace(/0+$/, '').replace(/\.$/, '')} CHR`;
}

/** Format bytes per second as a human-readable speed */
export function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec === 0) return '0 B/s';
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
}

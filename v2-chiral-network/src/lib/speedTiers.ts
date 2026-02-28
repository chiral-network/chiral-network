export type SpeedTier = 'standard' | 'premium' | 'ultra';

export interface TierConfig {
  id: SpeedTier;
  name: string;
  speedLimit: number; // bytes per second, 0 = unlimited
  speedLabel: string;
  costPerMb: number; // in CHI
  description: string;
}

export const TIERS: TierConfig[] = [
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
    speedLimit: 5 * 1024 * 1024, // 5 MB/s
    speedLabel: '5 MB/s',
    costPerMb: 0.005,
    description: 'Fast speed, balanced pricing',
  },
  {
    id: 'ultra',
    name: 'Ultra',
    speedLimit: 0, // unlimited
    speedLabel: 'Unlimited',
    costPerMb: 0.01,
    description: 'Maximum speed, premium pricing',
  },
];

export function getTierConfig(tier: SpeedTier): TierConfig {
  return TIERS.find((t) => t.id === tier)!;
}

/** Calculate total download cost in CHI for a given tier and file size */
export function calculateCost(tier: SpeedTier, fileSizeBytes: number): number {
  const config = getTierConfig(tier);
  if (config.costPerMb === 0) return 0;
  const sizeMb = fileSizeBytes / 1_000_000;
  return sizeMb * config.costPerMb;
}

/** Format a CHI cost for display */
export function formatCost(costChi: number): string {
  if (costChi === 0) return '0 CHI';
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

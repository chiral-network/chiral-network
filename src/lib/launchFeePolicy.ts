import launchFeePolicy from './launchFeePolicy.json';

export const WEI_PER_CHI = 1_000_000_000_000_000_000n;
export const DECIMAL_BYTES_PER_MB = 1_000_000n;
export const BASIS_POINTS_DENOMINATOR = 10_000n;

/** Fixed launch download cost: 0.01 CHI per MB. */
export const LAUNCH_DOWNLOAD_COST_PER_MB_CHI = Number(launchFeePolicy.downloadCostPerMbChi);
export const LAUNCH_DOWNLOAD_COST_PER_MB_WEI = launchFeePolicy.downloadCostPerMbWei;

/** Platform fee: 0.5% of all transactions. */
export const PLATFORM_FEE_BPS = launchFeePolicy.platformFeeBps;
export const PLATFORM_FEE_PERCENT = PLATFORM_FEE_BPS / 100;
export const PLATFORM_WALLET = launchFeePolicy.platformWallet;

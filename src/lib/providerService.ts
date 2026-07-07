import { invoke } from '@tauri-apps/api/core';
import type { ResourceClass } from './exchangeService';

/**
 * Desktop provider mode — run a light provider inside the app (full/advanced
 * mode). The app serves the contract handshake + data plane, publishes its offer
 * through the client-mode DHT, and verifies payments over remote RPC. A publicly
 * reachable `endpoint` is required (no NAT traversal in v1).
 */

export interface ProviderStatus {
  running: boolean;
  offerRef?: string;
  class?: ResourceClass;
  endpoint?: string;
  controlBind?: string;
  dataBind?: string | null;
  providerWallet?: string;
}

export interface StartProviderRequest {
  class: ResourceClass;
  /** Public HTTPS base URL advertised in the offer. */
  endpoint: string;
  /** Class-specific wei-per-unit price schedule (values are decimal wei strings). */
  priceSchedule: Record<string, string>;
  capacity?: unknown;
  minFundingWei: string;
  /** Control-plane bind, e.g. `0.0.0.0:8443`. */
  controlBind: string;
  /** Storage S3 data-plane bind (storage only), e.g. `0.0.0.0:8444`. */
  dataBind?: string;
  /** Provider wallet private key (from the unlocked account). Never leaves the process. */
  privateKey: string;
  region?: string;
  offerNonce?: number;
}

/** Start (or replace) the desktop provider. Returns the running status. */
export async function startProvider(req: StartProviderRequest): Promise<ProviderStatus> {
  // Nested struct fields are deserialized by serde, so they must be snake_case.
  return invoke<ProviderStatus>('start_provider', {
    req: {
      class: req.class,
      endpoint: req.endpoint,
      price_schedule: req.priceSchedule,
      capacity: req.capacity ?? {},
      min_funding_wei: req.minFundingWei,
      control_bind: req.controlBind,
      data_bind: req.dataBind ?? '',
      private_key: req.privateKey,
      region: req.region ?? '',
      offer_nonce: req.offerNonce ?? 1,
    },
  });
}

export async function stopProvider(): Promise<void> {
  return invoke('stop_provider');
}

export async function getProviderStatus(): Promise<ProviderStatus> {
  return invoke<ProviderStatus>('get_provider_status');
}

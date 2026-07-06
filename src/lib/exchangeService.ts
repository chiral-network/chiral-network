import { invoke } from '@tauri-apps/api/core';

/**
 * Resource-exchange marketplace service — typed wrappers over the backend
 * `discover_offers` / `open_service_contract` Tauri commands, plus direct
 * data-plane helpers that present a contract's session credential to a provider.
 *
 * Flow: `discoverOffers(class)` → pick an offer → `openContract(offer, …)` to get
 * a `SessionCredential` → use `storagePut` / `containerRun` / … against the
 * provider's data plane. The deposit funded in `openContract` is non-refundable
 * once the on-chain tx is broadcast; the private key never leaves the backend
 * process. This version offers storage + container (LLM inference is deferred).
 */

// LLM (inference) sharing is deferred to a future version — this version offers
// storage and container only. (The backend keeps the inference class dormant;
// the frontend simply doesn't surface it.)
export type ResourceClass = 'storage' | 'container';

/**
 * A signature-verified provider offer. Fields mirror the Rust `ResourceOffer`
 * serde output (snake_case). Every offer returned by {@link discoverOffers} has
 * already been signature-verified in the backend, so `provider_wallet`,
 * `endpoint`, and `price_schedule` are trustworthy. Treat the object as opaque
 * and hand it back to {@link openContract} unchanged so its signature still
 * verifies on the way in.
 */
export interface ResourceOffer {
  provider_wallet: string;
  resource_class: ResourceClass;
  capacity: unknown;
  price_schedule: Record<string, string>;
  endpoint: string;
  region: string;
  min_funding_wei: string;
  offer_nonce: number;
  valid_until: number;
  signature: string;
}

/** The credential minted by the provider on a successful open (snake_case, as
 *  the provider/HTTP wire emits it). */
export interface SessionCredential {
  contract_id: string;
  bearer: string;
  s3_access_key_id: string;
  s3_secret_access_key: string;
  bucket: string;
  expires_at: number;
}

/** The result of {@link openContract}. */
export interface OpenedContract {
  contractId: string;
  credential: SessionCredential;
  balanceWei: string;
  expiresAt: number;
  txHash: string;
}

/**
 * Discover signature-verified offers for a resource class off the running DHT
 * node. Requires the DHT to be started; rejects otherwise.
 */
export async function discoverOffers(resourceClass: ResourceClass): Promise<ResourceOffer[]> {
  // The Rust arg is `class`; Tauri maps this object key onto it directly.
  return invoke<ResourceOffer[]>('discover_offers', { class: resourceClass });
}

/**
 * Open a prepaid contract against a discovered offer: propose → fund the deposit
 * on-chain → open. Returns the contract id + session credential. The deposit
 * (`fundingChi`, in CHI) is **non-refundable** once broadcast; `privateKey` is
 * sent to the local backend only and never leaves the process.
 */
export async function openContract(
  offer: ResourceOffer,
  fundingChi: string,
  walletAddress: string,
  privateKey: string,
): Promise<OpenedContract> {
  return invoke<OpenedContract>('open_service_contract', {
    offer,
    fundingChi,
    walletAddress,
    privateKey,
  });
}

// ---- Data-plane helpers (direct provider HTTP with the bearer credential) ----
//
// These call the provider's data plane directly (not via a Tauri command), the
// same requests the `chiral exchange use` CLI issues. `endpoint` is the
// provider's data-plane base URL — for storage this is the S3 listener, which a
// well-deployed provider fronts on the same origin as its offer endpoint.

function objectUrl(endpoint: string, bucket: string, key: string): string {
  return `${endpoint.replace(/\/+$/, '')}/${bucket}/${key.replace(/^\/+/, '')}`;
}

async function assertOk(resp: Response, what: string): Promise<Response> {
  if (resp.ok) return resp;
  const body = await resp.text().catch(() => '');
  throw new Error(`${what} -> ${resp.status} ${resp.statusText}: ${body}`);
}

/** PUT an object into the contract's bucket (S3 storage). Returns the ETag. */
export async function storagePut(
  endpoint: string,
  credential: SessionCredential,
  key: string,
  body: Blob | ArrayBuffer | Uint8Array,
): Promise<string> {
  const url = objectUrl(endpoint, credential.bucket, key);
  const resp = await assertOk(
    await fetch(url, {
      method: 'PUT',
      headers: { Authorization: `Bearer ${credential.bearer}` },
      body: body as BodyInit,
    }),
    'storage PUT',
  );
  return resp.headers.get('etag') ?? '';
}

/** GET an object from the contract's bucket as bytes. */
export async function storageGet(
  endpoint: string,
  credential: SessionCredential,
  key: string,
): Promise<ArrayBuffer> {
  const url = objectUrl(endpoint, credential.bucket, key);
  const resp = await assertOk(
    await fetch(url, { headers: { Authorization: `Bearer ${credential.bearer}` } }),
    'storage GET',
  );
  return resp.arrayBuffer();
}

/** DELETE an object from the contract's bucket. */
export async function storageDelete(
  endpoint: string,
  credential: SessionCredential,
  key: string,
): Promise<void> {
  const url = objectUrl(endpoint, credential.bucket, key);
  await assertOk(
    await fetch(url, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${credential.bearer}` },
    }),
    'storage DELETE',
  );
}

/** Resources requested for a container (compute). */
export interface ContainerResources {
  vcpu: number;
  mem_gb: number;
  gpu: number;
}

/** Start a container (compute). Returns the provider's response JSON. */
export async function containerRun(
  endpoint: string,
  credential: SessionCredential,
  image: string,
  resources: ContainerResources,
): Promise<unknown> {
  const resp = await assertOk(
    await fetch(`${endpoint.replace(/\/+$/, '')}/v1/containers`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${credential.bearer}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ image, resources }),
    }),
    'container run',
  );
  return resp.json();
}

/** Stop a container by id (compute). */
export async function containerStop(
  endpoint: string,
  credential: SessionCredential,
  id: string,
): Promise<void> {
  await assertOk(
    await fetch(`${endpoint.replace(/\/+$/, '')}/v1/containers/${id}`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${credential.bearer}` },
    }),
    'container stop',
  );
}

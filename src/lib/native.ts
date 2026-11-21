import { Channel, invoke, isTauri } from '@tauri-apps/api/core';

export const nativeAvailable = () => isTauri();
/**
 * An unsigned 64-bit wire integer in an account, receipt, request or usage result. The desktop
 * delivers one beyond `Number.MAX_SAFE_INTEGER` as its exact decimal string, since parsing it as a
 * JSON number would round it; safe values stay numbers. Render with `formatCount`, never as a
 * Number.
 */
export type ExactInteger = number | string;
/** Accounting token counters (0 to 2^64 - 1). */
export interface TokenUsage { prompt_tokens: ExactInteger; completion_tokens: ExactInteger }
export interface NetworkIdentity { chain_id: ExactInteger; genesis_hash: string; deployment_id: string }
export interface EngineDevice { id: string; name: string; memory_mib: ExactInteger; driver_version: string; engine_device: string }
export interface ServingProfile {
  hf_repo: string; tokenizer_repo: string; engine: string; engine_version: string;
  runtime_sha256: string; artifact_manifest_sha256: string; platform: string; backend: string;
  memory_kind: string; devices: EngineDevice[]; memory_reserve_mib: ExactInteger;
  gpu_layers: number; split_mode: string; tensor_split: number[];
  cache_type_k: string; cache_type_v: string; flash_attention: boolean; batch_size: number; ubatch_size: number;
  capacity_tier: string; region: string; max_concurrency: number; arrival_milli_rps: number;
  output_tokens_per_second: number; policy: string;
  qualification: { evidence_sha256: string; measured_at: ExactInteger };
}
export interface ModelSpec { id: string; revision: string; tokenizer_revision: string; chat_template_sha256: string; quantization: string | null; context_length: ExactInteger; max_output_tokens: ExactInteger; capabilities: string[]; serving: ServingProfile | null }
export interface Price { input_wei_per_1k: string; output_wei_per_1k: string }
export interface Offer { network: NetworkIdentity; provider_wallet: string; peer_id: string; deployment_id: string; endpoint: string; model: ModelSpec; price: Price; min_funding_wei: string; fee_bps: number; expires_at: number; revision: number }
export interface Signed<T> { payload: T; signature: string }
export interface Quote { id: string; provider_wallet: string; consumer_wallet: string; model: ModelSpec; price: Price; funding_wei: string; fee_bps: number; expires_at: number }
export interface Account { contract_id: string; provider_wallet: string; consumer_wallet: string; network: NetworkIdentity; model: ModelSpec; price: Price; status: string; funded_wei: string; fee_wei: string; spent_wei: string; waived_wei: string; reserved_wei: string; available_wei: string; reversed_wei: string; provider_loss_wei: string; usage: TokenUsage }
export interface StreamEvent { kind: 'started' | 'chunk' | 'done' | 'cancelled' | 'error'; data: unknown }
export async function command<T = unknown>(operation: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!nativeAvailable()) throw new Error('Native desktop unavailable. Open the installed Chiral application to use wallet, discovery, funding, or inference. No preview data is simulated.');
  return invoke<T>('consumer_command', { operation, args });
}
export async function stream(contractId: string, request: Record<string, unknown>, onEvent: (event: StreamEvent) => void, kind: 'chat' | 'completions' = 'chat'): Promise<string> {
  if (!nativeAvailable()) throw new Error('Inference requires the native desktop application.');
  const onEventChannel = new Channel<StreamEvent>();
  onEventChannel.onmessage = onEvent;
  return invoke<string>('inference_stream', { contractId, request, onEvent: onEventChannel, kind });
}
export async function cancelStream(streamId: string) {
  if (!nativeAvailable()) throw new Error('Cancelling a stream requires the native desktop application.');
  return invoke<void>('inference_cancel', { streamId });
}
export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : typeof error === 'string' ? error : 'Operation failed. Inspect native diagnostics for details.';
}
const MAX_WEI = (1n << 128n) - 1n;
/** Canonical decimal wei (no sign, no leading zero, at most 2^128 - 1), as the wire requires. */
export function isCanonicalWei(wei: string): boolean {
  return /^(0|[1-9]\d*)$/.test(wei) && BigInt(wei) <= MAX_WEI;
}
export function formatCfx(wei: string): string {
  if (!isCanonicalWei(wei)) return 'unavailable';
  const value = BigInt(wei), unit = 10n ** 18n;
  const fractional = (value % unit).toString().padStart(18, '0').replace(/0+$/, '');
  return `${value / unit}${fractional ? '.' + fractional : ''}`;
}
const MAX_COUNT = (1n << 64n) - 1n;
/**
 * An unsigned 64-bit count (token counters, token limits) as exact grouped digits: a safe-integer
 * Number or the canonical decimal string the desktop delivers beyond 2^53 - 1. Anything else,
 * including a Number that JSON parsing may already have rounded, is unavailable, never a guess.
 */
export function formatCount(value: unknown): string {
  let count: bigint;
  if (typeof value === 'number' && Number.isSafeInteger(value) && value >= 0) count = BigInt(value);
  else if (typeof value === 'string' && /^(0|[1-9]\d*)$/.test(value)) count = BigInt(value);
  else return 'unavailable';
  return count <= MAX_COUNT ? count.toLocaleString() : 'unavailable';
}
export function parseCfx(amount: string): string {
  if (!/^(0|[1-9]\d*)(\.\d{1,18})?$/.test(amount)) throw new Error('Enter a non-negative CFX amount with at most 18 decimal places.');
  const [whole, fraction = ''] = amount.split('.');
  const value = BigInt(whole) * 10n ** 18n + BigInt(fraction.padEnd(18, '0'));
  if (value > MAX_WEI) throw new Error('Amount exceeds the supported range.');
  return value.toString();
}
// Render only text deltas. Tool calls and generated markup are never executed.
export function textDelta(data: Record<string, unknown>): string {
  if (!Array.isArray(data.choices)) return '';
  return data.choices.map(choice => {
    if (!choice || typeof choice !== 'object') return '';
    const item = choice as { delta?: { content?: unknown }; text?: unknown };
    return typeof item.delta?.content === 'string' ? item.delta.content : typeof item.text === 'string' ? item.text : '';
  }).join('');
}

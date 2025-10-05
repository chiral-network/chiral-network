import { walletService } from '$lib/wallet';

const encoder = new TextEncoder();

export interface SignedFetchOptions extends Omit<RequestInit, 'body'> {
  body?:
    | BodyInit
    | ArrayBuffer
    | ArrayBufferView
    | Uint8Array
    | Record<string, unknown>
    | null;
  /**
   * When false, the request is forwarded without adding Ethereum signature headers.
   * Defaults to true.
   */
  sign?: boolean;
  /** Optional UNIX timestamp (seconds) to use instead of Date.now()/1000. */
  timestampOverride?: number;
}

interface NormalizedBodyResult {
  bodyForFetch: BodyInit | null | undefined;
  canonicalBytes: Uint8Array;
  contentType?: string;
}

function isArrayBufferView(value: unknown): value is ArrayBufferView {
  return ArrayBuffer.isView(value as any);
}

async function normalizeBody(
  body: SignedFetchOptions['body'],
  method: string
): Promise<NormalizedBodyResult> {
  if (method === 'GET' || method === 'HEAD') {
    return { bodyForFetch: undefined, canonicalBytes: new Uint8Array() };
  }

  if (body == null) {
    return { bodyForFetch: undefined, canonicalBytes: new Uint8Array() };
  }

  if (typeof body === 'string') {
    return { bodyForFetch: body, canonicalBytes: encoder.encode(body) };
  }

  if (body instanceof URLSearchParams) {
    const serialized = body.toString();
    return { bodyForFetch: body, canonicalBytes: encoder.encode(serialized), contentType: 'application/x-www-form-urlencoded' };
  }

  if (body instanceof Blob) {
    const buffer = await body.arrayBuffer();
    return { bodyForFetch: body, canonicalBytes: new Uint8Array(buffer) };
  }

  if (body instanceof Uint8Array) {
    return { bodyForFetch: body as unknown as BodyInit, canonicalBytes: body };
  }

  if (body instanceof ArrayBuffer) {
    const view = new Uint8Array(body);
    return { bodyForFetch: body as unknown as BodyInit, canonicalBytes: view };
  }

  if (isArrayBufferView(body)) {
    const view = new Uint8Array(body.buffer.slice(body.byteOffset, body.byteOffset + body.byteLength));
    return { bodyForFetch: body as unknown as BodyInit, canonicalBytes: view };
  }

  if (body instanceof FormData) {
    throw new Error('FormData bodies are not yet supported for signed requests');
  }

  if (typeof body === 'object') {
    const serialized = JSON.stringify(body);
    return {
      bodyForFetch: serialized,
      canonicalBytes: encoder.encode(serialized),
      contentType: 'application/json',
    };
  }

  throw new Error('Unsupported request body type for signed fetch');
}

function extractPath(input: string): string {
  try {
    const testUrl = new URL(input);
    return testUrl.pathname + (testUrl.search ? `?${testUrl.search}` : '');
  } catch {
    // Treat as relative path; fall back to dummy origin
    const fallback = new URL(input, 'http://localhost');
    return fallback.pathname + (fallback.search ? `?${fallback.search}` : '');
  }
}

export async function signedFetch(
  input: string,
  init: SignedFetchOptions = {}
): Promise<Response> {
  const { sign = true, timestampOverride, ...rest } = init;
  const method = (rest.method ?? 'GET').toUpperCase();
  const shouldSign = sign !== false;

  const headers = new Headers(rest.headers ?? {});

  const { bodyForFetch, canonicalBytes, contentType } = await normalizeBody(rest.body, method);

  if (contentType && !headers.has('Content-Type')) {
    headers.set('Content-Type', contentType);
  }

  if (!shouldSign) {
    return fetch(input, { ...rest, headers, body: bodyForFetch });
  }

  if (!walletService.isDesktopEnvironment()) {
    throw new Error('Ethereum header authentication requires running the desktop app');
  }

  const timestamp = Math.floor(Date.now() / 1000);
  const signature = await walletService.signApiRequest(
    method,
    extractPath(input),
    canonicalBytes,
    { timestamp: timestampOverride ?? timestamp }
  );

  headers.set('X-Wallet-Address', signature.address);
  headers.set('X-Signature', signature.signature);
  headers.set('X-Timestamp', String(signature.timestamp));

  return fetch(input, {
    ...rest,
    headers,
    body: bodyForFetch,
  });
}

export async function signedJsonFetch<T = unknown>(
  input: string,
  init: SignedFetchOptions = {}
): Promise<T> {
  const response = await signedFetch(input, {
    ...init,
    headers: init.headers,
  });

  if (!response.ok) {
    const text = await response.text().catch(() => null);
    throw new Error(`Request failed with status ${response.status}${text ? `: ${text}` : ''}`);
  }

  return (await response.json()) as T;
}

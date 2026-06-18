export interface WalletBackupEmailRequest {
  email: string;
  recoveryPhrase: string;
  walletAddress: string;
  privateKey: string;
}

export interface EncryptedWalletBackupPayload {
  version: 'chiral-wallet-backup-v1';
  algorithm: 'AES-256-GCM';
  kdf: 'PBKDF2-SHA256';
  iterations: number;
  salt: string;
  iv: string;
  ciphertext: string;
}

export interface WalletBackupEmailResult {
  backupKey: string;
}

const BACKUP_VERSION = 'chiral-wallet-backup-v1';
const BACKUP_KDF_ITERATIONS = 210_000;

function normalizeErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }
  if (typeof error === 'string' && error.trim()) {
    return error.trim();
  }
  return 'Failed to send wallet backup email';
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

function bytesToArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(buffer).set(bytes);
  return buffer;
}

function randomBase64Url(byteLength: number): string {
  const bytes = globalThis.crypto.getRandomValues(new Uint8Array(byteLength));
  return bytesToBase64(bytes).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}

async function deriveBackupKey(secret: string, salt: ArrayBuffer): Promise<CryptoKey> {
  const encoder = new TextEncoder();
  const keyMaterial = await globalThis.crypto.subtle.importKey(
    'raw',
    bytesToArrayBuffer(encoder.encode(secret)),
    'PBKDF2',
    false,
    ['deriveKey']
  );

  return globalThis.crypto.subtle.deriveKey(
    {
      name: 'PBKDF2',
      salt,
      iterations: BACKUP_KDF_ITERATIONS,
      hash: 'SHA-256',
    },
    keyMaterial,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt']
  );
}

export async function createEncryptedWalletBackup(
  payload: WalletBackupEmailRequest
): Promise<{ encryptedBackup: EncryptedWalletBackupPayload; backupKey: string }> {
  if (!globalThis.crypto?.subtle) {
    throw new Error('Secure browser crypto is required for wallet backup encryption');
  }

  const backupKey = randomBase64Url(32);
  const salt = globalThis.crypto.getRandomValues(new Uint8Array(16));
  const iv = globalThis.crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveBackupKey(backupKey, bytesToArrayBuffer(salt));
  const encoder = new TextEncoder();
  const plaintext = encoder.encode(JSON.stringify({
    version: BACKUP_VERSION,
    recoveryPhrase: payload.recoveryPhrase,
    walletAddress: payload.walletAddress,
    privateKey: payload.privateKey,
    createdAt: new Date().toISOString(),
  }));
  const encrypted = await globalThis.crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: bytesToArrayBuffer(iv) },
    key,
    bytesToArrayBuffer(plaintext)
  );

  return {
    backupKey,
    encryptedBackup: {
      version: BACKUP_VERSION,
      algorithm: 'AES-256-GCM',
      kdf: 'PBKDF2-SHA256',
      iterations: BACKUP_KDF_ITERATIONS,
      salt: bytesToBase64(salt),
      iv: bytesToBase64(iv),
      ciphertext: bytesToBase64(new Uint8Array(encrypted)),
    },
  };
}

export const walletBackupService = {
  async sendBackupEmail(payload: WalletBackupEmailRequest): Promise<WalletBackupEmailResult> {
    const { encryptedBackup, backupKey } = await createEncryptedWalletBackup(payload);
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('send_wallet_backup_email', {
      email: payload.email,
      walletAddress: payload.walletAddress,
      encryptedBackup,
    });
    return { backupKey };
  },

  formatError(error: unknown): string {
    return normalizeErrorMessage(error);
  },
};

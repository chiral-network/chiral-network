/**
 * Validation Utilities for Chiral Network
 *
 * This file contains security validation functions integrated into the application.
 * For other existing validations, see:
 * - Ethereum address validation: src/pages/Account.svelte:314-321
 * - Proxy address validation: src/pages/Proxy.svelte:77-120+
 * - Password strength validation: src/pages/Account.svelte:236-273
 * - Mining parameter validation: src/pages/Mining.svelte:284-296
 * - ICE server sanitization: src/lib/services/webrtcService.ts:53-74
 * - BIP39 mnemonic validation: src/lib/wallet/bip39.ts:73-80
 */

/**
 * Validates Ethereum private key format
 *
 * Source: Ethereum Yellow Paper (https://ethereum.github.io/yellowpaper/paper.pdf)
 * Private keys are 256-bit (32 bytes) values, typically encoded as 64 hexadecimal characters.
 *
 * This validation catches user input errors (typos, wrong format) before expensive
 * cryptographic operations. The format is public knowledge documented in Ethereum specs.
 *
 * Used in: src/pages/Account.svelte:611-616 (importChiralAccount function)
 *
 * @param privateKey - The private key to validate (with or without 0x prefix)
 * @returns Object with isValid boolean and optional error message
 */
export function validatePrivateKeyFormat(privateKey: string): {
  isValid: boolean;
  error?: string;
} {
  if (!privateKey || !privateKey.trim()) {
    return { isValid: false, error: 'Private key cannot be empty' };
  }

  const trimmed = privateKey.trim();
  const normalized = trimmed.startsWith('0x') ? trimmed.slice(2) : trimmed;

  // Must be exactly 64 hex characters (32 bytes)
  if (normalized.length !== 64) {
    return {
      isValid: false,
      error: `Private key must be 64 hex characters (got ${normalized.length})`,
    };
  }

  // Must contain only hex characters
  if (!/^[0-9a-fA-F]{64}$/.test(normalized)) {
    return {
      isValid: false,
      error: 'Private key must contain only hexadecimal characters (0-9, a-f)',
    };
  }

  // Cannot be all zeros (invalid private key)
  if (/^0+$/.test(normalized)) {
    return {
      isValid: false,
      error: 'Private key cannot be all zeros',
    };
  }

  return { isValid: true };
}

/**
 * Validates a storage path to ensure it's a valid absolute path
 *
 * This prevents security issues where relative paths or tilde expansion
 * on Windows could create directories in unexpected locations.
 *
 * Rules:
 * - Path must be absolute (starts with / on Unix or drive letter on Windows)
 * - Path cannot be empty
 * - On Windows, tilde (~) is not expanded and is invalid
 * - Relative paths are not allowed
 *
 * Note: For platform-specific validation (e.g., checking if drive exists on Windows,
 * rejecting Unix paths on Windows), use the backend validate_storage_path command.
 *
 * Used in: src/pages/Settings.svelte (storage path input validation)
 *
 * @param path - The storage path to validate
 * @returns Object with isValid boolean and optional error message
 */
export function validateStoragePath(path: string): {
  isValid: boolean;
  error?: string;
} {
  if (!path || !path.trim()) {
    return { isValid: false, error: 'Storage path cannot be empty' };
  }

  const trimmed = path.trim();

  // Detect platform - in browser environment, we can't reliably detect OS
  // So we'll accept both Windows and Unix absolute paths
  // Windows absolute paths must have drive letter, separator, and at least one character
  const isWindowsAbsolute = /^[a-zA-Z]:[\\\/].+/.test(trimmed);
  const isUnixAbsolute = trimmed.startsWith('/');

  // Check if path starts with tilde (should use Tauri dialog instead)
  if (trimmed.startsWith('~')) {
    return {
      isValid: false,
      error: 'Please use the folder picker button or enter an absolute path (e.g., C:\\Users\\...) instead of using ~',
    };
  }

  // Path must be absolute
  if (!isWindowsAbsolute && !isUnixAbsolute) {
    return {
      isValid: false,
      error: 'Storage path must be an absolute path (e.g., C:\\Users\\... on Windows or /home/... on Unix)',
    };
  }

  return { isValid: true };
}

/**
 * Rate limiter for sensitive operations
 *
 * Source: OWASP Authentication Cheat Sheet
 * (https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html)
 *
 * Prevents brute force attacks on operations like keystore password attempts.
 * Uses a sliding window algorithm - old attempts expire after the time window.
 *
 * Used in: src/pages/Account.svelte:74,765-768,783,806 (loadFromKeystore function)
 *
 * Usage:
 * ```typescript
 * const limiter = new RateLimiter(5, 60000); // 5 attempts per minute
 * if (!limiter.checkLimit('operation-key')) {
 *   throw new Error('Too many attempts, please wait');
 * }
 * // On success:
 * limiter.reset('operation-key');
 * ```
 */
export class RateLimiter {
  private attempts: Map<string, number[]> = new Map();

  /**
   * @param maxAttempts - Maximum attempts allowed in the time window
   * @param windowMs - Time window in milliseconds
   */
  constructor(
    private readonly maxAttempts: number,
    private readonly windowMs: number
  ) {}

  /**
   * Check if operation is allowed under rate limit
   * @param key - Unique identifier for the operation (e.g., 'keystore-unlock')
   * @returns true if operation is allowed, false if rate limited
   */
  checkLimit(key: string): boolean {
    const now = Date.now();
    const timestamps = this.attempts.get(key) || [];

    // Remove timestamps outside the time window
    const recentTimestamps = timestamps.filter((t) => now - t < this.windowMs);

    if (recentTimestamps.length >= this.maxAttempts) {
      return false; // Rate limited
    }

    // Add current timestamp
    recentTimestamps.push(now);
    this.attempts.set(key, recentTimestamps);

    return true; // Allowed
  }

  /**
   * Reset rate limit for a specific key (call on successful operation)
   * @param key - Unique identifier for the operation
   */
  reset(key: string): void {
    this.attempts.delete(key);
  }

  /**
   * Clear all rate limit data
   */
  clearAll(): void {
    this.attempts.clear();
  }
}

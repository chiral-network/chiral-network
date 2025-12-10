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

/**
 * Validates IPv4 address format
 * @param ip - The IP address to validate
 * @returns Object with isValid boolean and optional error message
 */
export function validateIPv4(ip: string): {
  isValid: boolean;
  error?: string;
} {
  if (!ip || !ip.trim()) {
    return { isValid: false, error: 'IP address cannot be empty' };
  }

  const trimmed = ip.trim();
  const parts = trimmed.split('.');

  if (parts.length !== 4) {
    return {
      isValid: false,
      error: 'IP address must have 4 octets (e.g., 192.168.1.1)',
    };
  }

  for (let i = 0; i < parts.length; i++) {
    const num = parseInt(parts[i], 10);
    if (isNaN(num) || num < 0 || num > 255) {
      return {
        isValid: false,
        error: `Invalid octet "${parts[i]}" - must be a number between 0-255`,
      };
    }
    // Check for leading zeros (e.g., 192.168.01.1 is invalid)
    if (parts[i].length > 1 && parts[i].startsWith('0')) {
      return {
        isValid: false,
        error: `Octet "${parts[i]}" has leading zero - not allowed`,
      };
    }
  }

  return { isValid: true };
}

/**
 * Validates port number
 * @param port - The port number to validate
 * @param allowPrivileged - Whether to allow privileged ports (1-1023)
 * @returns Object with isValid boolean and optional error message
 */
export function validatePort(
  port: number | string,
  allowPrivileged = true
): {
  isValid: boolean;
  error?: string;
} {
  const portNum = typeof port === 'string' ? parseInt(port, 10) : port;

  if (isNaN(portNum)) {
    return { isValid: false, error: 'Port must be a number' };
  }

  if (!Number.isInteger(portNum)) {
    return { isValid: false, error: 'Port must be an integer' };
  }

  if (portNum < 1 || portNum > 65535) {
    return {
      isValid: false,
      error: 'Port must be between 1 and 65535',
    };
  }

  if (!allowPrivileged && portNum < 1024) {
    return {
      isValid: false,
      error: 'Privileged ports (1-1023) require administrator access',
    };
  }

  return { isValid: true };
}

/**
 * Validates a proxy address in format "host:port"
 * @param address - The proxy address to validate (e.g., "127.0.0.1:9050")
 * @returns Object with isValid boolean and optional error message
 */
export function validateProxyAddress(address: string): {
  isValid: boolean;
  error?: string;
} {
  if (!address || !address.trim()) {
    return { isValid: false, error: 'Proxy address cannot be empty' };
  }

  const trimmed = address.trim();
  const parts = trimmed.split(':');

  if (parts.length !== 2) {
    return {
      isValid: false,
      error: 'Proxy address must be in format "host:port" (e.g., 127.0.0.1:9050)',
    };
  }

  const [host, portStr] = parts;

  // Validate host (can be IP or hostname)
  if (!host) {
    return { isValid: false, error: 'Host cannot be empty' };
  }

  // Check if it's an IP address
  const ipValidation = validateIPv4(host);
  if (!ipValidation.isValid) {
    // If not a valid IP, check if it's a valid hostname
    const hostnameRegex = /^([a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?\.)*[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?$/;
    if (!hostnameRegex.test(host)) {
      return {
        isValid: false,
        error: `Invalid host "${host}" - must be a valid IP address or hostname`,
      };
    }
  }

  // Validate port
  const portValidation = validatePort(portStr);
  if (!portValidation.isValid) {
    return portValidation;
  }

  return { isValid: true };
}

/**
 * Validates bandwidth limit in KB/s
 * @param bandwidth - The bandwidth limit to validate
 * @param maxLimit - Maximum allowed bandwidth in KB/s (optional)
 * @returns Object with isValid boolean and optional error message
 */
export function validateBandwidth(
  bandwidth: number | string,
  maxLimit?: number
): {
  isValid: boolean;
  error?: string;
} {
  const bwNum = typeof bandwidth === 'string' ? parseFloat(bandwidth) : bandwidth;

  if (isNaN(bwNum)) {
    return { isValid: false, error: 'Bandwidth must be a number' };
  }

  if (bwNum < 0) {
    return {
      isValid: false,
      error: 'Bandwidth cannot be negative (use 0 for unlimited)',
    };
  }

  if (maxLimit && bwNum > maxLimit) {
    return {
      isValid: false,
      error: `Bandwidth cannot exceed ${maxLimit} KB/s`,
    };
  }

  return { isValid: true };
}

/**
 * Validates multiaddress format for libp2p
 * @param multiaddr - The multiaddress to validate (e.g., "/ip4/127.0.0.1/tcp/4001/p2p/12D3...")
 * @returns Object with isValid boolean and optional error message
 */
export function validateMultiaddr(multiaddr: string): {
  isValid: boolean;
  error?: string;
} {
  if (!multiaddr || !multiaddr.trim()) {
    return { isValid: false, error: 'Multiaddress cannot be empty' };
  }

  const trimmed = multiaddr.trim();

  if (!trimmed.startsWith('/')) {
    return {
      isValid: false,
      error: 'Multiaddress must start with / (e.g., /ip4/...)',
    };
  }

  // Basic validation: must have at least protocol/value pairs
  const parts = trimmed.split('/').filter((p) => p);
  if (parts.length < 2) {
    return {
      isValid: false,
      error: 'Multiaddress must have at least one protocol/value pair',
    };
  }

  // Check for common protocols
  const validProtocols = ['ip4', 'ip6', 'tcp', 'udp', 'p2p', 'ws', 'wss', 'quic'];
  const hasValidProtocol = parts.some((part) => validProtocols.includes(part.toLowerCase()));

  if (!hasValidProtocol) {
    return {
      isValid: false,
      error: `Multiaddress must contain a valid protocol (${validProtocols.join(', ')})`,
    };
  }

  return { isValid: true };
}

/**
 * Validates a percentage value
 * @param value - The percentage to validate
 * @param min - Minimum allowed percentage (default 0)
 * @param max - Maximum allowed percentage (default 100)
 * @returns Object with isValid boolean and optional error message
 */
export function validatePercentage(
  value: number | string,
  min = 0,
  max = 100
): {
  isValid: boolean;
  error?: string;
} {
  const num = typeof value === 'string' ? parseFloat(value) : value;

  if (isNaN(num)) {
    return { isValid: false, error: 'Percentage must be a number' };
  }

  if (num < min || num > max) {
    return {
      isValid: false,
      error: `Percentage must be between ${min} and ${max}`,
    };
  }

  return { isValid: true };
}

/**
 * Validates storage size in GB
 * @param size - The size to validate in GB
 * @param minSize - Minimum allowed size in GB (default 1)
 * @returns Object with isValid boolean and optional error message
 */
export function validateStorageSize(
  size: number | string,
  minSize = 1
): {
  isValid: boolean;
  error?: string;
} {
  const sizeNum = typeof size === 'string' ? parseFloat(size) : size;

  if (isNaN(sizeNum)) {
    return { isValid: false, error: 'Storage size must be a number' };
  }

  if (sizeNum < 0) {
    return { isValid: false, error: 'Storage size cannot be negative' };
  }

  if (sizeNum > 0 && sizeNum < minSize) {
    return {
      isValid: false,
      error: `Storage size must be at least ${minSize} GB or 0 for unlimited`,
    };
  }

  return { isValid: true };
}

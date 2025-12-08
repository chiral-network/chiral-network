/**
 * Validation Utilities Unit Tests
 *
 * Tests integrated validation functions in src/lib/utils/validation.ts
 *
 * Functions tested:
 * - validatePrivateKeyFormat: Used in Account.svelte for private key import
 * - RateLimiter: Used in Account.svelte for keystore unlock rate limiting
 */

import { describe, it, expect } from 'vitest';
import {
  validatePrivateKeyFormat,
  RateLimiter,
  validateStoragePath,
} from '../src/lib/utils/validation';

describe('validatePrivateKeyFormat', () => {
  /**
   * Test: Valid private key with 0x prefix
   * Based on: Ethereum Yellow Paper specification
   * Verifies: Accepts standard 64-char hex with 0x prefix
   */
  it('should accept valid private key with 0x prefix', () => {
    const privateKey = '0x' + 'a'.repeat(64);
    const result = validatePrivateKeyFormat(privateKey);
    expect(result.isValid).toBe(true);
    expect(result.error).toBeUndefined();
  });

  /**
   * Test: Valid private key without 0x prefix
   * Based on: Common user input patterns
   * Verifies: Accepts 64-char hex without prefix
   */
  it('should accept valid private key without 0x prefix', () => {
    const privateKey = 'b'.repeat(64);
    const result = validatePrivateKeyFormat(privateKey);
    expect(result.isValid).toBe(true);
    expect(result.error).toBeUndefined();
  });

  /**
   * Test: Reject empty private key
   * Based on: Basic input validation
   * Verifies: Empty string is invalid
   */
  it('should reject empty private key', () => {
    const result = validatePrivateKeyFormat('');
    expect(result.isValid).toBe(false);
    expect(result.error).toContain('cannot be empty');
  });

  /**
   * Test: Reject whitespace-only private key
   * Based on: Common user input errors
   * Verifies: Trims and validates
   */
  it('should reject whitespace-only private key', () => {
    const result = validatePrivateKeyFormat('   ');
    expect(result.isValid).toBe(false);
    expect(result.error).toContain('cannot be empty');
  });

  /**
   * Test: Reject private key that's too short
   * Based on: Ethereum 32-byte (64 hex char) requirement
   * Verifies: Length validation
   */
  it('should reject private key shorter than 64 characters', () => {
    const privateKey = '0x' + 'a'.repeat(63);
    const result = validatePrivateKeyFormat(privateKey);
    expect(result.isValid).toBe(false);
    expect(result.error).toContain('must be 64 hex characters');
    expect(result.error).toContain('got 63');
  });

  /**
   * Test: Reject private key that's too long
   * Based on: Ethereum 32-byte (64 hex char) requirement
   * Verifies: Length validation
   */
  it('should reject private key longer than 64 characters', () => {
    const privateKey = '0x' + 'a'.repeat(65);
    const result = validatePrivateKeyFormat(privateKey);
    expect(result.isValid).toBe(false);
    expect(result.error).toContain('must be 64 hex characters');
    expect(result.error).toContain('got 65');
  });

  /**
   * Test: Reject private key with non-hex characters
   * Based on: Hexadecimal encoding requirement
   * Verifies: Character set validation
   */
  it('should reject private key with non-hex characters', () => {
    const privateKey = '0x' + 'g'.repeat(64); // 'g' is not hex
    const result = validatePrivateKeyFormat(privateKey);
    expect(result.isValid).toBe(false);
    expect(result.error).toContain('hexadecimal characters');
  });

  /**
   * Test: Reject all-zeros private key
   * Based on: Invalid Ethereum private key (cannot derive public key)
   * Verifies: Value range validation
   */
  it('should reject all-zeros private key', () => {
    const privateKey = '0x' + '0'.repeat(64);
    const result = validatePrivateKeyFormat(privateKey);
    expect(result.isValid).toBe(false);
    expect(result.error).toContain('cannot be all zeros');
  });

  /**
   * Test: Accept mixed case hex characters
   * Based on: Hex encoding allows uppercase and lowercase
   * Verifies: Case-insensitive validation
   */
  it('should accept mixed case hex characters', () => {
    // Create exactly 64 hex characters with mixed case
    const privateKey = '0xAaBbCcDdEeFf' + '1234567890abcdef'.repeat(3) + '1234'; // 64 chars after 0x
    const result = validatePrivateKeyFormat(privateKey);
    expect(result.isValid).toBe(true);
  });
});

describe('RateLimiter', () => {
  /**
   * Test: Allow operations within rate limit
   * Based on: OWASP Authentication Cheat Sheet
   * Verifies: Normal operations are not blocked
   */
  it('should allow operations within rate limit', () => {
    const limiter = new RateLimiter(5, 60000); // 5 attempts per minute

    for (let i = 0; i < 5; i++) {
      expect(limiter.checkLimit('test-operation')).toBe(true);
    }
  });

  /**
   * Test: Block operations exceeding rate limit
   * Based on: Brute force attack prevention
   * Verifies: 6th attempt is blocked when limit is 5
   */
  it('should block operations exceeding rate limit', () => {
    const limiter = new RateLimiter(5, 60000);

    // Use up the limit
    for (let i = 0; i < 5; i++) {
      limiter.checkLimit('test-operation');
    }

    // 6th attempt should be blocked
    expect(limiter.checkLimit('test-operation')).toBe(false);
  });

  /**
   * Test: Different keys have independent limits
   * Based on: Per-operation rate limiting
   * Verifies: Keys are isolated from each other
   */
  it('should track different keys independently', () => {
    const limiter = new RateLimiter(2, 60000);

    limiter.checkLimit('operation-a');
    limiter.checkLimit('operation-a');

    // operation-a is at limit, but operation-b should work
    expect(limiter.checkLimit('operation-a')).toBe(false);
    expect(limiter.checkLimit('operation-b')).toBe(true);
  });

  /**
   * Test: Time window expiration
   * Based on: Sliding window rate limiting
   * Verifies: Old attempts are removed after time window
   */
  it('should allow operations after time window expires', () => {
    const limiter = new RateLimiter(3, 100); // 3 attempts per 100ms

    // Use up the limit
    for (let i = 0; i < 3; i++) {
      expect(limiter.checkLimit('test')).toBe(true);
    }

    // Should be blocked
    expect(limiter.checkLimit('test')).toBe(false);

    // Wait for time window to expire
    return new Promise((resolve) => {
      setTimeout(() => {
        // Should be allowed again
        expect(limiter.checkLimit('test')).toBe(true);
        resolve(undefined);
      }, 150); // Wait longer than 100ms window
    });
  });

  /**
   * Test: Reset specific key
   * Based on: Manual rate limit clearing (e.g., successful login)
   * Verifies: reset() clears attempts for one key
   */
  it('should reset specific key', () => {
    const limiter = new RateLimiter(2, 60000);

    limiter.checkLimit('test');
    limiter.checkLimit('test');

    // At limit
    expect(limiter.checkLimit('test')).toBe(false);

    // Reset
    limiter.reset('test');

    // Should work again
    expect(limiter.checkLimit('test')).toBe(true);
  });

  /**
   * Test: Clear all rate limits
   * Based on: Global reset functionality
   * Verifies: clearAll() removes all tracked attempts
   */
  it('should clear all rate limits', () => {
    const limiter = new RateLimiter(1, 60000);

    limiter.checkLimit('key1');
    limiter.checkLimit('key2');

    // Both at limit
    expect(limiter.checkLimit('key1')).toBe(false);
    expect(limiter.checkLimit('key2')).toBe(false);

    // Clear all
    limiter.clearAll();

    // Both should work again
    expect(limiter.checkLimit('key1')).toBe(true);
    expect(limiter.checkLimit('key2')).toBe(true);
  });

  /**
   * Test: Zero max attempts blocks immediately
   * Based on: Edge case handling
   * Verifies: maxAttempts = 0 means no operations allowed
   */
  it('should block all operations if maxAttempts is 0', () => {
    const limiter = new RateLimiter(0, 60000);
    expect(limiter.checkLimit('test')).toBe(false);
  });
});

describe('validateStoragePath', () => {
  /**
   * Test: Valid Windows absolute paths
   * Based on: Windows file system path format
   * Verifies: Accepts paths with drive letters (C:, D:, etc.)
   */
  describe('valid absolute paths', () => {
    it('should accept Windows absolute paths with drive letters', () => {
      const paths = [
        'C:\\Users\\John\\Documents',
        'D:\\Data\\Storage',
        'E:/Downloads/Files',  // Forward slash is also valid on Windows
        'C:\\Program Files\\App',
      ];

      paths.forEach(path => {
        const result = validateStoragePath(path);
        expect(result.isValid).toBe(true);
        expect(result.error).toBeUndefined();
      });
    });

    /**
     * Test: Valid Unix absolute paths
     * Based on: Unix/Linux file system path format
     * Verifies: Accepts paths starting with /
     */
    it('should accept Unix absolute paths starting with /', () => {
      const paths = [
        '/home/user/downloads',
        '/var/data/storage',
        '/mnt/storage/files',
        '/tmp/chiral',
      ];

      paths.forEach(path => {
        const result = validateStoragePath(path);
        expect(result.isValid).toBe(true);
        expect(result.error).toBeUndefined();
      });
    });
  });

  /**
   * Test: Invalid paths that should be rejected
   * Based on: Security requirement to prevent ambiguous paths
   * Verifies: Rejects relative paths, tilde expansion, and empty paths
   */
  describe('invalid paths', () => {
    it('should reject empty paths', () => {
      const result = validateStoragePath('');
      expect(result.isValid).toBe(false);
      expect(result.error).toBe('Storage path cannot be empty');
    });

    it('should reject whitespace-only paths', () => {
      const result = validateStoragePath('   ');
      expect(result.isValid).toBe(false);
      expect(result.error).toBe('Storage path cannot be empty');
    });

    /**
     * Test: Reject tilde paths (main issue case)
     * Based on: GitHub issue - prevents "~/chiral network" creating "~" folder
     * Verifies: User is guided to use absolute paths or folder picker
     */
    it('should reject paths starting with tilde', () => {
      const paths = [
        '~/Downloads',
        '~/Documents/Files',
        '~',
        '~/chiral network',  // The exact case from the GitHub issue
      ];

      paths.forEach(path => {
        const result = validateStoragePath(path);
        expect(result.isValid).toBe(false);
        expect(result.error).toContain('folder picker');
        expect(result.error).toContain('absolute path');
      });
    });

    it('should reject relative paths', () => {
      const paths = [
        'Downloads',
        './downloads',
        '../storage',
        'data/files',
        '.\\Downloads',  // Windows relative path
        '..\\storage',   // Windows relative path
      ];

      paths.forEach(path => {
        const result = validateStoragePath(path);
        expect(result.isValid).toBe(false);
        expect(result.error).toContain('absolute path');
      });
    });

    it('should reject paths with only a drive letter (no directory)', () => {
      const paths = [
        'C:',
        'D:',
      ];

      paths.forEach(path => {
        const result = validateStoragePath(path);
        expect(result.isValid).toBe(false);
        expect(result.error).toContain('absolute path');
      });
    });
  });

  /**
   * Test: Edge cases and special scenarios
   * Based on: Real-world usage patterns
   * Verifies: Handles spaces, trimming, and special characters correctly
   */
  describe('edge cases', () => {
    it('should handle paths with spaces correctly', () => {
      // Absolute paths with spaces should be valid
      const validPaths = [
        'C:\\Program Files\\My App',
        '/home/user/my documents',
      ];

      validPaths.forEach(path => {
        const result = validateStoragePath(path);
        expect(result.isValid).toBe(true);
      });
    });

    it('should trim whitespace from paths before validation', () => {
      const result = validateStoragePath('  C:\\Users\\Test  ');
      expect(result.isValid).toBe(true);
    });
  });

  /**
   * Test: Cross-platform compatibility
   * Based on: Application runs on both Windows and Unix systems
   * Verifies: Both path formats are accepted regardless of platform
   */
  describe('cross-platform compatibility', () => {
    it('should accept both Unix and Windows paths regardless of platform', () => {
      // The validation should work in both environments
      const windowsPath = validateStoragePath('C:\\Users\\Test');
      const unixPath = validateStoragePath('/home/test');

      expect(windowsPath.isValid).toBe(true);
      expect(unixPath.isValid).toBe(true);
    });
  });
});


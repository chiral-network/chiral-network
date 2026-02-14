# Validation Utilities Documentation

## Overview

This document explains the integrated validation utilities in `validation.ts` that enhance security and error handling in the Chiral Network application.

**Important**: These utilities provide **NEW** validations that didn't exist in the codebase. For existing validations, see:
- [src/pages/Account.svelte](../../pages/Account.svelte) - Ethereum address validation, password strength validation
- [src/pages/Proxy.svelte](../../pages/Proxy.svelte) - Proxy address validation
- [src/pages/Mining.svelte](../../pages/Mining.svelte) - Mining parameter validation (hardware-aware)
- [src/lib/services/webrtcService.ts](../services/webrtcService.ts) - ICE server sanitization
- [src/lib/wallet/bip39.ts](../wallet/bip39.ts) - BIP39 mnemonic validation

## Integrated Validation Functions

### 1. `validatePrivateKeyFormat(privateKey: string)`

**Purpose**: Validates Ethereum private key format to catch user input errors early.

**Source**: [Ethereum Yellow Paper](https://ethereum.github.io/yellowpaper/paper.pdf) - Private keys are 256-bit (32 bytes) values.

**Integration Status**: [OK] **INTEGRATED** in [src/pages/Account.svelte:611-616](../../pages/Account.svelte)

**Why This Validation Matters**:
- **Early error detection**: Catches typos/mistakes before expensive cryptographic operations
- **Better UX**: Provides clear error messages like "Private key must be 64 hex characters (got 63)" instead of cryptic backend errors
- **Not security-through-obscurity**: The format (64 hex chars) is public knowledge documented in Ethereum specs

**What It Checks**:
- [OK] Not empty or whitespace-only
- [OK] Exactly 64 hexadecimal characters (with or without 0x prefix)
- [OK] Only valid hex characters (0-9, a-f, A-F)
- [OK] Not all zeros (invalid private key in Ethereum)

**Usage Example**:
```typescript
import { validatePrivateKeyFormat } from '$lib/utils/validation';

async function importChiralAccount() {
  if (!importPrivateKey) return

  // Validate private key format before attempting import
  const validation = validatePrivateKeyFormat(importPrivateKey)
  if (!validation.isValid) {
    showToast(validation.error || 'Invalid private key format', 'error')
    return
  }

  // Proceed with import...
  const account = await walletService.importAccount(importPrivateKey)
}
```

**Test Coverage**: 9 tests in [tests/validation.test.ts](../../../tests/validation.test.ts)

---

### 2. `RateLimiter` Class

**Purpose**: Prevents brute force attacks on sensitive operations like keystore password attempts.

**Source**: [OWASP Authentication Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html)

**Integration Status**: [OK] **INTEGRATED** in [src/pages/Account.svelte:74,765-768,783,806](../../pages/Account.svelte)

**Why This Validation Matters**:
- **Security**: Prevents attackers from trying thousands of passwords
- **Sliding window**: Automatically expires old attempts after time window
- **Per-operation tracking**: Each operation has independent limits

**Algorithm**: Sliding window rate limiting - old attempts are automatically removed after the time window expires.

**Constructor**:
```typescript
new RateLimiter(maxAttempts: number, windowMs: number)
```

**Methods**:
- `checkLimit(key: string): boolean` - Check if operation is allowed
- `reset(key: string): void` - Clear attempts for specific key (call on success)
- `clearAll(): void` - Clear all rate limit data

**Usage Example** (from Account.svelte):
```typescript
import { RateLimiter } from '$lib/utils/validation';

// 5 attempts per minute for keystore unlock
const keystoreRateLimiter = new RateLimiter(5, 60000);

async function loadFromKeystore() {
  if (!selectedKeystoreAccount || !loadKeystorePassword) return;

  // Rate limiting: prevent brute force attacks
  if (!keystoreRateLimiter.checkLimit('keystore-unlock')) {
    keystoreLoadMessage = 'Too many unlock attempts. Please wait 1 minute before trying again.';
    return;
  }

  try {
    const account = await walletService.loadFromKeystore(selectedKeystoreAccount, loadKeystorePassword);

    // Success - reset rate limiter
    keystoreRateLimiter.reset('keystore-unlock');

    // ... handle success
  } catch (error) {
    // Note: Rate limiter is NOT reset on failure - failed attempts count toward limit
    keystoreLoadMessage = 'Failed to load from keystore: ' + error;
  }
}
```

**Recommended Limits**:
- Keystore unlock: 5 attempts per minute (implemented)
- 2FA verification: 3 attempts per 5 minutes
- Account import: 10 attempts per hour

**Test Coverage**: 8 tests in [tests/validation.test.ts](../../../tests/validation.test.ts) (including async time window test)

---

## Security Philosophy

### Why These Validations Don't Reveal Secrets

Some developers worry that validation error messages like "Private key must be 64 hex characters" give attackers information. This is a misconception:

1. **Public Standards**: Ethereum private key format is documented in the Yellow Paper - attackers already know
2. **Legitimate Use**: These validations help legitimate users catch mistakes early
3. **Defense in Depth**: Input validation is ONE layer of many security measures
4. **Not Security-Through-Obscurity**: Real security comes from cryptography, not hiding error messages

**Good analogy**: Email validation doesn't reveal secrets by requiring an `@` sign - everyone knows that format already.

---

## Not Integrated (And Why)

### Password Strength Validation

**Status**: [X] **NOT INTEGRATED**

**Reason**: Account.svelte already has excellent password strength validation (lines 236-273) with real-time strength indicators ("Weak", "Medium", "Strong"). The existing implementation is better integrated with the UI.

### File Upload Validation

**Status**: [X] **NOT INTEGRATED**

**Reason**:
1. **File size limits inappropriate**: Chiral Network is a BitTorrent-style P2P file sharing app - arbitrary size limits go against the design philosophy
2. **Security handled inline**: Upload.svelte (lines 254-260) blocks executable files directly for security
3. **Empty file check inline**: Upload.svelte (lines 263-267) validates non-empty files

### Mining Parameter Validation

**Status**: [X] **NOT INTEGRATED**

**Reason**: Mining.svelte already has hardware-aware validation (lines 284-296) that dynamically adjusts based on the user's CPU thread count. Static validation would be inferior.

### XSS Sanitization

**Status**: [X] **NOT INTEGRATED**

**Reason**: Svelte automatically escapes all interpolated values by default, providing XSS protection out of the box. Manual sanitization is only needed if using `{@html}` directive (which the app doesn't use).

---

## Testing

All integrated validation functions have comprehensive test coverage:

```bash
npm test -- validation.test.ts
```

**Test Results**: [OK] 16 tests passing

**Test breakdown**:
- `validatePrivateKeyFormat`: 9 tests
- `RateLimiter`: 8 tests (including async time window test)

**Test philosophy**:
- Test happy paths (valid inputs)
- Test edge cases (empty, null, boundary values)
- Test error messages (ensure they're helpful)
- Test security edge cases (all zeros private key, rate limit exhaustion, etc.)

---

## Integration Summary

### Files Modified

1. **[src/pages/Account.svelte](../../pages/Account.svelte)**
   - Line 37: Added imports for `validatePrivateKeyFormat` and `RateLimiter`
   - Line 74: Created `keystoreRateLimiter` instance (5 attempts per minute)
   - Lines 611-616: Private key validation in `importChiralAccount()`
   - Lines 765-820: Rate limiting in `loadFromKeystore()` with proper reset on success

2. **[src/pages/Upload.svelte](../../pages/Upload.svelte)**
   - Lines 254-260: Inline executable file blocking (`.exe`, `.bat`, `.cmd`, `.com`, `.msi`, `.scr`, `.vbs`)
   - Lines 263-267: Inline empty file validation

### Lines of Code

- **validation.ts**: 137 lines (2 exports)
- **validation.test.ts**: ~160 lines (16 tests)
- **Integration code**: ~30 lines across 2 files

---

## FAQ

### Q: Should I validate private keys if the backend will reject invalid ones anyway?

**A**: Yes. Early validation provides:
1. Better error messages for users
2. Avoids expensive backend calls
3. Catches typos before users wonder why it's not working

### Q: Do I need rate limiting if I'm using Tauri desktop app?

**A**: Yes. Desktop apps can still be attacked by:
1. Malicious users on shared computers
2. Malware trying to brute force keystore passwords
3. Compromised dependencies

### Q: Why not just block all file uploads instead of specific extensions?

**A**: Chiral Network is designed for legitimate P2P file sharing. Blocking executables is a security measure, but users should be able to share documents, media, archives, etc.

### Q: What about transaction API validations?

**A**: Transaction validations are handled separately. These utilities focus specifically on wallet/account security operations.

---

## Changelog

### 2025-01-XX - Initial Integration

**Integrated**:
- [OK] `validatePrivateKeyFormat()` - Account.svelte:611-616
- [OK] `RateLimiter` class - Account.svelte:74,765-820

**Not Integrated** (existing validations superior):
- [X] `sanitizeInput()` - Svelte auto-escapes
- [X] `validatePasswordStrength()` - Account.svelte has better implementation
- [X] `validateFileUpload()` - Inline validation more appropriate
- [X] `validateMiningParams()` - Mining.svelte has hardware-aware validation

**Test Coverage**: 16 tests, 100% passing

---

## References

1. [Ethereum Yellow Paper](https://ethereum.github.io/yellowpaper/paper.pdf) - Private key format specification
2. [OWASP Authentication Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html) - Rate limiting best practices

---

**Last Updated**: 2025-01-13
**Maintainer**: Chiral Network Team
**License**: Same as project

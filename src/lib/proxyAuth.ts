import { invoke } from "@tauri-apps/api/core";

/**
 * Proxy authentication service
 * Handles secure token generation and validation for proxy connections
 */
export class ProxyAuthService {
  private static readonly TOKEN_STORAGE_KEY = "proxy_auth_tokens";
  private static readonly TOKEN_EXPIRY_HOURS = 24; // Tokens expire after 24 hours

  /**
   * Generate a secure authentication token for proxy connection
   * @param proxyAddress - The proxy address to generate token for
   * @returns Promise<string> - Generated authentication token
   */
  static async generateProxyToken(proxyAddress: string): Promise<string> {
    try {
      // Generate a cryptographically secure token using the backend
      const tokenData = await invoke<{ token: string; expires_at: number }>(
        "generate_proxy_auth_token",
        {
          proxyAddress,
          expiryHours: this.TOKEN_EXPIRY_HOURS,
        }
      );

      // Store token locally for validation
      await this.storeToken(
        proxyAddress,
        tokenData.token,
        tokenData.expires_at
      );

      console.log(`Generated proxy auth token for ${proxyAddress}`);
      return tokenData.token;
    } catch (error) {
      console.error("Failed to generate proxy auth token:", error);
      // Fallback: generate a client-side token (less secure but better than dummy)
      return this.generateFallbackToken(proxyAddress);
    }
  }

  /**
   * Validate a proxy authentication token
   * @param proxyAddress - The proxy address
   * @param token - The token to validate
   * @returns Promise<boolean> - Whether the token is valid
   */
  static async validateProxyToken(
    proxyAddress: string,
    token: string
  ): Promise<boolean> {
    try {
      // Check local storage first
      const storedToken = await this.getStoredToken(proxyAddress);
      if (
        storedToken &&
        storedToken.token === token &&
        !this.isTokenExpired(storedToken.expiresAt)
      ) {
        return true;
      }

      // Validate with backend if available
      const isValid = await invoke<boolean>("validate_proxy_auth_token", {
        proxyAddress,
        token,
      });

      return isValid;
    } catch (error) {
      console.warn(
        "Backend token validation failed, using local validation:",
        error
      );
      // Fallback to local validation only
      const storedToken = await this.getStoredToken(proxyAddress);
      return storedToken
        ? storedToken.token === token &&
            !this.isTokenExpired(storedToken.expiresAt)
        : false;
    }
  }

  /**
   * Get a stored authentication token for a proxy
   * @param proxyAddress - The proxy address
   * @returns Promise<string | null> - The stored token or null if not found/expired
   */
  static async getProxyToken(proxyAddress: string): Promise<string | null> {
    const storedToken = await this.getStoredToken(proxyAddress);
    if (storedToken && !this.isTokenExpired(storedToken.expiresAt)) {
      return storedToken.token;
    }

    // Token expired or not found, clean it up
    await this.removeStoredToken(proxyAddress);
    return null;
  }

  /**
   * Check if a proxy requires authentication
   * @param proxyAddress - The proxy address to check
   * @returns Promise<boolean> - Whether authentication is required
   */
  static async requiresAuthentication(_proxyAddress: string): Promise<boolean> {
    try {
      // All proxies require authentication
      return true;
    } catch (error) {
      console.warn("Failed to check proxy authentication requirement:", error);
      return true; // Default to requiring authentication for security
    }
  }

  /**
   * Clean up expired tokens
   * @returns Promise<void>
   */
  static async cleanupExpiredTokens(): Promise<void> {
    try {
      const tokens = await this.getAllStoredTokens();
      const expiredAddresses: string[] = [];

      for (const [address, tokenData] of Object.entries(tokens)) {
        if (this.isTokenExpired(tokenData.expiresAt)) {
          expiredAddresses.push(address);
        }
      }

      if (expiredAddresses.length > 0) {
        const updatedTokens = { ...tokens };
        expiredAddresses.forEach((address) => delete updatedTokens[address]);

        if (typeof window !== "undefined" && window.localStorage) {
          localStorage.setItem(
            this.TOKEN_STORAGE_KEY,
            JSON.stringify(updatedTokens)
          );
        }

        console.log(
          `Cleaned up ${expiredAddresses.length} expired proxy tokens`
        );
      }
    } catch (error) {
      console.warn("Failed to cleanup expired tokens:", error);
    }
  }

  /**
   * Generate a fallback authentication token when backend is unavailable
   * @param proxyAddress - The proxy address to generate token for
   * @returns string - Generated fallback authentication token
   */
  static generateFallbackToken(proxyAddress: string): string {
    // Generate a simple token using timestamp and address
    // This is less secure but better than hardcoded "dummy-token"
    const timestamp = Date.now();
    const random = Math.random().toString(36).substring(2, 15);
    const data = `${proxyAddress}:${timestamp}:${random}`;
    return btoa(data)
      .replace(/[^a-zA-Z0-9]/g, "")
      .substring(0, 32);
  }

  private static async storeToken(
    proxyAddress: string,
    token: string,
    expiresAt: number
  ): Promise<void> {
    if (typeof window === "undefined" || !window.localStorage) {
      return; // No local storage available
    }

    try {
      const tokens = await this.getAllStoredTokens();
      tokens[proxyAddress] = { token, expiresAt };
      localStorage.setItem(this.TOKEN_STORAGE_KEY, JSON.stringify(tokens));
    } catch (error) {
      console.warn("Failed to store proxy token:", error);
    }
  }

  private static async getStoredToken(
    proxyAddress: string
  ): Promise<{ token: string; expiresAt: number } | null> {
    if (typeof window === "undefined" || !window.localStorage) {
      return null;
    }

    try {
      const tokens = await this.getAllStoredTokens();
      return tokens[proxyAddress] || null;
    } catch (error) {
      console.warn("Failed to get stored proxy token:", error);
      return null;
    }
  }

  private static async removeStoredToken(proxyAddress: string): Promise<void> {
    if (typeof window === "undefined" || !window.localStorage) {
      return;
    }

    try {
      const tokens = await this.getAllStoredTokens();
      delete tokens[proxyAddress];
      localStorage.setItem(this.TOKEN_STORAGE_KEY, JSON.stringify(tokens));
    } catch (error) {
      console.warn("Failed to remove stored proxy token:", error);
    }
  }

  private static async getAllStoredTokens(): Promise<
    Record<string, { token: string; expiresAt: number }>
  > {
    if (typeof window === "undefined" || !window.localStorage) {
      return {};
    }

    try {
      const stored = localStorage.getItem(this.TOKEN_STORAGE_KEY);
      return stored ? JSON.parse(stored) : {};
    } catch (error) {
      console.warn("Failed to parse stored tokens:", error);
      return {};
    }
  }

  private static isTokenExpired(expiresAt: number): boolean {
    return Date.now() > expiresAt;
  }
}

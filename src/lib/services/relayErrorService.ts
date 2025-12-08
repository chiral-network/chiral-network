/**
 * Relay Error Protocol Service
 *
 * Handles Circuit Relay v2 connection failures with intelligent retry logic,
 * automatic failover to backup relays, and comprehensive error tracking.
 *
 * Key Features:
 * - Exponential backoff retry strategy
 * - Multi-relay pool management
 * - Automatic relay health scoring
 * - Fallback relay discovery
 * - Error categorization and logging
 */

import { writable, derived, get } from "svelte/store";
import { dhtService, type DhtHealth } from "$lib/dht";

/**
 * Relay error categories for diagnosis
 */
export enum RelayErrorType {
  CONNECTION_REFUSED = "connection_refused", // Relay actively refused connection
  CONNECTION_TIMEOUT = "connection_timeout", // Relay didn't respond in time
  RESERVATION_FAILED = "reservation_failed", // Failed to reserve relay slot
  RESERVATION_EXPIRED = "reservation_expired", // Relay reservation expired
  RELAY_OVERLOADED = "relay_overloaded", // Relay at capacity
  RELAY_UNREACHABLE = "relay_unreachable", // Can't reach relay at all
  NETWORK_ERROR = "network_error", // General network issues
  AUTHENTICATION_FAILED = "auth_failed", // Authentication issues
  PROTOCOL_ERROR = "protocol_error", // Protocol-level errors
  UNKNOWN = "unknown", // Unclassified errors
}

/**
 * Relay connection state
 */
export enum RelayConnectionState {
  IDLE = "idle", // Not attempting connection
  CONNECTING = "connecting", // Attempting connection
  CONNECTED = "connected", // Successfully connected
  RESERVING = "reserving", // Requesting reservation
  RESERVED = "reserved", // Reservation successful
  RETRYING = "retrying", // Retrying after failure
  FAILED = "failed", // Failed after all retries
  FALLBACK = "fallback", // Using fallback relay
}

/**
 * Individual relay node information
 */
export interface RelayNode {
  id: string; // Peer ID
  multiaddr: string; // Full multiaddr
  state: RelayConnectionState;
  healthScore: number; // 0-100 health score
  lastAttempt: number | null; // Timestamp of last connection attempt
  lastSuccess: number | null; // Timestamp of last successful connection
  consecutiveFailures: number; // Count of consecutive failures
  totalAttempts: number; // Total connection attempts
  totalSuccesses: number; // Total successful connections
  avgLatency: number; // Average latency in ms
  reservationExpiry: number | null; // Reservation expiry timestamp
  isPrimary: boolean; // Whether this is a preferred relay
  errors: RelayError[]; // Recent error history (last 10)
}

/**
 * Relay error record
 */
export interface RelayError {
  type: RelayErrorType;
  message: string;
  timestamp: number;
  relayId: string;
  retryCount: number;
}

/**
 * Relay connection attempt result
 */
export interface RelayAttemptResult {
  success: boolean;
  relayId: string;
  latency?: number;
  error?: RelayError;
  reservationExpiry?: number;
}

/**
 * Relay error service configuration
 */
export interface RelayErrorConfig {
  maxRetries: number; // Max retry attempts per relay
  initialRetryDelay: number; // Initial retry delay in ms
  maxRetryDelay: number; // Maximum retry delay in ms
  backoffMultiplier: number; // Exponential backoff multiplier
  reservationRenewalThreshold: number; // Renew when X seconds remain
  healthScoreDecay: number;         // Health score reduction per failure
  errorHistoryLimit: number;        // Max errors to track per relay
  connectionTimeout: number;        // Connection timeout in ms
  autoDiscoverRelays: boolean;      // Auto-discover relay nodes via DHT
  minHealthScore: number;           // Minimum health score to attempt connection
  healthCheckIntervalSeconds: number; // Configurable interval for periodic health checks (seconds)
}

const DEFAULT_CONFIG: RelayErrorConfig = {
  maxRetries: 3,
  initialRetryDelay: 1000,
  maxRetryDelay: 30000,
  backoffMultiplier: 2,
  reservationRenewalThreshold: 300, // 5 minutes
  healthScoreDecay: 15,
  errorHistoryLimit: 10,
  connectionTimeout: 10000,
  autoDiscoverRelays: true,
  minHealthScore: 20,
  healthCheckIntervalSeconds: 30 // Default: check relay health every 30 seconds
};

/**
 * Relay Error Service
 * Manages relay connections with intelligent error handling and failover
 */
class RelayErrorService {
  private static instance: RelayErrorService | null = null;
  private config: RelayErrorConfig;
  private healthCheckTimer: ReturnType<typeof setInterval> | null = null; // Timer for periodic health checks

  // Relay pool management
  public relayPool = writable<Map<string, RelayNode>>(new Map());
  public activeRelay = writable<RelayNode | null>(null);
  public errorLog = writable<RelayError[]>([]);

  // Derived stores
  public healthyRelays = derived(this.relayPool, ($pool) =>
    Array.from($pool.values())
      .filter((relay) => relay.healthScore >= this.config.minHealthScore)
      .sort((a, b) => b.healthScore - a.healthScore)
  );

  public relayStats = derived(
    [this.relayPool, this.errorLog],
    ([$pool, $errors]) => ({
      totalRelays: $pool.size,
      healthyRelays: Array.from($pool.values()).filter(
        (r) => r.healthScore >= this.config.minHealthScore
      ).length,
      connectedRelays: Array.from($pool.values()).filter(
        (r) =>
          r.state === RelayConnectionState.CONNECTED ||
          r.state === RelayConnectionState.RESERVED
      ).length,
      totalErrors: $errors.length,
      avgHealthScore:
        Array.from($pool.values()).reduce((sum, r) => sum + r.healthScore, 0) /
          $pool.size || 0,
    })
  );

  private constructor(config?: Partial<RelayErrorConfig>) {
    this.config = { ...DEFAULT_CONFIG, ...config };
  }

  static getInstance(config?: Partial<RelayErrorConfig>): RelayErrorService {
    if (!RelayErrorService.instance) {
      RelayErrorService.instance = new RelayErrorService(config);
    }
    return RelayErrorService.instance;
  }

  /**
   * Initialize relay pool from configuration
   */
  async initialize(
    preferredRelays: string[],
    enableAutoDiscover: boolean = true
  ): Promise<void> {
    // Add preferred relays to pool
    for (const multiaddr of preferredRelays) {
      this.addRelayToPool(multiaddr, true);
    }

    // Auto-discover additional relays if enabled
    if (enableAutoDiscover && this.config.autoDiscoverRelays) {
      await this.discoverRelays();
    }
  }

  /**
   * Add a relay to the pool
   */
  private addRelayToPool(multiaddr: string, isPrimary: boolean = false): void {
    const id = this.extractPeerIdFromMultiaddr(multiaddr);
    if (!id) {
      console.warn(`Invalid multiaddr: ${multiaddr}`);
      return;
    }

    this.relayPool.update((pool) => {
      if (!pool.has(id)) {
        pool.set(id, {
          id,
          multiaddr,
          state: RelayConnectionState.IDLE,
          healthScore: isPrimary ? 100 : 75, // Primary relays start with higher score
          lastAttempt: null,
          lastSuccess: null,
          consecutiveFailures: 0,
          totalAttempts: 0,
          totalSuccesses: 0,
          avgLatency: 0,
          reservationExpiry: null,
          isPrimary,
          errors: [],
        });
      }
      return pool;
    });
  }

  /**
   * Attempt to connect to a relay with retry logic
   */
  async connectToRelay(relayId?: string): Promise<RelayAttemptResult> {
    const pool = get(this.relayPool);

    // Select relay: specified ID, active relay, or best available
    let relay: RelayNode | undefined;
    if (relayId) {
      relay = pool.get(relayId);
    } else {
      // Find best available relay
      relay = this.selectBestRelay();
    }

    if (!relay) {
      const error: RelayError = {
        type: RelayErrorType.RELAY_UNREACHABLE,
        message: "No healthy relays available in pool",
        timestamp: Date.now(),
        relayId: relayId || "unknown",
        retryCount: 0,
      };
      this.logError(error);
      return { success: false, relayId: relayId || "unknown", error };
    }

    // Attempt connection with retries
    return await this.attemptConnectionWithRetry(relay);
  }

  /**
   * Attempt connection with exponential backoff retry
   */
  private async attemptConnectionWithRetry(
    relay: RelayNode
  ): Promise<RelayAttemptResult> {
    let retryCount = 0;
    let delay = this.config.initialRetryDelay;

    while (retryCount <= this.config.maxRetries) {
      // Update state
      this.updateRelayState(relay.id, RelayConnectionState.CONNECTING);

      try {
        const result = await this.performConnection(relay);

        if (result.success) {
          // Connection successful
          this.handleConnectionSuccess(relay, result);
          return result;
        } else {
          // Connection failed
          retryCount++;
          if (retryCount > this.config.maxRetries) {
            // Max retries exceeded
            this.handleConnectionFailure(relay, result.error!, retryCount);
            return result;
          }

          // Retry with backoff
          console.log(
            `⚠️ Relay ${relay.id} failed, retrying in ${delay}ms (attempt ${retryCount}/${this.config.maxRetries})`
          );
          this.updateRelayState(relay.id, RelayConnectionState.RETRYING);
          await this.delay(delay);
          delay = Math.min(
            delay * this.config.backoffMultiplier,
            this.config.maxRetryDelay
          );
        }
      } catch (error) {
        retryCount++;
        const relayError: RelayError = {
          type: RelayErrorType.UNKNOWN,
          message: error instanceof Error ? error.message : String(error),
          timestamp: Date.now(),
          relayId: relay.id,
          retryCount,
        };

        if (retryCount > this.config.maxRetries) {
          this.handleConnectionFailure(relay, relayError, retryCount);
          return { success: false, relayId: relay.id, error: relayError };
        }

        console.error(`❌ Relay ${relay.id} error:`, error);
        await this.delay(delay);
        delay = Math.min(
          delay * this.config.backoffMultiplier,
          this.config.maxRetryDelay
        );
      }
    }

    // Should never reach here, but handle it
    const error: RelayError = {
      type: RelayErrorType.UNKNOWN,
      message: "Maximum retries exceeded",
      timestamp: Date.now(),
      relayId: relay.id,
      retryCount,
    };
    return { success: false, relayId: relay.id, error };
  }

  /**
   * Perform actual relay connection via backend
   */
  private async performConnection(
    relay: RelayNode
  ): Promise<RelayAttemptResult> {
    const startTime = Date.now();

    try {
      // Update attempt count
      this.updateRelayMetrics(relay.id, {
        totalAttempts: relay.totalAttempts + 1,
      });

      // Attempt connection through DHT service
      await dhtService.connectPeer(relay.multiaddr);

      const latency = Date.now() - startTime;

      // Connection successful, request reservation
      try {
        // In a real implementation, this would call a backend reservation command
        // For now, we'll simulate it
        const reservationExpiry = Date.now() + 3600 * 1000; // 1 hour

        return {
          success: true,
          relayId: relay.id,
          latency,
          reservationExpiry,
        };
      } catch (reservationError) {
        // Connected but reservation failed
        const error: RelayError = {
          type: RelayErrorType.RESERVATION_FAILED,
          message:
            reservationError instanceof Error
              ? reservationError.message
              : String(reservationError),
          timestamp: Date.now(),
          relayId: relay.id,
          retryCount: 0,
        };
        return { success: false, relayId: relay.id, error };
      }
    } catch (error) {
      // Connection failed - categorize error
      const relayError: RelayError = {
        type: this.categorizeError(error),
        message: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
        relayId: relay.id,
        retryCount: 0,
      };
      return { success: false, relayId: relay.id, error: relayError };
    }
  }

  /**
   * Handle successful connection
   */
  private handleConnectionSuccess(
    relay: RelayNode,
    result: RelayAttemptResult
  ): void {
    this.updateRelayState(
      relay.id,
      result.reservationExpiry
        ? RelayConnectionState.RESERVED
        : RelayConnectionState.CONNECTED
    );

    this.updateRelayMetrics(relay.id, {
      lastSuccess: Date.now(),
      consecutiveFailures: 0,
      totalSuccesses: relay.totalSuccesses + 1,
      avgLatency:
        (relay.avgLatency * relay.totalSuccesses + (result.latency || 0)) /
        (relay.totalSuccesses + 1),
      reservationExpiry: result.reservationExpiry || null,
      healthScore: Math.min(relay.healthScore + 10, 100), // Reward success
    });

    this.activeRelay.set(relay);
  }

  /**
   * Handle connection failure
   */
  private handleConnectionFailure(
    relay: RelayNode,
    error: RelayError,
    retryCount: number
  ): void {
    console.error(
      `❌ Relay ${relay.id} failed after ${retryCount} attempts:`,
      error.message
    );

    this.logError(error);
    this.updateRelayState(relay.id, RelayConnectionState.FAILED);

    const newHealthScore = Math.max(
      relay.healthScore - this.config.healthScoreDecay,
      0
    );

    this.updateRelayMetrics(relay.id, {
      consecutiveFailures: relay.consecutiveFailures + 1,
      healthScore: newHealthScore,
      errors: [...relay.errors, error].slice(-this.config.errorHistoryLimit),
    });

    // Attempt fallback to another relay
    this.attemptFallback(relay.id);
  }

  /**
   * Attempt to connect to fallback relay
   */
  private async attemptFallback(failedRelayId: string): Promise<void> {
    console.log(`🔄 Attempting fallback from relay ${failedRelayId}...`);

    const healthyRelays = get(this.healthyRelays);
    const fallbackRelay = healthyRelays.find((r) => r.id !== failedRelayId);

    if (fallbackRelay) {
      this.updateRelayState(fallbackRelay.id, RelayConnectionState.FALLBACK);
      const result = await this.connectToRelay(fallbackRelay.id);

      if (result.success) {
        console.log(`✅ Successfully failed over to relay ${fallbackRelay.id}`);
      } else {
        console.error(`❌ Fallback to relay ${fallbackRelay.id} also failed`);
      }
    } else {
      console.error("❌ No healthy fallback relays available");
      this.activeRelay.set(null);
    }
  }

  /**
   * Select best available relay based on health score and status
   */
  private selectBestRelay(): RelayNode | undefined {
    const pool = get(this.relayPool);
    const relays = Array.from(pool.values());

    // Filter to healthy relays
    const healthy = relays.filter(
      (r) =>
        r.healthScore >= this.config.minHealthScore &&
        r.state !== RelayConnectionState.FAILED
    );

    if (healthy.length === 0) return undefined;

    // Prioritize: primary relays > recently successful > highest health score
    return healthy.sort((a, b) => {
      if (a.isPrimary !== b.isPrimary) return a.isPrimary ? -1 : 1;
      if (a.lastSuccess && b.lastSuccess) {
        if (Math.abs(a.lastSuccess - b.lastSuccess) > 60000) {
          return b.lastSuccess - a.lastSuccess;
        }
      }
      return b.healthScore - a.healthScore;
    })[0];
  }

  /**
   * Discover additional relay nodes via DHT
   */
  private async discoverRelays(): Promise<void> {
    try {
      // In a real implementation, this would query the DHT for relay nodes
      // For now, this is a placeholder
      // Example: const relays = await invoke<string[]>('discover_relay_nodes');
    } catch (error) {
      console.error("Failed to discover relays:", error);
    }
  }

  /**
   * Monitor and renew relay reservations
   */
  async monitorReservations(): Promise<void> {
    const pool = get(this.relayPool);
    const now = Date.now();

    for (const relay of pool.values()) {
      if (
        relay.reservationExpiry &&
        relay.state === RelayConnectionState.RESERVED
      ) {
        const timeRemaining = (relay.reservationExpiry - now) / 1000;

        if (timeRemaining < this.config.reservationRenewalThreshold) {
          console.log(
            `⏰ Renewing reservation for relay ${relay.id} (${timeRemaining}s remaining)`
          );
          await this.renewReservation(relay);
        }
      }
    }
  }

  /**
   * Renew relay reservation
   */
  private async renewReservation(relay: RelayNode): Promise<void> {
    try {
      // In a real implementation, call backend to renew
      // For now, simulate renewal
      const newExpiry = Date.now() + 3600 * 1000;
      this.updateRelayMetrics(relay.id, { reservationExpiry: newExpiry });
      console.log(`✅ Renewed reservation for relay ${relay.id}`);
    } catch (error) {
      console.error(
        `❌ Failed to renew reservation for relay ${relay.id}:`,
        error
      );
      const relayError: RelayError = {
        type: RelayErrorType.RESERVATION_EXPIRED,
        message: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
        relayId: relay.id,
        retryCount: 0,
      };
      this.handleConnectionFailure(relay, relayError, 0);
    }
  }

  /**
   * Categorize error type from error message
   */
  private categorizeError(error: unknown): RelayErrorType {
    const message =
      error instanceof Error
        ? error.message.toLowerCase()
        : String(error).toLowerCase();

    if (message.includes("refused") || message.includes("rejected")) {
      return RelayErrorType.CONNECTION_REFUSED;
    } else if (message.includes("timeout") || message.includes("timed out")) {
      return RelayErrorType.CONNECTION_TIMEOUT;
    } else if (message.includes("reservation")) {
      return RelayErrorType.RESERVATION_FAILED;
    } else if (message.includes("overload") || message.includes("capacity")) {
      return RelayErrorType.RELAY_OVERLOADED;
    } else if (
      message.includes("unreachable") ||
      message.includes("not found")
    ) {
      return RelayErrorType.RELAY_UNREACHABLE;
    } else if (message.includes("auth")) {
      return RelayErrorType.AUTHENTICATION_FAILED;
    } else if (message.includes("protocol")) {
      return RelayErrorType.PROTOCOL_ERROR;
    } else if (message.includes("network")) {
      return RelayErrorType.NETWORK_ERROR;
    }

    return RelayErrorType.UNKNOWN;
  }

  /**
   * Update relay state
   */
  private updateRelayState(relayId: string, state: RelayConnectionState): void {
    this.relayPool.update((pool) => {
      const relay = pool.get(relayId);
      if (relay) {
        relay.state = state;
        relay.lastAttempt = Date.now();
      }
      return pool;
    });
  }

  /**
   * Update relay metrics
   */
  private updateRelayMetrics(
    relayId: string,
    updates: Partial<RelayNode>
  ): void {
    this.relayPool.update((pool) => {
      const relay = pool.get(relayId);
      if (relay) {
        Object.assign(relay, updates);
      }
      return pool;
    });
  }

  /**
   * Log error to error log
   */
  private logError(error: RelayError): void {
    this.errorLog.update((log) => {
      log.unshift(error);
      return log.slice(0, 100); // Keep last 100 errors
    });
  }

  /**
   * Extract peer ID from multiaddr
   */
  private extractPeerIdFromMultiaddr(multiaddr: string): string | null {
    const match = multiaddr.match(/\/p2p\/([^\/]+)/);
    return match ? match[1] : null;
  }

  /**
   * Utility: delay promise
   */
  private delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  /**
   * Get relay pool status
   */
  getPoolStatus(): {
    relayCount: number;
    healthyCount: number;
    connectedCount: number;
  } {
    const pool = get(this.relayPool);
    const relays = Array.from(pool.values());

    return {
      relayCount: relays.length,
      healthyCount: relays.filter(
        (r) => r.healthScore >= this.config.minHealthScore
      ).length,
      connectedCount: relays.filter(
        (r) =>
          r.state === RelayConnectionState.CONNECTED ||
          r.state === RelayConnectionState.RESERVED
      ).length,
    };
  }

  /**
   * Clear error log
   */
  clearErrorLog(): void {
    this.errorLog.set([]);
  }

  /**
   * Reset relay health scores (useful for testing)
   */
  resetRelayHealth(): void {
    this.relayPool.update((pool) => {
      for (const relay of pool.values()) {
        relay.healthScore = relay.isPrimary ? 100 : 75;
        relay.consecutiveFailures = 0;
        relay.errors = [];
      }
      return pool;
    });
  }

  /**
   * Sync relay pool from backend DHT health snapshot so UI reflects active relay even without preferred relays.
   */
  syncFromHealthSnapshot(health: DhtHealth): void {
    const peerId = health.activeRelayPeerId?.trim();

    if (!peerId) {
      this.activeRelay.set(null);
      return;
    }

    const now = Date.now();
    const healthScore =
      typeof health.relayHealthScore === "number"
        ? Math.round(health.relayHealthScore * 100)
        : 75;
    const state = health.relayReservationStatus
      ? RelayConnectionState.RESERVED
      : RelayConnectionState.CONNECTED;

    this.relayPool.update((pool) => {
      const existing = pool.get(peerId);
      if (existing) {
        existing.state = state;
        existing.lastAttempt = now;
        existing.lastSuccess = now;
        existing.healthScore = healthScore;
        existing.reservationExpiry = health.lastReservationRenewal
          ? health.lastReservationRenewal * 1000
          : null;
        existing.totalAttempts = Math.max(existing.totalAttempts, 1);
        existing.totalSuccesses = Math.max(existing.totalSuccesses, 1);
      } else {
        pool.set(peerId, {
          id: peerId,
          multiaddr: `/p2p/${peerId}`,
          state,
          healthScore,
          lastAttempt: now,
          lastSuccess: now,
          consecutiveFailures: 0,
          totalAttempts: 1,
          totalSuccesses: 1,
          avgLatency: 0,
          reservationExpiry: health.lastReservationRenewal
            ? health.lastReservationRenewal * 1000
            : null,
          isPrimary: false,
          errors: [],
        });
      }
      return pool;
    });

    const pool = get(this.relayPool);
    this.activeRelay.set(pool.get(peerId) ?? null);
  }

  /**
   * Start periodic health checks for all relays
   */
  startHealthChecks(): void {
    if (this.healthCheckTimer) {
      clearInterval(this.healthCheckTimer);
    }

    const intervalMs = this.config.healthCheckIntervalSeconds * 1000; // Convert seconds to milliseconds
    console.log(`🏥 Starting relay health checks every ${this.config.healthCheckIntervalSeconds}s`);

    this.healthCheckTimer = setInterval(async () => {
      await this.performHealthChecks();
    }, intervalMs);
  }

  /**
   * Stop periodic health checks
   */
  stopHealthChecks(): void {
    if (this.healthCheckTimer) {
      clearInterval(this.healthCheckTimer);
      this.healthCheckTimer = null;
      console.log('🛑 Stopped relay health checks');
    }
  }

  /**
   * Perform health check on all relays in pool
   */
  private async performHealthChecks(): Promise<void> {
    const pool = get(this.relayPool);
    console.log(`🏥 Running health check on ${pool.size} relays...`);

    for (const relay of pool.values()) {
      // Skip recently checked relays to avoid spam (5 second cooldown)
      if (relay.lastAttempt && (Date.now() - relay.lastAttempt) < 5000) {
        continue;
      }

      // Ping relay to check connectivity
      try {
        const startTime = Date.now();
        await dhtService.connectPeer(relay.multiaddr);
        const latency = Date.now() - startTime;

        // Update health based on response time (bonus points for fast relays)
        const healthBonus = latency < 100 ? 5 : latency < 500 ? 2 : 0;
        this.updateRelayMetrics(relay.id, {
          lastAttempt: Date.now(),
          avgLatency: (relay.avgLatency * 0.7) + (latency * 0.3), // Exponential moving average
          healthScore: Math.min(relay.healthScore + healthBonus, 100)
        });
      } catch (error) {
        // Health check failed - reduce health score
        const newHealthScore = Math.max(relay.healthScore - 5, 0);
        this.updateRelayMetrics(relay.id, {
          lastAttempt: Date.now(),
          healthScore: newHealthScore
        });
      }
    }
  }

  /**
   * Update health check interval (runtime configuration)
   */
  setHealthCheckInterval(seconds: number): void {
    if (seconds < 10) seconds = 10; // Minimum 10 seconds
    if (seconds > 300) seconds = 300; // Maximum 5 minutes

    this.config.healthCheckIntervalSeconds = seconds;

    // Restart timer with new interval if health checks are currently running
    if (this.healthCheckTimer) {
      this.stopHealthChecks();
      this.startHealthChecks();
    }

    console.log(`⏱️ Updated health check interval to ${seconds}s`);
  }

  /**
   * Get current health check interval
   */
  getHealthCheckInterval(): number {
    return this.config.healthCheckIntervalSeconds;
  }
}

// Export singleton instance
export const relayErrorService = RelayErrorService.getInstance();
export default RelayErrorService;

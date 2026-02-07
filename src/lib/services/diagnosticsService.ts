/**
 * Enhanced Diagnostics Service
 * Comprehensive system health checks for Chiral Network
 */

import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { dhtService } from "$lib/dht";
import { get } from "svelte/store";
import { settings } from "$lib/stores";

export type DiagStatus = "pass" | "fail" | "warn" | "info";

export interface DiagResult {
  id: string;
  category: "environment" | "network" | "storage" | "security" | "system";
  label: string;
  status: DiagStatus;
  details?: string;
  error?: string;
  timestamp: number;
}

export interface DiagReport {
  timestamp: number;
  results: DiagResult[];
  summary: {
    total: number;
    passed: number;
    failed: number;
    warnings: number;
  };
}

class DiagnosticsService {
  /**
   * Best-effort parent directory extraction that works for both POSIX and Windows paths.
   * This is used for fallbacks when the configured directory doesn't exist yet.
   */
  private getParentPath(inputPath: string): string {
    if (!inputPath) return "/";

    // Normalize Windows separators to '/' for parsing. Windows APIs accept 'C:/path' as well.
    let normalized = inputPath.replace(/\\/g, "/");

    // Keep drive root as-is.
    if (/^[A-Za-z]:\/$/.test(normalized)) {
      return normalized;
    }

    // Trim trailing slashes (but not the drive root which we handled above)
    normalized = normalized.replace(/\/+$/, "");

    const idx = normalized.lastIndexOf("/");
    if (idx < 0) {
      return normalized || "/";
    }

    const parent = normalized.slice(0, idx) || "/";
    // If parent is a bare drive letter like "C:", return "C:/"
    return /^[A-Za-z]:$/.test(parent) ? `${parent}/` : parent;
  }

  /**
   * Check if running in Tauri environment (dynamically at runtime)
   */
  private get isTauri(): boolean {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }

  /**
   * Run all diagnostic checks
   */
  async runAll(): Promise<DiagReport> {
    const results: DiagResult[] = [];

    // Run all checks in parallel for better performance
    const checks = await Promise.allSettled([
      this.checkEnvironment(),
      this.checkStorage(),
      this.checkDHTConnectivity(),
      this.checkPeerConnections(),
      this.checkBootstrapNodes(),
      this.checkNATTraversal(),
      this.checkWebRTCSupport(),
      this.checkProxyConfiguration(),
      this.checkEncryptionCapability(),
      this.checkBandwidthLimits(),
      this.checkDiskSpace(),
      this.checkI18nStorage(),
    ]);

    // Collect all results
    checks.forEach((check) => {
      if (check.status === "fulfilled") {
        results.push(check.value);
      } else {
        // Handle failed checks
        results.push({
          id: "unknown_check",
          category: "system",
          label: "Unknown Check",
          status: "fail",
          error: check.reason?.message || String(check.reason),
          timestamp: Date.now(),
        });
      }
    });

    // Calculate summary
    const summary = {
      total: results.length,
      passed: results.filter((r) => r.status === "pass").length,
      failed: results.filter((r) => r.status === "fail").length,
      warnings: results.filter((r) => r.status === "warn").length,
    };

    return {
      timestamp: Date.now(),
      results,
      summary,
    };
  }

  /**
   * Environment check: Tauri vs Web
   */
  private async checkEnvironment(): Promise<DiagResult> {
    try {
      if (this.isTauri) {
        try {
          const version = await getVersion();
          return {
            id: "env_check",
            category: "environment",
            label: "Environment",
            status: "pass",
            details: `Tauri Desktop App v${version}`,
            timestamp: Date.now(),
          };
        } catch (versionError) {
          // Tauri detected but version unavailable
          return {
            id: "env_check",
            category: "environment",
            label: "Environment",
            status: "pass",
            details: "Tauri Desktop App (version unknown)",
            timestamp: Date.now(),
          };
        }
      } else {
        return {
          id: "env_check",
          category: "environment",
          label: "Environment",
          status: "warn",
          details: "Web build - some features may be limited",
          timestamp: Date.now(),
        };
      }
    } catch (error) {
      return {
        id: "env_check",
        category: "environment",
        label: "Environment",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Storage check: Path validation and write permissions
   */
  private async checkStorage(): Promise<DiagResult> {
    try {
      if (!this.isTauri) {
        return {
          id: "storage_check",
          category: "storage",
          label: "Storage Path",
          status: "info",
          details: "Skipped in web build",
          timestamp: Date.now(),
        };
      }

      const storagePath = await invoke<string>("get_download_directory");

      // Validate path
      try {
        await invoke("validate_storage_path", { path: storagePath });
        return {
          id: "storage_check",
          category: "storage",
          label: "Storage Path",
          status: "pass",
          details: `${storagePath} (accessible)`,
          timestamp: Date.now(),
        };
      } catch (validateError) {
        const errorMsg = validateError instanceof Error ? validateError.message : String(validateError);

        // Directory doesn't exist is a warning, not a failure (app can create it)
        if (errorMsg.includes("does not exist") || errorMsg.includes("No such file")) {
          return {
            id: "storage_check",
            category: "storage",
            label: "Storage Path",
            status: "warn",
            details: `${storagePath} (will be created on first use)`,
            timestamp: Date.now(),
          };
        }

        // Other errors (permissions, invalid path) are failures
        return {
          id: "storage_check",
          category: "storage",
          label: "Storage Path",
          status: "fail",
          details: storagePath,
          error: errorMsg,
          timestamp: Date.now(),
        };
      }
    } catch (error) {
      return {
        id: "storage_check",
        category: "storage",
        label: "Storage Path",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * DHT connectivity check
   */
  private async checkDHTConnectivity(): Promise<DiagResult> {
    try {
      if (!this.isTauri) {
        return {
          id: "dht_check",
          category: "network",
          label: "DHT Connectivity",
          status: "info",
          details: "Skipped in web build",
          timestamp: Date.now(),
        };
      }

      const isRunning = await invoke<boolean>("is_dht_running");

      if (!isRunning) {
        return {
          id: "dht_check",
          category: "network",
          label: "DHT Connectivity",
          status: "warn",
          details: "DHT node not running",
          timestamp: Date.now(),
        };
      }

      const health = await dhtService.getHealth();

      if (!health) {
        return {
          id: "dht_check",
          category: "network",
          label: "DHT Connectivity",
          status: "warn",
          details: "DHT health unavailable",
          timestamp: Date.now(),
        };
      }

      if (health.peerCount === 0) {
        return {
          id: "dht_check",
          category: "network",
          label: "DHT Connectivity",
          status: "warn",
          details: "No peers connected",
          timestamp: Date.now(),
        };
      }

      return {
        id: "dht_check",
        category: "network",
        label: "DHT Connectivity",
        status: "pass",
        details: `${health.peerCount} peer(s) connected`,
        timestamp: Date.now(),
      };
    } catch (error) {
      return {
        id: "dht_check",
        category: "network",
        label: "DHT Connectivity",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Peer connections check
   */
  private async checkPeerConnections(): Promise<DiagResult> {
    try {
      if (!this.isTauri) {
        return {
          id: "peers_check",
          category: "network",
          label: "Peer Connections",
          status: "info",
          details: "Skipped in web build",
          timestamp: Date.now(),
        };
      }

      const peerCount = await dhtService.getPeerCount();

      if (peerCount === 0) {
        return {
          id: "peers_check",
          category: "network",
          label: "Peer Connections",
          status: "warn",
          details: "No active peer connections",
          timestamp: Date.now(),
        };
      }

      return {
        id: "peers_check",
        category: "network",
        label: "Peer Connections",
        status: "pass",
        details: `${peerCount} active connection(s)`,
        timestamp: Date.now(),
      };
    } catch (error) {
      return {
        id: "peers_check",
        category: "network",
        label: "Peer Connections",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Bootstrap nodes availability check
   */
  private async checkBootstrapNodes(): Promise<DiagResult> {
    try {
      if (!this.isTauri) {
        return {
          id: "bootstrap_check",
          category: "network",
          label: "Bootstrap Nodes",
          status: "info",
          details: "Skipped in web build",
          timestamp: Date.now(),
        };
      }

      const nodes = await invoke<string[]>("get_bootstrap_nodes_command");
      const count = Array.isArray(nodes) ? nodes.length : 0;

      if (count === 0) {
        return {
          id: "bootstrap_check",
          category: "network",
          label: "Bootstrap Nodes",
          status: "fail",
          details: "No bootstrap nodes configured",
          timestamp: Date.now(),
        };
      }

      return {
        id: "bootstrap_check",
        category: "network",
        label: "Bootstrap Nodes",
        status: "pass",
        details: `${count} node(s) configured`,
        timestamp: Date.now(),
      };
    } catch (error) {
      return {
        id: "bootstrap_check",
        category: "network",
        label: "Bootstrap Nodes",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * NAT traversal status check
   */
  private async checkNATTraversal(): Promise<DiagResult> {
    try {
      if (!this.isTauri) {
        return {
          id: "nat_check",
          category: "network",
          label: "NAT Traversal",
          status: "info",
          details: "Skipped in web build",
          timestamp: Date.now(),
        };
      }

      const health = await dhtService.getHealth();

      if (!health) {
        return {
          id: "nat_check",
          category: "network",
          label: "NAT Traversal",
          status: "warn",
          details: "DHT health unavailable",
          timestamp: Date.now(),
        };
      }

      const { reachability, reachabilityConfidence, autonatEnabled } = health;

      if (!autonatEnabled) {
        return {
          id: "nat_check",
          category: "network",
          label: "NAT Traversal (AutoNAT)",
          status: "info",
          details: "AutoNAT disabled",
          timestamp: Date.now(),
        };
      }

      const statusText = `${reachability} (${reachabilityConfidence} confidence)`;

      if (reachability === "public") {
        return {
          id: "nat_check",
          category: "network",
          label: "NAT Traversal (AutoNAT)",
          status: "pass",
          details: statusText,
          timestamp: Date.now(),
        };
      } else if (reachability === "private") {
        return {
          id: "nat_check",
          category: "network",
          label: "NAT Traversal (AutoNAT)",
          status: "warn",
          details: `${statusText} - relay may be needed`,
          timestamp: Date.now(),
        };
      } else {
        return {
          id: "nat_check",
          category: "network",
          label: "NAT Traversal (AutoNAT)",
          status: "info",
          details: statusText,
          timestamp: Date.now(),
        };
      }
    } catch (error) {
      return {
        id: "nat_check",
        category: "network",
        label: "NAT Traversal",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * WebRTC support check
   */
  private async checkWebRTCSupport(): Promise<DiagResult> {
    try {
      const hasWebRTC = typeof RTCPeerConnection !== "undefined";

      if (!hasWebRTC) {
        return {
          id: "webrtc_check",
          category: "system",
          label: "WebRTC Support",
          status: "fail",
          details: "WebRTC not available",
          timestamp: Date.now(),
        };
      }

      // Check for required WebRTC features
      const hasDataChannel = typeof RTCDataChannel !== "undefined" ||
                            (RTCPeerConnection as any).prototype.createDataChannel !== undefined;

      if (!hasDataChannel) {
        return {
          id: "webrtc_check",
          category: "system",
          label: "WebRTC Support",
          status: "warn",
          details: "WebRTC available but DataChannel support unclear",
          timestamp: Date.now(),
        };
      }

      return {
        id: "webrtc_check",
        category: "system",
        label: "WebRTC Support",
        status: "pass",
        details: "Full WebRTC support available",
        timestamp: Date.now(),
      };
    } catch (error) {
      return {
        id: "webrtc_check",
        category: "system",
        label: "WebRTC Support",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Proxy configuration check
   */
  private async checkProxyConfiguration(): Promise<DiagResult> {
    try {
      const appSettings = get(settings);
      const { ipPrivacyMode, trustedProxyRelays } = appSettings;
      const trustedCount = trustedProxyRelays?.length ?? 0;

      if (ipPrivacyMode === "off") {
        return {
          id: "proxy_check",
          category: "security",
          label: "Proxy Configuration",
          status: "info",
          details: "Privacy mode disabled",
          timestamp: Date.now(),
        };
      }

      if (trustedCount === 0) {
        return {
          id: "proxy_check",
          category: "security",
          label: "Proxy Configuration",
          status: "warn",
          details: "Privacy mode enabled but no trusted relays configured",
          timestamp: Date.now(),
        };
      }

      return {
        id: "proxy_check",
        category: "security",
        label: "Proxy Configuration",
        status: "pass",
        details: `Mode: ${ipPrivacyMode}, ${trustedCount} trusted relay(s)`,
        timestamp: Date.now(),
      };
    } catch (error) {
      return {
        id: "proxy_check",
        category: "security",
        label: "Proxy Configuration",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Encryption capability check
   */
  private async checkEncryptionCapability(): Promise<DiagResult> {
    try {
      // Check for Web Crypto API
      const hasCrypto = typeof crypto !== "undefined" &&
                       typeof crypto.subtle !== "undefined";

      if (!hasCrypto) {
        return {
          id: "encryption_check",
          category: "security",
          label: "Encryption Support",
          status: "fail",
          details: "Web Crypto API not available",
          timestamp: Date.now(),
        };
      }

      // Test basic encryption functionality
      try {
        const testKey = await crypto.subtle.generateKey(
          { name: "AES-GCM", length: 256 },
          true,
          ["encrypt", "decrypt"]
        );

        if (!testKey) {
          throw new Error("Key generation failed");
        }

        return {
          id: "encryption_check",
          category: "security",
          label: "Encryption Support",
          status: "pass",
          details: "AES-256-GCM encryption available",
          timestamp: Date.now(),
        };
      } catch (cryptoError) {
        return {
          id: "encryption_check",
          category: "security",
          label: "Encryption Support",
          status: "warn",
          details: "Web Crypto available but test failed",
          error: cryptoError instanceof Error ? cryptoError.message : String(cryptoError),
          timestamp: Date.now(),
        };
      }
    } catch (error) {
      return {
        id: "encryption_check",
        category: "security",
        label: "Encryption Support",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Bandwidth limits check
   */
  private async checkBandwidthLimits(): Promise<DiagResult> {
    try {
      const appSettings = get(settings);
      const { uploadBandwidth, downloadBandwidth, enableBandwidthScheduling } = appSettings;

      const uploadLimit = uploadBandwidth > 0 ? `${uploadBandwidth} KB/s` : "unlimited";
      const downloadLimit = downloadBandwidth > 0 ? `${downloadBandwidth} KB/s` : "unlimited";

      return {
        id: "bandwidth_check",
        category: "system",
        label: "Bandwidth Limits",
        status: "pass",
        details: `Upload: ${uploadLimit}, Download: ${downloadLimit}${enableBandwidthScheduling ? " (scheduling enabled)" : ""}`,
        timestamp: Date.now(),
      };
    } catch (error) {
      return {
        id: "bandwidth_check",
        category: "system",
        label: "Bandwidth Limits",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Disk space check
   */
  private async checkDiskSpace(): Promise<DiagResult> {
    try {
      if (!this.isTauri) {
        return {
          id: "disk_check",
          category: "storage",
          label: "Disk Space",
          status: "info",
          details: "Skipped in web build",
          timestamp: Date.now(),
        };
      }

      const storagePath = await invoke<string>("get_download_directory");

      try {
        const availableBytes = await invoke<number>("get_disk_space", { path: storagePath });
        const availableGB = (availableBytes / (1024 ** 3)).toFixed(2);

        if (availableBytes < 1024 ** 3) { // Less than 1 GB
          return {
            id: "disk_check",
            category: "storage",
            label: "Disk Space",
            status: "warn",
            details: `${availableGB} GB available (low)`,
            timestamp: Date.now(),
          };
        }

        return {
          id: "disk_check",
          category: "storage",
          label: "Disk Space",
          status: "pass",
          details: `${availableGB} GB available`,
          timestamp: Date.now(),
        };
      } catch (diskError) {
        const errorMsg = diskError instanceof Error ? diskError.message : String(diskError);

        // If directory doesn't exist, check parent directory
        if (errorMsg.includes("No such file") || errorMsg.includes("does not exist")) {
          try {
            // Get parent directory (e.g., /Users/spark/Downloads from /Users/spark/Downloads/Chiral-Network-Storage)
            const parentPath = this.getParentPath(storagePath);
            const availableBytes = await invoke<number>("get_disk_space", { path: parentPath });
            const availableGB = (availableBytes / (1024 ** 3)).toFixed(2);

            if (availableBytes < 1024 ** 3) { // Less than 1 GB
              return {
                id: "disk_check",
                category: "storage",
                label: "Disk Space",
                status: "warn",
                details: `${availableGB} GB available on parent volume (low)`,
                timestamp: Date.now(),
              };
            }

            return {
              id: "disk_check",
              category: "storage",
              label: "Disk Space",
              status: "pass",
              details: `${availableGB} GB available on parent volume`,
              timestamp: Date.now(),
            };
          } catch (parentError) {
            // Parent check also failed
            return {
              id: "disk_check",
              category: "storage",
              label: "Disk Space",
              status: "fail",
              error: `Cannot check disk space: ${parentError instanceof Error ? parentError.message : String(parentError)}`,
              timestamp: Date.now(),
            };
          }
        }

        // Other error
        return {
          id: "disk_check",
          category: "storage",
          label: "Disk Space",
          status: "fail",
          error: errorMsg,
          timestamp: Date.now(),
        };
      }
    } catch (error) {
      return {
        id: "disk_check",
        category: "storage",
        label: "Disk Space",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * i18n storage check
   */
  private async checkI18nStorage(): Promise<DiagResult> {
    try {
      // Test localStorage read/write
      const testKey = "__chiral_diag_test__";
      const testValue = "test_value";

      localStorage.setItem(testKey, testValue);
      const retrieved = localStorage.getItem(testKey);
      localStorage.removeItem(testKey);

      if (retrieved !== testValue) {
        return {
          id: "i18n_check",
          category: "system",
          label: "LocalStorage",
          status: "warn",
          details: "LocalStorage read/write test failed",
          timestamp: Date.now(),
        };
      }

      return {
        id: "i18n_check",
        category: "system",
        label: "LocalStorage",
        status: "pass",
        details: "LocalStorage functional",
        timestamp: Date.now(),
      };
    } catch (error) {
      return {
        id: "i18n_check",
        category: "system",
        label: "LocalStorage",
        status: "fail",
        error: error instanceof Error ? error.message : String(error),
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Format report as text
   */
  formatReport(report: DiagReport): string {
    const lines: string[] = [];

    lines.push(`Chiral Network Diagnostics Report`);
    lines.push(`Generated: ${new Date(report.timestamp).toISOString()}`);
    lines.push(``);
    lines.push(`Summary: ${report.summary.passed} passed, ${report.summary.failed} failed, ${report.summary.warnings} warnings`);
    lines.push(`─`.repeat(80));
    lines.push(``);

    // Group by category
    const categories = ["environment", "network", "storage", "security", "system"] as const;

    categories.forEach((category) => {
      const categoryResults = report.results.filter((r) => r.category === category);

      if (categoryResults.length > 0) {
        lines.push(`${category.toUpperCase()}`);
        lines.push(``);

        categoryResults.forEach((result) => {
          const statusIcon = {
            pass: "[OK]",
            fail: "[X]",
            warn: "[WARN]",
            info: "[INFO]",
          }[result.status];

          lines.push(`  ${statusIcon} ${result.label}: ${result.details || result.error || "no details"}`);
        });

        lines.push(``);
      }
    });

    return lines.join("\n");
  }
}

// Export singleton
export const diagnosticsService = new DiagnosticsService();

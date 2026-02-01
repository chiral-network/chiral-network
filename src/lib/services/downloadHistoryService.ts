/**
 * Download History Service
 *
 * Manages persistent download history with re-download capabilities.
 * Stores completed, failed, and canceled downloads for future reference.
 */

import type { Protocol } from "$lib/services/contentProtocols/types";
import type { ProtocolDetails } from "$lib/types/protocols";
import { writable } from "svelte/store";

export type DownloadHistoryStatus = "completed" | "failed" | "canceled";
export type DownloadPaymentStatus = "not_sent" | "pending" | "completed";

export interface DownloadHistoryInput {
  id?: string;
  hash: string;
  name: string;
  size: number;
  status: DownloadHistoryStatus;
  downloadPath?: string;
  price: number;
  seederAddresses: string;
  paymentStatus?: DownloadPaymentStatus;
  paymentTxHash?: string;
  encrypted?: boolean;
  isEncrypted?: boolean;
  protocol: Protocol;
  protocolDetails?: ProtocolDetails;
  peerId: string;
}

export interface DownloadHistoryEntry {
  id: string;
  hash: string;
  name: string;
  size: number;
  status: DownloadHistoryStatus;
  downloadDate: number; // Timestamp
  downloadPath?: string;
  price: number;
  seederAddresses: string;
  paymentStatus?: DownloadPaymentStatus;
  paymentTxHash?: string;
  encrypted?: boolean;
  protocol: Protocol;
  protocolDetails?: ProtocolDetails;
  peerId: string;
}

const STORAGE_KEY = "chiral.downloadHistory";
const MAX_HISTORY_ENTRIES = 1000; // Limit history to prevent storage bloat

export const downloadHistoryVersion = writable(0);

class DownloadHistoryService {
  private history: DownloadHistoryEntry[] = [];

  constructor() {
    this.loadHistory();
  }

  /**
   * Load history from localStorage
   */
  private loadHistory(): void {
    if (typeof window === "undefined") return;

    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) {
        this.history = JSON.parse(stored);
      }
    } catch (error) {
      console.error("Failed to load download history:", error);
      this.history = [];
    }
  }

  /**
   * Save history to localStorage
   */
  private saveHistory(): void {
    if (typeof window === "undefined") return;

    try {
      // Limit history size
      if (this.history.length > MAX_HISTORY_ENTRIES) {
        this.history = this.history.slice(0, MAX_HISTORY_ENTRIES);
      }

      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.history));
      downloadHistoryVersion.update((version) => version + 1);
    } catch (error) {
      console.error("Failed to save download history:", error);
    }
  }

  /**
   * Add a download to history
   */
  addToHistory(file: DownloadHistoryInput): void {
    const allowedStatuses: DownloadHistoryStatus[] = [
      "completed",
      "failed",
      "canceled",
    ];

    // Only add completed, failed, or canceled downloads
    if (!allowedStatuses.includes(file.status)) {
      return;
    }

    const hash = file.hash;

    // Check if already in history
    const existingIndex = this.history.findIndex(
      (entry) => entry.hash === hash,
    );

    const entry: DownloadHistoryEntry = {
      id: file.id ?? hash,
      hash,
      name: file.name,
      size: file.size,
      status: file.status,
      downloadDate: Date.now(),
      downloadPath: file.downloadPath,
      price: file.price,
      seederAddresses: file.seederAddresses,
      paymentStatus: file.paymentStatus,
      paymentTxHash: file.paymentTxHash,
      // Prioritize isEncrypted if both fields exist, otherwise use encrypted, default to false
      encrypted: file.isEncrypted ?? file.encrypted ?? false,
      protocol: file.protocol,
      protocolDetails: file.protocolDetails,
      peerId: file.peerId,
    };

    if (existingIndex >= 0) {
      // Update existing entry (move to top)
      this.history.splice(existingIndex, 1);
    }

    // Add to beginning (most recent first)
    this.history.unshift(entry);

    this.saveHistory();
  }

  /**
   * Get all history entries
   */
  getHistory(): DownloadHistoryEntry[] {
    return [...this.history];
  }

  /**
   * Update payment status for a history entry
   */
  updatePaymentStatus(
    hash: string,
    status: DownloadPaymentStatus,
    txHash?: string,
  ): void {
    const index = this.history.findIndex((entry) => entry.hash === hash);
    if (index < 0) return;

    const existing = this.history[index];
    this.history[index] = {
      ...existing,
      paymentStatus: status,
      paymentTxHash: txHash ?? existing.paymentTxHash,
    };
    this.saveHistory();
  }

  /**
   * Get history entries with filters
   */
  getFilteredHistory(
    status?: DownloadHistoryStatus,
    searchQuery?: string,
  ): DownloadHistoryEntry[] {
    let filtered = this.history;

    if (status) {
      filtered = filtered.filter((entry) => entry.status === status);
    }

    if (searchQuery && searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      filtered = filtered.filter(
        (entry) =>
          entry.name.toLowerCase().includes(query) ||
          entry.hash.toLowerCase().includes(query),
      );
    }

    return filtered;
  }

  /**
   * Remove a specific entry from history
   */
  removeFromHistory(hash: string): void {
    const index = this.history.findIndex((entry) => entry.hash === hash);
    if (index >= 0) {
      this.history.splice(index, 1);
      this.saveHistory();
    }
  }

  /**
   * Clear all history
   */
  clearHistory(): void {
    this.history = [];
    this.saveHistory();
  }

  /**
   * Clear only failed downloads
   */
  clearFailedDownloads(): void {
    this.history = this.history.filter((entry) => entry.status !== "failed");
    this.saveHistory();
  }

  /**
   * Clear only canceled downloads
   */
  clearCanceledDownloads(): void {
    this.history = this.history.filter((entry) => entry.status !== "canceled");
    this.saveHistory();
  }

  /**
   * Get history statistics
   */
  getStatistics(): {
    total: number;
    completed: number;
    failed: number;
    canceled: number;
    totalSize: number;
    totalPrice: number;
  } {
    return {
      total: this.history.length,
      completed: this.history.filter((e) => e.status === "completed").length,
      failed: this.history.filter((e) => e.status === "failed").length,
      canceled: this.history.filter((e) => e.status === "canceled").length,
      totalSize: this.history.reduce((sum, e) => sum + e.size, 0),
      totalPrice: this.history.reduce((sum, e) => sum + (e.price || 0), 0),
    };
  }

  /**
   * Export history as JSON
   */
  exportHistory(): string {
    return JSON.stringify(
      {
        version: "1.0",
        exportDate: new Date().toISOString(),
        entries: this.history,
      },
      null,
      2,
    );
  }

  /**
   * Import history from JSON
   */
  importHistory(jsonData: string): {
    success: boolean;
    imported: number;
    error?: string;
  } {
    try {
      const data = JSON.parse(jsonData);

      if (!data.entries || !Array.isArray(data.entries)) {
        return {
          success: false,
          imported: 0,
          error: "Invalid format: missing entries array",
        };
      }

      let importedCount = 0;
      let skippedCount = 0;
      let duplicateCount = 0;

      for (const entry of data.entries) {
        // Validate required fields
        if (
          !entry.hash ||
          !entry.name ||
          entry.size === undefined ||
          !entry.status
        ) {
          console.warn("Skipping entry with missing required fields:", entry);
          skippedCount++;
          continue;
        }

        // Validate status is one of allowed values
        if (!["completed", "failed", "canceled"].includes(entry.status)) {
          console.warn("Skipping entry with invalid status:", entry.status);
          skippedCount++;
          continue;
        }

        const existingIndex = this.history.findIndex(
          (e) => e.hash === entry.hash,
        );
        if (existingIndex === -1) {
          this.history.push(entry);
          importedCount++;
        } else {
          duplicateCount++;
        }
      }

      // Only fail if we had entries but ALL of them were invalid (not duplicates)
      if (importedCount === 0 && skippedCount > 0 && duplicateCount === 0) {
        return {
          success: false,
          imported: 0,
          error: "Missing required fields: all entries were invalid",
        };
      }

      this.history.sort((a, b) => b.downloadDate - a.downloadDate);
      this.history = this.history.slice(0, MAX_HISTORY_ENTRIES);
      this.saveHistory();

      return { success: true, imported: importedCount };
    } catch (error) {
      console.error("Failed to import history:", error);
      return {
        success: false,
        imported: 0,
        error: error instanceof Error ? error.message : "Unknown error",
      };
    }
  }
}

// Export singleton instance
export const downloadHistoryService = new DownloadHistoryService();

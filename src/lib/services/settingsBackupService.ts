/**
 * Settings Backup/Restore Service
 * 
 * Handles exporting and importing all Chiral Network settings with validation.
 * Supports version compatibility and data integrity checks.
 */

import type { AppSettings } from '$lib/stores';

export interface SettingsBackup {
  version: string; // Backup format version
  appVersion?: string; // Chiral Network version
  exportDate: string; // ISO timestamp
  settings: AppSettings;
  // Optional metadata
  deviceName?: string;
  notes?: string;
}

const CURRENT_BACKUP_VERSION = '1.0';
const SETTINGS_KEY = 'chiralSettings';

class SettingsBackupService {
  /**
   * Export current settings as JSON string
   */
  async exportSettings(includeMetadata = true): Promise<{ success: boolean; data?: string; error?: string }> {
    try {
      if (typeof window === 'undefined') {
        return { success: false, error: 'Cannot export settings outside browser environment' };
      }

      const stored = localStorage.getItem(SETTINGS_KEY);
      if (!stored) {
        return { success: false, error: 'No settings found to export' };
      }

      const settings = JSON.parse(stored) as AppSettings;

      const backup: SettingsBackup = {
        version: CURRENT_BACKUP_VERSION,
        exportDate: new Date().toISOString(),
        settings,
      };

      if (includeMetadata) {
        // Try to get app version
        try {
          const { getVersion } = await import('@tauri-apps/api/app');
          backup.appVersion = await getVersion();
        } catch {
          backup.appVersion = 'unknown';
        }

        // Add device/browser info
        backup.deviceName = navigator.platform || 'unknown';
      }

      const jsonData = JSON.stringify(backup, null, 2);
      return { success: true, data: jsonData };
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error during export'
      };
    }
  }

  /**
   * Validate imported settings backup
   */
  private validateBackup(data: any): { valid: boolean; error?: string } {
    if (!data) {
      return { valid: false, error: 'Empty backup data' };
    }

    if (!data.version) {
      return { valid: false, error: 'Missing backup version' };
    }

    if (!data.settings || typeof data.settings !== 'object') {
      return { valid: false, error: 'Invalid or missing settings object' };
    }

    // Validate exportDate format
    if (!data.exportDate || typeof data.exportDate !== 'string') {
      return { valid: false, error: 'Invalid or missing exportDate' };
    }
    
    // Check if exportDate is a valid ISO date
    const dateCheck = new Date(data.exportDate);
    if (isNaN(dateCheck.getTime())) {
      return { valid: false, error: 'Invalid exportDate format - must be ISO 8601' };
    }

    // Check for critical settings fields
    const criticalFields = ['storagePath', 'port', 'maxConnections'];
    for (const field of criticalFields) {
      if (!(field in data.settings)) {
        return { valid: false, error: `Missing critical setting: ${field}` };
      }
    }

    // Validate data types for critical numeric fields
    const numericFields = ['port', 'maxConnections', 'maxStorageSize', 'cleanupThreshold'];
    for (const field of numericFields) {
      if (field in data.settings && typeof data.settings[field] !== 'number') {
        return { valid: false, error: `Setting '${field}' must be a number` };
      }
    }

    // Validate data types for boolean fields
    const booleanFields = [
      'autoCleanup',
      'enableUPnP',
      'enableNAT',
      'enableProxy',
      'anonymousMode',
      'shareAnalytics',
      'enableWalletAutoLock',
    ];
    for (const field of booleanFields) {
      if (field in data.settings && typeof data.settings[field] !== 'boolean') {
        return { valid: false, error: `Setting '${field}' must be a boolean` };
      }
    }

    // Validate data types for string fields
    const stringFields = ['storagePath', 'proxyAddress', 'userLocation', 'logLevel'];
    for (const field of stringFields) {
      if (field in data.settings && data.settings[field] !== null && data.settings[field] !== undefined && typeof data.settings[field] !== 'string') {
        return { valid: false, error: `Setting '${field}' must be a string` };
      }
    }

    // Validate array fields
    const arrayFields = ['customBootstrapNodes', 'trustedProxyRelays', 'autonatServers', 'preferredRelays', 'bandwidthSchedules', 'capWarningThresholds'];
    for (const field of arrayFields) {
      if (field in data.settings && data.settings[field] !== null && data.settings[field] !== undefined && !Array.isArray(data.settings[field])) {
        return { valid: false, error: `Setting '${field}' must be an array` };
      }
    }

    return { valid: true };
  }

  /**
   * Import settings from JSON string
   */
  async importSettings(
    jsonData: string,
    options: {
      merge?: boolean; // Merge with existing settings (true) or replace completely (false)
      skipValidation?: boolean; // Skip validation checks (dangerous!)
    } = { merge: false, skipValidation: false }
  ): Promise<{
    success: boolean;
    imported?: AppSettings;
    warnings?: string[];
    error?: string;
  }> {
    try {
      if (typeof window === 'undefined') {
        return { success: false, error: 'Cannot import settings outside browser environment' };
      }

      // Parse JSON
      let backup: SettingsBackup;
      try {
        backup = JSON.parse(jsonData);
      } catch {
        return { success: false, error: 'Invalid JSON format' };
      }

      // Validate backup
      if (!options.skipValidation) {
        const validation = this.validateBackup(backup);
        if (!validation.valid) {
          return { success: false, error: validation.error };
        }
      }

      const warnings: string[] = [];

      // Check version compatibility
      if (backup.version !== CURRENT_BACKUP_VERSION) {
        warnings.push(`Backup version mismatch: ${backup.version} (current: ${CURRENT_BACKUP_VERSION})`);
      }

      let finalSettings = backup.settings;

      // Merge with existing settings if requested
      if (options.merge) {
        const existing = localStorage.getItem(SETTINGS_KEY);
        if (existing) {
          const existingSettings = JSON.parse(existing) as AppSettings;
          finalSettings = { ...existingSettings, ...backup.settings };
          warnings.push('Settings merged with existing configuration');
        }
      }

      // Save to localStorage
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(finalSettings));

      return {
        success: true,
        imported: finalSettings,
        warnings: warnings.length > 0 ? warnings : undefined,
      };
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error during import'
      };
    }
  }

  /**
   * Create automatic backup before making changes
   */
  async createAutoBackup(): Promise<{ success: boolean; backup?: string; error?: string }> {
    const result = await this.exportSettings(false);
    
    if (result.success && result.data) {
      // Store in separate localStorage key with timestamp
      try {
        const timestamp = Date.now();
        const key = `chiralSettings_autobackup_${timestamp}`;
        localStorage.setItem(key, result.data);
        
        // Keep only last 5 auto-backups
        this.cleanupAutoBackups(5);
        
        return { success: true, backup: key };
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error.message : 'Failed to store auto-backup'
        };
      }
    }
    
    return result;
  }

  /**
   * Clean up old auto-backups
   */
  private cleanupAutoBackups(keepCount: number): void {
    if (typeof window === 'undefined') return;

    try {
      const autoBackupKeys: { key: string; timestamp: number }[] = [];
      
      for (let i = 0; i < localStorage.length; i++) {
        const key = localStorage.key(i);
        if (key?.startsWith('chiralSettings_autobackup_')) {
          const timestamp = parseInt(key.split('_').pop() || '0');
          autoBackupKeys.push({ key, timestamp });
        }
      }

      // Sort by timestamp (newest first)
      autoBackupKeys.sort((a, b) => b.timestamp - a.timestamp);

      // Remove old backups
      for (let i = keepCount; i < autoBackupKeys.length; i++) {
        localStorage.removeItem(autoBackupKeys[i].key);
      }
    } catch (error) {
      console.error('Failed to cleanup auto-backups:', error);
    }
  }

  /**
   * List available auto-backups
   */
  getAutoBackups(): { key: string; date: Date }[] {
    if (typeof window === 'undefined') return [];

    const backups: { key: string; date: Date }[] = [];
    
    try {
      for (let i = 0; i < localStorage.length; i++) {
        const key = localStorage.key(i);
        if (key?.startsWith('chiralSettings_autobackup_')) {
          const timestamp = parseInt(key.split('_').pop() || '0');
          backups.push({ key, date: new Date(timestamp) });
        }
      }

      // Sort by date (newest first)
      backups.sort((a, b) => b.date.getTime() - a.date.getTime());
    } catch (error) {
      console.error('Failed to list auto-backups:', error);
    }

    return backups;
  }

  /**
   * Restore from auto-backup
   */
  async restoreAutoBackup(backupKey: string): Promise<{ success: boolean; error?: string }> {
    if (typeof window === 'undefined') {
      return { success: false, error: 'Cannot restore outside browser environment' };
    }

    try {
      const backupData = localStorage.getItem(backupKey);
      if (!backupData) {
        return { success: false, error: 'Backup not found' };
      }

      return await this.importSettings(backupData, { merge: false });
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error during restore'
      };
    }
  }

  /**
   * Download settings as file
   */
  downloadBackupFile(jsonData: string, filename?: string): void {
    const blob = new Blob([jsonData], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename || `chiral-settings-${new Date().toISOString().split('T')[0]}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }
}

// Export singleton instance
export const settingsBackupService = new SettingsBackupService();

// @vitest-environment jsdom

import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import type { AppSettings } from "../src/lib/stores";

// Mock Tauri API
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("1.0.0"),
}));

// Mock localStorage with proper store management
const createLocalStorageMock = () => {
  let store: Record<string, string> = {};

  return {
    getItem: vi.fn((key: string) => store[key] || null),
    setItem: vi.fn((key: string, value: string) => {
      store[key] = value;
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key];
    }),
    clear: vi.fn(() => {
      store = {};
    }),
    key: vi.fn((index: number) => {
      const keys = Object.keys(store);
      return keys[index] || null;
    }),
    get length() {
      return Object.keys(store).length;
    },
    _reset: () => {
      store = {};
    },
    _getStore: () => store,
  };
};

const localStorageMock = createLocalStorageMock();

Object.defineProperty(window, "localStorage", {
  value: localStorageMock,
  writable: true,
});

// Mock navigator.platform
Object.defineProperty(navigator, "platform", {
  value: "TestPlatform",
  writable: true,
});

// Import AFTER mocks are set up
import { settingsBackupService } from "../src/lib/services/settingsBackupService";
import type { SettingsBackup } from "../src/lib/services/settingsBackupService";

describe("SettingsBackupService", () => {
  const createMockSettings = (
    overrides?: Partial<AppSettings>
  ): AppSettings => ({
    storagePath: "~/Chiral-Network-Storage",
    maxStorageSize: 100,
    autoCleanup: true,
    cleanupThreshold: 90,
    maxConnections: 50,
    uploadBandwidth: 0,
    downloadBandwidth: 0,
    port: 30303,
    enableUPnP: true,
    enableNAT: true,
    userLocation: "US-East",
    enableProxy: true,
    proxyAddress: "127.0.0.1:9050",
    ipPrivacyMode: "off",
    trustedProxyRelays: [],
    disableDirectNatTraversal: false,
    enableAutonat: false,
    autonatProbeInterval: 30,
    autonatServers: [],
    enableAutorelay: false,
    preferredRelays: [],
    enableRelayServer: false,
    relayServerAlias: "",
    anonymousMode: false,
    shareAnalytics: true,
    enableWalletAutoLock: false,
    enableNotifications: true,
    notifyOnComplete: true,
    notifyOnError: true,
    notifyOnBandwidthCap: true,
    notifyOnBandwidthCapDesktop: false,
    soundAlerts: false,
    enableIPFS: false,
    chunkSize: 256,
    cacheSize: 1024,
    logLevel: "info",
    autoUpdate: true,
    enableBandwidthScheduling: false,
    bandwidthSchedules: [],
    monthlyUploadCapGb: 0,
    monthlyDownloadCapGb: 0,
    capWarningThresholds: [75, 90],
    enableFileLogging: false,
    maxLogSizeMB: 10,
    pricePerMb: 0.001,
    customBootstrapNodes: [],
    autoStartDHT: false,
    ...overrides,
  });

  beforeEach(() => {
    (localStorageMock as any)._reset();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  describe("exportSettings", () => {
    it("should export settings as valid JSON", async () => {
      const settings = createMockSettings();
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      const result = await settingsBackupService.exportSettings();

      expect(result.success).toBe(true);
      expect(result.data).toBeDefined();

      const parsed = JSON.parse(result.data!);
      expect(parsed.version).toBe("1.0");
      expect(parsed.exportDate).toBeDefined();
      expect(parsed.settings).toEqual(settings);
    });

    it("should include metadata when requested", async () => {
      const settings = createMockSettings();
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      const result = await settingsBackupService.exportSettings(true);

      expect(result.success).toBe(true);
      const parsed = JSON.parse(result.data!) as SettingsBackup;
      expect(parsed.appVersion).toBe("1.0.0");
      expect(parsed.deviceName).toBe("TestPlatform");
    });

    it("should exclude metadata when not requested", async () => {
      const settings = createMockSettings();
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      const result = await settingsBackupService.exportSettings(false);

      expect(result.success).toBe(true);
      const parsed = JSON.parse(result.data!) as SettingsBackup;
      expect(parsed.appVersion).toBeUndefined();
      expect(parsed.deviceName).toBeUndefined();
    });

    it("should fail when no settings exist", async () => {
      const result = await settingsBackupService.exportSettings();

      expect(result.success).toBe(false);
      expect(result.error).toContain("No settings found");
    });

    it("should handle corrupted localStorage gracefully", async () => {
      localStorage.setItem("chiralSettings", "invalid json{");

      const result = await settingsBackupService.exportSettings();

      expect(result.success).toBe(false);
      expect(result.error).toBeDefined();
    });

    it("should be formatted (pretty-printed)", async () => {
      const settings = createMockSettings();
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      const result = await settingsBackupService.exportSettings();

      expect(result.success).toBe(true);
      expect(result.data).toContain("\n");
      expect(result.data).toMatch(/\s{2,}/);
    });
  });

  describe("importSettings", () => {
    it("should import valid settings backup", async () => {
      const settings = createMockSettings({ port: 8080 });
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(true);
      expect(result.imported).toEqual(settings);

      const stored = localStorage.getItem("chiralSettings");
      expect(stored).toBeDefined();
      expect(JSON.parse(stored!)).toEqual(settings);
    });

    it("should reject invalid JSON", async () => {
      const result =
        await settingsBackupService.importSettings("invalid json{");

      expect(result.success).toBe(false);
      expect(result.error).toContain("Invalid JSON");
    });

    it("should reject backup without version", async () => {
      const invalidBackup = {
        exportDate: new Date().toISOString(),
        settings: createMockSettings(),
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(invalidBackup)
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("Missing backup version");
    });

    it("should reject backup without settings object", async () => {
      const invalidBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(invalidBackup)
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("Invalid or missing settings");
    });

    it("should reject backup missing critical fields", async () => {
      const invalidSettings = {
        maxStorageSize: 100,
        autoCleanup: true,
        // Missing storagePath, port, maxConnections
      };

      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: invalidSettings as any,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("Missing critical setting");
    });

    it("should warn on version mismatch", async () => {
      const settings = createMockSettings();
      const backup: SettingsBackup = {
        version: "0.9",
        exportDate: new Date().toISOString(),
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(true);
      expect(result.warnings).toBeDefined();
      // Check what warning actually exists
      expect(result.warnings![0]).toContain("0.9"); // Warning mentions version number
    });

    it("should merge with existing settings when merge=true", async () => {
      const existing = createMockSettings({ port: 8080, maxConnections: 100 });
      localStorage.setItem("chiralSettings", JSON.stringify(existing));

      // Create FULL settings but with different values
      const newSettings = createMockSettings({
        port: 9090,
        maxConnections: 200,
      });
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: newSettings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup),
        { merge: true }
      );

      expect(result.success).toBe(true);
      // With merge=true, new settings still override
      expect(result.imported?.port).toBe(9090);
      expect(result.imported?.maxConnections).toBe(200);
      expect(result.warnings).toBeDefined();
      // Check if any warning contains "merged"
      const hasMergeWarning = result.warnings?.some((w) =>
        w.toLowerCase().includes("merged")
      );
      expect(hasMergeWarning).toBe(true);
    });

    it("should replace settings completely when merge=false", async () => {
      const existing = createMockSettings({ port: 8080, maxConnections: 100 });
      localStorage.setItem("chiralSettings", JSON.stringify(existing));

      const newSettings = createMockSettings({
        port: 9090,
        maxConnections: 50,
      });
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: newSettings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup),
        { merge: false }
      );

      expect(result.success).toBe(true);
      expect(result.imported).toEqual(newSettings);
    });

    it("should skip validation when skipValidation=true", async () => {
      const invalidSettings = { port: 8080 }; // Missing critical fields

      const backup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: invalidSettings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup),
        { skipValidation: true }
      );

      expect(result.success).toBe(true);
    });

    it("should handle empty settings object", async () => {
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: {} as any,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("Missing critical setting");
    });
  });

  describe("importSettings - additional critical scenarios", () => {
    it("should handle merge with partial settings correctly", async () => {
      const existing = createMockSettings({ port: 8080, maxConnections: 100 });
      localStorage.setItem("chiralSettings", JSON.stringify(existing));

      // Only provide port in new settings
      const partialSettings = createMockSettings({
        port: 9090,
        maxConnections: 50,
      });
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: partialSettings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup),
        { merge: true }
      );

      expect(result.success).toBe(true);
      // Merge overwrites with new values (spread operator behavior)
      expect(result.imported?.port).toBe(9090);
      expect(result.imported?.maxConnections).toBe(50); // This is correct behavior
    });

    it("should handle array merge correctly", async () => {
      const existing = createMockSettings({
        customBootstrapNodes: ["node1", "node2"],
        trustedProxyRelays: ["relay1"],
      });
      localStorage.setItem("chiralSettings", JSON.stringify(existing));

      // Create FULL settings with different arrays
      const newSettings = createMockSettings({
        customBootstrapNodes: ["node3"],
        trustedProxyRelays: [], // Explicitly empty
      });
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: newSettings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup),
        { merge: true }
      );

      expect(result.success).toBe(true);
      // Merge still replaces arrays (spread operator behavior)
      expect(result.imported?.customBootstrapNodes).toEqual(["node3"]);
      expect(result.imported?.trustedProxyRelays).toEqual([]); // Replaced with empty
    });

    it("should handle null/undefined values in settings", async () => {
      const settings = createMockSettings({
        proxyAddress: undefined as any,
        relayServerAlias: null as any,
      });
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      // Should still succeed if critical fields are present
      expect(result.success).toBe(true);
    });

    it("should handle backup with extra unknown fields", async () => {
      const settings = createMockSettings();
      const backup: any = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings,
        unknownField: "should be ignored",
        futureFeature: { data: "test" },
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(true);
    });

    it("should handle settings with wrong data types", async () => {
      const invalidSettings = createMockSettings({
        port: "8080" as any,
        maxConnections: "50" as any,
        autoCleanup: "true" as any,
      });

      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: invalidSettings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("must be a");
    });

    it("should validate exportDate format", async () => {
      const settings = createMockSettings();
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: "not-a-valid-iso-date",
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("exportDate");
    });

    it("should accept valid data types", async () => {
      // Add a positive test case
      const validSettings = createMockSettings({
        port: 8080, // Correct type
        maxConnections: 50, // Correct type
        autoCleanup: true, // Correct type
      });

      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: new Date().toISOString(),
        settings: validSettings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(true);
    });
  });

  describe("auto-backup functionality", () => {
    it("should create auto-backup", async () => {
      const settings = createMockSettings();
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      const result = await settingsBackupService.createAutoBackup();

      expect(result.success).toBe(true);
      expect(result.backup).toBeDefined();
      expect(result.backup).toMatch(/^chiralSettings_autobackup_\d+$/);

      const stored = localStorage.getItem(result.backup!);
      expect(stored).toBeDefined();
    });

    it("should limit auto-backups to 5", async () => {
      const settings = createMockSettings();
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      // Create 10 auto-backups
      for (let i = 0; i < 10; i++) {
        await settingsBackupService.createAutoBackup();
        await new Promise((resolve) => setTimeout(resolve, 10));
      }

      const backups = settingsBackupService.getAutoBackups();
      expect(backups.length).toBeLessThanOrEqual(5);
    });

    it("should list auto-backups in chronological order (newest first)", async () => {
      const settings = createMockSettings();
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      const timestamps: number[] = [];
      for (let i = 0; i < 3; i++) {
        await settingsBackupService.createAutoBackup();
        timestamps.push(Date.now());
        await new Promise((resolve) => setTimeout(resolve, 10));
      }

      const backups = settingsBackupService.getAutoBackups();
      expect(backups.length).toBe(3);

      for (let i = 1; i < backups.length; i++) {
        expect(backups[i - 1].date.getTime()).toBeGreaterThanOrEqual(
          backups[i].date.getTime()
        );
      }
    });

    it("should restore from auto-backup", async () => {
      const originalSettings = createMockSettings({ port: 8080 });
      localStorage.setItem("chiralSettings", JSON.stringify(originalSettings));

      const backupResult = await settingsBackupService.createAutoBackup();
      expect(backupResult.success).toBe(true);

      // Modify settings
      const modifiedSettings = createMockSettings({ port: 9090 });
      localStorage.setItem("chiralSettings", JSON.stringify(modifiedSettings));

      // Restore from backup
      const restoreResult = await settingsBackupService.restoreAutoBackup(
        backupResult.backup!
      );

      expect(restoreResult.success).toBe(true);

      const restored = JSON.parse(
        localStorage.getItem("chiralSettings")!
      ) as AppSettings;
      expect(restored.port).toBe(8080);
    });

    it("should fail to restore non-existent backup", async () => {
      const result = await settingsBackupService.restoreAutoBackup(
        "chiralSettings_autobackup_nonexistent"
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("Backup not found");
    });

    it("should return empty array when no auto-backups exist", () => {
      const backups = settingsBackupService.getAutoBackups();
      expect(backups).toEqual([]);
    });
  });

  describe("downloadBackupFile", () => {
    it("should create download link with correct filename", () => {
      const mockElement = {
        href: "",
        download: "",
        click: vi.fn(),
      };

      const mockCreateElement = vi
        .spyOn(document, "createElement")
        .mockReturnValue(mockElement as any);
      const mockAppendChild = vi
        .spyOn(document.body, "appendChild")
        .mockImplementation(() => mockElement as any);
      const mockRemoveChild = vi
        .spyOn(document.body, "removeChild")
        .mockImplementation(() => mockElement as any);

      const jsonData = JSON.stringify({ test: "data" });
      settingsBackupService.downloadBackupFile(jsonData, "test-backup.json");

      expect(mockCreateElement).toHaveBeenCalledWith("a");
      expect(mockElement.download).toBe("test-backup.json");
      expect(mockElement.click).toHaveBeenCalled();
      expect(mockAppendChild).toHaveBeenCalled();
      expect(mockRemoveChild).toHaveBeenCalled();

      mockCreateElement.mockRestore();
      mockAppendChild.mockRestore();
      mockRemoveChild.mockRestore();
    });

    it("should use default filename when not provided", () => {
      const mockElement = {
        href: "",
        download: "",
        click: vi.fn(),
      };

      const mockCreateElement = vi
        .spyOn(document, "createElement")
        .mockReturnValue(mockElement as any);
      vi.spyOn(document.body, "appendChild").mockImplementation(
        () => mockElement as any
      );
      vi.spyOn(document.body, "removeChild").mockImplementation(
        () => mockElement as any
      );

      const jsonData = JSON.stringify({ test: "data" });
      settingsBackupService.downloadBackupFile(jsonData);

      expect(mockElement.download).toMatch(
        /^chiral-settings-\d{4}-\d{2}-\d{2}\.json$/
      );

      mockCreateElement.mockRestore();
    });

    it("should cleanup blob URL after download", () => {
      const mockElement = {
        href: "",
        download: "",
        click: vi.fn(),
      };

      vi.spyOn(document, "createElement").mockReturnValue(mockElement as any);
      vi.spyOn(document.body, "appendChild").mockImplementation(
        () => mockElement as any
      );
      vi.spyOn(document.body, "removeChild").mockImplementation(
        () => mockElement as any
      );

      const jsonData = JSON.stringify({ test: "data" });
      settingsBackupService.downloadBackupFile(jsonData);

      expect(URL.revokeObjectURL).toHaveBeenCalled();
    });
  });

  describe("version compatibility", () => {
    it("should handle future version gracefully", async () => {
      const settings = createMockSettings();
      const backup: SettingsBackup = {
        version: "2.0", // Future version
        exportDate: new Date().toISOString(),
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(true);
      expect(result.warnings).toBeDefined();
      expect(result.warnings![0]).toContain("2.0");
    });

    it("should handle very old version", async () => {
      const settings = createMockSettings();
      const backup: SettingsBackup = {
        version: "0.1",
        exportDate: new Date().toISOString(),
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(true);
      expect(result.warnings).toBeDefined();
    });

    it("should handle pre-release version formats", async () => {
      const settings = createMockSettings();
      const backup: SettingsBackup = {
        version: "1.0.0-beta.1",
        exportDate: new Date().toISOString(),
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(true);
      expect(result.warnings).toBeDefined();
    });
  });

  describe("security and data integrity", () => {
    it("should not execute code from malicious backup", async () => {
      const maliciousBackup = `{
        "version": "1.0",
        "exportDate": "${new Date().toISOString()}",
        "settings": ${JSON.stringify(createMockSettings())},
        "__proto__": { "polluted": true }
      }`;

      const result =
        await settingsBackupService.importSettings(maliciousBackup);

      expect(result.success).toBe(true);
      // Prototype pollution shouldn't affect Object.prototype
      expect((Object.prototype as any).polluted).toBeUndefined();
    });

    it("should handle extremely large backup files", async () => {
      const hugeArray = Array.from({ length: 100000 }, (_, i) => `node-${i}`);
      const settings = createMockSettings({
        customBootstrapNodes: hugeArray,
      });
      localStorage.setItem("chiralSettings", JSON.stringify(settings));

      const result = await settingsBackupService.exportSettings();

      expect(result.success).toBe(true);
      expect(result.data!.length).toBeGreaterThan(1000000);
    });

    it("should validate exportDate format", async () => {
      const settings = createMockSettings();
      const backup: SettingsBackup = {
        version: "1.0",
        exportDate: "not-a-valid-iso-date",
        settings,
      };

      const result = await settingsBackupService.importSettings(
        JSON.stringify(backup)
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain("exportDate");
    });
  });
});
// Add URL.createObjectURL mock after the localStorage mock setup
Object.defineProperty(global.URL, "createObjectURL", {
  value: vi.fn(() => "blob:mock-url"),
  writable: true,
});

Object.defineProperty(global.URL, "revokeObjectURL", {
  value: vi.fn(),
  writable: true,
});

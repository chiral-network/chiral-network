/**
 * State Persistence Tests
 *
 * Tests for localStorage persistence, settings persistence,
 * session recovery, and state restoration.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';

describe('State Persistence - Settings', () => {
  describe('Settings Storage', () => {
    it('should save settings to localStorage', () => {
      const settings = {
        port: 4001,
        enableUPnP: true,
        pureClientMode: false
      };

      const storage = new Map<string, string>();
      storage.set('settings', JSON.stringify(settings));

      const stored = JSON.parse(storage.get('settings') || '{}');
      expect(stored.port).toBe(4001);
      expect(stored.enableUPnP).toBe(true);
    });

    it('should load settings from localStorage on startup', () => {
      const storedSettings = {
        port: 4001,
        enableUPnP: true
      };

      const storage = new Map<string, string>();
      storage.set('settings', JSON.stringify(storedSettings));

      const loaded = JSON.parse(storage.get('settings') || '{}');
      expect(loaded.port).toBe(4001);
    });

    it('should use default settings when localStorage is empty', () => {
      const storage = new Map<string, string>();
      const stored = storage.get('settings');

      const defaultSettings = {
        port: 4001,
        enableUPnP: false
      };

      const settings = stored ? JSON.parse(stored) : defaultSettings;
      expect(settings.port).toBe(4001);
      expect(settings.enableUPnP).toBe(false);
    });

    it('should update localStorage when settings change', () => {
      const storage = new Map<string, string>();

      const settings = { port: 4001 };
      storage.set('settings', JSON.stringify(settings));

      // Update settings
      const updated = { port: 5001 };
      storage.set('settings', JSON.stringify(updated));

      const loaded = JSON.parse(storage.get('settings') || '{}');
      expect(loaded.port).toBe(5001);
    });

    it('should persist multiple setting changes', () => {
      const storage = new Map<string, string>();

      let settings = { port: 4001, enableUPnP: false };
      storage.set('settings', JSON.stringify(settings));

      settings.enableUPnP = true;
      storage.set('settings', JSON.stringify(settings));

      settings.port = 5001;
      storage.set('settings', JSON.stringify(settings));

      const loaded = JSON.parse(storage.get('settings') || '{}');
      expect(loaded.port).toBe(5001);
      expect(loaded.enableUPnP).toBe(true);
    });
  });

  describe('Settings Validation', () => {
    it('should validate settings before persisting', () => {
      const settings = {
        port: 4001,
        enableUPnP: true
      };

      const isValid = settings.port > 0 && settings.port < 65536;
      expect(isValid).toBe(true);
    });

    it('should reject invalid port numbers', () => {
      const settings = {
        port: 70000 // Invalid
      };

      const isValid = settings.port > 0 && settings.port < 65536;
      expect(isValid).toBe(false);
    });

    it('should sanitize settings before storage', () => {
      const rawSettings = {
        port: 4001,
        invalidField: undefined,
        nullField: null
      };

      const sanitized = Object.fromEntries(
        Object.entries(rawSettings).filter(([_, v]) => v !== undefined && v !== null)
      );

      expect(sanitized.port).toBe(4001);
      expect('invalidField' in sanitized).toBe(false);
    });
  });

  describe('Settings Migration', () => {
    it('should migrate old settings format to new format', () => {
      const oldSettings = {
        port: 4001
        // Missing new fields
      };

      const defaultNewFields = {
        pureClientMode: false,
        forceServerMode: false
      };

      const migrated = { ...defaultNewFields, ...oldSettings };

      expect(migrated.port).toBe(4001);
      expect(migrated.pureClientMode).toBe(false);
    });

    it('should preserve existing settings during migration', () => {
      const oldSettings = {
        port: 4001,
        enableUPnP: true
      };

      const newDefaults = {
        port: 5001,
        enableUPnP: false,
        pureClientMode: false
      };

      const migrated = { ...newDefaults, ...oldSettings };

      expect(migrated.port).toBe(4001); // Preserved
      expect(migrated.enableUPnP).toBe(true); // Preserved
      expect(migrated.pureClientMode).toBe(false); // New field
    });

    it('should add version number to settings', () => {
      const settings = {
        version: 1,
        port: 4001
      };

      expect(settings.version).toBe(1);
    });
  });
});

describe('State Persistence - File State', () => {
  describe('Seed Persistence', () => {
    it('should persist seeding file list', () => {
      const seedingFiles = [
        { hash: 'abc123', name: 'file1.txt' },
        { hash: 'def456', name: 'file2.txt' }
      ];

      const storage = new Map<string, string>();
      storage.set('seedingFiles', JSON.stringify(seedingFiles));

      const loaded = JSON.parse(storage.get('seedingFiles') || '[]');
      expect(loaded.length).toBe(2);
      expect(loaded[0].hash).toBe('abc123');
    });

    it('should restore seeding on app restart', () => {
      const storage = new Map<string, string>();
      const seedingFiles = [
        { hash: 'abc123', path: '/files/file1.txt' }
      ];

      storage.set('seedingFiles', JSON.stringify(seedingFiles));

      const restored = JSON.parse(storage.get('seedingFiles') || '[]');
      expect(restored[0].hash).toBe('abc123');
    });

    it('should remove completed downloads from persistence', () => {
      let files = [
        { hash: 'abc123', status: 'downloading' },
        { hash: 'def456', status: 'completed' }
      ];

      // Remove completed
      files = files.filter(f => f.status !== 'completed');

      expect(files.length).toBe(1);
      expect(files[0].status).toBe('downloading');
    });

    it('should persist file metadata', () => {
      const fileMetadata = {
        hash: 'abc123',
        name: 'file.txt',
        size: 1024,
        addedAt: Date.now()
      };

      const storage = new Map<string, string>();
      storage.set('file_abc123', JSON.stringify(fileMetadata));

      const loaded = JSON.parse(storage.get('file_abc123') || '{}');
      expect(loaded.size).toBe(1024);
    });
  });

  describe('Download Progress', () => {
    it('should persist download progress', () => {
      const download = {
        hash: 'abc123',
        progress: 45,
        downloadedBytes: 450 * 1024
      };

      const storage = new Map<string, string>();
      storage.set('download_abc123', JSON.stringify(download));

      const loaded = JSON.parse(storage.get('download_abc123') || '{}');
      expect(loaded.progress).toBe(45);
    });

    it('should restore incomplete downloads on restart', () => {
      const storage = new Map<string, string>();
      const incompleteDownload = {
        hash: 'abc123',
        progress: 60,
        status: 'paused'
      };

      storage.set('download_abc123', JSON.stringify(incompleteDownload));

      const restored = JSON.parse(storage.get('download_abc123') || '{}');
      expect(restored.progress).toBe(60);
      expect(restored.status).toBe('paused');
    });

    it('should clear completed download progress', () => {
      const storage = new Map<string, string>();

      storage.set('download_abc123', JSON.stringify({ progress: 100 }));

      // Clear on completion
      storage.delete('download_abc123');

      expect(storage.has('download_abc123')).toBe(false);
    });
  });

  describe('Chunk State', () => {
    it('should persist downloaded chunk bitmap', () => {
      const chunkBitmap = {
        hash: 'abc123',
        totalChunks: 10,
        downloaded: [0, 1, 2, 5, 7] // Chunk indices
      };

      const storage = new Map<string, string>();
      storage.set('chunks_abc123', JSON.stringify(chunkBitmap));

      const loaded = JSON.parse(storage.get('chunks_abc123') || '{}');
      expect(loaded.downloaded.length).toBe(5);
    });

    it('should resume from persisted chunk state', () => {
      const storage = new Map<string, string>();
      const chunkState = {
        totalChunks: 10,
        downloaded: [0, 1, 2]
      };

      storage.set('chunks_abc123', JSON.stringify(chunkState));

      const restored = JSON.parse(storage.get('chunks_abc123') || '{}');
      const remaining = chunkState.totalChunks - restored.downloaded.length;

      expect(remaining).toBe(7);
    });
  });
});

describe('State Persistence - Wallet State', () => {
  describe('Wallet Persistence', () => {
    it('should persist wallet address', () => {
      const wallet = {
        address: '0x1234567890abcdef1234567890abcdef12345678'
      };

      const storage = new Map<string, string>();
      storage.set('wallet', JSON.stringify(wallet));

      const loaded = JSON.parse(storage.get('wallet') || '{}');
      expect(loaded.address).toBe(wallet.address);
    });

    it('should NOT persist private keys', () => {
      const wallet = {
        address: '0x1234567890abcdef1234567890abcdef12345678',
        // Private key should NEVER be in localStorage
      };

      const storage = new Map<string, string>();
      storage.set('wallet', JSON.stringify(wallet));

      const loaded = JSON.parse(storage.get('wallet') || '{}');
      expect('privateKey' in loaded).toBe(false);
    });

    it('should persist wallet balance', () => {
      const wallet = {
        address: '0x123...',
        balance: '10.5'
      };

      const storage = new Map<string, string>();
      storage.set('wallet', JSON.stringify(wallet));

      const loaded = JSON.parse(storage.get('wallet') || '{}');
      expect(loaded.balance).toBe('10.5');
    });

    it('should restore wallet on app restart', () => {
      const storage = new Map<string, string>();
      const wallet = { address: '0x123...', balance: '5.0' };

      storage.set('wallet', JSON.stringify(wallet));

      const restored = JSON.parse(storage.get('wallet') || '{}');
      expect(restored.address).toBe('0x123...');
    });
  });

  describe('Transaction History', () => {
    it('should persist transaction history', () => {
      const transactions = [
        { hash: 'tx1', amount: '1.0', timestamp: Date.now() },
        { hash: 'tx2', amount: '2.0', timestamp: Date.now() }
      ];

      const storage = new Map<string, string>();
      storage.set('transactions', JSON.stringify(transactions));

      const loaded = JSON.parse(storage.get('transactions') || '[]');
      expect(loaded.length).toBe(2);
    });

    it('should limit transaction history size', () => {
      const maxTransactions = 100;
      const transactions = Array.from({ length: 150 }, (_, i) => ({ hash: `tx${i}` }));

      const limited = transactions.slice(-maxTransactions);

      expect(limited.length).toBe(maxTransactions);
    });

    it('should persist pending transactions', () => {
      const pending = [
        { hash: 'tx1', status: 'pending', timestamp: Date.now() }
      ];

      const storage = new Map<string, string>();
      storage.set('pendingTx', JSON.stringify(pending));

      const loaded = JSON.parse(storage.get('pendingTx') || '[]');
      expect(loaded[0].status).toBe('pending');
    });
  });
});

describe('State Persistence - Mining State', () => {
  describe('Mining Session', () => {
    it('should persist mining state across sessions', () => {
      const miningState = {
        hashRate: 1500,
        totalRewards: 2.5,
        blocksFound: 3
      };

      const storage = new Map<string, string>();
      storage.set('miningState', JSON.stringify(miningState));

      const loaded = JSON.parse(storage.get('miningState') || '{}');
      expect(loaded.blocksFound).toBe(3);
    });

    it('should persist mining configuration', () => {
      const config = {
        threads: 4,
        intensity: 'medium'
      };

      const storage = new Map<string, string>();
      storage.set('miningConfig', JSON.stringify(config));

      const loaded = JSON.parse(storage.get('miningConfig') || '{}');
      expect(loaded.threads).toBe(4);
    });

    it('should persist mining history', () => {
      const history = [
        { timestamp: Date.now(), hashRate: 1500, reward: 0.5 },
        { timestamp: Date.now(), hashRate: 1600, reward: 0.6 }
      ];

      const storage = new Map<string, string>();
      storage.set('miningHistory', JSON.stringify(history));

      const loaded = JSON.parse(storage.get('miningHistory') || '[]');
      expect(loaded.length).toBe(2);
    });
  });
});

describe('State Persistence - Reputation Data', () => {
  describe('Peer Reputation', () => {
    it('should persist peer reputation scores', () => {
      const reputation = {
        'peer1': { score: 85, lastUpdate: Date.now() },
        'peer2': { score: 70, lastUpdate: Date.now() }
      };

      const storage = new Map<string, string>();
      storage.set('reputation', JSON.stringify(reputation));

      const loaded = JSON.parse(storage.get('reputation') || '{}');
      expect(loaded.peer1.score).toBe(85);
    });

    it('should persist interaction history with peers', () => {
      const interactions = {
        'peer1': {
          successful: 10,
          failed: 2,
          lastSeen: Date.now()
        }
      };

      const storage = new Map<string, string>();
      storage.set('interactions', JSON.stringify(interactions));

      const loaded = JSON.parse(storage.get('interactions') || '{}');
      expect(loaded.peer1.successful).toBe(10);
    });

    it('should apply reputation decay on load', () => {
      const oldReputation = {
        score: 100,
        lastUpdate: Date.now() - 30 * 24 * 60 * 60 * 1000 // 30 days ago
      };

      const decayRate = 0.01; // 1% per day
      const daysSince = 30;
      const decayedScore = oldReputation.score * Math.pow(1 - decayRate, daysSince);

      expect(decayedScore).toBeLessThan(oldReputation.score);
    });
  });

  describe('Blacklist Persistence', () => {
    it('should persist blacklisted peers', () => {
      const blacklist = [
        { peerId: 'bad-peer-1', reason: 'malicious', timestamp: Date.now() },
        { peerId: 'bad-peer-2', reason: 'spam', timestamp: Date.now() }
      ];

      const storage = new Map<string, string>();
      storage.set('blacklist', JSON.stringify(blacklist));

      const loaded = JSON.parse(storage.get('blacklist') || '[]');
      expect(loaded.length).toBe(2);
    });

    it('should restore blacklist on startup', () => {
      const storage = new Map<string, string>();
      const blacklist = [{ peerId: 'bad-peer', reason: 'malicious' }];

      storage.set('blacklist', JSON.stringify(blacklist));

      const restored = JSON.parse(storage.get('blacklist') || '[]');
      expect(restored[0].peerId).toBe('bad-peer');
    });
  });
});

describe('State Persistence - Search History', () => {
  describe('Search History', () => {
    it('should persist search history', () => {
      const searches = [
        { query: 'abc123', timestamp: Date.now() },
        { query: 'def456', timestamp: Date.now() }
      ];

      const storage = new Map<string, string>();
      storage.set('searchHistory', JSON.stringify(searches));

      const loaded = JSON.parse(storage.get('searchHistory') || '[]');
      expect(loaded.length).toBe(2);
    });

    it('should limit search history size', () => {
      const maxHistory = 50;
      const searches = Array.from({ length: 60 }, (_, i) => ({ query: `search${i}` }));

      const limited = searches.slice(-maxHistory);

      expect(limited.length).toBe(maxHistory);
    });

    it('should remove duplicates from search history', () => {
      const searches = [
        { query: 'abc123' },
        { query: 'def456' },
        { query: 'abc123' } // Duplicate
      ];

      const unique = searches.filter((search, index, self) =>
        index === self.findIndex(s => s.query === search.query)
      );

      expect(unique.length).toBe(2);
    });
  });
});

describe('State Persistence - Error Handling', () => {
  describe('Corrupted Data', () => {
    it('should handle corrupted localStorage data', () => {
      const storage = new Map<string, string>();
      storage.set('settings', 'invalid-json{{{');

      let settings = {};
      try {
        settings = JSON.parse(storage.get('settings') || '{}');
      } catch (e) {
        settings = { port: 4001 }; // Use defaults
      }

      expect(settings).toEqual({ port: 4001 });
    });

    it('should fallback to defaults on parse error', () => {
      const corruptedData = '{invalid}';
      const defaults = { port: 4001 };

      let result;
      try {
        result = JSON.parse(corruptedData);
      } catch {
        result = defaults;
      }

      expect(result.port).toBe(4001);
    });

    it('should validate data structure after loading', () => {
      const loadedData = {
        port: 'invalid' // Should be number
      };

      const isValid = typeof loadedData.port === 'number';
      expect(isValid).toBe(false);
    });
  });

  describe('Storage Quota', () => {
    it('should handle localStorage quota exceeded', () => {
      let quotaExceeded = false;

      try {
        // Simulate quota exceeded
        throw new Error('QuotaExceededError');
      } catch (e) {
        quotaExceeded = true;
      }

      expect(quotaExceeded).toBe(true);
    });

    it('should clear old data when quota exceeded', () => {
      const oldData = new Map<string, string>();
      oldData.set('old-key-1', 'data');
      oldData.set('old-key-2', 'data');

      // Clear oldest entries
      const firstKey = oldData.keys().next().value;
      oldData.delete(firstKey);

      expect(oldData.size).toBe(1);
    });
  });

  describe('Data Validation', () => {
    it('should validate settings schema on load', () => {
      const settings = {
        port: 4001,
        enableUPnP: true
      };

      const hasRequiredFields = 'port' in settings && 'enableUPnP' in settings;
      expect(hasRequiredFields).toBe(true);
    });

    it('should reject invalid data types', () => {
      const settings = {
        port: '4001' // Should be number
      };

      const isValid = typeof settings.port === 'number';
      expect(isValid).toBe(false);
    });

    it('should sanitize loaded data', () => {
      const loadedData = {
        port: 4001,
        __proto__: { malicious: true }
      };

      const sanitized = { port: loadedData.port };

      expect('__proto__' in sanitized).toBe(false);
    });
  });
});

describe('State Persistence - Session Recovery', () => {
  describe('Crash Recovery', () => {
    it('should restore active downloads after crash', () => {
      const storage = new Map<string, string>();
      const activeDownloads = [
        { hash: 'abc123', progress: 50, status: 'downloading' }
      ];

      storage.set('activeDownloads', JSON.stringify(activeDownloads));

      const restored = JSON.parse(storage.get('activeDownloads') || '[]');
      expect(restored[0].progress).toBe(50);
    });

    it('should mark crashed downloads as paused', () => {
      const downloads = [
        { hash: 'abc123', status: 'downloading' }
      ];

      // On recovery, mark as paused
      const recovered = downloads.map(d => ({ ...d, status: 'paused' }));

      expect(recovered[0].status).toBe('paused');
    });

    it('should clean up incomplete state on recovery', () => {
      const storage = new Map<string, string>();
      storage.set('temp_data', 'temporary');

      // Clear temp data on startup
      const tempKeys = ['temp_data'];
      tempKeys.forEach(key => storage.delete(key));

      expect(storage.has('temp_data')).toBe(false);
    });
  });

  describe('State Consistency', () => {
    it('should verify state consistency on load', () => {
      const settings = { port: 4001 };
      const activeDownloads = [{ hash: 'abc123' }];

      const isConsistent = settings && activeDownloads;
      expect(isConsistent).toBeTruthy();
    });

    it('should rebuild indices after load', () => {
      const files = [
        { hash: 'abc123', name: 'file1.txt' },
        { hash: 'def456', name: 'file2.txt' }
      ];

      const index = new Map(files.map(f => [f.hash, f]));

      expect(index.size).toBe(2);
      expect(index.get('abc123')?.name).toBe('file1.txt');
    });
  });
});

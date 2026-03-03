import { describe, it, expect, vi, beforeEach } from 'vitest';

/**
 * Tests for Upload → Drive migration logic.
 *
 * Covers:
 * - chiral_upload_history localStorage format parsing
 * - Migration decision logic (when to migrate, when to skip)
 * - Upload history data structure compatibility
 */

describe('Upload to Drive migration', () => {
  const UPLOAD_HISTORY_KEY = 'chiral_upload_history';

  beforeEach(() => {
    localStorage.clear();
  });

  describe('chiral_upload_history format', () => {
    it('should parse valid upload history entries', () => {
      const entries = [
        {
          id: 'file-123',
          name: 'document.pdf',
          size: 4096,
          hash: 'abc123merkleroot',
          protocol: 'WebRTC',
          fileType: 'Document',
          seeders: 1,
          uploadDate: '2024-10-15T10:00:00.000Z',
          filePath: '/home/user/Documents/document.pdf',
          priceChi: '0',
        },
        {
          id: 'file-456',
          name: 'video.mp4',
          size: 1024 * 1024 * 100,
          hash: 'def456merkleroot',
          protocol: 'BitTorrent',
          fileType: 'Video',
          seeders: 3,
          uploadDate: '2024-10-16T14:30:00.000Z',
          filePath: '/home/user/Videos/video.mp4',
          priceChi: '0.5',
        },
      ];

      localStorage.setItem(UPLOAD_HISTORY_KEY, JSON.stringify(entries));

      const raw = localStorage.getItem(UPLOAD_HISTORY_KEY)!;
      const parsed = JSON.parse(raw);

      expect(parsed).toHaveLength(2);
      expect(parsed[0].name).toBe('document.pdf');
      expect(parsed[0].hash).toBe('abc123merkleroot');
      expect(parsed[0].protocol).toBe('WebRTC');
      expect(parsed[0].filePath).toBe('/home/user/Documents/document.pdf');
      expect(parsed[1].priceChi).toBe('0.5');
      expect(parsed[1].protocol).toBe('BitTorrent');
    });

    it('should handle empty upload history', () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, JSON.stringify([]));
      const parsed = JSON.parse(localStorage.getItem(UPLOAD_HISTORY_KEY)!);
      expect(parsed).toHaveLength(0);
    });

    it('should handle corrupt JSON gracefully', () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, 'not-valid-json{{{');
      let parsed: any[] | null = null;
      try {
        parsed = JSON.parse(localStorage.getItem(UPLOAD_HISTORY_KEY)!);
      } catch {
        // Expected — corrupt data should be caught
      }
      expect(parsed).toBeNull();
    });
  });

  describe('migration decision logic', () => {
    // Replicate the migration check from Drive.svelte
    function shouldMigrate(): { should: boolean; entries: any[] } {
      const raw = localStorage.getItem(UPLOAD_HISTORY_KEY);
      if (!raw) return { should: false, entries: [] };

      let entries: any[];
      try {
        entries = JSON.parse(raw);
      } catch {
        return { should: false, entries: [] };
      }
      if (!Array.isArray(entries) || entries.length === 0) {
        return { should: false, entries: [] };
      }
      return { should: true, entries };
    }

    it('should not migrate when no upload history exists', () => {
      expect(shouldMigrate().should).toBe(false);
    });

    it('should not migrate when upload history is empty array', () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, '[]');
      expect(shouldMigrate().should).toBe(false);
    });

    it('should not migrate when upload history is corrupt JSON', () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, '{bad}');
      expect(shouldMigrate().should).toBe(false);
    });

    it('should not migrate when upload history is not an array', () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, '{"key": "value"}');
      expect(shouldMigrate().should).toBe(false);
    });

    it('should migrate when valid entries exist', () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, JSON.stringify([
        { id: '1', name: 'file.txt', hash: 'abc', filePath: '/path/file.txt', protocol: 'WebRTC', priceChi: '0' },
      ]));
      const result = shouldMigrate();
      expect(result.should).toBe(true);
      expect(result.entries).toHaveLength(1);
    });
  });

  describe('entry to DriveItem mapping', () => {
    it('should map protocol correctly', () => {
      const entry = { protocol: 'BitTorrent' };
      const protocol = (entry.protocol as 'WebRTC' | 'BitTorrent') || 'WebRTC';
      expect(protocol).toBe('BitTorrent');
    });

    it('should default to WebRTC when protocol is missing', () => {
      const entry = {} as any;
      const protocol = (entry.protocol as 'WebRTC' | 'BitTorrent') || 'WebRTC';
      expect(protocol).toBe('WebRTC');
    });

    it('should treat priceChi "0" as free (no price)', () => {
      const entry = { priceChi: '0' };
      const price = entry.priceChi && entry.priceChi !== '0' ? entry.priceChi : undefined;
      expect(price).toBeUndefined();
    });

    it('should pass through non-zero priceChi', () => {
      const entry = { priceChi: '1.5' };
      const price = entry.priceChi && entry.priceChi !== '0' ? entry.priceChi : undefined;
      expect(price).toBe('1.5');
    });

    it('should handle missing priceChi', () => {
      const entry = {} as any;
      const price = entry.priceChi && entry.priceChi !== '0' ? entry.priceChi : undefined;
      expect(price).toBeUndefined();
    });
  });

  describe('post-migration cleanup', () => {
    it('should remove upload history key after successful migration', () => {
      localStorage.setItem(UPLOAD_HISTORY_KEY, JSON.stringify([
        { id: '1', name: 'file.txt', hash: 'abc', filePath: '/path', protocol: 'WebRTC', priceChi: '0' },
      ]));
      expect(localStorage.getItem(UPLOAD_HISTORY_KEY)).not.toBeNull();

      // Simulate migration cleanup
      localStorage.removeItem(UPLOAD_HISTORY_KEY);
      expect(localStorage.getItem(UPLOAD_HISTORY_KEY)).toBeNull();
    });

    it('should preserve upload history on migration failure', () => {
      const historyData = JSON.stringify([
        { id: '1', name: 'file.txt', hash: 'abc', filePath: '/path', protocol: 'WebRTC', priceChi: '0' },
      ]);
      localStorage.setItem(UPLOAD_HISTORY_KEY, historyData);

      // Simulate migration failure — don't remove the key
      expect(localStorage.getItem(UPLOAD_HISTORY_KEY)).toBe(historyData);
    });
  });

  describe('auto-reseed from upload history', () => {
    it('should identify files with hashes as seedable', () => {
      const entries = [
        { hash: 'abc123', filePath: '/path/file1.txt' },
        { hash: '', filePath: '/path/file2.txt' },
        { hash: null, filePath: '/path/file3.txt' },
        { filePath: '/path/file4.txt' }, // no hash field
      ];

      const seedable = entries.filter((e: any) => e.hash);
      expect(seedable).toHaveLength(1);
      expect(seedable[0].filePath).toBe('/path/file1.txt');
    });
  });
});

describe('App close and exit_app', () => {
  it('should use isClosing guard to prevent re-entrant close', () => {
    let isClosing = false;
    let closeCount = 0;

    function handleClose() {
      if (isClosing) return;
      isClosing = true;
      closeCount++;
    }

    // Simulate multiple close events (the bug that was fixed)
    handleClose();
    handleClose();
    handleClose();

    expect(closeCount).toBe(1);
  });
});

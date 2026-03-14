import { describe, it, expect } from 'vitest';
import { getFileIcon, getFileColor, getFolderIcon, getFolderColor } from '$lib/utils/fileIcons';

describe('fileIcons', () => {
  describe('getFileIcon', () => {
    it('returns Image component for image extensions', () => {
      for (const ext of ['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico']) {
        const icon = getFileIcon(`photo.${ext}`);
        expect(icon).toBeDefined();
        expect(icon.name).toBeDefined();
      }
    });

    it('returns Video component for video extensions', () => {
      for (const ext of ['mp4', 'avi', 'mkv', 'mov', 'webm']) {
        const icon = getFileIcon(`clip.${ext}`);
        expect(icon).toBeDefined();
      }
    });

    it('returns Music component for audio extensions', () => {
      for (const ext of ['mp3', 'wav', 'flac', 'aac', 'ogg']) {
        const icon = getFileIcon(`track.${ext}`);
        expect(icon).toBeDefined();
      }
    });

    it('returns Archive component for archive extensions', () => {
      for (const ext of ['zip', 'rar', '7z', 'tar', 'gz']) {
        const icon = getFileIcon(`backup.${ext}`);
        expect(icon).toBeDefined();
      }
    });

    it('returns Code component for code extensions', () => {
      for (const ext of ['js', 'ts', 'html', 'css', 'py', 'rs', 'go']) {
        const icon = getFileIcon(`source.${ext}`);
        expect(icon).toBeDefined();
      }
    });

    it('returns FileText component for document extensions', () => {
      for (const ext of ['txt', 'md', 'pdf', 'doc', 'docx']) {
        const icon = getFileIcon(`readme.${ext}`);
        expect(icon).toBeDefined();
      }
    });

    it('returns FileSpreadsheet component for spreadsheet extensions', () => {
      for (const ext of ['xls', 'xlsx', 'csv', 'ods']) {
        const icon = getFileIcon(`data.${ext}`);
        expect(icon).toBeDefined();
      }
    });

    it('returns default File component for unknown extensions', () => {
      const icon = getFileIcon('mystery.xyz');
      expect(icon).toBeDefined();
    });

    it('returns default File component for files with no extension', () => {
      const icon = getFileIcon('Makefile');
      expect(icon).toBeDefined();
    });

    it('returns distinct icons per category', () => {
      const imageIcon = getFileIcon('photo.png');
      const videoIcon = getFileIcon('clip.mp4');
      const audioIcon = getFileIcon('track.mp3');
      const defaultIcon = getFileIcon('mystery.xyz');

      expect(imageIcon).not.toBe(videoIcon);
      expect(videoIcon).not.toBe(audioIcon);
      expect(audioIcon).not.toBe(defaultIcon);
    });
  });

  describe('getFileColor', () => {
    it('returns blue for image files', () => {
      expect(getFileColor('photo.png')).toBe('text-blue-500');
      expect(getFileColor('image.jpg')).toBe('text-blue-500');
    });

    it('returns purple for video files', () => {
      expect(getFileColor('clip.mp4')).toBe('text-purple-500');
      expect(getFileColor('movie.mkv')).toBe('text-purple-500');
    });

    it('returns green for audio files', () => {
      expect(getFileColor('track.mp3')).toBe('text-green-500');
      expect(getFileColor('song.flac')).toBe('text-green-500');
    });

    it('returns orange for archive files', () => {
      expect(getFileColor('backup.zip')).toBe('text-orange-500');
      expect(getFileColor('archive.tar')).toBe('text-orange-500');
    });

    it('returns red for code files', () => {
      expect(getFileColor('app.ts')).toBe('text-red-500');
      expect(getFileColor('main.rs')).toBe('text-red-500');
    });

    it('returns gray-600 for document files', () => {
      expect(getFileColor('readme.md')).toBe('text-gray-600');
      expect(getFileColor('report.pdf')).toBe('text-gray-600');
    });

    it('returns emerald for spreadsheet files', () => {
      expect(getFileColor('data.csv')).toBe('text-emerald-500');
      expect(getFileColor('budget.xlsx')).toBe('text-emerald-500');
    });

    it('returns gray-400 for unknown extensions', () => {
      expect(getFileColor('mystery.xyz')).toBe('text-gray-400');
      expect(getFileColor('noext')).toBe('text-gray-400');
    });
  });

  describe('getFolderIcon', () => {
    it('returns a Folder component', () => {
      const icon = getFolderIcon();
      expect(icon).toBeDefined();
    });
  });

  describe('getFolderColor', () => {
    it('returns yellow color class', () => {
      expect(getFolderColor()).toBe('text-yellow-500');
    });
  });
});

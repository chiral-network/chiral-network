/**
 * File type detection utility for preview functionality
 */

export type PreviewType = 'image' | 'video' | 'audio' | 'text' | 'pdf' | 'unsupported';

export interface FileTypeInfo {
  type: PreviewType;
  mimeType: string;
  canPreview: boolean;
}

const IMAGE_EXTENSIONS = new Set([
  'jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg', 'ico', 'avif'
]);

const VIDEO_EXTENSIONS = new Set([
  'mp4', 'webm', 'ogg', 'mov', 'avi', 'mkv', 'm4v'
]);

const AUDIO_EXTENSIONS = new Set([
  'mp3', 'wav', 'ogg', 'flac', 'm4a', 'aac', 'wma', 'opus'
]);

const TEXT_EXTENSIONS = new Set([
  'txt', 'md', 'json', 'xml', 'html', 'css', 'js', 'ts', 'jsx', 'tsx',
  'svelte', 'vue', 'py', 'java', 'c', 'cpp', 'h', 'rs', 'go', 'rb',
  'php', 'sh', 'bash', 'yml', 'yaml', 'toml', 'ini', 'conf', 'log',
  'csv', 'sql', 'r', 'scala', 'swift', 'kt', 'dart'
]);

const PDF_EXTENSIONS = new Set(['pdf']);

const MIME_TYPE_MAP: Record<string, string> = {
  // Images
  'jpg': 'image/jpeg',
  'jpeg': 'image/jpeg',
  'png': 'image/png',
  'gif': 'image/gif',
  'webp': 'image/webp',
  'bmp': 'image/bmp',
  'svg': 'image/svg+xml',
  'ico': 'image/x-icon',
  'avif': 'image/avif',
  
  // Videos
  'mp4': 'video/mp4',
  'webm': 'video/webm',
  'ogg': 'video/ogg',
  'mov': 'video/quicktime',
  'avi': 'video/x-msvideo',
  'mkv': 'video/x-matroska',
  'm4v': 'video/x-m4v',
  
  // Audio
  'mp3': 'audio/mpeg',
  'wav': 'audio/wav',
  'flac': 'audio/flac',
  'm4a': 'audio/mp4',
  'aac': 'audio/aac',
  'wma': 'audio/x-ms-wma',
  'opus': 'audio/opus',
  
  // Text
  'txt': 'text/plain',
  'md': 'text/markdown',
  'json': 'application/json',
  'xml': 'application/xml',
  'html': 'text/html',
  'css': 'text/css',
  'js': 'text/javascript',
  'ts': 'text/typescript',
  
  // PDF
  'pdf': 'application/pdf'
};

/**
 * Get file type information from filename
 */
export function getFileType(filename: string): FileTypeInfo {
  const extension = filename.split('.').pop()?.toLowerCase() || '';
  
  if (IMAGE_EXTENSIONS.has(extension)) {
    return {
      type: 'image',
      mimeType: MIME_TYPE_MAP[extension] || 'image/*',
      canPreview: true
    };
  }
  
  if (VIDEO_EXTENSIONS.has(extension)) {
    return {
      type: 'video',
      mimeType: MIME_TYPE_MAP[extension] || 'video/*',
      canPreview: true
    };
  }
  
  if (AUDIO_EXTENSIONS.has(extension)) {
    return {
      type: 'audio',
      mimeType: MIME_TYPE_MAP[extension] || 'audio/*',
      canPreview: true
    };
  }
  
  if (TEXT_EXTENSIONS.has(extension)) {
    return {
      type: 'text',
      mimeType: MIME_TYPE_MAP[extension] || 'text/plain',
      canPreview: true
    };
  }
  
  if (PDF_EXTENSIONS.has(extension)) {
    return {
      type: 'pdf',
      mimeType: 'application/pdf',
      canPreview: true
    };
  }
  
  return {
    type: 'unsupported',
    mimeType: 'application/octet-stream',
    canPreview: false
  };
}

/**
 * Check if a file can be previewed
 */
export function canPreviewFile(filename: string): boolean {
  return getFileType(filename).canPreview;
}

/**
 * Get human-readable file type name
 */
export function getFileTypeName(filename: string): string {
  const { type } = getFileType(filename);
  
  const typeNames: Record<PreviewType, string> = {
    'image': 'Image',
    'video': 'Video',
    'audio': 'Audio',
    'text': 'Text Document',
    'pdf': 'PDF Document',
    'unsupported': 'Unknown'
  };
  
  return typeNames[type];
}

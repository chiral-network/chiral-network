import {
  File as FileIcon,
  Image,
  Video,
  Music,
  Archive,
  Code,
  FileText,
  FileSpreadsheet,
  Folder
} from 'lucide-svelte';

export function getFileIcon(fileName: string) {
  const ext = fileName.split('.').pop()?.toLowerCase() || '';

  if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return Image;
  if (['mp4', 'avi', 'mkv', 'mov', 'wmv', 'webm', 'flv', 'm4v'].includes(ext)) return Video;
  if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma'].includes(ext)) return Music;
  if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return Archive;
  if (['js', 'ts', 'html', 'css', 'py', 'java', 'cpp', 'c', 'php', 'rb', 'go', 'rs'].includes(ext)) return Code;
  if (['txt', 'md', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return FileText;
  if (['xls', 'xlsx', 'csv', 'ods'].includes(ext)) return FileSpreadsheet;

  return FileIcon;
}

export function getFileColor(fileName: string): string {
  const ext = fileName.split('.').pop()?.toLowerCase() || '';

  if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return 'text-blue-500';
  if (['mp4', 'avi', 'mkv', 'mov', 'wmv', 'webm', 'flv', 'm4v'].includes(ext)) return 'text-purple-500';
  if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma'].includes(ext)) return 'text-green-500';
  if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return 'text-orange-500';
  if (['js', 'ts', 'html', 'css', 'py', 'java', 'cpp', 'c', 'php', 'rb', 'go', 'rs'].includes(ext)) return 'text-red-500';
  if (['txt', 'md', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return 'text-gray-600';
  if (['xls', 'xlsx', 'csv', 'ods'].includes(ext)) return 'text-emerald-500';

  return 'text-gray-400';
}

export function getFolderIcon() {
  return Folder;
}

export function getFolderColor(): string {
  return 'text-yellow-500';
}

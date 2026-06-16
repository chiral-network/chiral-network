export type DiagnosticsLogLevel = 'info' | 'warn' | 'error' | 'debug';

export interface DiagnosticsLogEntry {
  id: number;
  timestamp: Date;
  level: DiagnosticsLogLevel;
  source: string;
  message: string;
}

export const VISIBLE_DIAGNOSTICS_LOG_LIMIT = 500;
export const EXPORT_DIAGNOSTICS_LOG_LIMIT = 5_000;

function trimToLimit<T>(entries: T[], limit: number): T[] {
  if (limit <= 0) return [];
  return entries.slice(-limit);
}

export function appendDiagnosticsLogEntry(
  visibleEntries: DiagnosticsLogEntry[],
  historyEntries: DiagnosticsLogEntry[],
  entry: DiagnosticsLogEntry,
  limits: {
    visibleLimit?: number;
    historyLimit?: number;
  } = {}
): { visibleEntries: DiagnosticsLogEntry[]; historyEntries: DiagnosticsLogEntry[] } {
  const visibleLimit = limits.visibleLimit ?? VISIBLE_DIAGNOSTICS_LOG_LIMIT;
  const historyLimit = limits.historyLimit ?? EXPORT_DIAGNOSTICS_LOG_LIMIT;

  return {
    visibleEntries: trimToLimit([...visibleEntries, entry], visibleLimit),
    historyEntries: trimToLimit([...historyEntries, entry], historyLimit),
  };
}

export function formatDiagnosticsLogEntry(entry: DiagnosticsLogEntry): string {
  return `[${entry.timestamp.toISOString()}] [${entry.level.toUpperCase()}] [${entry.source}] ${entry.message}`;
}

export function formatDiagnosticsLogEntries(entries: DiagnosticsLogEntry[]): string {
  return entries.map(formatDiagnosticsLogEntry).join('\n');
}

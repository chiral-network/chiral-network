import { describe, expect, it } from 'vitest';
import {
  appendDiagnosticsLogEntry,
  formatDiagnosticsLogEntries,
  type DiagnosticsLogEntry,
} from '$lib/diagnosticsLogHistory';

function makeEntry(id: number): DiagnosticsLogEntry {
  return {
    id,
    timestamp: new Date(`2026-01-01T00:00:${String(id % 60).padStart(2, '0')}Z`),
    level: id % 2 === 0 ? 'info' : 'warn',
    source: id % 2 === 0 ? 'system' : 'dht',
    message: `event-${id}`,
  };
}

describe('diagnostics log history', () => {
  it('keeps visible logs bounded while preserving a larger export history', () => {
    let visibleEntries: DiagnosticsLogEntry[] = [];
    let historyEntries: DiagnosticsLogEntry[] = [];

    for (let id = 0; id < 6; id += 1) {
      const next = appendDiagnosticsLogEntry(visibleEntries, historyEntries, makeEntry(id), {
        visibleLimit: 3,
        historyLimit: 10,
      });
      visibleEntries = next.visibleEntries;
      historyEntries = next.historyEntries;
    }

    expect(visibleEntries.map((entry) => entry.id)).toEqual([3, 4, 5]);
    expect(historyEntries.map((entry) => entry.id)).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it('trims export history only after the larger history limit is exceeded', () => {
    let visibleEntries: DiagnosticsLogEntry[] = [];
    let historyEntries: DiagnosticsLogEntry[] = [];

    for (let id = 0; id < 8; id += 1) {
      const next = appendDiagnosticsLogEntry(visibleEntries, historyEntries, makeEntry(id), {
        visibleLimit: 3,
        historyLimit: 5,
      });
      visibleEntries = next.visibleEntries;
      historyEntries = next.historyEntries;
    }

    expect(visibleEntries.map((entry) => entry.id)).toEqual([5, 6, 7]);
    expect(historyEntries.map((entry) => entry.id)).toEqual([3, 4, 5, 6, 7]);
  });

  it('formats export history entries that are no longer visible', () => {
    const visibleOnly = [makeEntry(4), makeEntry(5)];
    const exportHistory = [makeEntry(0), makeEntry(1), ...visibleOnly];

    const visibleText = formatDiagnosticsLogEntries(visibleOnly);
    const exportText = formatDiagnosticsLogEntries(exportHistory);

    expect(visibleText).not.toContain('event-0');
    expect(exportText).toContain('[2026-01-01T00:00:00.000Z] [INFO] [system] event-0');
    expect(exportText).toContain('[2026-01-01T00:00:01.000Z] [WARN] [dht] event-1');
  });
});

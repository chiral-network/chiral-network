// Centralized logging service for Chiral Network
// - Structured logs with context
// - In-memory ring buffer with configurable size
// - Register external sinks (eg. native log file, remote error tracker)

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';
export type LogContext = {
  component?: string;
  userId?: string;
  peerId?: string;
  fileHash?: string;
  operation?: string;
  [key: string]: any;
};

export interface LogEntry {
  timestamp: string;
  level: LogLevel;
  message: string;
  context?: LogContext;
  error?: Error | null;
}

type LogSink = (entry: LogEntry) => void;

class Logger {
  private isDevelopment = import.meta.env.DEV;
  private logs: LogEntry[] = [];
  private maxLogs = 1000; // Keep last N logs in memory
  private sinks: Map<string, LogSink> = new Map();

  // Create a structured log entry
  private createEntry(level: LogLevel, message: string, context?: LogContext, error?: Error | null): LogEntry {
    return {
      timestamp: new Date().toISOString(),
      level,
      message,
      context,
      error: error ?? null
    };
  }

  // Add to buffer and notify sinks
  private addLog(entry: LogEntry) {
    this.logs.push(entry);
    if (this.logs.length > this.maxLogs) {
      this.logs.shift();
    }
    // Notify sinks asynchronously to avoid blocking
    for (const sink of Array.from(this.sinks.values())) {
      try {
        // call sinks but don't await
        void sink(entry);
      } catch (e) {
        // swallow sink errors
        // eslint-disable-next-line no-console
        console.warn('Log sink error', e);
      }
    }
  }

  private formatMessage(entry: LogEntry): string {
    const { timestamp, level, message, context } = entry;
    const contextStr = context ? ` [${Object.entries(context).map(([k, v]) => `${k}=${v}`).join(', ')}]` : '';
    return `[${timestamp}] ${level.toUpperCase()}: ${message}${contextStr}`;
  }

  debug(message: string, context?: LogContext) {
    const entry = this.createEntry('debug', message, context, null);
    this.addLog(entry);
    if (this.isDevelopment) {
      // eslint-disable-next-line no-console
      console.debug(this.formatMessage(entry));
    }
  }

  info(message: string, context?: LogContext) {
    const entry = this.createEntry('info', message, context, null);
    this.addLog(entry);
    // eslint-disable-next-line no-console
    console.info(this.formatMessage(entry));
  }

  warn(message: string, context?: LogContext, error?: Error | null) {
    const entry = this.createEntry('warn', message, context, error ?? null);
    this.addLog(entry);
    // eslint-disable-next-line no-console
    console.warn(this.formatMessage(entry), error || '');
  }

  error(message: string, context?: LogContext, error?: Error | null) {
    const entry = this.createEntry('error', message, context, error ?? null);
    this.addLog(entry);
    // eslint-disable-next-line no-console
    console.error(this.formatMessage(entry), error || '');

    // In production, forward to error-tracking/sinks if configured
    // Sinks registered via registerSink will already receive this entry
  }

  /**
   * Return recent logs. By default returns up to maxLogs (the full buffer).
   * Level filters logs at or above the specified level (debug < info < warn < error).
   */
  getRecentLogs(level?: LogLevel, limit?: number): LogEntry[] {
    const effectiveLimit = typeof limit === 'number' ? limit : this.maxLogs;
    let filtered = this.logs;
    if (level) {
      const levels: LogLevel[] = ['debug', 'info', 'warn', 'error'];
      const minLevelIndex = levels.indexOf(level);
      filtered = this.logs.filter(log => levels.indexOf(log.level) >= minLevelIndex);
    }
    return filtered.slice(-effectiveLimit);
  }

  // Clear logs (useful for testing)
  clear() {
    this.logs = [];
  }

  // Adjust the in-memory buffer size
  setMaxLogs(n: number) {
    if (n <= 0) return;
    this.maxLogs = n;
    while (this.logs.length > this.maxLogs) this.logs.shift();
  }

  // Register an external sink. Backwards compatible: if only a function is provided, we generate an id and return it.
  registerSink(sink: LogSink): string {
    const id = `sink-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    this.sinks.set(id, sink);
    return id;
  }

  // Register a sink with a specific id (useful for toggling sinks by feature name)
  registerSinkWithId(id: string, sink: LogSink) {
    if (this.sinks.has(id)) return;
    this.sinks.set(id, sink);
  }

  // Unregister a sink by function reference (backwards compatibility with existing tests)
  unregisterSink(sink: LogSink) {
    for (const [key, val] of this.sinks.entries()) {
      if (val === sink) {
        this.sinks.delete(key);
        return;
      }
    }
  }

  // Unregister a sink by id
  unregisterSinkById(id: string) {
    this.sinks.delete(id);
  }

  // List registered sink ids
  listSinks() {
    return Array.from(this.sinks.keys());
  }

  /**
   * Enable a Tauri file sink which batches log entries and forwards them to the backend.
   * It will attempt to use a Tauri invoke named `append_log_entries` with payload { entries: LogEntry[] }.
   */
  enableTauriFileSink(id = 'tauri-file-sink', batchMs = 2000) {
    if (this.sinks.has(id)) return;

    // Redact helper - shallow redact for common secret keys
    const redact = (obj: any) => {
      if (!obj || typeof obj !== 'object') return obj;
      const REDACT_KEYS = [/pass(word)?/i, /secret/i, /token/i, /mnemonic/i, /privateKey/i, /seed/i, /authorization/i];
      const out: any = Array.isArray(obj) ? [] : {};
      for (const [k, v] of Object.entries(obj)) {
        if (REDACT_KEYS.some(rx => rx.test(k))) {
          out[k] = '[REDACTED]';
        } else if (v && typeof v === 'object') {
          out[k] = redact(v);
        } else {
          out[k] = v;
        }
      }
      return out;
    };

    let buffer: any[] = [];
    let timer: any = null;

    const flush = async () => {
      if (buffer.length === 0) return;
      const toSend = buffer.splice(0, buffer.length);
      try {
        const invokeFn = (globalThis as any).__tauri_invoke ?? (globalThis as any).invoke ?? null;
        if (!invokeFn) return;
        // Send serialized, redacted entries
        const payload = toSend.map((e: LogEntry) => ({ ...this.toSerializable(e), context: redact(e.context) }));
        await invokeFn('append_log_entries', { entries: payload });
      } catch (e) {
        // swallow errors
        // eslint-disable-next-line no-console
        console.warn('Tauri file sink failed', e);
      } finally {
        if (timer) {
          clearTimeout(timer);
          timer = null;
        }
      }
    };

    const sink: LogSink = (entry) => {
      buffer.push(entry);
      if (buffer.length >= 100) {
        void flush();
        return;
      }
      if (!timer) {
        timer = setTimeout(() => void flush(), batchMs) as any;
      }
    };

    this.sinks.set(id, sink);
  }

  disableTauriFileSink(id = 'tauri-file-sink') {
    this.unregisterSinkById(id);
  }

  // Serialize logs to plain JSON-friendly objects
  toSerializable(entry: LogEntry) {
    return {
      timestamp: entry.timestamp,
      level: entry.level,
      message: entry.message,
      context: entry.context,
      error: entry.error ? { name: entry.error.name, message: entry.error.message, stack: entry.error.stack } : undefined
    };
  }
}

// Singleton instance
export const logger = new Logger();

// Convenience functions for common patterns and components
export const log = {
  dht: (message: string, context?: Omit<LogContext, 'component'>) =>
    logger.info(message, { ...context, component: 'DHT' }),

  wallet: (message: string, context?: Omit<LogContext, 'component'>) =>
    logger.info(message, { ...context, component: 'Wallet' }),

  upload: (message: string, context?: Omit<LogContext, 'component'>) =>
    logger.info(message, { ...context, component: 'Upload' }),

  download: (message: string, context?: Omit<LogContext, 'component'>) =>
    logger.info(message, { ...context, component: 'Download' }),

  network: (message: string, context?: Omit<LogContext, 'component'>) =>
    logger.info(message, { ...context, component: 'Network' }),

  error: {
    dht: (message: string, context?: Omit<LogContext, 'component'>, error?: Error | null) =>
      logger.error(message, { ...context, component: 'DHT' }, error ?? null),

    wallet: (message: string, context?: Omit<LogContext, 'component'>, error?: Error | null) =>
      logger.error(message, { ...context, component: 'Wallet' }, error ?? null),

    upload: (message: string, context?: Omit<LogContext, 'component'>, error?: Error | null) =>
      logger.error(message, { ...context, component: 'Upload' }, error ?? null),

    download: (message: string, context?: Omit<LogContext, 'component'>, error?: Error | null) =>
      logger.error(message, { ...context, component: 'Download' }, error ?? null),

    network: (message: string, context?: Omit<LogContext, 'component'>, error?: Error | null) =>
      logger.error(message, { ...context, component: 'Network' }, error ?? null),
  }
};

export default logger;

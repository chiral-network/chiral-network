/**
 * Diagnostic Logging Utility
 * Provides centralized logging for DHT, file operations, and system events
 */

export enum LogLevel {
  DEBUG = 'DEBUG',
  INFO = 'INFO',
  WARN = 'WARN',
  ERROR = 'ERROR'
}

export interface LogEntry {
  timestamp: string;
  level: LogLevel;
  component: string;
  message: string;
  data?: Record<string, unknown>;
}

class DiagnosticLogger {
  private logs: LogEntry[] = [];
  private maxLogs: number = 1000;

  // CSS styles for console.log %c formatting
  private readonly STYLES = {
    [LogLevel.DEBUG]: 'color: #888; font-weight: normal;',
    [LogLevel.INFO]: 'color: #2196F3; font-weight: normal;',
    [LogLevel.WARN]: 'color: #FF9800; font-weight: bold;',
    [LogLevel.ERROR]: 'color: #F44336; font-weight: bold;'
  } as const;

  log(level: LogLevel, component: string, message: string, data?: Record<string, unknown>): void {
    const entry: LogEntry = {
      timestamp: new Date().toISOString(),
      level,
      component,
      message,
      data
    };

    this.logs.push(entry);

    // Keep logs size manageable
    if (this.logs.length > this.maxLogs) {
      this.logs.shift();
    }

    // Console output
    this.printLog(entry);
  }

  debug(component: string, message: string, data?: Record<string, unknown>): void {
    this.log(LogLevel.DEBUG, component, message, data);
  }

  info(component: string, message: string, data?: Record<string, unknown>): void {
    this.log(LogLevel.INFO, component, message, data);
  }

  warn(component: string, message: string, data?: Record<string, unknown>): void {
    this.log(LogLevel.WARN, component, message, data);
  }

  error(component: string, message: string, data?: Record<string, unknown>): void {
    this.log(LogLevel.ERROR, component, message, data);
  }

  private printLog(entry: LogEntry): void {
    const prefix = `[${entry.timestamp}] ${entry.level} [${entry.component}]`;
    const style = this.getStyle(entry.level);

    if (typeof console !== 'undefined') {
      // Use %c placeholder with CSS string as second argument for styled console output
      console.log(`%c${prefix} ${entry.message}`, style, entry.data || '');
    }
  }

  private getStyle(level: LogLevel): string {
    return this.STYLES[level];
  }

  getLogs(component?: string, level?: LogLevel): LogEntry[] {
    return this.logs.filter(log => {
      if (component && log.component !== component) return false;
      if (level && log.level !== level) return false;
      return true;
    });
  }

  getErrorLogs(): LogEntry[] {
    return this.getLogs(undefined, LogLevel.ERROR);
  }

  clearLogs(): void {
    this.logs = [];
  }

  exportLogs(): string {
    return JSON.stringify(this.logs, null, 2);
  }
}

// Singleton instance
export const diagnosticLogger = new DiagnosticLogger();

// DHT-specific logging
export const dhtLogger = {
  peerConnected: (peerId: string, address: string) => {
    diagnosticLogger.info('DHT', `Peer connected: ${peerId}`, { peerId, address });
  },

  peerDisconnected: (peerId: string, reason: string) => {
    diagnosticLogger.info('DHT', `Peer disconnected: ${peerId}`, { peerId, reason });
  },

  searchStarted: (query: string) => {
    diagnosticLogger.debug('DHT_SEARCH', `Searching for: ${query}`, { query });
  },

  searchCompleted: (query: string, results: number, duration: number) => {
    diagnosticLogger.info('DHT_SEARCH', `Search completed: ${query}`, { query, results, duration });
  },

  uploadStarted: (fileName: string, size: number) => {
    diagnosticLogger.info('DHT_UPLOAD', `Upload started: ${fileName}`, { fileName, size });
  },

  uploadCompleted: (fileName: string, hash: string) => {
    diagnosticLogger.info('DHT_UPLOAD', `Upload completed: ${fileName}`, { fileName, hash });
  }
};

// File operations logging
export const fileLogger = {
  uploadQueued: (fileName: string) => {
    diagnosticLogger.debug('FILE_OPS', `File queued for upload: ${fileName}`, { fileName });
  },

  uploadProgress: (fileName: string, progress: number) => {
    diagnosticLogger.debug('FILE_OPS', `Upload progress: ${fileName}`, { fileName, progress });
  },

  downloadStarted: (fileName: string) => {
    diagnosticLogger.info('FILE_OPS', `Download started: ${fileName}`, { fileName });
  },

  downloadCompleted: (fileName: string) => {
    diagnosticLogger.info('FILE_OPS', `Download completed: ${fileName}`, { fileName });
  }
};

// Network status logging
export const networkLogger = {
  statusChanged: (status: 'connected' | 'disconnected', peerCount: number) => {
    diagnosticLogger.info('NETWORK', `Status changed: ${status}`, { status, peerCount });
  },

  connectionAttempt: (targetPeerId: string) => {
    diagnosticLogger.debug('NETWORK', `Attempting connection to: ${targetPeerId}`, { targetPeerId });
  },

  connectionFailed: (targetPeerId: string, reason: string) => {
    diagnosticLogger.warn('NETWORK', `Connection failed: ${targetPeerId}`, { targetPeerId, reason });
  }
};

// Error logging
export const errorLogger = {
  dhtInitError: (error: string) => {
    diagnosticLogger.error('DHT_INIT', `DHT initialization failed: ${error}`, { error });
  },

  fileOperationError: (operation: string, error: string) => {
    diagnosticLogger.error('FILE_OPS', `File operation failed: ${operation}`, { operation, error });
  },

  networkError: (error: string) => {
    diagnosticLogger.error('NETWORK', `Network error: ${error}`, { error });
  }
};

import logger from './logger';

/**
 * Utility functions for debugging and log management
 */
export const debugUtils = {
  /**
   * Export logs as JSON for debugging/support
   */
  exportLogs(level?: 'debug' | 'info' | 'warn' | 'error', limit = 500) {
    const logs = logger.getRecentLogs(level, limit);
    const exportData = {
      timestamp: new Date().toISOString(),
      level: level || 'all',
      count: logs.length,
      logs: logs.map(log => logger.toSerializable(log as any))
    };
    
    return JSON.stringify(exportData, null, 2);
  },

  /**
   * Download logs as a file (for browser environments)
   */
  downloadLogs(filename = 'chiral-logs.json', level?: 'debug' | 'info' | 'warn' | 'error') {
    if (typeof window === 'undefined') return;
    
    const logData = this.exportLogs(level);
    const blob = new Blob([logData], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    
    const link = document.createElement('a');
    link.href = url;
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  },

  /**
   * Get system info for bug reports
   */
  getSystemInfo() {
    if (typeof window === 'undefined') return {};
    
    return {
      userAgent: navigator.userAgent,
      platform: navigator.platform,
      language: navigator.language,
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      timestamp: new Date().toISOString(),
      url: window.location.href,
      isDev: import.meta.env.DEV
    };
  },

  /**
   * Generate a bug report with logs and system info
   */
  generateBugReport(description: string, steps: string[] = []) {
    return {
      description,
      steps,
      systemInfo: this.getSystemInfo(),
      recentLogs: logger.getRecentLogs('warn', 100).map(l => logger.toSerializable(l as any)),
      timestamp: new Date().toISOString()
    };
  }
};

// Export for development/debugging in browser console
if (typeof window !== 'undefined' && import.meta.env.DEV) {
  (window as any).chiralDebug = {
    logger,
    debugUtils,
    exportLogs: debugUtils.exportLogs.bind(debugUtils),
    downloadLogs: debugUtils.downloadLogs.bind(debugUtils),
    generateBugReport: debugUtils.generateBugReport.bind(debugUtils)
  };
}

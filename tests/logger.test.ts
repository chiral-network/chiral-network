import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import logger, { log } from '../src/lib/utils/logger';

describe('Logger Service', () => {
  beforeEach(() => {
    logger.clear();
    logger.setMaxLogs(1000);
  });

  it('should log debug messages in development mode', () => {
    logger.debug('Test debug message', { component: 'Test' });
    const logs = logger.getRecentLogs();
    expect(logs.length).toBe(1);
    expect(logs[0].level).toBe('debug');
    expect(logs[0].message).toBe('Test debug message');
  });

  it('should log errors with context', () => {
    const error = new Error('Test error');
    logger.error('Test error message', { component: 'DHT', operation: 'connect' }, error);
    
    const logs = logger.getRecentLogs();
    expect(logs.length).toBe(1);
    expect(logs[0].level).toBe('error');
    expect(logs[0].context?.component).toBe('DHT');
    expect(logs[0].error).toBe(error);
  });

  it('should filter logs by level', () => {
    logger.debug('Debug message');
    logger.info('Info message');
    logger.warn('Warn message');
    logger.error('Error message');

    const errorLogs = logger.getRecentLogs('error');
    expect(errorLogs.length).toBe(1);
    expect(errorLogs[0].level).toBe('error');
  });

  it('should use convenience log functions and sinks', () => {
    const received: any[] = [];
    const sink = (entry: any) => received.push(entry);
    // registerSink still accepts a function and returns an id
    const id = logger.registerSink(sink);
    expect(typeof id).toBe('string');

    log.dht('DHT connected', { peerId: 'abc123' });
    log.error.upload('Upload failed', { fileHash: 'def456' }, new Error('Network error'));

    const logs = logger.getRecentLogs();
    expect(logs.length).toBe(2);
    expect(logs[0].context?.component).toBe('DHT');
    expect(logs[1].context?.component).toBe('Upload');
    expect(logs[1].level).toBe('error');

    // Sink should have been called twice
    expect(received.length).toBe(2);

    // unregister by function (backwards compatible)
    logger.unregisterSink(sink);

    // register with explicit id and then unregister by id
    const received2: any[] = [];
    const id2 = 'test-sink-id';
    logger.registerSinkWithId(id2, (entry) => received2.push(entry));
    logger.info('Hello');
    expect(received2.length).toBe(1);
    logger.unregisterSinkById(id2);
    logger.info('Hello2');
    expect(received2.length).toBe(1); // no new entries
  });

  it('should maintain log history limit', () => {
    // Add more logs than the limit (1000)
    for (let i = 0; i < 1010; i++) {
      logger.info(`Log message ${i}`);
    }

    const logs = logger.getRecentLogs('info', 1010); // Get more than limit to test
    expect(logs.length).toBe(1000);
    expect(logs[0].message).toBe('Log message 10'); // First 10 should be removed
  });

  it('should serialize logs correctly', () => {
    const err = new Error('Serialize me');
    logger.error('Serialize test', { component: 'Test' }, err);
    const last = logger.getRecentLogs().pop();
    const serial = logger.toSerializable(last as any);
    expect(serial).toHaveProperty('timestamp');
    expect(serial.error?.message).toBe('Serialize me');
  });

  // New tests for sink-by-id and Tauri file sink
  it('registerSink returns id and register/unregister by id works', () => {
    const arr: any[] = [];
    const id = logger.registerSink((e) => arr.push(e));
    expect(typeof id).toBe('string');
    expect(logger.listSinks()).toContain(id);

    logger.unregisterSinkById(id);
    expect(logger.listSinks()).not.toContain(id);
  });

  it('enableTauriFileSink batches and invokes append_log_entries', async () => {
    const mockInvoke = vi.fn(async (_cmd: string, _args?: any) => true);
    (globalThis as any).invoke = mockInvoke;

    // enable with short batch interval for test
    logger.enableTauriFileSink('tauri-file-sink-test', 10);

    try {
      // use fake timers to advance flush
      vi.useFakeTimers();
      log.upload('u1');
      log.upload('u2');

      // advance timers to trigger flush
      await vi.runAllTimersAsync();

      // allow microtasks
      await Promise.resolve();

      expect(mockInvoke).toHaveBeenCalled();
      const called = mockInvoke.mock.calls[0];
      expect(called[0]).toBe('append_log_entries');
      expect(called[1]).toHaveProperty('entries');
      expect(Array.isArray(called[1].entries)).toBe(true);
      expect(called[1].entries.length).toBeGreaterThanOrEqual(2);
    } finally {
      // cleanup
      logger.disableTauriFileSink('tauri-file-sink-test');
      vi.useRealTimers();
      delete (globalThis as any).invoke;
    }
  });
});

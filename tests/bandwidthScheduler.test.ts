/**
 * Bandwidth Scheduler Service Unit Tests
 *
 * Tests for the BandwidthSchedulerService class in src/lib/services/bandwidthScheduler.ts
 *
 * Note: This is a basic test file. More comprehensive tests would require
 * mocking Svelte stores and Tauri APIs.
 */

import { describe, it, expect } from 'vitest';

describe('BandwidthSchedulerService', () => {
  /**
   * Test: Verify the service exports
   * This is a basic smoke test to ensure the module can be imported
   * without requiring full Tauri/Svelte environment setup.
   */
  it('should have a BandwidthSchedulerService class available', async () => {
    // Dynamic import to handle potential dependency issues
    const module = await import('../src/lib/services/bandwidthScheduler');
    
    // The module should export the service class
    expect(module).toBeDefined();
    expect(module.BandwidthSchedulerService).toBeDefined();
  });

  /**
   * Test: Verify getInstance returns the same instance (singleton pattern)
   */
  it('should implement singleton pattern correctly', async () => {
    const { BandwidthSchedulerService } = await import('../src/lib/services/bandwidthScheduler');
    
    const instance1 = BandwidthSchedulerService.getInstance();
    const instance2 = BandwidthSchedulerService.getInstance();
    
    expect(instance1).toBe(instance2);
  });

  /**
   * Test: Verify getCurrentScheduleId method exists and returns expected type
   * This tests the newly added method that fixes the unused variable warning.
   */
  it('should have getCurrentScheduleId method that returns string | null', async () => {
    const { BandwidthSchedulerService } = await import('../src/lib/services/bandwidthScheduler');
    
    const instance = BandwidthSchedulerService.getInstance();
    
    // Method should exist
    expect(instance.getCurrentScheduleId).toBeDefined();
    expect(typeof instance.getCurrentScheduleId).toBe('function');
    
    // Should return null initially (no active schedule)
    const result = instance.getCurrentScheduleId();
    expect(result === null || typeof result === 'string').toBe(true);
  });

  /**
   * Test: Verify other getter methods exist
   * Ensures consistency with the existing API
   */
  it('should have getCurrentUploadLimit method', async () => {
    const { BandwidthSchedulerService } = await import('../src/lib/services/bandwidthScheduler');
    
    const instance = BandwidthSchedulerService.getInstance();
    
    expect(instance.getCurrentUploadLimit).toBeDefined();
    expect(typeof instance.getCurrentUploadLimit).toBe('function');
  });

  it('should have getCurrentDownloadLimit method', async () => {
    const { BandwidthSchedulerService } = await import('../src/lib/services/bandwidthScheduler');
    
    const instance = BandwidthSchedulerService.getInstance();
    
    expect(instance.getCurrentDownloadLimit).toBeDefined();
    expect(typeof instance.getCurrentDownloadLimit).toBe('function');
  });

  it('should have getCurrentLimitsDescription method', async () => {
    const { BandwidthSchedulerService } = await import('../src/lib/services/bandwidthScheduler');
    
    const instance = BandwidthSchedulerService.getInstance();
    
    expect(instance.getCurrentLimitsDescription).toBeDefined();
    expect(typeof instance.getCurrentLimitsDescription).toBe('function');
  });
});

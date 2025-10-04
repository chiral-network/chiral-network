import { get } from 'svelte/store';
import { settings } from '$lib/stores';
import type { BandwidthScheduleEntry } from '$lib/stores';

/**
 * Bandwidth Scheduler Service
 * 
 * This service manages bandwidth scheduling based on time of day and day of week.
 * It checks active schedules and applies appropriate bandwidth limits.
 */
export class BandwidthSchedulerService {
  private static instance: BandwidthSchedulerService | null = null;
  private checkInterval: number | null = null;
  private readonly CHECK_INTERVAL_MS = 60000; // Check every minute
  
  private currentUploadLimit: number = 0;
  private currentDownloadLimit: number = 0;

  private constructor() {}

  static getInstance(): BandwidthSchedulerService {
    if (!BandwidthSchedulerService.instance) {
      BandwidthSchedulerService.instance = new BandwidthSchedulerService();
    }
    return BandwidthSchedulerService.instance;
  }

  /**
   * Start the bandwidth scheduler
   */
  start() {
    if (this.checkInterval !== null) {
      return; // Already running
    }

    // Initial check
    this.checkAndApplySchedule();

    // Set up periodic checking
    this.checkInterval = window.setInterval(() => {
      this.checkAndApplySchedule();
    }, this.CHECK_INTERVAL_MS);

    console.log('Bandwidth scheduler started');
  }

  /**
   * Stop the bandwidth scheduler
   */
  stop() {
    if (this.checkInterval !== null) {
      clearInterval(this.checkInterval);
      this.checkInterval = null;
      console.log('Bandwidth scheduler stopped');
    }
  }

  /**
   * Check current time and apply appropriate bandwidth schedule
   */
  private checkAndApplySchedule() {
    const currentSettings = get(settings);

    // If bandwidth scheduling is disabled, use the default limits
    if (!currentSettings.enableBandwidthScheduling) {
      this.currentUploadLimit = currentSettings.uploadBandwidth;
      this.currentDownloadLimit = currentSettings.downloadBandwidth;
      return;
    }

    const now = new Date();
    const currentTime = this.formatTime(now);
    const currentDay = now.getDay(); // 0-6, where 0 = Sunday

    // Find active schedule
    const activeSchedule = this.findActiveSchedule(
      currentSettings.bandwidthSchedules,
      currentTime,
      currentDay
    );

    if (activeSchedule) {
      // Apply schedule limits
      this.currentUploadLimit = activeSchedule.uploadLimit;
      this.currentDownloadLimit = activeSchedule.downloadLimit;
      
      console.log(`Applied bandwidth schedule "${activeSchedule.name}":`, {
        upload: activeSchedule.uploadLimit === 0 ? 'unlimited' : `${activeSchedule.uploadLimit} KB/s`,
        download: activeSchedule.downloadLimit === 0 ? 'unlimited' : `${activeSchedule.downloadLimit} KB/s`,
      });
    } else {
      // No active schedule, use default settings
      this.currentUploadLimit = currentSettings.uploadBandwidth;
      this.currentDownloadLimit = currentSettings.downloadBandwidth;
    }
  }

  /**
   * Find the active schedule for the current time and day
   */
  private findActiveSchedule(
    schedules: BandwidthScheduleEntry[],
    currentTime: string,
    currentDay: number
  ): BandwidthScheduleEntry | null {
    // Filter to only enabled schedules for today
    const applicableSchedules = schedules.filter(
      (schedule) =>
        schedule.enabled &&
        schedule.daysOfWeek.includes(currentDay)
    );

    // Find schedules where current time falls within the range
    for (const schedule of applicableSchedules) {
      if (this.isTimeInRange(currentTime, schedule.startTime, schedule.endTime)) {
        return schedule;
      }
    }

    return null;
  }

  /**
   * Check if a time falls within a range
   * Handles ranges that cross midnight (e.g., 22:00 to 06:00)
   */
  private isTimeInRange(time: string, start: string, end: string): boolean {
    const timeMinutes = this.timeToMinutes(time);
    const startMinutes = this.timeToMinutes(start);
    const endMinutes = this.timeToMinutes(end);

    if (startMinutes <= endMinutes) {
      // Normal range (doesn't cross midnight)
      return timeMinutes >= startMinutes && timeMinutes < endMinutes;
    } else {
      // Range crosses midnight
      return timeMinutes >= startMinutes || timeMinutes < endMinutes;
    }
  }

  /**
   * Convert time string (HH:MM) to minutes since midnight
   */
  private timeToMinutes(time: string): number {
    const [hours, minutes] = time.split(':').map(Number);
    return hours * 60 + minutes;
  }

  /**
   * Format date to HH:MM
   */
  private formatTime(date: Date): string {
    const hours = String(date.getHours()).padStart(2, '0');
    const minutes = String(date.getMinutes()).padStart(2, '0');
    return `${hours}:${minutes}`;
  }

  /**
   * Get current upload bandwidth limit (KB/s, 0 = unlimited)
   */
  getCurrentUploadLimit(): number {
    return this.currentUploadLimit;
  }

  /**
   * Get current download bandwidth limit (KB/s, 0 = unlimited)
   */
  getCurrentDownloadLimit(): number {
    return this.currentDownloadLimit;
  }

  /**
   * Get human-readable description of current limits
   */
  getCurrentLimitsDescription(): string {
    const upload = this.currentUploadLimit === 0 
      ? 'unlimited' 
      : `${this.currentUploadLimit} KB/s`;
    const download = this.currentDownloadLimit === 0 
      ? 'unlimited' 
      : `${this.currentDownloadLimit} KB/s`;
    
    return `Upload: ${upload}, Download: ${download}`;
  }

  /**
   * Force immediate check and update
   */
  forceUpdate() {
    this.checkAndApplySchedule();
  }
}

// Export singleton instance
export const bandwidthScheduler = BandwidthSchedulerService.getInstance();


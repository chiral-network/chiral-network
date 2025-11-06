import { get } from "svelte/store";
import { settings, activeBandwidthLimits } from "$lib/stores";
import type {
  BandwidthScheduleEntry,
  ActiveBandwidthLimits,
} from "$lib/stores";

type ScheduleMatch = {
  schedule: BandwidthScheduleEntry;
  day: number;
};

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
  private currentScheduleId: string | null = null;

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

    console.log("Bandwidth scheduler started");
  }

  /**
   * Stop the bandwidth scheduler
   */
  stop() {
    if (this.checkInterval !== null) {
      clearInterval(this.checkInterval);
      this.checkInterval = null;
      console.log("Bandwidth scheduler stopped");
    }

    const currentSettings = get(settings);
    this.applyLimits(
      currentSettings.uploadBandwidth,
      currentSettings.downloadBandwidth,
      {
        source: "default",
        nextChangeAt: null,
      }
    );
  }

  /**
   * Check current time and apply appropriate bandwidth schedule
   */
  private checkAndApplySchedule() {
    const currentSettings = get(settings);

    const now = new Date();
    const currentTime = this.formatTime(now);
    const currentDay = now.getDay(); // 0-6, where 0 = Sunday
    const enabledSchedules =
      currentSettings.bandwidthSchedules?.filter((entry) => entry.enabled) ??
      [];

    const activeMatch = currentSettings.enableBandwidthScheduling
      ? this.findActiveSchedule(enabledSchedules, currentTime, currentDay)
      : null;

    const nextChangeAt = this.computeNextChangeTimestamp(
      now,
      enabledSchedules,
      activeMatch
    );

    // Find active schedule
    if (activeMatch) {
      this.applyLimits(
        activeMatch.schedule.uploadLimit,
        activeMatch.schedule.downloadLimit,
        {
          source: "schedule",
          schedule: activeMatch.schedule,
          nextChangeAt,
        }
      );
      return;
    }

    // No active schedule or scheduling disabled: fall back to defaults.
    this.applyLimits(
      currentSettings.uploadBandwidth,
      currentSettings.downloadBandwidth,
      {
        source: "default",
        nextChangeAt,
      }
    );
  }

  /**
   * Find the active schedule for the current time and day
   */
  private findActiveSchedule(
    schedules: BandwidthScheduleEntry[],
    currentTime: string,
    currentDay: number
  ): ScheduleMatch | null {
    // Filter to only enabled schedules for today
    const applicableSchedules = schedules.filter((schedule) =>
      schedule.daysOfWeek.includes(currentDay)
    );

    // Find schedules where current time falls within the range
    for (const schedule of applicableSchedules) {
      if (
        this.isTimeInRange(currentTime, schedule.startTime, schedule.endTime)
      ) {
        return { schedule, day: currentDay };
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
    const [hours, minutes] = time.split(":").map(Number);
    return hours * 60 + minutes;
  }

  /**
   * Format date to HH:MM
   */
  private formatTime(date: Date): string {
    const hours = String(date.getHours()).padStart(2, "0");
    const minutes = String(date.getMinutes()).padStart(2, "0");
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
   * Get current active schedule ID
   * Returns null if no schedule is active (using default limits)
   */
  getCurrentScheduleId(): string | null {
    return this.currentScheduleId;
  }

  /**
   * Get human-readable description of current limits
   */
  getCurrentLimitsDescription(): string {
    const upload =
      this.currentUploadLimit === 0
        ? "unlimited"
        : `${this.currentUploadLimit} KB/s`;
    const download =
      this.currentDownloadLimit === 0
        ? "unlimited"
        : `${this.currentDownloadLimit} KB/s`;

    return `Upload: ${upload}, Download: ${download}`;
  }

  /**
   * Force immediate check and update
   */
  forceUpdate() {
    this.checkAndApplySchedule();
  }

  private applyLimits(
    uploadLimit: number,
    downloadLimit: number,
    meta: {
      source: ActiveBandwidthLimits["source"];
      schedule?: BandwidthScheduleEntry;
      nextChangeAt?: number | null;
    }
  ) {
    this.currentUploadLimit = uploadLimit;
    this.currentDownloadLimit = downloadLimit;
    this.currentScheduleId = meta.schedule?.id ?? null;

    const limits: ActiveBandwidthLimits = {
      uploadLimitKbps: uploadLimit,
      downloadLimitKbps: downloadLimit,
      source: meta.source,
      scheduleId: meta.schedule?.id ?? undefined,
      scheduleName: meta.schedule?.name ?? undefined,
      nextChangeAt: meta.nextChangeAt ?? undefined,
    };

    activeBandwidthLimits.set(limits);

    if (meta.schedule) {
      console.log(`Applied bandwidth schedule "${meta.schedule.name}":`, {
        upload: uploadLimit === 0 ? "unlimited" : `${uploadLimit} KB/s`,
        download: downloadLimit === 0 ? "unlimited" : `${downloadLimit} KB/s`,
        nextChangeAt: meta.nextChangeAt,
      });
    }
  }

  private computeNextChangeTimestamp(
    now: Date,
    schedules: BandwidthScheduleEntry[],
    activeMatch: ScheduleMatch | null
  ): number | null {
    let candidate: number | null = null;

    if (activeMatch) {
      const endTs = this.getActiveOccurrenceEndTimestamp(
        activeMatch.schedule,
        now,
        activeMatch.day
      );
      if (endTs !== null && endTs > now.getTime()) {
        candidate = endTs;
      }
    }

    for (const schedule of schedules) {
      for (const day of schedule.daysOfWeek) {
        const startTs = this.getNextOccurrenceTimestamp(
          schedule.startTime,
          day,
          now
        );
        if (startTs !== null && startTs > now.getTime()) {
          if (candidate === null || startTs < candidate) {
            candidate = startTs;
          }
        }
      }
    }

    return candidate;
  }

  private getNextOccurrenceTimestamp(
    time: string,
    day: number,
    reference: Date
  ): number | null {
    if (time.trim().length === 0) {
      return null;
    }

    const [hours, minutes] = time.split(":").map(Number);
    const occurrence = new Date(reference);
    const diff = (day - reference.getDay() + 7) % 7;
    occurrence.setDate(occurrence.getDate() + diff);
    occurrence.setHours(hours, minutes, 0, 0);

    if (occurrence <= reference) {
      occurrence.setDate(occurrence.getDate() + 7);
    }

    return occurrence.getTime();
  }

  private getActiveOccurrenceEndTimestamp(
    schedule: BandwidthScheduleEntry,
    reference: Date,
    activeDay: number
  ): number | null {
    const durationMinutes = this.getScheduleDurationMinutes(schedule);
    if (durationMinutes <= 0) {
      return null;
    }

    const startTs = this.getMostRecentOccurrenceTimestamp(
      schedule.startTime,
      schedule.daysOfWeek,
      reference,
      activeDay
    );

    if (startTs === null) {
      return null;
    }

    return startTs + durationMinutes * 60 * 1000;
  }

  private getMostRecentOccurrenceTimestamp(
    time: string,
    days: number[],
    reference: Date,
    preferredDay: number
  ): number | null {
    if (time.trim().length === 0 || days.length === 0) {
      return null;
    }

    const [hours, minutes] = time.split(":").map(Number);
    let latest: number | null = null;

    const orderedDays = [
      preferredDay,
      ...days.filter((day) => day !== preferredDay),
    ];

    for (const day of orderedDays) {
      const occurrence = new Date(reference);
      const diff = (reference.getDay() - day + 7) % 7;
      occurrence.setDate(occurrence.getDate() - diff);
      occurrence.setHours(hours, minutes, 0, 0);

      if (occurrence > reference) {
        occurrence.setDate(occurrence.getDate() - 7);
      }

      const timestamp = occurrence.getTime();
      if (timestamp <= reference.getTime()) {
        if (latest === null || timestamp > latest) {
          latest = timestamp;
        }
      }
    }

    return latest;
  }

  private getScheduleDurationMinutes(schedule: BandwidthScheduleEntry): number {
    const startMinutes = this.timeToMinutes(schedule.startTime);
    const endMinutes = this.timeToMinutes(schedule.endTime);

    if (isNaN(startMinutes) || isNaN(endMinutes)) {
      return 0;
    }

    if (startMinutes === endMinutes) {
      // Treat as full-day schedule
      return 24 * 60;
    }

    if (startMinutes < endMinutes) {
      return endMinutes - startMinutes;
    }

    // Crosses midnight
    return 24 * 60 - startMinutes + endMinutes;
  }
}

// Export singleton instance
export const bandwidthScheduler = BandwidthSchedulerService.getInstance();

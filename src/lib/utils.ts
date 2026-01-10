import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";
import { cubicOut } from "svelte/easing";
import type { TransitionConfig } from "svelte/transition";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

type FlyAndScaleParams = {
  y?: number;
  x?: number;
  start?: number;
  duration?: number;
};

export const flyAndScale = (
  node: Element,
  params: FlyAndScaleParams = {}
): TransitionConfig => {
  const style = getComputedStyle(node);
  const transform = style.transform === "none" ? "" : style.transform;

  const scaleConversion = (
    valueA: number,
    scaleA: [number, number],
    scaleB: [number, number]
  ) => {
    const [minA, maxA] = scaleA;
    const [minB, maxB] = scaleB;

    const percentage = (valueA - minA) / (maxA - minA);
    const valueB = percentage * (maxB - minB) + minB;

    return valueB;
  };

  const styleToString = (
    style: Record<string, number | string | undefined>
  ): string => {
    return Object.keys(style).reduce((str, key) => {
      if (style[key] === undefined) return str;
      return str + key + ":" + style[key] + ";";
    }, "");
  };

  return {
    duration: params.duration ?? 200,
    delay: 0,
    css: (t) => {
      const y = scaleConversion(t, [0, 1], [params.y ?? 5, 0]);
      const x = scaleConversion(t, [0, 1], [params.x ?? 0, 0]);
      const scale = scaleConversion(t, [0, 1], [params.start ?? 0.95, 1]);

      return styleToString({
        transform: `${transform} translate3d(${x}px, ${y}px, 0) scale(${scale})`,
        opacity: t,
      });
    },
    easing: cubicOut,
  };
};

const SIZE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

export function toHumanReadableSize(bytes: number, fractionDigits = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }

  let value = bytes;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < SIZE_UNITS.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const decimals = unitIndex === 0 ? 0 : fractionDigits;
  return `${value.toFixed(decimals)} ${SIZE_UNITS[unitIndex]}`;
}

const RELATIVE_FORMATTER = new Intl.RelativeTimeFormat(undefined, {
  numeric: "auto",
});

export function formatRelativeTime(input: Date | number): string {
  // normalize input into a milliseconds timestamp
  let timestampMs: number;

  if (input instanceof Date) {
    timestampMs = input.getTime();
  } else if (typeof input === "number" && Number.isFinite(input)) {
    // Heuristic: if the number looks like seconds (<= 1e12) treat as seconds and convert
    // If it already looks like ms (> 1e12) keep as-is.
    timestampMs = input < 1e12 ? input * 1000 : input;
  } else {
    return RELATIVE_FORMATTER.format(0, "second");
  }

  if (!Number.isFinite(timestampMs)) {
    return RELATIVE_FORMATTER.format(0, "second");
  }

  const now = Date.now();
  let diffMs = timestampMs - now;

  // If diff is absurdly large (e.g. unit confusion), try the alternate scale once
  const hundredYearsMs = 100 * YEAR;
  if (Math.abs(diffMs) > hundredYearsMs) {
    const alt =
      timestampMs < 1e12 ? timestampMs * 1000 : Math.floor(timestampMs / 1000);
    if (Number.isFinite(alt) && Math.abs(alt - now) < Math.abs(diffMs)) {
      diffMs = alt - now;
    }
  }

  const absDiff = Math.abs(diffMs);

  if (absDiff < MINUTE) {
    const value = Math.round(diffMs / SECOND);
    return RELATIVE_FORMATTER.format(value, "second");
  }

  if (absDiff < HOUR) {
    const value = Math.round(diffMs / MINUTE);
    return RELATIVE_FORMATTER.format(value, "minute");
  }

  if (absDiff < DAY) {
    const value = Math.round(diffMs / HOUR);
    return RELATIVE_FORMATTER.format(value, "hour");
  }

  if (absDiff < WEEK) {
    const value = Math.round(diffMs / DAY);
    return RELATIVE_FORMATTER.format(value, "day");
  }

  if (absDiff < MONTH) {
    const value = Math.round(diffMs / WEEK);
    return RELATIVE_FORMATTER.format(value, "week");
  }

  if (absDiff < YEAR) {
    const value = Math.round(diffMs / MONTH);
    return RELATIVE_FORMATTER.format(value, "month");
  }

  const value = Math.round(diffMs / YEAR);
  return RELATIVE_FORMATTER.format(value, "year");
}

/**
 * Format Chiral amounts with consistent decimal places for display
 * @param amount - The Chiral amount to format
 * @param decimals - Number of decimal places (default: 4)
 * @returns Formatted Chiral amount string
 */
export function formatChiral(amount: number, decimals: number = 4): string {
  if (!Number.isFinite(amount)) {
    return "0.0000";
  }
  return amount.toFixed(decimals);
}

/**
 * Format hash strings for display by truncating middle
 * @param hash - The hash string to format
 * @param prefixLen - Number of characters from start (default: 10)
 * @param suffixLen - Number of characters from end (default: 8)
 * @returns Formatted hash like "0x1234abcd...ef567890"
 */
export function formatHash(hash: string | null | undefined, prefixLen: number = 10, suffixLen: number = 8): string {
  if (!hash) return 'N/A';
  if (hash.length <= prefixLen + suffixLen) return hash;
  return `${hash.substring(0, prefixLen)}...${hash.substring(hash.length - suffixLen)}`;
}

/**
 * Format latency in milliseconds
 * @param ms - Latency in milliseconds
 * @param decimals - Number of decimal places (default: 0)
 * @returns Formatted latency like "123 ms"
 */
export function formatLatency(ms: number, decimals: number = 0): string {
  if (!Number.isFinite(ms)) return 'N/A';
  return `${ms.toFixed(decimals)} ms`;
}

/**
 * Format percentage value
 * @param value - Percentage value (0-100)
 * @param decimals - Number of decimal places (default: 1)
 * @returns Formatted percentage like "12.3%"
 */
export function formatPercent(value: number, decimals: number = 1): string {
  if (!Number.isFinite(value)) return '0%';
  return `${value.toFixed(decimals)}%`;
}

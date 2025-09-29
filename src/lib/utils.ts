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
  params: FlyAndScaleParams = {},
): TransitionConfig => {
  const style = getComputedStyle(node);
  const transform = style.transform === "none" ? "" : style.transform;

  const scaleConversion = (
    valueA: number,
    scaleA: [number, number],
    scaleB: [number, number],
  ) => {
    const [minA, maxA] = scaleA;
    const [minB, maxB] = scaleB;

    const percentage = (valueA - minA) / (maxA - minA);
    const valueB = percentage * (maxB - minB) + minB;

    return valueB;
  };

  const styleToString = (
    style: Record<string, number | string | undefined>,
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

export function formatRelativeTime(date: Date): string {
  const timestamp = date instanceof Date ? date.getTime() : Number.NaN;

  if (!Number.isFinite(timestamp)) {
    return RELATIVE_FORMATTER.format(0, "second");
  }

  const now = Date.now();
  const diffMs = timestamp - now;

  if (!Number.isFinite(diffMs)) {
    return RELATIVE_FORMATTER.format(0, "second");
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

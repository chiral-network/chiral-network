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
  const now = Date.now();
  const diffMs = date.getTime() - now;

  const minute = 60 * 1000;
  const hour = 60 * minute;
  const day = 24 * hour;

  if (Math.abs(diffMs) < minute) {
    const value = Math.round(diffMs / 1000);
    return RELATIVE_FORMATTER.format(value, "second");
  }

  if (Math.abs(diffMs) < hour) {
    const value = Math.round(diffMs / minute);
    return RELATIVE_FORMATTER.format(value, "minute");
  }

  if (Math.abs(diffMs) < day) {
    const value = Math.round(diffMs / hour);
    return RELATIVE_FORMATTER.format(value, "hour");
  }

  const value = Math.round(diffMs / day);
  return RELATIVE_FORMATTER.format(value, "day");
}

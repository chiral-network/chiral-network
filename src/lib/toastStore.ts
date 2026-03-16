import { writable } from 'svelte/store';

export interface Toast {
  id: number;
  message: string;
  description?: string;
  type: 'success' | 'error' | 'info' | 'warning';
  duration: number;
  createdAt: number;
}

export type ToastType = Toast['type'];

const DURATIONS: Record<ToastType, number> = {
  success: 4000,
  info: 5000,
  warning: 6000,
  error: 8000,
};

function createToastStore() {
  const { subscribe, update } = writable<Toast[]>([]);
  let nextId = 0;

  return {
    subscribe,
    show: (message: string, type: ToastType = 'info', duration?: number) => {
      const id = nextId++;
      const dur = duration ?? DURATIONS[type];
      const toast: Toast = { id, message, type, duration: dur, createdAt: Date.now() };

      update(toasts => [...toasts, toast]);

      setTimeout(() => {
        update(toasts => toasts.filter(t => t.id !== id));
      }, dur);

      return id;
    },
    /** Show a toast with a title and description body */
    detail: (message: string, description: string, type: ToastType = 'info', duration?: number) => {
      const id = nextId++;
      const dur = duration ?? DURATIONS[type];
      const toast: Toast = { id, message, description, type, duration: dur, createdAt: Date.now() };

      update(toasts => [...toasts, toast]);

      setTimeout(() => {
        update(toasts => toasts.filter(t => t.id !== id));
      }, dur);

      return id;
    },
    remove: (id: number) => {
      update(toasts => toasts.filter(t => t.id !== id));
    }
  };
}

export const toasts = createToastStore();

/** @deprecated Use toasts.show() directly */
export function showToast(message: string, type: ToastType = 'info', duration?: number) {
  toasts.show(message, type, duration);
}

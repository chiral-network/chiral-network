import { writable } from 'svelte/store';

interface Toast {
  id: number;
  message: string;
  type: 'success' | 'error' | 'info' | 'warning';
}

function createToastStore() {
  const { subscribe, update } = writable<Toast[]>([]);
  let nextId = 0;

  return {
    subscribe,
    show: (message: string, type: 'success' | 'error' | 'info' | 'warning' = 'info', duration = 5000) => {
      const id = nextId++;
      const toast: Toast = { id, message, type };

      update(toasts => [...toasts, toast]);

      setTimeout(() => {
        update(toasts => toasts.filter(t => t.id !== id));
      }, duration);
    },
    remove: (id: number) => {
      update(toasts => toasts.filter(t => t.id !== id));
    }
  };
}

export const toasts = createToastStore();

export function showToast(message: string, type: 'success' | 'error' | 'info' | 'warning' = 'info', duration = 5000) {
  toasts.show(message, type === 'warning' ? 'info' : type, duration);
}

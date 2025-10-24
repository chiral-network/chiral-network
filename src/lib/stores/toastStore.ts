import { writable } from 'svelte/store';

export type ToastType = 'success' | 'error' | 'info' | 'warning';

export interface ToastMessage {
  id: string;
  message: string;
  type: ToastType;
  timeout?: number; // Milliseconds before auto-dismiss
}

const createToastStore = () => {
  const { subscribe, update } = writable<ToastMessage[]>([]);

  const add = (message: string, type: ToastType, timeout: number = 3000) => {
    const id = Math.random().toString(36).substring(2, 9); // Simple unique ID
    update(toasts => [...toasts, { id, message, type, timeout }]);
  };

  const remove = (id: string) => {
    update(toasts => toasts.filter(toast => toast.id !== id));
  };

  return {
    subscribe,
    success: (message: string, timeout?: number) => add(message, 'success', timeout),
    error: (message: string, timeout?: number) => add(message, 'error', timeout),
    info: (message: string, timeout?: number) => add(message, 'info', timeout),
    warning: (message: string, timeout?: number) => add(message, 'warning', timeout),
    remove,
  };
};

export const toast = createToastStore();
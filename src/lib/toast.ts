import { writable } from "svelte/store";

interface SimpleToast {
  id: string;
  message: string;
  type: "success" | "error" | "info" | "warning";
}

const toasts = writable<SimpleToast[]>([]);

// Counter to ensure unique IDs even when toasts are added in the same millisecond
let toastCounter = 0;

export function showToast(
  message: string,
  type: "success" | "error" | "info" | "warning" = "success",
  duration: number = 3000
) {
  const id = `${Date.now()}-${toastCounter++}`;

  // Add new notifications, with a maximum of 3 retained
  toasts.update((currentToasts) => [
    ...currentToasts.slice(-2),
    { id, message, type },
  ]);

  // Automatically removed after duration ms
  setTimeout(() => {
    toasts.update((currentToasts) =>
      currentToasts.filter((toast) => toast.id !== id)
    );
  }, duration);
}

export { toasts };

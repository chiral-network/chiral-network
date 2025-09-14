import { writable } from 'svelte/store'

interface SimpleToast {
  id: string
  message: string
  type: 'success' | 'error'
}

const toasts = writable<SimpleToast[]>([])

export function showToast(message: string, type: 'success' | 'error' = 'success') {
  const id = Date.now().toString()
  
  // Add new notifications, with a maximum of 3 retained
  toasts.update(currentToasts => [...currentToasts.slice(-2), { id, message, type }])
  
  // Automatically removed after 3 seconds
  setTimeout(() => {
    toasts.update(currentToasts => currentToasts.filter(toast => toast.id !== id))
  }, 3000)
}

export { toasts }
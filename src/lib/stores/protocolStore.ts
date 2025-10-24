// src/lib/stores/protocolStore.ts
import { writable } from 'svelte/store';

type Protocol = 'WebRTC' | 'Bitswap' | null;

// Check if we're in a browser environment
const isBrowser = typeof window !== 'undefined' && typeof localStorage !== 'undefined';

// Initialize from localStorage if available
const getInitialProtocol = (): Protocol => {
  if (isBrowser) {
    try {
      const stored = localStorage.getItem('selectedProtocol');
      return (stored as Protocol) || null;
    } catch (e) {
      console.warn('Failed to read from localStorage:', e);
      return null;
    }
  }
  return null;
};

function createProtocolStore() {
  const { subscribe, set } = writable<Protocol>(getInitialProtocol());

  return {
    subscribe,
    set: (value: Protocol) => {
      if (isBrowser) {
        try {
          if (value) {
            localStorage.setItem('selectedProtocol', value);
          } else {
            localStorage.removeItem('selectedProtocol');
          }
        } catch (e) {
          console.warn('Failed to write to localStorage:', e);
        }
      }
      set(value);
    },
    reset: () => {
      if (isBrowser) {
        try {
          localStorage.removeItem('selectedProtocol');
        } catch (e) {
          console.warn('Failed to remove from localStorage:', e);
        }
      }
      set(null);
    }
  };
}

export const selectedProtocol = createProtocolStore();
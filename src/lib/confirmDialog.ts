import { writable } from 'svelte/store';

export interface ConfirmDialogOptions {
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  confirmVariant?: 'danger' | 'warning' | 'primary';
  onConfirm?: () => void | Promise<void>;
  onCancel?: () => void;
}

interface ConfirmDialogState extends ConfirmDialogOptions {
  isOpen: boolean;
  isProcessing: boolean;
}

const initialState: ConfirmDialogState = {
  isOpen: false,
  isProcessing: false,
  title: '',
  message: '',
  confirmText: 'Confirm',
  cancelText: 'Cancel',
  confirmVariant: 'primary',
};

function createConfirmDialogStore() {
  const { subscribe, set, update } = writable<ConfirmDialogState>(initialState);

  return {
    subscribe,
    
    /**
     * Show a confirmation dialog and return a promise
     * @param options Dialog configuration
     * @returns Promise that resolves to true if confirmed, false if canceled
     */
    confirm: (options: ConfirmDialogOptions): Promise<boolean> => {
      return new Promise((resolve) => {
        set({
          ...initialState,
          ...options,
          isOpen: true,
          onConfirm: async () => {
            update(state => ({ ...state, isProcessing: true }));
            
            try {
              if (options.onConfirm) {
                await options.onConfirm();
              }
              resolve(true);
            } catch (error) {
              console.error('Confirm action failed:', error);
              resolve(false);
            } finally {
              set(initialState);
            }
          },
          onCancel: () => {
            if (options.onCancel) {
              options.onCancel();
            }
            resolve(false);
            set(initialState);
          },
        });
      });
    },

    /**
     * Close the dialog (equivalent to cancel)
     */
    close: () => {
      update(state => {
        if (state.onCancel) {
          state.onCancel();
        }
        return initialState;
      });
    },

    /**
     * Preset: Confirm file deletion
     */
    confirmDelete: (fileName: string, onConfirm: () => void | Promise<void>) => {
      return createConfirmDialogStore().confirm({
        title: 'Delete File',
        message: `Are you sure you want to delete "${fileName}"? This action cannot be undone.`,
        confirmText: 'Delete',
        cancelText: 'Cancel',
        confirmVariant: 'danger',
        onConfirm,
      });
    },

    /**
     * Preset: Confirm download cancellation
     */
    confirmCancelDownload: (fileName: string, onConfirm: () => void | Promise<void>) => {
      return createConfirmDialogStore().confirm({
        title: 'Cancel Download',
        message: `Are you sure you want to cancel the download of "${fileName}"? Progress will be lost.`,
        confirmText: 'Cancel Download',
        cancelText: 'Keep Downloading',
        confirmVariant: 'warning',
        onConfirm,
      });
    },

    /**
     * Preset: Confirm peer blacklist
     */
    confirmBlacklist: (peerName: string, onConfirm: () => void | Promise<void>) => {
      return createConfirmDialogStore().confirm({
        title: 'Blacklist Peer',
        message: `Are you sure you want to blacklist "${peerName}"? You will no longer download from or upload to this peer.`,
        confirmText: 'Blacklist',
        cancelText: 'Cancel',
        confirmVariant: 'danger',
        onConfirm,
      });
    },
  };
}

export const confirmDialog = createConfirmDialogStore();
export interface WalletBackupEmailRequest {
  email: string;
  recoveryPhrase: string;
  walletAddress: string;
  privateKey: string;
}

function normalizeErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }
  if (typeof error === 'string' && error.trim()) {
    return error.trim();
  }
  return 'Failed to send wallet backup email';
}

export const walletBackupService = {
  async sendBackupEmail(payload: WalletBackupEmailRequest): Promise<boolean> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('send_wallet_backup_email', {
      email: payload.email,
      recoveryPhrase: payload.recoveryPhrase,
      walletAddress: payload.walletAddress,
      privateKey: payload.privateKey,
    });
    return true;
  },

  formatError(error: unknown): string {
    return normalizeErrorMessage(error);
  },
};

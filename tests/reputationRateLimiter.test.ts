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
  it('lowers caps when adaptive caps sees repeated blocks', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    reputationRateLimiter.setConfig({
      ...baseConfig,
      dailyCap: 5,
      perTargetDailyCap: 2,
      mode: 'enforce',
      burst: { count: 100, intervalMs: 60_000, cooldownMs: 1_000 },
      adaptiveCaps: {
        enabled: true,
        windowMs: 10_000,
        blockThreshold: 1,
        cooldownMs: 0,
        dailyStep: 1,
        perTargetStep: 1,
        minDailyCap: 2,
        minPerTargetCap: 1,
        reasons: ['target_daily_cap'],
      },
    });

    const t0 = 50_000;
    const first = buildVerdict('peer-z', t0);
    const d1 = reputationRateLimiter.evaluate(first, t0);
    reputationRateLimiter.recordDecision(first, d1, { sent: d1.allowed, now: t0 });

    const second = buildVerdict('peer-z', t0 + 10);
    const d2 = reputationRateLimiter.evaluate(second, t0 + 10);
    reputationRateLimiter.recordDecision(second, d2, { sent: d2.allowed, now: t0 + 10 });

    const third = buildVerdict('peer-z', t0 + 20);
    const d3 = reputationRateLimiter.evaluate(third, t0 + 20);
    reputationRateLimiter.recordDecision(third, d3, { sent: false, now: t0 + 20 });

    expect(d3.allowed).toBe(false);

    const status = reputationRateLimiter.getStatus('peer-z');
    expect(status.dailyCap).toBe(4);
    expect(status.perTargetCap).toBe(1);
    expect(status.adaptive.reductions).toBe(1);
    expect(status.adaptive.enabled).toBe(true);
  });

  it('does not raise caps automatically after an adaptive reduction', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    reputationRateLimiter.setConfig({
      ...baseConfig,
      dailyCap: 6,
      perTargetDailyCap: 3,
      mode: 'enforce',
      burst: { count: 100, intervalMs: 60_000, cooldownMs: 1_000 },
      adaptiveCaps: {
        enabled: true,
        windowMs: 10_000,
        blockThreshold: 1,
        cooldownMs: 0,
        dailyStep: 2,
        perTargetStep: 1,
        minDailyCap: 2,
        minPerTargetCap: 1,
        reasons: ['daily_cap'],
      },
    });

    const baseTime = 90_000;
    for (let i = 0; i < 6; i++) {
      const ts = baseTime + i * 100;
      const verdict = buildVerdict(`peer-${i}`, ts);
      const decision = reputationRateLimiter.evaluate(verdict, ts);
      reputationRateLimiter.recordDecision(verdict, decision, { sent: decision.allowed, now: ts });
    }

    const cappedVerdict = buildVerdict('peer-over', baseTime + 700);
    const blocked = reputationRateLimiter.evaluate(cappedVerdict, baseTime + 700);
    reputationRateLimiter.recordDecision(cappedVerdict, blocked, { sent: false, now: baseTime + 700 });
    expect(blocked.allowed).toBe(false);
    expect(blocked.reason).toBe('daily_cap');

    const statusAfterDrop = reputationRateLimiter.getStatus();
    expect(statusAfterDrop.dailyCap).toBe(4);

    const later = reputationRateLimiter.getStatus();
    expect(later.dailyCap).toBe(4);
    expect(later.baseDailyCap).toBe(6);
  });

  it('falls back to safe mode on corrupted persisted state', async () => {
    const badStorage = new MemoryStorage();
    (globalThis as any).localStorage = badStorage;
    badStorage.setItem('chiral.reputation.rateLimiter.v1', '{not-json');
    vi.resetModules();
    const mod = (await import('../src/lib/services/reputationRateLimiter')) as LimiterModule;
    const status = mod.reputationRateLimiter.getStatus();
    expect(status.health.safeMode).toBe(true);
    expect(status.mode).toBe('log-only');
  });

  it('recovers from malformed state arrays and reports recovery', async () => {
    const now = Date.now();
    const badStorage = new MemoryStorage();
    (globalThis as any).localStorage = badStorage;
    badStorage.setItem(
      'chiral.reputation.rateLimiter.v1',
      JSON.stringify({
        events: [
          { target: 'old', ts: now - 10 * 24 * 60 * 60 * 1000 },
          { target: 'good', ts: now, outcome: 'good' },
          { target: null, ts: 'oops' },
        ],
        decisions: [{ ts: now, target: 'good', allowed: true, shadowBlocked: false, mode: 'enforce' }],
        burstCooldownUntil: 'not-a-number',
      })
    );
    vi.resetModules();
    const mod = (await import('../src/lib/services/reputationRateLimiter')) as LimiterModule;
    mod.reputationRateLimiter.setConfig({ ...baseConfig, mode: 'enforce' });
    const status = mod.reputationRateLimiter.getStatus('good');
    expect(status.dailyUsed).toBe(1);
    expect(status.health.recoveredFromCorruption).toBe(true);
    expect(status.health.safeMode).toBe(false);
  });

  it('allows a limited grace credit on per-target cap breach', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    reputationRateLimiter.setConfig({
      ...baseConfig,
      dailyCap: 5,
      perTargetDailyCap: 1,
      perTargetCooldownMs: 0,
      mode: 'enforce',
      perTargetGrace: {
        enabled: true,
        creditsPerWindow: 1,
        windowMs: 10_000,
        allowReasons: ['target_daily_cap'],
      },
    });

    const t0 = Date.now();
    const first = buildVerdict('peer-grace', t0);
    const d1 = reputationRateLimiter.evaluate(first, t0);
    reputationRateLimiter.recordDecision(first, d1, { sent: true, now: t0 });
    expect(d1.allowed).toBe(true);

    const second = buildVerdict('peer-grace', t0 + 1);
    const d2 = reputationRateLimiter.evaluate(second, t0 + 1);
    reputationRateLimiter.recordDecision(second, d2, { sent: true, now: t0 + 1 });
    expect(d2.allowed).toBe(true);
    expect(d2.graceApplied).toBe(true);
    expect(d2.graceRemaining).toBe(0);

    const third = buildVerdict('peer-grace', t0 + 2);
    const d3 = reputationRateLimiter.evaluate(third, t0 + 2);
    expect(d3.allowed).toBe(false);
    expect(d3.reason).toBe('target_daily_cap');
  });

  it('resets grace credits after the window', async () => {
    const { reputationRateLimiter } = await loadLimiter();
    reputationRateLimiter.setConfig({
      ...baseConfig,
      dailyCap: 5,
      perTargetDailyCap: 1,
      perTargetCooldownMs: 0,
      mode: 'enforce',
      perTargetGrace: {
        enabled: true,
        creditsPerWindow: 1,
        windowMs: 5_000,
        allowReasons: ['target_daily_cap'],
      },
    });

    const t0 = 1_000_000;
    const first = buildVerdict('peer-reset', t0);
    const d1 = reputationRateLimiter.evaluate(first, t0);
    reputationRateLimiter.recordDecision(first, d1, { sent: true, now: t0 });

    const second = buildVerdict('peer-reset', t0 + 1);
    const d2 = reputationRateLimiter.evaluate(second, t0 + 1);
    reputationRateLimiter.recordDecision(second, d2, { sent: true, now: t0 + 1 });
    expect(d2.graceApplied).toBe(true);

    const afterWindow = t0 + 6_000;
    const third = buildVerdict('peer-reset', afterWindow);
    const d3 = reputationRateLimiter.evaluate(third, afterWindow);
    expect(d3.allowed).toBe(true);
    expect(d3.graceApplied).toBe(true);
  });
});

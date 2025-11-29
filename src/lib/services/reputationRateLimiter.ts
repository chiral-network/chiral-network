import type { TransactionVerdict } from '$lib/types/reputation';

const STORAGE_KEY = 'chiral.reputation.rateLimiter.v1';
const CONFIG_STORAGE_KEY = 'chiral.reputation.rateLimiter.config';
const DAY_MS = 24 * 60 * 60 * 1000;

export type RateLimitMode = 'log-only' | 'enforce';
export type RateLimitReason =
  | 'daily_cap'
  | 'target_cooldown'
  | 'target_daily_cap'
  | 'burst_cooldown';

export interface RateLimitConfig {
  enabled: boolean;
  mode: RateLimitMode;
  dailyCap: number;
  perTargetDailyCap: number;
  perTargetCooldownMs: number;
  burst: {
    count: number;
    intervalMs: number;
    cooldownMs: number;
  };
  maxEventEntries: number;
  maxAuditEntries: number;
  maxWindowMs: number;
}

const DEFAULT_CONFIG: RateLimitConfig = {
  enabled: true,
  mode: 'log-only', // start in shadow (log-only) mode to avoid regressions
  dailyCap: 30,
  perTargetDailyCap: 6,
  perTargetCooldownMs: 90 * 60 * 1000, // 90 minutes between repeats to the same target
  burst: {
    count: 8,
    intervalMs: 5 * 60 * 1000, // 8 verdicts within 5 minutes triggers cooldown
    cooldownMs: 60 * 60 * 1000, // 1h cooldown after a burst
  },
  maxEventEntries: 300,
  maxAuditEntries: 100,
  maxWindowMs: 7 * DAY_MS,
};

export interface RateLimitDecision {
  allowed: boolean;
  shadowBlocked: boolean;
  reason?: RateLimitReason;
  retryAfterMs?: number;
  metrics: {
    enabled: boolean;
    mode: RateLimitMode;
    dailyCount: number;
    dailyCap: number;
    perTargetCount: number;
    perTargetCap: number;
    perTargetCooldownRemainingMs: number;
    burstCount: number;
    burstWindowMs: number;
    burstCooldownRemainingMs: number;
  };
}

export interface RateLimitStatus {
  enabled: boolean;
  mode: RateLimitMode;
  dailyUsed: number;
  dailyCap: number;
  nextDailyResetMs: number;
  perTargetUsed: number;
  perTargetCap: number;
  perTargetCooldownRemainingMs: number;
  burstCount: number;
  burstWindowMs: number;
  burstCooldownRemainingMs: number;
  shadowBlocked: boolean;
  lastDecision?: RateLimitAudit;
}

interface RateLimitEvent {
  target: string;
  ts: number;
  outcome: TransactionVerdict['outcome'];
}

interface RateLimitAudit {
  ts: number;
  target: string;
  allowed: boolean;
  shadowBlocked: boolean;
  reason?: RateLimitReason;
  mode: RateLimitMode;
  retryAfterMs?: number;
}

interface RateLimitState {
  events: RateLimitEvent[];
  burstCooldownUntil?: number;
  decisions: RateLimitAudit[];
  lastDecision?: RateLimitAudit;
}

export class RateLimitError extends Error {
  decision: RateLimitDecision;
  constructor(message: string, decision: RateLimitDecision) {
    super(message);
    this.name = 'RateLimitError';
    this.decision = decision;
  }
}

class ReputationRateLimiter {
  private state: RateLimitState = { events: [], decisions: [] };
  private config: RateLimitConfig = { ...DEFAULT_CONFIG };
  private readonly hasStorage = typeof localStorage !== 'undefined';

  constructor() {
    this.config = this.loadConfig();
    this.state = this.loadState();
    this.prune(Date.now());
  }

  setConfig(partial: Partial<RateLimitConfig>) {
    this.config = { ...this.config, ...partial };
    this.persistConfig();
  }

  evaluate(verdict: TransactionVerdict, now: number = Date.now()): RateLimitDecision {
    this.prune(now);
    const target = verdict.target_id;
    const burstCooldownRemainingMs = Math.max(0, (this.state.burstCooldownUntil ?? 0) - now);

    const dailyEvents = this.state.events.filter((e) => e.ts >= now - DAY_MS);
    const perTargetEvents = target ? dailyEvents.filter((e) => e.target === target) : [];
    const lastTargetEvent = this.findLastEventForTarget(target);
    const perTargetCooldownRemainingMs = lastTargetEvent
      ? Math.max(0, this.config.perTargetCooldownMs - (now - lastTargetEvent.ts))
      : 0;
    const burstEvents = this.state.events.filter((e) => e.ts >= now - this.config.burst.intervalMs);

    const decision: RateLimitDecision = {
      allowed: true,
      shadowBlocked: false,
      metrics: {
        enabled: this.config.enabled,
        mode: this.config.mode,
        dailyCount: dailyEvents.length,
        dailyCap: this.config.dailyCap,
        perTargetCount: perTargetEvents.length,
        perTargetCap: this.config.perTargetDailyCap,
        perTargetCooldownRemainingMs,
        burstCount: burstEvents.length,
        burstWindowMs: this.config.burst.intervalMs,
        burstCooldownRemainingMs,
      },
    };

    if (!this.config.enabled) {
      return decision;
    }

    const blockedState = {
      blocked: false,
      reason: undefined as RateLimitReason | undefined,
      retryAfterMs: undefined as number | undefined,
    };

    if (burstCooldownRemainingMs > 0) {
      blockedState.blocked = true;
      blockedState.reason = 'burst_cooldown';
      blockedState.retryAfterMs = burstCooldownRemainingMs;
    } else if (perTargetCooldownRemainingMs > 0) {
      blockedState.blocked = true;
      blockedState.reason = 'target_cooldown';
      blockedState.retryAfterMs = perTargetCooldownRemainingMs;
    } else if (dailyEvents.length >= this.config.dailyCap) {
      blockedState.blocked = true;
      blockedState.reason = 'daily_cap';
      blockedState.retryAfterMs = this.computeResetMs(dailyEvents, now);
    } else if (perTargetEvents.length >= this.config.perTargetDailyCap) {
      blockedState.blocked = true;
      blockedState.reason = 'target_daily_cap';
      blockedState.retryAfterMs = this.computeResetMs(perTargetEvents, now);
    } else if (burstEvents.length >= this.config.burst.count) {
      blockedState.blocked = true;
      blockedState.reason = 'burst_cooldown';
      blockedState.retryAfterMs = this.config.burst.cooldownMs;
      const until = now + this.config.burst.cooldownMs;
      this.state.burstCooldownUntil = Math.max(this.state.burstCooldownUntil ?? 0, until);
    }

    if (blockedState.blocked) {
      decision.shadowBlocked = true;
      decision.allowed = this.config.mode !== 'enforce';
      decision.reason = blockedState.reason;
      decision.retryAfterMs = blockedState.retryAfterMs;
    }

    this.persistState();
    return decision;
  }

  recordDecision(
    verdict: TransactionVerdict,
    decision: RateLimitDecision,
    opts?: { sent?: boolean; now?: number }
  ) {
    const now = opts?.now ?? Date.now();
    this.prune(now);

    const audit: RateLimitAudit = {
      ts: now,
      target: verdict.target_id,
      allowed: decision.allowed,
      shadowBlocked: decision.shadowBlocked,
      reason: decision.reason,
      mode: this.config.mode,
      retryAfterMs: decision.retryAfterMs,
    };

    this.state.decisions.unshift(audit);
    this.state.decisions = this.state.decisions.slice(0, this.config.maxAuditEntries);
    this.state.lastDecision = audit;

    if (opts?.sent ?? decision.allowed) {
      this.state.events.push({
        target: verdict.target_id,
        ts: now,
        outcome: verdict.outcome ?? 'good',
      });
      if (this.state.events.length > this.config.maxEventEntries) {
        this.state.events = this.state.events.slice(-this.config.maxEventEntries);
      }
    }

    this.persistState();
  }

  getStatus(targetId?: string): RateLimitStatus {
    const now = Date.now();
    this.prune(now);
    const focusTarget = targetId || this.state.lastDecision?.target || '';
    const dailyEvents = this.state.events.filter((e) => e.ts >= now - DAY_MS);
    const perTargetEvents = focusTarget
      ? dailyEvents.filter((e) => e.target === focusTarget)
      : [];
    const lastTargetEvent = focusTarget ? this.findLastEventForTarget(focusTarget) : undefined;
    const earliest = dailyEvents.reduce(
      (min, e) => Math.min(min, e.ts),
      dailyEvents.length ? Number.POSITIVE_INFINITY : now
    );

    const burstCooldownRemainingMs = Math.max(0, (this.state.burstCooldownUntil ?? 0) - now);

    return {
      enabled: this.config.enabled,
      mode: this.config.mode,
      dailyUsed: dailyEvents.length,
      dailyCap: this.config.dailyCap,
      nextDailyResetMs:
        dailyEvents.length === 0 || earliest === Number.POSITIVE_INFINITY
          ? 0
          : Math.max(0, earliest + DAY_MS - now),
      perTargetUsed: perTargetEvents.length,
      perTargetCap: this.config.perTargetDailyCap,
      perTargetCooldownRemainingMs: lastTargetEvent
        ? Math.max(0, this.config.perTargetCooldownMs - (now - lastTargetEvent.ts))
        : 0,
      burstCount: this.state.events.filter((e) => e.ts >= now - this.config.burst.intervalMs)
        .length,
      burstWindowMs: this.config.burst.intervalMs,
      burstCooldownRemainingMs,
      shadowBlocked: !!this.state.lastDecision?.shadowBlocked,
      lastDecision: this.state.lastDecision,
    };
  }

  reset() {
    this.state = { events: [], decisions: [], burstCooldownUntil: undefined, lastDecision: undefined };
    this.persistState();
  }

  private computeResetMs(events: RateLimitEvent[], now: number) {
    if (!events.length) return 0;
    const oldest = events.reduce((min, e) => Math.min(min, e.ts), Number.POSITIVE_INFINITY);
    return Math.max(0, oldest + DAY_MS - now);
  }

  private findLastEventForTarget(target?: string): RateLimitEvent | undefined {
    if (!target) return undefined;
    for (let i = this.state.events.length - 1; i >= 0; i -= 1) {
      if (this.state.events[i].target === target) return this.state.events[i];
    }
    return undefined;
  }

  private prune(now: number) {
    const windowStart = now - Math.max(DAY_MS, this.config.maxWindowMs);
    this.state.events = this.state.events.filter((e) => e.ts >= windowStart);
    this.state.decisions = this.state.decisions.slice(0, this.config.maxAuditEntries);
    if ((this.state.burstCooldownUntil ?? 0) < now) {
      this.state.burstCooldownUntil = undefined;
    }
  }

  private loadState(): RateLimitState {
    if (!this.hasStorage) return { events: [], decisions: [] };
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return { events: [], decisions: [] };
      const parsed = JSON.parse(raw) as RateLimitState;
      return {
        events: Array.isArray(parsed.events) ? parsed.events : [],
        decisions: Array.isArray(parsed.decisions) ? parsed.decisions : [],
        burstCooldownUntil: parsed.burstCooldownUntil,
        lastDecision: parsed.lastDecision,
      };
    } catch (err) {
      console.warn('Failed to load reputation rate limit state:', err);
      return { events: [], decisions: [] };
    }
  }

  private loadConfig(): RateLimitConfig {
    const config: RateLimitConfig = { ...DEFAULT_CONFIG };
    // Prefer persisted config if present
    if (this.hasStorage) {
      try {
        const raw = localStorage.getItem(CONFIG_STORAGE_KEY);
        if (raw) {
          Object.assign(config, JSON.parse(raw));
        }
      } catch (err) {
        console.warn('Failed to load reputation rate limit config, using defaults:', err);
      }
    }

    // Allow simple env override for mode to keep stability toggles outside code changes
    const envMode = (import.meta as any)?.env?.VITE_REPUTATION_RATE_LIMIT_MODE;
    if (envMode === 'enforce' || envMode === 'log-only') {
      config.mode = envMode;
    }
    return config;
  }

  private persistConfig() {
    if (!this.hasStorage) return;
    try {
      localStorage.setItem(CONFIG_STORAGE_KEY, JSON.stringify(this.config));
    } catch (err) {
      console.warn('Failed to persist reputation rate limit config:', err);
    }
  }

  private persistState() {
    if (!this.hasStorage) return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.state));
    } catch (err) {
      console.warn('Failed to persist reputation rate limit state:', err);
    }
  }
}

export const reputationRateLimiter = new ReputationRateLimiter();


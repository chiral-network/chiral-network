import type { TransactionVerdict } from '$lib/types/reputation';

const STORAGE_VERSION = 2;
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
  adaptiveCaps: {
    enabled: boolean;
    windowMs: number;
    blockThreshold: number;
    cooldownMs: number;
    dailyStep: number;
    perTargetStep: number;
    minDailyCap: number;
    minPerTargetCap: number;
    reasons: RateLimitReason[];
  };
  burst: {
    count: number;
    intervalMs: number;
    cooldownMs: number;
  };
  perTargetGrace: {
    enabled: boolean;
    creditsPerWindow: number;
    windowMs: number;
    allowReasons: RateLimitReason[];
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
  adaptiveCaps: {
    enabled: false,
    windowMs: 15 * 60 * 1000, // look at the last 15 minutes for volatility
    blockThreshold: 3, // require 3 blocks in the window to react
    cooldownMs: 10 * 60 * 1000, // wait 10 minutes between adjustments
    dailyStep: 5, // lower daily cap by 5 when triggered
    perTargetStep: 1, // lower per-target cap by 1 when triggered
    minDailyCap: 5, // never drop below 5/day automatically
    minPerTargetCap: 1, // never drop below 1/target/day automatically
    reasons: ['daily_cap', 'target_daily_cap', 'burst_cooldown'],
  },
  burst: {
    count: 8,
    intervalMs: 5 * 60 * 1000, // 8 verdicts within 5 minutes triggers cooldown
    cooldownMs: 60 * 60 * 1000, // 1h cooldown after a burst
  },
  perTargetGrace: {
    enabled: false,
    creditsPerWindow: 1,
    windowMs: DAY_MS,
    allowReasons: ['target_daily_cap', 'target_cooldown'],
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
  graceApplied?: boolean;
  graceRemaining?: number;
  graceWindowRemainingMs?: number;
  metrics: {
    enabled: boolean;
    mode: RateLimitMode;
    dailyCount: number;
    baseDailyCap: number;
    dailyCap: number;
    perTargetCount: number;
    basePerTargetCap: number;
    perTargetCap: number;
    perTargetCooldownRemainingMs: number;
    burstCount: number;
    burstWindowMs: number;
    burstCooldownRemainingMs: number;
    adaptiveEnabled: boolean;
    adaptiveActive: boolean;
    adaptiveReductions: number;
    adaptiveLastAdjustmentTs?: number;
    adaptiveLastReason?: RateLimitReason;
    graceEnabled: boolean;
    graceRemaining?: number;
    graceWindowRemainingMs?: number;
  };
}

export interface RateLimitStatus {
  enabled: boolean;
  mode: RateLimitMode;
  dailyUsed: number;
  dailyCap: number;
  baseDailyCap: number;
  nextDailyResetMs: number;
  perTargetUsed: number;
  perTargetCap: number;
  basePerTargetCap: number;
  perTargetCooldownRemainingMs: number;
  burstCount: number;
  burstWindowMs: number;
  burstCooldownRemainingMs: number;
  shadowBlocked: boolean;
  adaptive: {
    enabled: boolean;
    active: boolean;
    reductions: number;
    lastAdjustmentTs?: number;
    lastReason?: RateLimitReason;
  };
  grace: {
    enabled: boolean;
    remaining: number;
    windowRemainingMs: number;
  };
  health: RateLimitHealth;
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
  adaptiveCaps?: {
    dailyCap: number;
    perTargetDailyCap: number;
    reductions: number;
    lastAdjustmentTs?: number;
    lastReason?: RateLimitReason;
  };
  graceCredits?: Record<
    string,
    {
      used: number;
      windowStart: number;
    }
  >;
}

interface RateLimitHealth {
  safeMode: boolean;
  recoveredFromCorruption: boolean;
  lastLoadError?: string;
  lastPersistError?: string;
  version: number;
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
  private safeMode = false;
  private health: RateLimitHealth = {
    safeMode: false,
    recoveredFromCorruption: false,
    version: STORAGE_VERSION,
  };

  constructor() {
    this.config = this.loadConfig();
    this.state = this.loadState();
    this.syncAdaptiveCapsWithConfig();
    this.prune(Date.now());
  }

  setConfig(partial: Partial<RateLimitConfig>) {
    const nextConfig = { ...this.config, ...partial };
    if (partial.adaptiveCaps) {
      nextConfig.adaptiveCaps = { ...this.config.adaptiveCaps, ...partial.adaptiveCaps };
    }
    if (partial.perTargetGrace) {
      nextConfig.perTargetGrace = { ...this.config.perTargetGrace, ...partial.perTargetGrace };
    }
    if (!nextConfig.adaptiveCaps) {
      nextConfig.adaptiveCaps = { ...DEFAULT_CONFIG.adaptiveCaps };
    }
    if (!nextConfig.perTargetGrace) {
      nextConfig.perTargetGrace = { ...DEFAULT_CONFIG.perTargetGrace };
    }
    this.config = nextConfig;
    const resetAdaptive =
      'dailyCap' in partial || 'perTargetDailyCap' in partial || 'adaptiveCaps' in partial;
    this.syncAdaptiveCapsWithConfig(resetAdaptive);
    this.persistConfig();
    this.persistState();
  }

  evaluate(verdict: TransactionVerdict, now: number = Date.now()): RateLimitDecision {
    this.prune(now);
    const target = verdict.target_id;
    const { dailyCap: effectiveDailyCap, perTargetDailyCap: effectivePerTargetCap, adaptiveApplied, reductions, lastAdjustmentTs, lastReason } =
      this.getEffectiveCaps();
    const burstCooldownRemainingMs = Math.max(0, (this.state.burstCooldownUntil ?? 0) - now);
    const graceInfo = this.getGraceInfo(target, now);

    const dailyEvents = this.state.events.filter((e) => e.ts >= now - DAY_MS);
    const perTargetEvents = target ? dailyEvents.filter((e) => e.target === target) : [];
    const lastTargetEvent = this.findLastEventForTarget(target);
    const perTargetCooldownRemainingMs = lastTargetEvent
      ? Math.max(0, this.config.perTargetCooldownMs - (now - lastTargetEvent.ts))
      : 0;
    const burstEvents = this.state.events.filter((e) => e.ts >= now - this.config.burst.intervalMs);

    const mode = this.getActiveMode();
    const decision: RateLimitDecision = {
      allowed: true,
      shadowBlocked: false,
      metrics: {
        enabled: this.config.enabled,
        mode,
        dailyCount: dailyEvents.length,
        baseDailyCap: this.config.dailyCap,
        dailyCap: effectiveDailyCap,
        perTargetCount: perTargetEvents.length,
        basePerTargetCap: this.config.perTargetDailyCap,
        perTargetCap: effectivePerTargetCap,
        perTargetCooldownRemainingMs,
        burstCount: burstEvents.length,
        burstWindowMs: this.config.burst.intervalMs,
        burstCooldownRemainingMs,
        adaptiveEnabled: this.config.adaptiveCaps?.enabled ?? false,
        adaptiveActive: adaptiveApplied,
        adaptiveReductions: reductions,
        adaptiveLastAdjustmentTs: lastAdjustmentTs,
        adaptiveLastReason: lastReason,
        graceEnabled: this.config.perTargetGrace?.enabled ?? false,
        graceRemaining: graceInfo.remaining,
        graceWindowRemainingMs: graceInfo.windowRemainingMs,
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
    } else if (dailyEvents.length >= effectiveDailyCap) {
      blockedState.blocked = true;
      blockedState.reason = 'daily_cap';
      blockedState.retryAfterMs = this.computeResetMs(dailyEvents, now);
    } else if (perTargetEvents.length >= effectivePerTargetCap) {
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
      const grace = this.maybeConsumeGrace(target, blockedState.reason, now);
      if (grace.applied) {
        decision.allowed = true;
        decision.shadowBlocked = false;
        decision.reason = blockedState.reason;
        decision.retryAfterMs = blockedState.retryAfterMs;
        decision.graceApplied = true;
        decision.graceRemaining = grace.remaining;
        decision.graceWindowRemainingMs = grace.windowRemainingMs;
      } else {
        decision.shadowBlocked = true;
        decision.allowed = mode !== 'enforce';
        decision.reason = blockedState.reason;
        decision.retryAfterMs = blockedState.retryAfterMs;
      }
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

    this.maybeAdjustAdaptiveCaps(now);
    this.persistState();
  }

  private getEffectiveCaps() {
    this.syncAdaptiveCapsWithConfig();
    const adaptiveState = this.state.adaptiveCaps;
    const adaptiveEnabled = this.config.adaptiveCaps?.enabled ?? false;

    if (!adaptiveEnabled || !adaptiveState) {
      return {
        dailyCap: this.config.dailyCap,
        perTargetDailyCap: this.config.perTargetDailyCap,
        adaptiveApplied: false,
        reductions: 0,
        lastAdjustmentTs: undefined as number | undefined,
        lastReason: undefined as RateLimitReason | undefined,
      };
    }

    const dailyCap = Math.min(adaptiveState.dailyCap, this.config.dailyCap);
    const perTargetDailyCap = Math.min(adaptiveState.perTargetDailyCap, this.config.perTargetDailyCap);

    return {
      dailyCap,
      perTargetDailyCap,
      adaptiveApplied: dailyCap < this.config.dailyCap || perTargetDailyCap < this.config.perTargetDailyCap,
      reductions: adaptiveState.reductions ?? 0,
      lastAdjustmentTs: adaptiveState.lastAdjustmentTs,
      lastReason: adaptiveState.lastReason,
    };
  }

  private syncAdaptiveCapsWithConfig(forceBase = false) {
    const baseDaily = this.config.dailyCap;
    const basePerTarget = this.config.perTargetDailyCap;

    if (forceBase || !this.config.adaptiveCaps?.enabled) {
      this.state.adaptiveCaps = {
        dailyCap: baseDaily,
        perTargetDailyCap: basePerTarget,
        reductions: 0,
        lastAdjustmentTs: undefined,
        lastReason: undefined,
      };
      return;
    }

    const current = this.state.adaptiveCaps ?? {
      dailyCap: baseDaily,
      perTargetDailyCap: basePerTarget,
      reductions: 0,
      lastAdjustmentTs: undefined as number | undefined,
      lastReason: undefined as RateLimitReason | undefined,
    };

    this.state.adaptiveCaps = {
      dailyCap: Math.max(1, Math.min(current.dailyCap, baseDaily)),
      perTargetDailyCap: Math.max(1, Math.min(current.perTargetDailyCap, basePerTarget)),
      reductions: current.reductions ?? 0,
      lastAdjustmentTs: current.lastAdjustmentTs,
      lastReason: current.lastReason,
    };
  }

  private isAdaptiveReason(reason?: RateLimitReason) {
    if (!reason) return false;
    return (this.config.adaptiveCaps?.reasons ?? []).includes(reason);
  }

  private maybeAdjustAdaptiveCaps(now: number) {
    const adaptive = this.config.adaptiveCaps;
    if (!adaptive?.enabled) return;

    this.syncAdaptiveCapsWithConfig();
    const adaptiveState = this.state.adaptiveCaps!;

    if (
      adaptiveState.lastAdjustmentTs &&
      adaptive.cooldownMs > 0 &&
      now - adaptiveState.lastAdjustmentTs < adaptive.cooldownMs
    ) {
      return;
    }

    const windowStart = now - adaptive.windowMs;
    const recentBlocks = this.state.decisions.filter(
      (d) =>
        d.ts >= windowStart &&
        this.isAdaptiveReason(d.reason) &&
        (d.shadowBlocked || d.allowed === false)
    );

    if (recentBlocks.length < adaptive.blockThreshold) {
      return;
    }

    const dailyFloor = Math.max(1, adaptive.minDailyCap);
    const perTargetFloor = Math.max(1, adaptive.minPerTargetCap);

    const nextDailyCap = Math.max(
      dailyFloor,
      Math.min(adaptiveState.dailyCap, this.config.dailyCap) - adaptive.dailyStep
    );
    const nextPerTargetCap = Math.max(
      perTargetFloor,
      Math.min(adaptiveState.perTargetDailyCap, this.config.perTargetDailyCap) - adaptive.perTargetStep
    );

    const shouldAdjust =
      nextDailyCap < adaptiveState.dailyCap || nextPerTargetCap < adaptiveState.perTargetDailyCap;

    if (!shouldAdjust) return;

    this.state.adaptiveCaps = {
      dailyCap: nextDailyCap,
      perTargetDailyCap: nextPerTargetCap,
      reductions: (adaptiveState.reductions ?? 0) + 1,
      lastAdjustmentTs: now,
      lastReason: recentBlocks[0]?.reason,
    };
  }

  private getActiveMode(): RateLimitMode {
    if (this.safeMode) {
      return 'log-only';
    }
    return this.config.mode;
  }

  private sanitizeConfig(config: RateLimitConfig): RateLimitConfig {
    const clampPositive = (val: number, fallback: number, min = 0) =>
      Number.isFinite(val) && val >= min ? val : fallback;

    const sanitized: RateLimitConfig = {
      enabled: !!config.enabled,
      mode: config.mode === 'enforce' ? 'enforce' : 'log-only',
      dailyCap: clampPositive(config.dailyCap, DEFAULT_CONFIG.dailyCap, 1),
      perTargetDailyCap: clampPositive(config.perTargetDailyCap, DEFAULT_CONFIG.perTargetDailyCap, 1),
      perTargetCooldownMs: clampPositive(
        config.perTargetCooldownMs,
        DEFAULT_CONFIG.perTargetCooldownMs,
        0
      ),
      adaptiveCaps: {
        enabled: !!config.adaptiveCaps?.enabled,
        windowMs: clampPositive(config.adaptiveCaps?.windowMs, DEFAULT_CONFIG.adaptiveCaps.windowMs, 1_000),
        blockThreshold: clampPositive(
          config.adaptiveCaps?.blockThreshold,
          DEFAULT_CONFIG.adaptiveCaps.blockThreshold,
          1
        ),
        cooldownMs: clampPositive(
          config.adaptiveCaps?.cooldownMs,
          DEFAULT_CONFIG.adaptiveCaps.cooldownMs,
          0
        ),
        dailyStep: clampPositive(config.adaptiveCaps?.dailyStep, DEFAULT_CONFIG.adaptiveCaps.dailyStep, 1),
        perTargetStep: clampPositive(
          config.adaptiveCaps?.perTargetStep,
          DEFAULT_CONFIG.adaptiveCaps.perTargetStep,
          1
        ),
        minDailyCap: clampPositive(
          config.adaptiveCaps?.minDailyCap,
          DEFAULT_CONFIG.adaptiveCaps.minDailyCap,
          1
        ),
        minPerTargetCap: clampPositive(
          config.adaptiveCaps?.minPerTargetCap,
          DEFAULT_CONFIG.adaptiveCaps.minPerTargetCap,
          1
        ),
        reasons: Array.isArray(config.adaptiveCaps?.reasons) && config.adaptiveCaps!.reasons.length
          ? config.adaptiveCaps!.reasons.filter((r): r is RateLimitReason =>
              ['daily_cap', 'target_cooldown', 'target_daily_cap', 'burst_cooldown'].includes(r)
            )
          : [...DEFAULT_CONFIG.adaptiveCaps.reasons],
      },
      burst: {
        count: clampPositive(config.burst.count, DEFAULT_CONFIG.burst.count, 1),
        intervalMs: clampPositive(config.burst.intervalMs, DEFAULT_CONFIG.burst.intervalMs, 1),
        cooldownMs: clampPositive(config.burst.cooldownMs, DEFAULT_CONFIG.burst.cooldownMs, 1),
      },
      perTargetGrace: {
        enabled: !!config.perTargetGrace?.enabled,
        creditsPerWindow: clampPositive(
          config.perTargetGrace?.creditsPerWindow,
          DEFAULT_CONFIG.perTargetGrace.creditsPerWindow,
          0
        ),
        windowMs: clampPositive(
          config.perTargetGrace?.windowMs,
          DEFAULT_CONFIG.perTargetGrace.windowMs,
          1_000
        ),
        allowReasons:
          Array.isArray(config.perTargetGrace?.allowReasons) && config.perTargetGrace!.allowReasons.length
            ? config.perTargetGrace!.allowReasons.filter((r): r is RateLimitReason =>
                ['daily_cap', 'target_cooldown', 'target_daily_cap', 'burst_cooldown'].includes(r)
              )
            : [...DEFAULT_CONFIG.perTargetGrace.allowReasons],
      },
      maxEventEntries: clampPositive(config.maxEventEntries, DEFAULT_CONFIG.maxEventEntries, 1),
      maxAuditEntries: clampPositive(config.maxAuditEntries, DEFAULT_CONFIG.maxAuditEntries, 1),
      maxWindowMs: clampPositive(config.maxWindowMs, DEFAULT_CONFIG.maxWindowMs, DAY_MS),
    };

    return sanitized;
  }

  private sanitizeState(raw: RateLimitState, version: number) {
    let recovered = false;
    const state: RateLimitState = {
      events: [],
      decisions: [],
      burstCooldownUntil: undefined,
      lastDecision: undefined,
      adaptiveCaps: undefined,
    };

    const windowStart = Date.now() - Math.max(DAY_MS, this.config.maxWindowMs);
    if (Array.isArray(raw.events)) {
      for (const e of raw.events) {
        if (!e || typeof e.target !== 'string' || !Number.isFinite((e as any).ts)) {
          recovered = true;
          continue;
        }
        const ts = (e as any).ts as number;
        if (ts < windowStart) {
          recovered = true;
          continue;
        }
        state.events.push({ target: e.target, ts, outcome: (e as any).outcome ?? 'good' });
      }
    } else {
      recovered = true;
    }
    if (state.events.length > this.config.maxEventEntries) {
      state.events = state.events.slice(-this.config.maxEventEntries);
      recovered = true;
    }

    if (Array.isArray(raw.decisions)) {
      for (const d of raw.decisions) {
        if (!d || !Number.isFinite((d as any).ts) || typeof (d as any).target !== 'string') {
          recovered = true;
          continue;
        }
        state.decisions.push({
          ts: (d as any).ts,
          target: (d as any).target,
          allowed: !!(d as any).allowed,
          shadowBlocked: !!(d as any).shadowBlocked,
          reason: (d as any).reason,
          mode: (d as any).mode === 'enforce' ? 'enforce' : 'log-only',
          retryAfterMs: Number.isFinite((d as any).retryAfterMs) ? (d as any).retryAfterMs : undefined,
        });
      }
    } else {
      recovered = true;
    }
    if (state.decisions.length > this.config.maxAuditEntries) {
      state.decisions = state.decisions.slice(0, this.config.maxAuditEntries);
      recovered = true;
    }

    if (raw.lastDecision && Number.isFinite((raw.lastDecision as any).ts)) {
      state.lastDecision = raw.lastDecision;
    } else if (raw.lastDecision) {
      recovered = true;
    }

    if (Number.isFinite((raw as any).burstCooldownUntil)) {
      state.burstCooldownUntil = (raw as any).burstCooldownUntil;
    }
    if ((state.burstCooldownUntil ?? 0) < Date.now()) {
      state.burstCooldownUntil = undefined;
    }

    if (raw.adaptiveCaps) {
      const dailyCap = Number.isFinite((raw.adaptiveCaps as any).dailyCap)
        ? (raw.adaptiveCaps as any).dailyCap
        : this.config.dailyCap;
      const perTargetDailyCap = Number.isFinite((raw.adaptiveCaps as any).perTargetDailyCap)
        ? (raw.adaptiveCaps as any).perTargetDailyCap
        : this.config.perTargetDailyCap;
      state.adaptiveCaps = {
        dailyCap: Math.max(1, Math.min(dailyCap, this.config.dailyCap)),
        perTargetDailyCap: Math.max(1, Math.min(perTargetDailyCap, this.config.perTargetDailyCap)),
        reductions: Number.isFinite((raw.adaptiveCaps as any).reductions)
          ? (raw.adaptiveCaps as any).reductions
          : 0,
        lastAdjustmentTs: Number.isFinite((raw.adaptiveCaps as any).lastAdjustmentTs)
          ? (raw.adaptiveCaps as any).lastAdjustmentTs
          : undefined,
        lastReason: (raw.adaptiveCaps as any).lastReason,
      };
    }

    if (version !== STORAGE_VERSION) {
      recovered = true;
    }

    if (raw.graceCredits && typeof raw.graceCredits === 'object') {
      state.graceCredits = {};
      const graceWindow = this.config.perTargetGrace.windowMs;
      for (const [key, value] of Object.entries(raw.graceCredits as Record<string, any>)) {
        if (typeof key !== 'string') {
          recovered = true;
          continue;
        }
        const used = Number.isFinite(value?.used) ? value.used : 0;
        const windowStart = Number.isFinite(value?.windowStart) ? value.windowStart : undefined;
        if (windowStart === undefined) {
          recovered = true;
          continue;
        }
        if (Date.now() - windowStart >= graceWindow) {
          recovered = true;
          continue;
        }
        state.graceCredits[key] = { used: Math.max(0, used), windowStart };
      }
    }

    if (!state.graceCredits) {
      state.graceCredits = {};
    }

    return { state, recovered };
  }

  private getGraceInfo(target: string, now: number) {
    if (!target || !this.config.perTargetGrace?.enabled) {
      return { remaining: 0, windowRemainingMs: 0 };
    }
    const graceWindow = this.config.perTargetGrace.windowMs;
    const creditsPerWindow = this.config.perTargetGrace.creditsPerWindow;
    const entry = this.state.graceCredits?.[target];
    if (!entry || now - entry.windowStart >= graceWindow) {
      return { remaining: creditsPerWindow, windowRemainingMs: graceWindow };
    }
    const remaining = Math.max(0, creditsPerWindow - entry.used);
    const windowRemainingMs = Math.max(0, graceWindow - (now - entry.windowStart));
    return { remaining, windowRemainingMs };
  }

  private maybeConsumeGrace(target: string | undefined, reason: RateLimitReason | undefined, now: number) {
    if (!target || !reason || !this.config.perTargetGrace?.enabled) {
      return { applied: false, remaining: 0, windowRemainingMs: 0 };
    }
    if (!(this.config.perTargetGrace.allowReasons ?? []).includes(reason)) {
      return { applied: false, remaining: 0, windowRemainingMs: 0 };
    }
    const graceWindow = this.config.perTargetGrace.windowMs;
    const creditsPerWindow = this.config.perTargetGrace.creditsPerWindow;
    if (!this.state.graceCredits) {
      this.state.graceCredits = {};
    }
    const entry = this.state.graceCredits[target];
    const resetNeeded = !entry || now - entry.windowStart >= graceWindow;
    const used = resetNeeded ? 0 : entry.used;
    const windowStart = resetNeeded ? now : entry.windowStart;

    if (used >= creditsPerWindow) {
      const windowRemainingMs = Math.max(0, graceWindow - (now - windowStart));
      return { applied: false, remaining: 0, windowRemainingMs };
    }

    const nextUsed = used + 1;
    this.state.graceCredits[target] = { used: nextUsed, windowStart };
    const remaining = Math.max(0, creditsPerWindow - nextUsed);
    const windowRemainingMs = Math.max(0, graceWindow - (now - windowStart));
    return { applied: true, remaining, windowRemainingMs };
  }

  getStatus(targetId?: string): RateLimitStatus {
    const now = Date.now();
    this.prune(now);
    const {
      dailyCap: effectiveDailyCap,
      perTargetDailyCap: effectivePerTargetCap,
      adaptiveApplied,
      reductions,
      lastAdjustmentTs,
      lastReason,
    } = this.getEffectiveCaps();
    const mode = this.getActiveMode();
    const focusTarget = targetId || this.state.lastDecision?.target || '';
    const graceInfo = this.getGraceInfo(focusTarget, now);
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
      mode,
      dailyUsed: dailyEvents.length,
      dailyCap: effectiveDailyCap,
      baseDailyCap: this.config.dailyCap,
      nextDailyResetMs:
        dailyEvents.length === 0 || earliest === Number.POSITIVE_INFINITY
          ? 0
          : Math.max(0, earliest + DAY_MS - now),
      perTargetUsed: perTargetEvents.length,
      perTargetCap: effectivePerTargetCap,
      basePerTargetCap: this.config.perTargetDailyCap,
      perTargetCooldownRemainingMs: lastTargetEvent
        ? Math.max(0, this.config.perTargetCooldownMs - (now - lastTargetEvent.ts))
        : 0,
      burstCount: this.state.events.filter((e) => e.ts >= now - this.config.burst.intervalMs)
        .length,
      burstWindowMs: this.config.burst.intervalMs,
      burstCooldownRemainingMs,
      shadowBlocked: !!this.state.lastDecision?.shadowBlocked,
      adaptive: {
        enabled: this.config.adaptiveCaps?.enabled ?? false,
        active: adaptiveApplied,
        reductions,
        lastAdjustmentTs,
        lastReason,
      },
      grace: {
        enabled: this.config.perTargetGrace?.enabled ?? false,
        remaining: graceInfo.remaining,
        windowRemainingMs: graceInfo.windowRemainingMs,
      },
      health: { ...this.health },
      lastDecision: this.state.lastDecision,
    };
  }

  reset() {
    this.state = {
      events: [],
      decisions: [],
      burstCooldownUntil: undefined,
      lastDecision: undefined,
      adaptiveCaps: undefined,
      graceCredits: {},
    };
    this.syncAdaptiveCapsWithConfig(true);
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
    if (this.state.graceCredits && this.config.perTargetGrace?.enabled) {
      const graceWindow = this.config.perTargetGrace.windowMs;
      for (const [key, value] of Object.entries(this.state.graceCredits)) {
        if (!value || !Number.isFinite(value.windowStart) || now - value.windowStart >= graceWindow) {
          delete this.state.graceCredits[key];
        }
      }
    } else if (this.state.graceCredits) {
      this.state.graceCredits = {};
    }
  }

  private loadState(): RateLimitState {
    if (!this.hasStorage) return { events: [], decisions: [], graceCredits: {} };
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return { events: [], decisions: [], graceCredits: {} };
      const parsed = JSON.parse(raw) as any;
      const payload =
        parsed && typeof parsed === 'object' && 'state' in parsed && typeof parsed.version === 'number'
          ? (parsed as { version: number; state: RateLimitState })
          : { version: 1, state: parsed as RateLimitState };
      const sanitized = this.sanitizeState(payload.state, payload.version);
      if (sanitized.recovered) {
        this.health.recoveredFromCorruption = true;
      }
      this.health.version = STORAGE_VERSION;
      return sanitized.state;
    } catch (err) {
      console.warn('Failed to load reputation rate limit state:', err);
      this.health.lastLoadError = err instanceof Error ? err.message : String(err);
      this.safeMode = true;
      this.health.safeMode = true;
      return { events: [], decisions: [], graceCredits: {} };
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
        this.health.lastLoadError = err instanceof Error ? err.message : String(err);
      }
    }

    // Allow simple env override for mode to keep stability toggles outside code changes
    const envMode = (import.meta as any)?.env?.VITE_REPUTATION_RATE_LIMIT_MODE;
    if (envMode === 'enforce' || envMode === 'log-only') {
      config.mode = envMode;
    }
    if (!config.adaptiveCaps) {
      config.adaptiveCaps = { ...DEFAULT_CONFIG.adaptiveCaps };
    }
    if (!config.perTargetGrace) {
      config.perTargetGrace = { ...DEFAULT_CONFIG.perTargetGrace };
    }
    return this.sanitizeConfig(config);
  }

  private persistConfig() {
    if (!this.hasStorage) return;
    try {
      localStorage.setItem(CONFIG_STORAGE_KEY, JSON.stringify(this.config));
    } catch (err) {
      console.warn('Failed to persist reputation rate limit config:', err);
      this.health.lastPersistError = err instanceof Error ? err.message : String(err);
      this.safeMode = true;
      this.health.safeMode = true;
    }
  }

  private persistState() {
    if (!this.hasStorage) return;
    try {
      const payload = { version: STORAGE_VERSION, state: this.state };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    } catch (err) {
      console.warn('Failed to persist reputation rate limit state:', err);
      this.health.lastPersistError = err instanceof Error ? err.message : String(err);
      this.safeMode = true;
      this.health.safeMode = true;
    }
  }
}

export const reputationRateLimiter = new ReputationRateLimiter();

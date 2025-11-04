import { describe, it, expect } from 'vitest';
import { computeProxyWeight } from '../src/lib/services/routingWeights';

describe('computeProxyWeight', () => {
  it('prefers lower latency', () => {
    const fast = computeProxyWeight({ latencyMs: 30, uptimePct: 90, status: 'online' });
    const slow = computeProxyWeight({ latencyMs: 300, uptimePct: 90, status: 'online' });
    expect(fast).toBeGreaterThan(slow);
  });

  it('prefers higher uptime', () => {
    const high = computeProxyWeight({ latencyMs: 120, uptimePct: 99, status: 'online' });
    const low = computeProxyWeight({ latencyMs: 120, uptimePct: 50, status: 'online' });
    expect(high).toBeGreaterThan(low);
  });

  it('penalizes offline and error states', () => {
    const online = computeProxyWeight({ latencyMs: 120, uptimePct: 90, status: 'online' });
    const offline = computeProxyWeight({ latencyMs: 120, uptimePct: 90, status: 'offline' });
    const error = computeProxyWeight({ latencyMs: 120, uptimePct: 90, status: 'error' });
    expect(online).toBeGreaterThan(offline);
    expect(offline).toBeGreaterThan(error);
  });

  it('applies recent failure penalty', () => {
    const clean = computeProxyWeight({ latencyMs: 120, uptimePct: 90, status: 'online', recentFailures: 0 });
    const flaky = computeProxyWeight({ latencyMs: 120, uptimePct: 90, status: 'online', recentFailures: 5 });
    expect(clean).toBeGreaterThan(flaky);
  });
});
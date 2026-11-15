import { describe, expect, it } from 'vitest';
import { command, formatCfx, nativeAvailable, parseCfx, textDelta } from '$lib/native';

describe('exact CFX amounts', () => {
  it('round trips the entire supported integer range without Number conversion', () => {
    for (const wei of ['0', '1', '999999999999999999', '1000000000000000000', '9007199254740993000000000000000001', ((1n << 128n) - 1n).toString()]) {
      expect(parseCfx(formatCfx(wei))).toBe(wei);
    }
  });
  it('rejects precision loss, exponents, signed values and overflow', () => {
    for (const amount of ['1e3', '-1', '+1', '0.0000000000000000001', 'NaN', 'Infinity', '1.', ' 1', '01', '340282366920938463463.374607431768211456']) {
      expect(() => parseCfx(amount)).toThrow();
    }
  });
  it('does not silently display invalid monetary data as zero', () => {
    expect(formatCfx('not-a-number')).toBe('unavailable');
    expect(formatCfx('-1')).toBe('unavailable');
    expect(formatCfx('100000000000000001')).toBe('0.100000000000000001');
  });
});

describe('text-only streaming extraction', () => {
  it('accepts text chat and completion deltas only', () => {
    expect(textDelta({ choices: [{ delta: { content: 'hello' } }, { text: ' world' }] })).toBe('hello world');
  });
  it('never executes or renders structured tool calls', () => {
    expect(textDelta({ choices: [{ delta: { tool_calls: [{ function: { name: 'shell', arguments: 'rm -rf /' } }] } }] })).toBe('');
    expect(textDelta({ choices: [null, 3, { delta: { content: { html: '<script>' } } }] })).toBe('');
  });
  it('preserves markup as plain text for Svelte escaping', () => {
    expect(textDelta({ choices: [{ delta: { content: '<img src=x onerror=alert(1)>' } }] })).toBe('<img src=x onerror=alert(1)>');
  });
});

describe('honest browser boundary', () => {
  it('rejects paid/native operations without a desktop runtime', async () => {
    expect(nativeAvailable()).toBe(false);
    await expect(command('fund', {})).rejects.toBeInstanceOf(Error);
    await expect(command('credential_export', {})).rejects.toBeInstanceOf(Error);
  });
});

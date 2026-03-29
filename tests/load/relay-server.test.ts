/**
 * Load / stress tests for the Chiral relay server at http://130.245.173.73:8080
 *
 * All test suites use `describe.skip` so they never run in CI.
 * To run locally:
 *   1. Change `describe.skip` to `describe` for the suite(s) you want.
 *   2. `npx vitest run tests/load/relay-server.test.ts`
 */
import { describe, it, expect } from "vitest";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const BASE_URL = "http://130.245.173.73:8080";
const REQUEST_TIMEOUT_MS = 10_000;

// Fake but validly-formatted wallet addresses for read-only tests
function fakeWallet(index: number): string {
  const hex = index.toString(16).padStart(40, "0");
  return `0x${hex}`;
}

// ---------------------------------------------------------------------------
// Timing helpers
// ---------------------------------------------------------------------------

interface TimingResult<T> {
  data: T;
  durationMs: number;
}

async function timed<T>(fn: () => Promise<T>): Promise<TimingResult<T>> {
  const start = performance.now();
  const data = await fn();
  return { data, durationMs: performance.now() - start };
}

interface LatencyStats {
  count: number;
  min: number;
  max: number;
  mean: number;
  p50: number;
  p95: number;
  p99: number;
  errorCount: number;
  errorRate: number;
}

function computeStats(
  durations: number[],
  errorCount: number
): LatencyStats {
  const sorted = [...durations].sort((a, b) => a - b);
  const count = sorted.length;
  const sum = sorted.reduce((a, b) => a + b, 0);

  const percentile = (p: number) => {
    if (count === 0) return 0;
    const idx = Math.ceil((p / 100) * count) - 1;
    return sorted[Math.max(0, idx)];
  };

  return {
    count,
    min: sorted[0] ?? 0,
    max: sorted[count - 1] ?? 0,
    mean: count > 0 ? sum / count : 0,
    p50: percentile(50),
    p95: percentile(95),
    p99: percentile(99),
    errorCount,
    errorRate: count + errorCount > 0 ? errorCount / (count + errorCount) : 0,
  };
}

function printStats(label: string, stats: LatencyStats): void {
  console.log(
    `[${label}] n=${stats.count} | min=${stats.min.toFixed(0)}ms ` +
      `mean=${stats.mean.toFixed(0)}ms p50=${stats.p50.toFixed(0)}ms ` +
      `p95=${stats.p95.toFixed(0)}ms p99=${stats.p99.toFixed(0)}ms ` +
      `max=${stats.max.toFixed(0)}ms | errors=${stats.errorCount} (${(stats.errorRate * 100).toFixed(1)}%)`
  );
}

// ---------------------------------------------------------------------------
// Fetch helpers
// ---------------------------------------------------------------------------

async function postJson(
  path: string,
  body: unknown,
  headers?: Record<string, string>
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    return await fetch(`${BASE_URL}${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...headers },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timer);
  }
}

async function getJson(
  path: string,
  headers?: Record<string, string>
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    return await fetch(`${BASE_URL}${path}`, {
      method: "GET",
      headers: { Accept: "application/json", ...headers },
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Fire `concurrency` requests in parallel, return per-request durations and
 * how many errored (non-2xx or network failure).
 */
async function runConcurrent(
  concurrency: number,
  requestFn: (index: number) => Promise<Response>
): Promise<{ durations: number[]; errorCount: number }> {
  const durations: number[] = [];
  let errorCount = 0;

  const results = await Promise.allSettled(
    Array.from({ length: concurrency }, (_, i) =>
      timed(() => requestFn(i))
    )
  );

  for (const r of results) {
    if (r.status === "fulfilled") {
      durations.push(r.value.durationMs);
      if (!r.value.data.ok) {
        errorCount++;
      }
    } else {
      errorCount++;
    }
  }

  return { durations, errorCount };
}

// ---------------------------------------------------------------------------
// 1. Ratings API load tests
// ---------------------------------------------------------------------------

describe.skip("Ratings API — load tests", () => {
  // ---- POST /api/ratings/batch ----

  describe("POST /api/ratings/batch — concurrent batch lookups", () => {
    const batchBody = (size: number) => ({
      wallets: Array.from({ length: size }, (_, i) => fakeWallet(i)),
    });

    for (const concurrency of [10, 50, 100]) {
      it(`handles ${concurrency} concurrent batch requests (10 wallets each)`, async () => {
        const { durations, errorCount } = await runConcurrent(
          concurrency,
          () => postJson("/api/ratings/batch", batchBody(10))
        );

        const stats = computeStats(durations, errorCount);
        printStats(`batch x${concurrency}`, stats);

        expect(stats.errorRate).toBeLessThan(0.05); // <5% error rate
        expect(stats.p95).toBeLessThan(REQUEST_TIMEOUT_MS);
      }, 30_000);
    }

    it("handles batch request with 200 wallets", async () => {
      const { data, durationMs } = await timed(() =>
        postJson("/api/ratings/batch", batchBody(200))
      );
      console.log(`[batch-200-wallets] ${durationMs.toFixed(0)}ms — status ${data.status}`);

      expect(data.status).toBeLessThan(500);
      expect(durationMs).toBeLessThan(REQUEST_TIMEOUT_MS);
    }, 15_000);
  });

  // ---- GET /api/ratings/:wallet ----

  describe("GET /api/ratings/:wallet — concurrent single lookups", () => {
    for (const concurrency of [10, 50, 100]) {
      it(`handles ${concurrency} concurrent single-wallet lookups`, async () => {
        const { durations, errorCount } = await runConcurrent(
          concurrency,
          (i) => getJson(`/api/ratings/${fakeWallet(i)}`)
        );

        const stats = computeStats(durations, errorCount);
        printStats(`single-wallet x${concurrency}`, stats);

        expect(stats.errorRate).toBeLessThan(0.05);
        expect(stats.p95).toBeLessThan(REQUEST_TIMEOUT_MS);
      }, 30_000);
    }

    it("returns valid JSON structure for a single wallet", async () => {
      const res = await getJson(`/api/ratings/${fakeWallet(999)}`);
      expect(res.status).toBeLessThan(500);

      if (res.ok) {
        const body = await res.json();
        expect(body).toHaveProperty("elo");
        expect(body).toHaveProperty("wallet");
      }
    }, 10_000);
  });

  // ---- POST /api/ratings/transfer ----

  describe("POST /api/ratings/transfer — concurrent transfer recording", () => {
    function transferBody(index: number) {
      return {
        transferId: `load-test-transfer-${Date.now()}-${index}`,
        seederWallet: fakeWallet(index),
        fileHash: "0".repeat(64),
        outcome: "completed",
        amountWei: "1000000000000000",
      };
    }

    for (const concurrency of [10, 50]) {
      it(`handles ${concurrency} concurrent transfer submissions`, async () => {
        const { durations, errorCount } = await runConcurrent(
          concurrency,
          (i) =>
            postJson("/api/ratings/transfer", transferBody(i), {
              "x-owner": fakeWallet(i + 1000),
            })
        );

        const stats = computeStats(durations, errorCount);
        printStats(`transfer x${concurrency}`, stats);

        expect(stats.errorRate).toBeLessThan(0.1); // <10% error rate
        expect(stats.p95).toBeLessThan(REQUEST_TIMEOUT_MS);
      }, 30_000);
    }
  });

  // ---- Response time measurement across ratings endpoints ----

  describe("Response time measurement — ratings endpoints", () => {
    it("measures p50/p95/p99 over 100 sequential GET requests", async () => {
      const durations: number[] = [];
      let errorCount = 0;

      for (let i = 0; i < 100; i++) {
        try {
          const { data, durationMs } = await timed(() =>
            getJson(`/api/ratings/${fakeWallet(i)}`)
          );
          durations.push(durationMs);
          if (!data.ok) errorCount++;
        } catch {
          errorCount++;
        }
      }

      const stats = computeStats(durations, errorCount);
      printStats("sequential-get x100", stats);

      expect(stats.errorRate).toBeLessThan(0.05);
      expect(stats.p99).toBeLessThan(REQUEST_TIMEOUT_MS);
    }, 120_000);
  });
});

// ---------------------------------------------------------------------------
// 2. Email endpoint stress test
// ---------------------------------------------------------------------------

describe.skip("Email endpoint — stress tests", () => {
  describe("POST /api/wallet/backup-email — invalid data rejection", () => {
    const invalidBodies = [
      { label: "empty object", body: {} },
      { label: "missing fields", body: { email: "bad" } },
      {
        label: "invalid email format",
        body: {
          email: "not-an-email",
          recoveryPhrase: "one two three four five six seven eight nine ten eleven twelve",
          walletAddress: fakeWallet(1),
          privateKey: "0x" + "a".repeat(64),
        },
      },
      {
        label: "invalid wallet address",
        body: {
          email: "test@example.com",
          recoveryPhrase: "one two three four five six seven eight nine ten eleven twelve",
          walletAddress: "not-a-wallet",
          privateKey: "0x" + "a".repeat(64),
        },
      },
      {
        label: "invalid private key",
        body: {
          email: "test@example.com",
          recoveryPhrase: "one two three four five six seven eight nine ten eleven twelve",
          walletAddress: fakeWallet(1),
          privateKey: "short",
        },
      },
      {
        label: "invalid recovery phrase (too few words)",
        body: {
          email: "test@example.com",
          recoveryPhrase: "one two three",
          walletAddress: fakeWallet(1),
          privateKey: "0x" + "a".repeat(64),
        },
      },
    ];

    for (const { label, body } of invalidBodies) {
      it(`rejects ${label} quickly`, async () => {
        const { data, durationMs } = await timed(() =>
          postJson("/api/wallet/backup-email", body)
        );

        console.log(
          `[email-invalid: ${label}] ${durationMs.toFixed(0)}ms — status ${data.status}`
        );

        // Server should respond (not hang). We accept 4xx or 5xx but it
        // must not take forever.
        expect(durationMs).toBeLessThan(5_000);
      }, 10_000);
    }
  });

  describe("Concurrent validation requests", () => {
    const invalidBody = {
      email: "loadtest@example.com",
      recoveryPhrase: "bad",
      walletAddress: "0xinvalid",
      privateKey: "nope",
    };

    for (const concurrency of [10, 50]) {
      it(`handles ${concurrency} concurrent invalid requests`, async () => {
        const { durations, errorCount } = await runConcurrent(
          concurrency,
          () => postJson("/api/wallet/backup-email", invalidBody)
        );

        const stats = computeStats(durations, errorCount);
        printStats(`email-invalid x${concurrency}`, stats);

        // All should respond (even if 4xx/5xx)
        expect(stats.count + stats.errorCount).toBe(concurrency);
        expect(stats.p95).toBeLessThan(REQUEST_TIMEOUT_MS);
      }, 30_000);
    }
  });
});

// ---------------------------------------------------------------------------
// 3. General server health
// ---------------------------------------------------------------------------

describe.skip("General server health — stress tests", () => {
  describe("Burst test — rapid sequential requests", () => {
    it("survives 200 rapid sequential requests to /api/ratings/batch", async () => {
      const durations: number[] = [];
      let errorCount = 0;
      const body = { wallets: [fakeWallet(0)] };

      for (let i = 0; i < 200; i++) {
        try {
          const { data, durationMs } = await timed(() =>
            postJson("/api/ratings/batch", body)
          );
          durations.push(durationMs);
          if (!data.ok) errorCount++;
        } catch {
          errorCount++;
        }
      }

      const stats = computeStats(durations, errorCount);
      printStats("burst-sequential x200", stats);

      expect(stats.errorRate).toBeLessThan(0.1);
      expect(stats.p99).toBeLessThan(REQUEST_TIMEOUT_MS);
    }, 180_000);
  });

  describe("Mixed endpoint burst", () => {
    it("handles 100 concurrent requests across all endpoints", async () => {
      const requests: Array<() => Promise<Response>> = [];

      // 40 batch lookups
      for (let i = 0; i < 40; i++) {
        requests.push(() =>
          postJson("/api/ratings/batch", { wallets: [fakeWallet(i)] })
        );
      }
      // 40 single wallet lookups
      for (let i = 0; i < 40; i++) {
        requests.push(() => getJson(`/api/ratings/${fakeWallet(i)}`));
      }
      // 20 email validation (invalid, to avoid side effects)
      for (let i = 0; i < 20; i++) {
        requests.push(() =>
          postJson("/api/wallet/backup-email", { email: "x" })
        );
      }

      const durations: number[] = [];
      let errorCount = 0;

      const results = await Promise.allSettled(
        requests.map((fn) => timed(fn))
      );

      for (const r of results) {
        if (r.status === "fulfilled") {
          durations.push(r.value.durationMs);
          if (!r.value.data.ok) {
            // Non-2xx is expected for invalid email bodies; only count
            // server errors (5xx) as real errors here.
            if (r.value.data.status >= 500) errorCount++;
          }
        } else {
          errorCount++;
        }
      }

      const stats = computeStats(durations, errorCount);
      printStats("mixed-burst x100", stats);

      expect(stats.errorRate).toBeLessThan(0.1);
      expect(stats.p95).toBeLessThan(REQUEST_TIMEOUT_MS);
    }, 60_000);
  });

  describe("Sustained load", () => {
    it("maintains performance over 5 waves of 50 concurrent requests", async () => {
      const waveCount = 5;
      const concurrency = 50;
      const allStats: LatencyStats[] = [];

      for (let wave = 0; wave < waveCount; wave++) {
        const { durations, errorCount } = await runConcurrent(
          concurrency,
          (i) =>
            postJson("/api/ratings/batch", {
              wallets: [fakeWallet(wave * concurrency + i)],
            })
        );

        const stats = computeStats(durations, errorCount);
        printStats(`sustained-wave-${wave + 1}`, stats);
        allStats.push(stats);
      }

      // No wave should have >10% errors
      for (const stats of allStats) {
        expect(stats.errorRate).toBeLessThan(0.1);
      }

      // Last wave should not be significantly slower than first
      // (degradation check — allow 3x tolerance)
      const firstMean = allStats[0].mean;
      const lastMean = allStats[allStats.length - 1].mean;
      if (firstMean > 0) {
        console.log(
          `[sustained] first-wave mean=${firstMean.toFixed(0)}ms, ` +
            `last-wave mean=${lastMean.toFixed(0)}ms, ` +
            `ratio=${(lastMean / firstMean).toFixed(2)}x`
        );
        expect(lastMean).toBeLessThan(firstMean * 3);
      }
    }, 120_000);
  });

  describe("Timeout detection", () => {
    it("no requests time out under moderate load (30 concurrent)", async () => {
      const { durations, errorCount } = await runConcurrent(30, (i) =>
        getJson(`/api/ratings/${fakeWallet(i)}`)
      );

      const stats = computeStats(durations, errorCount);
      printStats("timeout-check x30", stats);

      // Zero network-level failures (AbortError / fetch rejection)
      expect(errorCount).toBe(0);
      // All responses arrived well under the timeout
      expect(stats.max).toBeLessThan(REQUEST_TIMEOUT_MS);
    }, 30_000);
  });
});

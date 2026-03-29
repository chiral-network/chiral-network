/**
 * Load / stress tests for the local gateway Drive API (localhost:9419).
 *
 * These tests are skipped by default (describe.skip) so they never run in CI.
 * To run locally:
 *   1. Start the gateway server (e.g. via `npm run tauri:dev` or the daemon).
 *   2. Change `describe.skip` to `describe` below.
 *   3. Run: npx vitest run tests/load/gateway-drive.test.ts
 *
 * Requirements: vitest, native fetch (Node 18+).
 */

import { describe, it, expect, beforeAll, afterAll } from "vitest";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const BASE = "http://localhost:9419";

// Adjust if you have known IDs / tokens from a pre-seeded drive.
// The tests will attempt to discover them dynamically where possible.
let knownFileId = "";
let knownFileName = "";
let knownShareToken = "";

// IDs of items created during the test run, cleaned up in afterAll.
const createdItemIds: string[] = [];
const createdShareTokens: string[] = [];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface RequestResult {
  status: number;
  timeMs: number;
  bodySize: number;
  error?: string;
}

/** Measure a single HTTP request. */
async function measureRequest(
  url: string,
  options: RequestInit = {},
): Promise<RequestResult> {
  const start = performance.now();
  try {
    const res = await fetch(url, options);
    const buf = await res.arrayBuffer();
    return {
      status: res.status,
      timeMs: performance.now() - start,
      bodySize: buf.byteLength,
    };
  } catch (err: any) {
    return {
      status: 0,
      timeMs: performance.now() - start,
      bodySize: 0,
      error: err?.message ?? String(err),
    };
  }
}

interface Stats {
  count: number;
  p50: number;
  p95: number;
  p99: number;
  min: number;
  max: number;
  mean: number;
  errorRate: number;
  totalTimeMs: number;
}

function computeStats(results: RequestResult[]): Stats {
  const times = results.map((r) => r.timeMs).sort((a, b) => a - b);
  const errors = results.filter((r) => r.status === 0 || r.status >= 500);
  const n = times.length;
  return {
    count: n,
    p50: times[Math.floor(n * 0.5)] ?? 0,
    p95: times[Math.floor(n * 0.95)] ?? 0,
    p99: times[Math.floor(n * 0.99)] ?? 0,
    min: times[0] ?? 0,
    max: times[n - 1] ?? 0,
    mean: times.reduce((a, b) => a + b, 0) / (n || 1),
    errorRate: errors.length / (n || 1),
    totalTimeMs: Math.max(...results.map((r) => r.timeMs)),
  };
}

/** Fire `count` concurrent invocations of `fn` and return aggregated stats. */
async function runConcurrent(
  fn: () => Promise<RequestResult>,
  count: number,
): Promise<{ results: RequestResult[]; stats: Stats }> {
  const promises = Array.from({ length: count }, () => fn());
  const results = await Promise.all(promises);
  return { results, stats: computeStats(results) };
}

/**
 * Sustained-load runner.
 * Fires `rps` requests per second for `durationSec` seconds.
 * Returns aggregated stats over the entire run.
 */
async function runSustained(
  fn: () => Promise<RequestResult>,
  rps: number,
  durationSec: number,
): Promise<{ results: RequestResult[]; stats: Stats }> {
  const results: RequestResult[] = [];
  const intervalMs = 1000 / rps;
  const endTime = performance.now() + durationSec * 1000;
  const pending: Promise<void>[] = [];

  while (performance.now() < endTime) {
    const batchStart = performance.now();
    pending.push(
      fn().then((r) => {
        results.push(r);
      }),
    );
    const elapsed = performance.now() - batchStart;
    const sleepMs = Math.max(0, intervalMs - elapsed);
    if (sleepMs > 0) {
      await new Promise((r) => setTimeout(r, sleepMs));
    }
  }

  await Promise.all(pending);
  return { results, stats: computeStats(results) };
}

/** Generate a random buffer of the given size. */
function randomBuffer(sizeBytes: number): Buffer {
  const buf = Buffer.alloc(sizeBytes);
  for (let i = 0; i < sizeBytes; i += 4) {
    const val = (Math.random() * 0xffffffff) >>> 0;
    buf.writeUInt32LE(val, Math.min(i, sizeBytes - 4));
  }
  return buf;
}

/** Upload a file via multipart and return the created item id. */
async function uploadTestFile(
  name: string,
  content: Buffer,
): Promise<{ id: string; status: number; timeMs: number }> {
  const boundary = `----LoadTestBoundary${Date.now()}${Math.random()}`;
  const header = Buffer.from(
    `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${name}"\r\nContent-Type: application/octet-stream\r\n\r\n`,
  );
  const footer = Buffer.from(`\r\n--${boundary}--\r\n`);
  const body = Buffer.concat([header, content, footer]);

  const start = performance.now();
  const res = await fetch(`${BASE}/api/drive/upload`, {
    method: "POST",
    headers: { "Content-Type": `multipart/form-data; boundary=${boundary}` },
    body,
  });
  const timeMs = performance.now() - start;
  let id = "";
  try {
    const json = (await res.json()) as any;
    id = json?.id ?? json?.item?.id ?? "";
  } catch {
    // ignore parse errors
  }
  return { id, status: res.status, timeMs };
}

/** Pretty-print stats to console for manual review. */
function logStats(label: string, stats: Stats) {
  console.log(
    `[${label}] n=${stats.count}  p50=${stats.p50.toFixed(1)}ms  p95=${stats.p95.toFixed(1)}ms  p99=${stats.p99.toFixed(1)}ms  ` +
      `min=${stats.min.toFixed(1)}ms  max=${stats.max.toFixed(1)}ms  mean=${stats.mean.toFixed(1)}ms  errorRate=${(stats.errorRate * 100).toFixed(1)}%`,
  );
}

// ---------------------------------------------------------------------------
// Test suites
// ---------------------------------------------------------------------------

describe.skip("Gateway Drive — Load Tests", () => {
  // --------------------------------------------------
  // Setup: ensure the server is reachable and seed data
  // --------------------------------------------------

  beforeAll(async () => {
    // Health check
    try {
      const res = await fetch(`${BASE}/api/drive/items`, {
        signal: AbortSignal.timeout(3000),
      });
      expect(res.ok).toBe(true);
    } catch {
      throw new Error(
        `Gateway server not reachable at ${BASE}. Start the app or daemon first.`,
      );
    }

    // Discover or create a file we can use for download/view tests.
    const listRes = await fetch(`${BASE}/api/drive/items`);
    const items = (await listRes.json()) as any[];
    const existingFile = items?.find(
      (i: any) => i.item_type === "file" && i.size > 0,
    );

    if (existingFile) {
      knownFileId = existingFile.id;
      knownFileName = existingFile.name;
    } else {
      // Upload a seed file
      const seed = randomBuffer(4096);
      const uploaded = await uploadTestFile("load-test-seed.bin", seed);
      expect(uploaded.status).toBeLessThan(300);
      knownFileId = uploaded.id;
      knownFileName = "load-test-seed.bin";
      if (uploaded.id) createdItemIds.push(uploaded.id);
    }

    // Discover or create a share token
    const sharesRes = await fetch(`${BASE}/api/drive/shares`);
    const shares = (await sharesRes.json()) as any[];
    const existingShare = shares?.find((s: any) => s.token);

    if (existingShare) {
      knownShareToken = existingShare.token;
    } else if (knownFileId) {
      const shareRes = await fetch(`${BASE}/api/drive/share`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ item_id: knownFileId }),
      });
      if (shareRes.ok) {
        const shareData = (await shareRes.json()) as any;
        knownShareToken = shareData?.token ?? "";
        if (knownShareToken) createdShareTokens.push(knownShareToken);
      }
    }
  }, 15_000);

  afterAll(async () => {
    // Clean up shares created during tests
    for (const token of createdShareTokens) {
      try {
        await fetch(`${BASE}/api/drive/share/${token}`, { method: "DELETE" });
      } catch {
        // best-effort cleanup
      }
    }
    // Clean up uploaded files
    for (const id of createdItemIds) {
      try {
        await fetch(`${BASE}/api/drive/items/${id}`, { method: "DELETE" });
      } catch {
        // best-effort cleanup
      }
    }
  }, 15_000);

  // ====================================================================
  // 1. Drive API CRUD under load
  // ====================================================================

  describe("Drive API CRUD under load", () => {
    it("GET /api/drive/items — 10 concurrent list requests", async () => {
      const { stats } = await runConcurrent(
        () => measureRequest(`${BASE}/api/drive/items`),
        10,
      );
      logStats("list-items x10", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(2000);
    });

    it("GET /api/drive/items — 50 concurrent list requests", async () => {
      const { stats } = await runConcurrent(
        () => measureRequest(`${BASE}/api/drive/items`),
        50,
      );
      logStats("list-items x50", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(5000);
    });

    it("POST /api/drive/upload — 10 concurrent small file uploads", async () => {
      const results: RequestResult[] = [];

      const uploads = Array.from({ length: 10 }, async (_, i) => {
        const buf = randomBuffer(1024);
        const r = await uploadTestFile(`load-upload-${i}.bin`, buf);
        if (r.id) createdItemIds.push(r.id);
        results.push({ status: r.status, timeMs: r.timeMs, bodySize: 0 });
      });
      await Promise.all(uploads);

      const stats = computeStats(results);
      logStats("upload x10", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(5000);
    }, 30_000);

    it("GET /api/drive/download/:id/:filename — 10 concurrent downloads", async () => {
      expect(knownFileId).toBeTruthy();
      const { stats } = await runConcurrent(
        () =>
          measureRequest(
            `${BASE}/api/drive/download/${knownFileId}/${knownFileName}`,
          ),
        10,
      );
      logStats("download x10", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(3000);
    });

    it("GET /api/drive/download/:id/:filename — 50 concurrent downloads", async () => {
      expect(knownFileId).toBeTruthy();
      const { stats } = await runConcurrent(
        () =>
          measureRequest(
            `${BASE}/api/drive/download/${knownFileId}/${knownFileName}`,
          ),
        50,
      );
      logStats("download x50", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(5000);
    });

    it("GET /api/drive/view/:id/:filename — 10 concurrent preview loads", async () => {
      expect(knownFileId).toBeTruthy();
      const { stats } = await runConcurrent(
        () =>
          measureRequest(
            `${BASE}/api/drive/view/${knownFileId}/${knownFileName}`,
          ),
        10,
      );
      logStats("view x10", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(3000);
    });

    it("GET /api/drive/view/:id/:filename — 50 concurrent preview loads", async () => {
      expect(knownFileId).toBeTruthy();
      const { stats } = await runConcurrent(
        () =>
          measureRequest(
            `${BASE}/api/drive/view/${knownFileId}/${knownFileName}`,
          ),
        50,
      );
      logStats("view x50", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(5000);
    });
  });

  // ====================================================================
  // 2. File serving performance
  // ====================================================================

  describe("File serving performance", () => {
    const fileSizes = [
      { label: "1KB", bytes: 1024 },
      { label: "100KB", bytes: 100 * 1024 },
      { label: "1MB", bytes: 1024 * 1024 },
      { label: "10MB", bytes: 10 * 1024 * 1024 },
    ];

    for (const { label, bytes } of fileSizes) {
      it(`upload + download speed for ${label} file`, async () => {
        const buf = randomBuffer(bytes);
        const uploaded = await uploadTestFile(
          `perf-test-${label}.bin`,
          buf,
        );
        expect(uploaded.status).toBeLessThan(300);
        if (uploaded.id) createdItemIds.push(uploaded.id);

        const uploadSpeedMBs =
          bytes / 1024 / 1024 / (uploaded.timeMs / 1000);
        console.log(
          `[upload ${label}] ${uploaded.timeMs.toFixed(1)}ms  (${uploadSpeedMBs.toFixed(2)} MB/s)`,
        );

        // Download and measure
        const dlResult = await measureRequest(
          `${BASE}/api/drive/download/${uploaded.id}/perf-test-${label}.bin`,
        );
        expect(dlResult.status).toBe(200);
        const dlSpeedMBs =
          dlResult.bodySize / 1024 / 1024 / (dlResult.timeMs / 1000);
        console.log(
          `[download ${label}] ${dlResult.timeMs.toFixed(1)}ms  body=${dlResult.bodySize}  (${dlSpeedMBs.toFixed(2)} MB/s)`,
        );

        // Sanity: downloaded size should roughly match uploaded size
        expect(dlResult.bodySize).toBeGreaterThanOrEqual(bytes * 0.9);
      }, 60_000);
    }

    it("concurrent download throughput — 20 parallel downloads", async () => {
      expect(knownFileId).toBeTruthy();
      const { stats } = await runConcurrent(
        () =>
          measureRequest(
            `${BASE}/api/drive/download/${knownFileId}/${knownFileName}`,
          ),
        20,
      );
      logStats("concurrent-download x20", stats);
      expect(stats.errorRate).toBe(0);
    });

    it("preview page render time — 20 parallel requests", async () => {
      expect(knownFileId).toBeTruthy();
      const { stats } = await runConcurrent(
        () =>
          measureRequest(
            `${BASE}/api/drive/view/${knownFileId}/${knownFileName}`,
          ),
        20,
      );
      logStats("preview-render x20", stats);
      expect(stats.errorRate).toBe(0);
      // Preview pages should be fast since they are server-rendered HTML
      expect(stats.p95).toBeLessThan(3000);
    });
  });

  // ====================================================================
  // 3. Share link performance
  // ====================================================================

  describe("Share link performance", () => {
    it("GET /drive/:token — 10 concurrent public share page loads", async () => {
      if (!knownShareToken) {
        console.log("  (skipped: no share token available)");
        return;
      }
      const { stats } = await runConcurrent(
        () => measureRequest(`${BASE}/drive/${knownShareToken}`),
        10,
      );
      logStats("public-share x10", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(3000);
    });

    it("GET /drive/:token — 50 concurrent public share page loads", async () => {
      if (!knownShareToken) {
        console.log("  (skipped: no share token available)");
        return;
      }
      const { stats } = await runConcurrent(
        () => measureRequest(`${BASE}/drive/${knownShareToken}`),
        50,
      );
      logStats("public-share x50", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(5000);
    });

    it("POST /api/drive/share — 10 concurrent share creation", async () => {
      expect(knownFileId).toBeTruthy();

      const results: RequestResult[] = [];
      const tokens: string[] = [];

      const tasks = Array.from({ length: 10 }, async () => {
        const start = performance.now();
        try {
          const res = await fetch(`${BASE}/api/drive/share`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ item_id: knownFileId }),
          });
          const json = (await res.json()) as any;
          const token = json?.token ?? "";
          if (token) tokens.push(token);
          results.push({
            status: res.status,
            timeMs: performance.now() - start,
            bodySize: 0,
          });
        } catch (err: any) {
          results.push({
            status: 0,
            timeMs: performance.now() - start,
            bodySize: 0,
            error: err?.message,
          });
        }
      });
      await Promise.all(tasks);

      // Register tokens for cleanup
      createdShareTokens.push(...tokens);

      const stats = computeStats(results);
      logStats("share-create x10", stats);
      expect(stats.errorRate).toBe(0);
      expect(stats.p95).toBeLessThan(3000);
    }, 15_000);
  });

  // ====================================================================
  // 4. Stress patterns
  // ====================================================================

  describe("Stress patterns", () => {
    it("burst: 100 requests in ~1 second", async () => {
      const { stats } = await runConcurrent(
        () => measureRequest(`${BASE}/api/drive/items`),
        100,
      );
      logStats("burst x100", stats);
      // Allow some errors under extreme burst but flag if too many
      expect(stats.errorRate).toBeLessThan(0.1);
      expect(stats.p99).toBeLessThan(10_000);
    }, 30_000);

    it("sustained: 10 req/s for 30 seconds (list items)", async () => {
      const { stats } = await runSustained(
        () => measureRequest(`${BASE}/api/drive/items`),
        10,
        30,
      );
      logStats("sustained 10rps/30s", stats);
      expect(stats.errorRate).toBeLessThan(0.05);
      expect(stats.p95).toBeLessThan(5000);
      expect(stats.count).toBeGreaterThanOrEqual(250); // ~300 expected
    }, 60_000);

    it("sustained: 10 req/s for 30 seconds (downloads)", async () => {
      expect(knownFileId).toBeTruthy();
      const { stats } = await runSustained(
        () =>
          measureRequest(
            `${BASE}/api/drive/download/${knownFileId}/${knownFileName}`,
          ),
        10,
        30,
      );
      logStats("sustained-download 10rps/30s", stats);
      expect(stats.errorRate).toBeLessThan(0.05);
      expect(stats.p95).toBeLessThan(5000);
      expect(stats.count).toBeGreaterThanOrEqual(250);
    }, 60_000);

    it("response time percentiles — mixed workload burst", async () => {
      // Interleave list, download, and view requests
      const fns = [
        () => measureRequest(`${BASE}/api/drive/items`),
        () =>
          measureRequest(
            `${BASE}/api/drive/download/${knownFileId}/${knownFileName}`,
          ),
        () =>
          measureRequest(
            `${BASE}/api/drive/view/${knownFileId}/${knownFileName}`,
          ),
      ];
      const promises = Array.from({ length: 90 }, (_, i) => fns[i % 3]());
      const results = await Promise.all(promises);
      const stats = computeStats(results);
      logStats("mixed-burst x90", stats);

      console.log(
        `  Percentile breakdown:\n` +
          `    p50  = ${stats.p50.toFixed(1)} ms\n` +
          `    p95  = ${stats.p95.toFixed(1)} ms\n` +
          `    p99  = ${stats.p99.toFixed(1)} ms\n` +
          `    min  = ${stats.min.toFixed(1)} ms\n` +
          `    max  = ${stats.max.toFixed(1)} ms\n` +
          `    mean = ${stats.mean.toFixed(1)} ms`,
      );

      expect(stats.errorRate).toBeLessThan(0.1);
    }, 30_000);
  });
});

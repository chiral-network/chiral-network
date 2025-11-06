import { describe, it, expect } from "vitest";
import { spawn } from "child_process";
import WebSocket from "ws";
import path from "path";

const SERVER_PATH = path.resolve("src/lib/services/server/server.cjs");

function wait(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

async function startServer(
  port: number = 9000
): Promise<{ node: any; port: number }> {
  const node = spawn(process.execPath, [SERVER_PATH], {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, PORT: port.toString(), HOST: "127.0.0.1" },
  });
  node.stdout.on("data", (d: Buffer) => process.stdout.write(`[server] ${d}`));
  node.stderr.on("data", (d: Buffer) => process.stderr.write(`[server] ${d}`));

  // Wait until server prints listening message or timeout. If the process exits
  // before printing the listening message, fail early.
  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      node.stdout.off("data", onData);
      node.removeListener("exit", onExit as any);
      reject(new Error("server startup timeout"));
    }, 8000);
    const onData = (d: Buffer) => {
      const s = d.toString();
      if (s.includes("SignalingServer] listening")) {
        clearTimeout(timeout);
        node.stdout.off("data", onData);
        node.removeListener("exit", onExit as any);
        resolve();
      }
    };
    const onExit = (code: number | null) => {
      clearTimeout(timeout);
      node.stdout.off("data", onData);
      reject(new Error("server process exited before ready: " + code));
    };
    node.on("exit", onExit as any);
    node.stdout.on("data", onData);
  });

  return { node, port };
}

async function stopServer(serverInfo: {
  node: any;
  port: number;
}): Promise<void> {
  return new Promise((resolve) => {
    serverInfo.node.on("exit", () => resolve());
    serverInfo.node.kill();
  });
}

describe("Signaling Server", () => {
  it("should register peers and broadcast peer list", async () => {
    const serverInfo = await startServer(9002);

  const a = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);
  const b = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);

    await Promise.all([
      new Promise<void>((r) => a.on("open", r)),
      new Promise<void>((r) => b.on("open", r)),
    ]);

    let aPeers: string[] | null = null;
    let bPeers: string[] | null = null;

    a.on("message", (data: Buffer) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === "peers") aPeers = msg.peers;
    });
    b.on("message", (data: Buffer) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === "peers") bPeers = msg.peers;
    });

    // Register both
    a.send(JSON.stringify({ type: "register", clientId: "A" }));
    b.send(JSON.stringify({ type: "register", clientId: "B" }));

    // wait
    await wait(300);

    expect(Array.isArray(aPeers)).toBe(true);
    expect(Array.isArray(bPeers)).toBe(true);
    // Each should be aware of the other (server sends peers excluding self on initial register)
    expect(aPeers!.includes("B") || bPeers!.includes("A")).toBe(true);

    a.close();
    b.close();
    await stopServer(serverInfo);
  }, 20000);

  it("should route direct messages using `to` field", async () => {
    const serverInfo = await startServer(9003);

  const a = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);
  const b = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);

    await Promise.all([
      new Promise<void>((r) => a.on("open", r)),
      new Promise<void>((r) => b.on("open", r)),
    ]);

    // Register
    a.send(JSON.stringify({ type: "register", clientId: "A2" }));
    b.send(JSON.stringify({ type: "register", clientId: "B2" }));

    let received: any = null;
    b.on("message", (d: Buffer) => {
      const m = JSON.parse(d.toString());
      if (m.message && m.message.test === "hello") received = m;
    });

    await wait(200);

    // Send direct message A -> B
    a.send(
      JSON.stringify({
        type: "message",
        to: "B2",
        from: "A2",
        message: { test: "hello" },
      })
    );

    await wait(200);

    expect(received).toBeTruthy();
    expect(received.message.test).toBe("hello");

    a.close();
    b.close();
    await stopServer(serverInfo);
  }, 20000);

  it("should fall back to broadcast when no `to` field", async () => {
    const serverInfo = await startServer(9004);

  const a = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);
  const b = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);
  const c = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);

    await Promise.all([
      new Promise<void>((r) => a.on("open", r)),
      new Promise<void>((r) => b.on("open", r)),
      new Promise<void>((r) => c.on("open", r)),
    ]);

    a.send(JSON.stringify({ type: "register", clientId: "A3" }));
    b.send(JSON.stringify({ type: "register", clientId: "B3" }));
    c.send(JSON.stringify({ type: "register", clientId: "C3" }));

    let bGot = false;
    let cGot = false;

    b.on("message", (d: Buffer) => {
      const m = JSON.parse(d.toString());
      if (m.broadcast === "ping") bGot = true;
    });
    c.on("message", (d: Buffer) => {
      const m = JSON.parse(d.toString());
      if (m.broadcast === "ping") cGot = true;
    });

    await wait(200);

    a.send(JSON.stringify({ type: "broadcast", broadcast: "ping" }));

    await wait(200);

    expect(bGot && cGot).toBe(true);

    a.close();
    b.close();
    c.close();
    await stopServer(serverInfo);
  }, 20000);

  it("should update peer list on disconnect", async () => {
    const serverInfo = await startServer(9005);

  const a = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);
  const b = new WebSocket(`ws://127.0.0.1:${serverInfo.port}`);

    await Promise.all([
      new Promise<void>((r) => a.on("open", r)),
      new Promise<void>((r) => b.on("open", r)),
    ]);

    a.send(JSON.stringify({ type: "register", clientId: "A4" }));
    b.send(JSON.stringify({ type: "register", clientId: "B4" }));

    let latestPeers: string[] | null = null;
    b.on("message", (d: Buffer) => {
      const m = JSON.parse(d.toString());
      if (m.type === "peers") latestPeers = m.peers;
    });

    await wait(200);
    // Close A, server should notify B
    a.close();
    await wait(300);

    expect(Array.isArray(latestPeers)).toBe(true);
    expect(latestPeers!.includes("A4")).toBe(false);

    b.close();
    await stopServer(serverInfo);
  }, 20000);
});

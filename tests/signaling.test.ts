import { describe, it, expect } from "vitest";
import { spawn } from "child_process";
import WebSocket from "ws";
import path from "path";

function wait(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

async function startServer(
  port: number = 9000
): Promise<{ node: any; port: number }> {
  const serverPath = path.resolve("src/lib/services/server/server.cjs");
  const node = spawn(process.execPath, [serverPath], {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, PORT: port.toString() },
  });

  node.stdout.on("data", (d: Buffer) => process.stdout.write(`[server] ${d}`));
  node.stderr.on("data", (d: Buffer) => process.stderr.write(`[server] ${d}`));

  // Wait a bit for server to start
  await wait(300);
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

describe("Signaling Integration", () => {
  it("should relay messages between peers", async () => {
    const serverInfo = await startServer(9001); // Use a different port

    const url = `ws://localhost:${serverInfo.port}`;

    const a = new WebSocket(url);
    const b = new WebSocket(url);

    const results: {
      aPeers: string[] | null;
      bPeers: string[] | null;
      bReceived: any;
    } = {
      aPeers: null,
      bPeers: null,
      bReceived: null,
    };

    const openPromises: Promise<void>[] = [];
    openPromises.push(new Promise((res) => a.on("open", res)));
    openPromises.push(new Promise((res) => b.on("open", res)));

    await Promise.all(openPromises);

    // Register clients with specific IDs first
    a.send(JSON.stringify({ type: "register", clientId: "test-a" }));
    b.send(JSON.stringify({ type: "register", clientId: "test-b" }));

    a.on("message", (data: Buffer) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === "peers") {
        results.aPeers = msg.peers;
      }
    });

    b.on("message", (data: Buffer) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === "peers") {
        results.bPeers = msg.peers;
      }

      // if it's a direct message, capture it
      if (msg.message) {
        results.bReceived = msg;
      }
    });

    // wait a moment to receive peer lists
    await wait(300);

    // send a test message from A to B using server forwarding format (to)
    const testMsg = {
      type: "message",
      to: "test-b",
      from: "test-a",
      message: { hello: "world" },
    };
    a.send(JSON.stringify(testMsg));

    // wait for relay
    await wait(300);

    expect(results.bReceived).toBeTruthy();
    expect(results.bReceived.message.hello).toBe("world");

    // cleanup
    a.close();
    b.close();
    await stopServer(serverInfo);
  });
});

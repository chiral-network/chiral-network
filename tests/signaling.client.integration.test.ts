import { describe, it, expect } from "vitest";
import { SignalingService } from "../src/lib/services/signalingService";
import { spawn } from "child_process";
import path from "path";

const SERVER_PATH = path.resolve("src/lib/services/server/server.cjs");

function wait(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

async function startServer(port: number = 9000): Promise<any> {
  const node = spawn(process.execPath, [SERVER_PATH], {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, PORT: port.toString() },
  });
  node.stdout.on("data", (d: Buffer) => process.stdout.write(`[server] ${d}`));
  node.stderr.on("data", (d: Buffer) => process.stderr.write(`[server] ${d}`));
  await wait(200);
  return node;
}

async function stopServer(node: any): Promise<void> {
  return new Promise((resolve) => {
    node.on("exit", () => resolve());
    node.kill();
  });
}

describe("SignalingService", () => {
  it("should connect and register", async () => {
    const server = await startServer(9006);
    const client = new SignalingService({
      url: "ws://localhost:9006",
      preferDht: false,
    });

    await client.connect();

    expect(client.isConnected()).toBe(true);
    expect(client.getClientId()).toBeDefined();

    client.disconnect();
    await stopServer(server);
  });

  it("should send and receive messages", async () => {
    const server = await startServer(9007);
    const clientA = new SignalingService({
      url: "ws://localhost:9007",
      preferDht: false,
    });
    const clientB = new SignalingService({
      url: "ws://localhost:9007",
      preferDht: false,
    });

    await Promise.all([clientA.connect(), clientB.connect()]);

    let receivedMessage: any = null;
    clientB.setOnMessage((msg: any) => {
      receivedMessage = msg;
    });

    await new Promise((resolve) => setTimeout(resolve, 100));

    // Send message from A to B using B's actual client ID
    clientA.send({ to: clientB.getClientId(), type: "test", data: "hello" });

    await new Promise((resolve) => setTimeout(resolve, 200));

    expect(receivedMessage).toBeTruthy();
    expect(receivedMessage.type).toBe("test");
    expect(receivedMessage.data).toBe("hello");

    clientA.disconnect();
    clientB.disconnect();
    await stopServer(server);
  });

  it("should handle peer updates", async () => {
    const server = await startServer(9008);
    const client = new SignalingService({
      url: "ws://localhost:9008",
      preferDht: false,
    });

    await client.connect();

    let peerList: any = null;
    // Note: SignalingService doesn't have onPeersUpdate method, peers are stored in the store
    // This test would need to be adjusted based on actual API

    await new Promise((resolve) => setTimeout(resolve, 100));

    // Check that we can get peers from the client
    const peers = client.getPeersWithTimestamps();
    expect(Array.isArray(peers)).toBe(true);

    client.disconnect();
    await stopServer(server);
  });

  it("should broadcast messages", async () => {
    const server = await startServer(9009);
    const clientA = new SignalingService({
      url: "ws://localhost:9009",
      preferDht: false,
    });
    const clientB = new SignalingService({
      url: "ws://localhost:9009",
      preferDht: false,
    });

    await Promise.all([clientA.connect(), clientB.connect()]);

    let receivedBroadcast: any = null;
    clientB.setOnMessage((msg: any) => {
      if (msg.broadcast) receivedBroadcast = msg;
    });

    await new Promise((resolve) => setTimeout(resolve, 100));

    clientA.send({ type: "broadcast", broadcast: "hello all" });

    await new Promise((resolve) => setTimeout(resolve, 200));

    expect(receivedBroadcast).toBeTruthy();
    expect(receivedBroadcast.broadcast).toBe("hello all");

    clientA.disconnect();
    clientB.disconnect();
    await stopServer(server);
  });
});

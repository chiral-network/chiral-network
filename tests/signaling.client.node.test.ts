import { describe, it, expect } from "vitest";
import { SignalingService } from "../src/lib/services/signalingService";

describe("SignalingService Node.js Environment", () => {
  it("should handle connection errors gracefully", async () => {
    const client = new SignalingService({
      url: "ws://nonexistent:9000",
      preferDht: false,
    });

    let errorOccurred = false;
    try {
      await client.connect();
    } catch (error) {
      errorOccurred = true;
    }

    expect(errorOccurred).toBe(true);
  });

  it("should handle reconnection attempts when server is unavailable", async () => {
    const client = new SignalingService({
      url: "ws://localhost:9999",
      preferDht: false,
    }); // Use a port with no server

    let connectionFailed = false;
    try {
      await client.connect();
    } catch (error) {
      connectionFailed = true;
    }

    expect(connectionFailed).toBe(true);
    expect(client.isConnected()).toBe(false);
  });

  it("should handle message queuing when disconnected", async () => {
    const client = new SignalingService({
      url: "ws://localhost:9000",
      preferDht: false,
    });

    // Try to send message when not connected - should not throw, should enqueue
    expect(() => {
      client.send({ to: "peer", type: "test" });
    }).not.toThrow();

    client.disconnect();
  });

  it("should validate client ID format", () => {
    const client = new SignalingService({
      url: "ws://localhost:9000",
      preferDht: false,
    });
    const clientId = client.getClientId();

    expect(typeof clientId).toBe("string");
    expect(clientId.length).toBeGreaterThan(0);
  });
});

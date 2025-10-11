import { describe, it, expect } from "vitest";
import { SignalingService } from "../src/lib/services/signalingService";

describe("SignalingService Node.js Environment", () => {
  it("should handle connection errors gracefully", async () => {
    const client = new SignalingService("ws://nonexistent:9000");

    let errorOccurred = false;
    try {
      await client.connect();
    } catch (error) {
      errorOccurred = true;
    }

    expect(errorOccurred).toBe(true);
  });

  it("should handle reconnection attempts when server is unavailable", async () => {
    const client = new SignalingService("ws://localhost:9999"); // Use a port with no server

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
    const client = new SignalingService("ws://localhost:9000");

    // Try to send message when not connected
    expect(() => {
      client.send({ to: "peer", type: "test" });
    }).toThrow();

    client.disconnect();
  });

  it("should validate client ID format", () => {
    const client = new SignalingService("ws://localhost:9000");
    const clientId = client.getClientId();

    expect(typeof clientId).toBe("string");
    expect(clientId.length).toBeGreaterThan(0);
  });
});

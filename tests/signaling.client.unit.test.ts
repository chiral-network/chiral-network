import { describe, it, expect, vi } from "vitest";
import { SignalingService } from "../src/lib/services/signalingService.ts";

describe("SignalingService TypeScript Tests", () => {
  it("should create service with default URL", () => {
    const service = new SignalingService();
    expect(service.getClientId()).toBeDefined();
    expect(typeof service.getClientId()).toBe("string");
  });

  it("should create service with custom URL", () => {
    const customUrl = "ws://custom-server:8080";
    const service = new SignalingService(customUrl);
    expect(service.getClientId()).toBeDefined();
  });

  it("should generate unique client IDs", () => {
    const service1 = new SignalingService();
    const service2 = new SignalingService();

    expect(service1.getClientId()).not.toBe(service2.getClientId());
  });

  it("should handle message handler assignment", () => {
    const service = new SignalingService();
    const mockHandler = vi.fn();

    expect(() => service.setOnMessage(mockHandler)).not.toThrow();
  });
});

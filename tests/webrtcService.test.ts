import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { createWebRTCSession, type WebRTCOptions, type IceServer } from "../src/lib/services/webrtcService";
import { get } from "svelte/store";

// Mock RTCPeerConnection
class MockRTCPeerConnection {
  connectionState: RTCPeerConnectionState = "new";
  localDescription: RTCSessionDescriptionInit | null = null;
  remoteDescription: RTCSessionDescriptionInit | null = null;
  onconnectionstatechange: (() => void) | null = null;
  ondatachannel: ((ev: { channel: MockRTCDataChannel }) => void) | null = null;
  onicecandidate: ((ev: { candidate: RTCIceCandidate | null }) => void) | null = null;

  constructor(config?: RTCConfiguration) {
    // Store config for testing
    (this as any)._config = config;
  }

  createDataChannel(label: string, options?: any): MockRTCDataChannel {
    return new MockRTCDataChannel(label, options);
  }

  async createOffer(): Promise<RTCSessionDescriptionInit> {
    return { type: "offer", sdp: "mock-offer-sdp" };
  }

  async createAnswer(): Promise<RTCSessionDescriptionInit> {
    return { type: "answer", sdp: "mock-answer-sdp" };
  }

  async setLocalDescription(desc: RTCSessionDescriptionInit): Promise<void> {
    this.localDescription = desc;
  }

  async setRemoteDescription(desc: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescription = desc;
  }

  async addIceCandidate(candidate: RTCIceCandidateInit): Promise<void> {
    // Mock implementation
  }

  close(): void {
    this.connectionState = "closed";
  }
}

class MockRTCDataChannel {
  label: string;
  readyState: RTCDataChannelState = "connecting";
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: ((e: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;

  constructor(label: string, options?: any) {
    this.label = label;
  }

  send(data: string | ArrayBuffer | Blob): void {
    // Mock implementation
  }

  close(): void {
    this.readyState = "closed";
    this.onclose?.();
  }
}

// Setup global mocks
beforeEach(() => {
  global.RTCPeerConnection = MockRTCPeerConnection as any;
  global.RTCDataChannel = MockRTCDataChannel as any;
});

describe("webrtcService", () => {
  describe("createWebRTCSession", () => {
    it("should create session with default options", () => {
      const session = createWebRTCSession();

      expect(session.pc).toBeDefined();
      expect(session.channel).toBeDefined();
      expect(get(session.connectionState)).toBe("new");
      expect(get(session.channelState)).toBe("connecting");
    });

    it("should create session as non-initiator", () => {
      const session = createWebRTCSession({ isInitiator: false });

      expect(session.pc).toBeDefined();
      expect(session.channel).toBeNull();
    });

    it("should sanitize ICE servers with query params", () => {
      const iceServers: IceServer[] = [
        { urls: "stun:stun.example.com:3478?transport=udp" },
      ];

      const session = createWebRTCSession({ iceServers });
      
      const config = (session.pc as any)._config;
      expect(config.iceServers[0].urls).toBe("stun:stun.example.com:3478");
    });

    it("should handle array of ICE server URLs", () => {
      const iceServers: IceServer[] = [
        { urls: ["stun:stun1.example.com:3478?foo=bar", "stun:stun2.example.com:3478"] },
      ];

      const session = createWebRTCSession({ iceServers });
      
      const config = (session.pc as any)._config;
      expect(config.iceServers[0].urls).toEqual([
        "stun:stun1.example.com:3478",
        "stun:stun2.example.com:3478"
      ]);
    });

    describe("createOffer", () => {
      it("should create and set local description", async () => {
        const onLocalDescription = vi.fn();
        const session = createWebRTCSession({ onLocalDescription });

        const offer = await session.createOffer();

        expect(offer.type).toBe("offer");
        expect(offer.sdp).toBe("mock-offer-sdp");
        expect(onLocalDescription).toHaveBeenCalledWith(offer);
      });
    });

    describe("acceptOfferCreateAnswer", () => {
      it("should accept offer and create answer", async () => {
        const onLocalDescription = vi.fn();
        const session = createWebRTCSession({ 
          isInitiator: false,
          onLocalDescription 
        });

        const offer: RTCSessionDescriptionInit = {
          type: "offer",
          sdp: "remote-offer-sdp",
        };

        const answer = await session.acceptOfferCreateAnswer(offer);

        expect(answer.type).toBe("answer");
        expect(session.pc.remoteDescription).toEqual(offer);
        expect(onLocalDescription).toHaveBeenCalledWith(answer);
      });
    });

    describe("acceptAnswer", () => {
      it("should set remote description", async () => {
        const session = createWebRTCSession();

        const answer: RTCSessionDescriptionInit = {
          type: "answer",
          sdp: "remote-answer-sdp",
        };

        await session.acceptAnswer(answer);

        expect(session.pc.remoteDescription).toEqual(answer);
      });
    });

    describe("send", () => {
      it("should send string data when channel is open", () => {
        const session = createWebRTCSession();
        const mockSend = vi.spyOn(session.channel!, "send");
        
        // Simulate channel opening
        session.channel!.readyState = "open";

        session.send("test message");

        expect(mockSend).toHaveBeenCalledWith("test message");
      });

      it("should send ArrayBuffer data", () => {
        const session = createWebRTCSession();
        const mockSend = vi.spyOn(session.channel!, "send");
        
        session.channel!.readyState = "open";
        const buffer = new ArrayBuffer(8);

        session.send(buffer);

        expect(mockSend).toHaveBeenCalledWith(buffer);
      });

      it("should throw error if channel not open", () => {
        const session = createWebRTCSession();
        session.channel!.readyState = "connecting";

        expect(() => session.send("test")).toThrow("DataChannel not open");
      });

      it("should throw error if channel is null", () => {
        const session = createWebRTCSession({ isInitiator: false });

        expect(() => session.send("test")).toThrow("DataChannel not open");
      });
    });

    describe("close", () => {
      it("should close channel and peer connection", () => {
        const session = createWebRTCSession();
        const channelCloseSpy = vi.spyOn(session.channel!, "close");
        const pcCloseSpy = vi.spyOn(session.pc, "close");

        session.close();

        expect(channelCloseSpy).toHaveBeenCalled();
        expect(pcCloseSpy).toHaveBeenCalled();
      });

      it("should handle errors during close gracefully", () => {
        const session = createWebRTCSession();
        vi.spyOn(session.channel!, "close").mockImplementation(() => {
          throw new Error("Close failed");
        });

        const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});

        expect(() => session.close()).not.toThrow();
        expect(consoleSpy).toHaveBeenCalled();

        consoleSpy.mockRestore();
      });
    });

    describe("event handlers", () => {
      it("should call onDataChannelOpen when channel opens", () => {
        const onDataChannelOpen = vi.fn();
        const session = createWebRTCSession({ onDataChannelOpen });

        session.channel!.readyState = "open";
        session.channel!.onopen?.();

        expect(onDataChannelOpen).toHaveBeenCalled();
        expect(get(session.channelState)).toBe("open");
      });

      it("should call onDataChannelClose when channel closes", () => {
        const onDataChannelClose = vi.fn();
        const session = createWebRTCSession({ onDataChannelClose });

        session.channel!.readyState = "closed";
        session.channel!.onclose?.();

        expect(onDataChannelClose).toHaveBeenCalled();
        expect(get(session.channelState)).toBe("closed");
      });

      it("should call onMessage when message received", () => {
        const onMessage = vi.fn();
        const session = createWebRTCSession({ onMessage });

        const mockData = "test message";
        session.channel!.onmessage?.({ data: mockData } as MessageEvent);

        expect(onMessage).toHaveBeenCalledWith(mockData);
      });

      it("should call onConnectionStateChange", () => {
        const onConnectionStateChange = vi.fn();
        const session = createWebRTCSession({ onConnectionStateChange });

        session.pc.connectionState = "connected";
        session.pc.onconnectionstatechange?.();

        expect(onConnectionStateChange).toHaveBeenCalledWith("connected");
        expect(get(session.connectionState)).toBe("connected");
      });

      it("should call onLocalIceCandidate", () => {
        const onLocalIceCandidate = vi.fn();
        const session = createWebRTCSession({ onLocalIceCandidate });

        const mockCandidate = {
          candidate: "candidate:1 1 UDP 1234 192.168.1.1 5000 typ host",
          sdpMid: "0",
          sdpMLineIndex: 0,
          toJSON: () => ({ candidate: "mock" }),
        };

        session.pc.onicecandidate?.({ candidate: mockCandidate as any });

        expect(onLocalIceCandidate).toHaveBeenCalled();
      });
    });

    describe("non-initiator receives datachannel", () => {
      it("should bind to remote-created datachannel", () => {
        const onDataChannelOpen = vi.fn();
        const session = createWebRTCSession({ 
          isInitiator: false,
          onDataChannelOpen 
        });

        expect(session.channel).toBeNull();

        const remoteChannel = new MockRTCDataChannel("remote-channel");
        session.pc.ondatachannel?.({ channel: remoteChannel as any });

        expect(session.channel).toBe(remoteChannel);
      });
    });
  });
});
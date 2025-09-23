import { writable, type Writable } from "svelte/store";
import type { SignalingService } from "./signalingService";

export type IceServer = RTCIceServer;

export type WebRTCOptions = {
  iceServers?: IceServer[];
  label?: string; // datachannel label
  ordered?: boolean;
  maxRetransmits?: number;
  isInitiator?: boolean; // if true, create datachannel + offer
  peerId?: string; // target peer to connect with
  signaling?: SignalingService; // signaling service instance
  onLocalDescription?: (sdp: RTCSessionDescriptionInit) => void;
  onLocalIceCandidate?: (candidate: RTCIceCandidateInit) => void;
  onMessage?: (data: ArrayBuffer | string) => void;
  onConnectionStateChange?: (state: RTCPeerConnectionState) => void;
  onDataChannelOpen?: () => void;
  onDataChannelClose?: () => void;
  onError?: (e: unknown) => void;
};

// Strongly typed signaling messages
type SignalingMessage =
  | { type: "offer"; sdp: RTCSessionDescriptionInit; from: string; to?: string }
  | { type: "answer"; sdp: RTCSessionDescriptionInit; from: string; to?: string }
  | { type: "candidate"; candidate: RTCIceCandidateInit; from: string; to?: string };

export type WebRTCSession = {
  pc: RTCPeerConnection;
  channel: RTCDataChannel | null;
  connectionState: Writable<RTCPeerConnectionState>;
  channelState: Writable<RTCDataChannelState>;
  // signaling helpers
  createOffer: () => Promise<RTCSessionDescriptionInit>;
  acceptOfferCreateAnswer: (
    remote: RTCSessionDescriptionInit
  ) => Promise<RTCSessionDescriptionInit>;
  acceptAnswer: (remote: RTCSessionDescriptionInit) => Promise<void>;
  addRemoteIceCandidate: (candidate: RTCIceCandidateInit) => Promise<void>;
  // messaging
  send: (data: string | ArrayBuffer | Blob) => void;
  close: () => void;
  peerId?: string;
};

const defaultIceServers: IceServer[] = [
  { urls: "stun:stun.l.google.com:19302" },
  { urls: "stun:global.stun.twilio.com:3478?transport=udp" },
];

export function createWebRTCSession(opts: WebRTCOptions = {}): WebRTCSession {
  const {
    iceServers = defaultIceServers,
    label = "chiral-data",
    ordered = true,
    maxRetransmits,
    isInitiator = true,
    peerId,
    signaling,
    onLocalDescription,
    onLocalIceCandidate,
    onMessage,
    onConnectionStateChange,
    onDataChannelOpen,
    onDataChannelClose,
    onError,
  } = opts;

  const pc = new RTCPeerConnection({ iceServers });
  const connectionState = writable<RTCPeerConnectionState>(pc.connectionState);
  const channelState = writable<RTCDataChannelState>("closed");

  let channel: RTCDataChannel | null = null;

  function bindChannel(dc: RTCDataChannel) {
    channel = dc;
    channelState.set(dc.readyState);
    dc.onopen = () => {
      channelState.set(dc.readyState);
      onDataChannelOpen?.();
    };
    dc.onclose = () => {
      channelState.set(dc.readyState);
      onDataChannelClose?.();
    };
    dc.onerror = (e) => onError?.(e);
    dc.onmessage = (ev) => {
      onMessage?.(ev.data);
    };
  }

  // If not initiator, listen for remote-created channel
  pc.ondatachannel = (ev) => bindChannel(ev.channel);

  pc.onconnectionstatechange = () => {
    connectionState.set(pc.connectionState);
    onConnectionStateChange?.(pc.connectionState);
  };

  pc.onicecandidate = (ev) => {
    if (ev.candidate) {
      onLocalIceCandidate?.(ev.candidate.toJSON());
      if (signaling && peerId) {
        signaling.send({
          type: "candidate",
          candidate: ev.candidate.toJSON(),
          to: peerId,
        });
      }
    }
  };

  if (isInitiator) {
    const dc = pc.createDataChannel(label, {
      ordered,
      maxRetransmits,
    });
    bindChannel(dc);
  }

  async function createOffer(): Promise<RTCSessionDescriptionInit> {
    const offer = await pc.createOffer();
    await pc.setLocalDescription(offer);
    const sdp = pc.localDescription!;
    onLocalDescription?.(sdp);
    
    if (signaling && peerId) {
      signaling.send({ type: "offer", sdp, to: peerId });
    }

    return sdp;
  }

  async function acceptOfferCreateAnswer(
    remote: RTCSessionDescriptionInit
  ): Promise<RTCSessionDescriptionInit> {
    await pc.setRemoteDescription(remote);
    const answer = await pc.createAnswer();
    await pc.setLocalDescription(answer);
    const sdp = pc.localDescription!;
    onLocalDescription?.(sdp);
    return sdp;
  }

  async function acceptAnswer(
    remote: RTCSessionDescriptionInit
  ): Promise<void> {
    await pc.setRemoteDescription(remote);
  }

  async function addRemoteIceCandidate(
    candidate: RTCIceCandidateInit
  ): Promise<void> {
    await pc.addIceCandidate(candidate);
  }

  function send(data: string | ArrayBuffer | Blob) {
    if (!channel || channel.readyState !== "open")
      throw new Error("DataChannel not open");
    channel.send(data as any);
  }

  function close() {
    try {
      channel?.close();
    } catch (e) {
      console.error("DataChannel close error:", e);
    }
    try {
      pc.close();
    } catch (e) {
      console.error("RTCPeerConnection close error:", e);
    }
  }

  // Hook into signaling for remote messages
  if (signaling && peerId) {
    signaling.setOnMessage(async (msg: SignalingMessage) => {
      if (msg.from !== peerId) return;

      try {
        if (msg.type === "offer") {
          const answer = await acceptOfferCreateAnswer(msg.sdp);
          signaling.send({ type: "answer", sdp: answer, to: peerId });
        } else if (msg.type === "answer") {
          await acceptAnswer(msg.sdp);
        } else if (msg.type === "candidate") {
          await addRemoteIceCandidate(msg.candidate);
        }
      } catch (e) {
        console.error("Error handling signaling message:", e);
      }
    });
  }

  return {
    pc,
    channel,
    connectionState,
    channelState,
    createOffer,
    acceptOfferCreateAnswer,
    acceptAnswer,
    addRemoteIceCandidate,
    send,
    close,
    peerId,
  };
}

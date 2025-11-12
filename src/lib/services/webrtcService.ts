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
  // Keep simple/stable STUN servers (avoid query params which some browsers reject)
  { urls: "stun:stun.l.google.com:19302" },
  { urls: "stun:global.stun.twilio.com:3478" },
];

function sanitizeIceServers(servers: IceServer[]): IceServer[] {
  return servers.map((s) => {
    try {
      if (!s || !s.urls) return s;
      // urls may be string or string[]
      const normalize = (u: any) => {
        if (!u || typeof u !== 'string') return u;
        // Remove query string from URL (e.g., ?transport=udp) because some browsers reject it
        const idx = u.indexOf('?');
        if (idx > -1) return u.substring(0, idx);
        return u;
      };

      if (Array.isArray(s.urls)) {
        return { ...s, urls: s.urls.map(normalize) } as IceServer;
      }
      return { ...s, urls: normalize(s.urls as string) } as IceServer;
    } catch (e) {
      return s;
    }
  });
}

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

  const effectiveIceServers = sanitizeIceServers(iceServers);
  const pc = new RTCPeerConnection({ iceServers: effectiveIceServers });
  
  const connectionState = writable<RTCPeerConnectionState>(pc.connectionState);
  const channelState = writable<RTCDataChannelState>("closed");

  // Create session object first so we can mutate channel property
  const session: WebRTCSession = {
    pc,
    channel: null,
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

  function bindChannel(dc: RTCDataChannel) {
    session.channel = dc; // Update session.channel instead of local variable
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
    if (!session.channel || session.channel.readyState !== "open")
      throw new Error("DataChannel not open");
    session.channel.send(data as any);
  }

  function close() {
    try {
      session.channel?.close();
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

  return session;
}

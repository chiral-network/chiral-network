import { writable, type Writable } from "svelte/store";

export type IceServer = RTCIceServer;

export type WebRTCOptions = {
  iceServers?: IceServer[];
  label?: string; // datachannel label
  ordered?: boolean;
  maxRetransmits?: number;
  isInitiator?: boolean; // if true, create datachannel + offer
  onLocalDescription?: (sdp: RTCSessionDescriptionInit) => void;
  onLocalIceCandidate?: (candidate: RTCIceCandidateInit) => void;
  onMessage?: (data: ArrayBuffer | string) => void;
  onConnectionStateChange?: (state: RTCPeerConnectionState) => void;
  onDataChannelOpen?: () => void;
  onDataChannelClose?: () => void;
  onError?: (e: unknown) => void;
};

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
    } catch {}
    try {
      pc.close();
    } catch {}
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
  };
}

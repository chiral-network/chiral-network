import { ProtocolManager } from "./ProtocolManager";
import { BitTorrentProtocol } from "./bittorrentProtocol";
import { Ed2kProtocol } from "./ed2kProtocol";
import { FtpProtocol } from "./ftpProtocol";
import { HttpProtocol } from "./httpProtocol";
import { WebRTCProtocol } from "./webrtcProtocol";

export const protocolManager = new ProtocolManager("WebRTC");

protocolManager.register(new WebRTCProtocol());
protocolManager.register(new BitTorrentProtocol());
protocolManager.register(new HttpProtocol());
protocolManager.register(new FtpProtocol());
protocolManager.register(new Ed2kProtocol());

export type { Protocol } from "./types";
export type { FileIdentification, UploadOptions, UploadResult } from "./types";
export { ProtocolManager } from "./ProtocolManager";

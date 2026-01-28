import type {
  DownloadResult,
  FileIdentification,
  IContentProtocol,
  ProgressUpdate,
  PeerInfo,
  Protocol,
  UploadOptions,
  UploadResult,
} from "./types";

export class ProtocolManager {
  private protocols = new Map<Protocol, IContentProtocol>();
  private activeProtocol: Protocol;

  constructor(initialProtocol: Protocol | string) {
    this.activeProtocol = this.normalizeProtocol(initialProtocol);
  }

  private normalizeProtocol(protocol: Protocol | string): Protocol {
    return protocol.toUpperCase() as Protocol;
  }

  register(protocolImpl: IContentProtocol): void {
    const normalized = this.normalizeProtocol(protocolImpl.getName());
    this.protocols.set(normalized, protocolImpl);
  }

  setProtocol(protocol: Protocol): void {
    this.activeProtocol = this.normalizeProtocol(protocol);
  }

  getProtocolImpl(protocol?: Protocol): IContentProtocol {
    const resolvedProtocol = this.normalizeProtocol(
      protocol ?? this.activeProtocol,
    );
    const impl = this.protocols.get(resolvedProtocol);
    if (!impl) {
      throw new Error(`Protocol not registered: ${resolvedProtocol}`);
    }
    return impl;
  }

  async getPeersServing(
    identification: FileIdentification,
  ): Promise<PeerInfo[]> {
    return await this.getProtocolImpl(identification.protocol).getPeersServing(
      identification,
    );
  }

  async downloadFile(
    peerId: string,
    identification: FileIdentification,
    progressUpdate: ProgressUpdate = () => {},
    outputPath?: string,
  ): Promise<DownloadResult> {
    return await this.getProtocolImpl(identification.protocol).getContentFrom(
      peerId,
      identification,
      progressUpdate,
      outputPath,
    );
  }

  async uploadFile(
    options: UploadOptions,
    progressUpdate: ProgressUpdate = () => {},
  ): Promise<UploadResult> {
    return await this.getProtocolImpl(options.protocol).uploadFile(
      options,
      progressUpdate,
    );
  }

  async stopSharing(identification: FileIdentification): Promise<boolean> {
    return await this.getProtocolImpl(identification.protocol).stopSeeding(
      identification,
    );
  }

  async pauseDownload(identification: FileIdentification): Promise<boolean> {
    return await this.getProtocolImpl(identification.protocol).pauseDownload(
      identification,
    );
  }

  async resumeDownload(identification: FileIdentification): Promise<boolean> {
    return await this.getProtocolImpl(identification.protocol).resumeDownload(
      identification,
    );
  }

  async cancelDownload(identification: FileIdentification): Promise<boolean> {
    return await this.getProtocolImpl(identification.protocol).cancelDownload(
      identification,
    );
  }

  async cleanup(): Promise<void> {
    return;
  }
}

declare module 'qrcode' {
  type QRCodeErrorLevel = 'low' | 'medium' | 'quartile' | 'high';

  interface QRCodeRenderersOptions {
    errorCorrectionLevel?: QRCodeErrorLevel;
    margin?: number;
    scale?: number;
    width?: number;
    color?: {
      dark?: string;
      light?: string;
    };
  }

  interface QRCodeToDataURLOptions extends QRCodeRenderersOptions {
    type?: 'image/png' | 'image/jpeg' | 'image/webp';
    rendererOpts?: {
      quality?: number;
    };
  }

  export function toDataURL(
    text: string,
    options?: QRCodeToDataURLOptions
  ): Promise<string>;

  const QRCode: {
    toDataURL: typeof toDataURL;
  };

  export default QRCode;
}

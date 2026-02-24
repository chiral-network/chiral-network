function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    out += bytes[i]!.toString(16).padStart(2, "0");
  }
  return out;
}

async function sha1Hex(bytes: Uint8Array): Promise<string> {
  // Tauri WebView should have WebCrypto; Vitest/node also exposes it on globalThis.crypto in recent Node.
  if (!globalThis.crypto?.subtle) {
    throw new Error("WebCrypto (crypto.subtle) is not available for SHA-1 hashing");
  }
  const digest = await globalThis.crypto.subtle.digest("SHA-1", bytes as BufferSource);
  return bytesToHex(new Uint8Array(digest));
}

type ParseResult = { end: number };

function parseBencodeAny(bytes: Uint8Array, offset: number, onDictKeyValue?: (key: string, valueStart: number, valueEnd: number) => void): ParseResult {
  const b = bytes[offset];
  if (b === undefined) throw new Error("Unexpected end of bencode input");

  // Integer: i<number>e
  if (b === 0x69 /* 'i' */) {
    let i = offset + 1;
    while (i < bytes.length && bytes[i] !== 0x65 /* 'e' */) i++;
    if (i >= bytes.length) throw new Error("Invalid bencode integer (missing terminator)");
    return { end: i + 1 };
  }

  // List: l<items>e
  if (b === 0x6c /* 'l' */) {
    let i = offset + 1;
    while (true) {
      const ch = bytes[i];
      if (ch === undefined) throw new Error("Invalid bencode list (unexpected EOF)");
      if (ch === 0x65 /* 'e' */) return { end: i + 1 };
      const r = parseBencodeAny(bytes, i, onDictKeyValue);
      i = r.end;
    }
  }

  // Dict: d<key><value>e
  if (b === 0x64 /* 'd' */) {
    const decoder = new TextDecoder();
    let i = offset + 1;
    while (true) {
      const ch = bytes[i];
      if (ch === undefined) throw new Error("Invalid bencode dict (unexpected EOF)");
      if (ch === 0x65 /* 'e' */) return { end: i + 1 };

      // key is a byte string
      const keyParsed = parseBencodeByteString(bytes, i);
      const keyBytes = bytes.subarray(keyParsed.valueStart, keyParsed.valueEnd);
      const key = decoder.decode(keyBytes);
      i = keyParsed.end;

      const valueStart = i;
      const valueParsed = parseBencodeAny(bytes, i, onDictKeyValue);
      const valueEnd = valueParsed.end;
      onDictKeyValue?.(key, valueStart, valueEnd);
      i = valueEnd;
    }
  }

  // Byte string: <len>:<bytes>
  if (b >= 0x30 /* '0' */ && b <= 0x39 /* '9' */) {
    const s = parseBencodeByteString(bytes, offset);
    return { end: s.end };
  }

  throw new Error(`Invalid bencode prefix byte: 0x${b.toString(16)}`);
}

function parseBencodeByteString(bytes: Uint8Array, offset: number): { end: number; valueStart: number; valueEnd: number } {
  let i = offset;
  let len = 0;
  let sawDigit = false;
  while (i < bytes.length) {
    const ch = bytes[i]!;
    if (ch === 0x3a /* ':' */) break;
    if (ch < 0x30 || ch > 0x39) throw new Error("Invalid bencode string length");
    sawDigit = true;
    len = len * 10 + (ch - 0x30);
    i++;
  }
  if (!sawDigit) throw new Error("Invalid bencode string length (empty)");
  if (i >= bytes.length || bytes[i] !== 0x3a) throw new Error("Invalid bencode string (missing ':')");
  const valueStart = i + 1;
  const valueEnd = valueStart + len;
  if (valueEnd > bytes.length) throw new Error("Invalid bencode string (truncated data)");
  return { end: valueEnd, valueStart, valueEnd };
}

/**
 * Extract BitTorrent infohash (hex) from a `.torrent` file's bencoded bytes.
 * infohash = SHA1(bencode(info_dict_bytes))
 */
export async function extractInfoHashFromTorrentBytes(bytes: Uint8Array): Promise<string> {
  if (bytes.length === 0) throw new Error("Empty torrent file");
  if (bytes[0] !== 0x64 /* 'd' */) throw new Error("Invalid torrent file (expected bencoded dict)");

  let infoStart: number | null = null;
  let infoEnd: number | null = null;

  parseBencodeAny(bytes, 0, (key, valueStart, valueEnd) => {
    if (key === "info" && infoStart === null) {
      infoStart = valueStart;
      infoEnd = valueEnd;
    }
  });

  if (infoStart === null || infoEnd === null) {
    throw new Error("Torrent file does not contain an 'info' dictionary");
  }

  const infoBytes = bytes.subarray(infoStart, infoEnd);
  return (await sha1Hex(infoBytes)).toLowerCase();
}



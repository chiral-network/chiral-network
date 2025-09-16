import { WORDLIST_EN } from "./wordlist-en";

// Utilities
async function sha256(data: Uint8Array): Promise<Uint8Array> {
  const digest = await crypto.subtle.digest("SHA-256", data as BufferSource);
  return new Uint8Array(digest);
}

function bytesToBinary(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(2).padStart(8, "0"))
    .join("");
}

function binaryToBytes(bin: string): Uint8Array {
  const len = Math.ceil(bin.length / 8);
  const out = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    out[i] = parseInt(bin.slice(i * 8, i * 8 + 8).padEnd(8, "0"), 2);
  }
  return out;
}

export function generateEntropy(
  strength: 128 | 160 | 192 | 224 | 256 = 128
): Uint8Array {
  const out = new Uint8Array(strength / 8);
  crypto.getRandomValues(out);
  return out;
}

export async function entropyToMnemonic(entropy: Uint8Array): Promise<string> {
  if (![16, 20, 24, 28, 32].includes(entropy.length)) {
    throw new Error("Invalid entropy length");
  }
  const checksumLen = entropy.length / 4; // in bits
  const hash = await sha256(entropy);
  const entropyBits = bytesToBinary(entropy);
  const checksumBits = bytesToBinary(hash).slice(0, checksumLen);
  const bits = entropyBits + checksumBits;
  const words: string[] = [];
  for (let i = 0; i < bits.length / 11; i++) {
    const chunk = bits.slice(i * 11, (i + 1) * 11);
    const idx = parseInt(chunk, 2);
    words.push(WORDLIST_EN[idx]);
  }
  return words.join(" ");
}

export async function mnemonicToEntropy(mnemonic: string): Promise<Uint8Array> {
  const words = mnemonic.trim().split(/\s+/);
  if (words.length % 3 !== 0) throw new Error("Invalid mnemonic length");
  const bits = words
    .map((w) => {
      const idx = WORDLIST_EN.indexOf(w);
      if (idx === -1) throw new Error(`Unknown word: ${w}`);
      return idx.toString(2).padStart(11, "0");
    })
    .join("");
  const divider = Math.floor(bits.length / 33) * 32;
  const entropyBits = bits.slice(0, divider);
  const checksumBits = bits.slice(divider);
  const entropy = binaryToBytes(entropyBits);
  const newChecksum = bytesToBinary(await sha256(entropy)).slice(
    0,
    checksumBits.length
  );
  if (newChecksum !== checksumBits)
    throw new Error("Invalid mnemonic checksum");
  return entropy;
}

export async function validateMnemonic(m: string): Promise<boolean> {
  try {
    await mnemonicToEntropy(m);
    return true;
  } catch {
    return false;
  }
}

export async function mnemonicToSeed(
  mnemonic: string,
  passphrase = ""
): Promise<Uint8Array> {
  const pw = new TextEncoder().encode(mnemonic);
  const salt = new TextEncoder().encode("mnemonic" + passphrase);
  const key = await crypto.subtle.importKey(
    "raw",
    pw,
    { name: "PBKDF2" },
    false,
    ["deriveBits"]
  );
  const bits = await crypto.subtle.deriveBits(
    { name: "PBKDF2", hash: "SHA-512", iterations: 2048, salt },
    key,
    64 * 8
  );
  return new Uint8Array(bits);
}

export async function generateMnemonic(
  strength: 128 | 160 | 192 | 224 | 256 = 128
): Promise<string> {
  return entropyToMnemonic(generateEntropy(strength));
}

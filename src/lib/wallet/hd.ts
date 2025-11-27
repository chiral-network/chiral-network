import { generateMnemonic, mnemonicToSeed, validateMnemonic } from "./bip39";
import { fromSeed, derivePath } from "./bip32";
import { invoke } from "@tauri-apps/api/core";

export interface DerivedAccount {
  index: number;
  change: number;
  path: string;
  privateKeyHex: string;
  address: string;
}

const DEFAULT_COIN_TYPE = 98765; // Fallback per docs

// Cache for the coin type (chain ID)
let cachedCoinType: number | null = null;

async function getCoinType(): Promise<number> {
  if (cachedCoinType !== null) {
    return cachedCoinType;
  }

  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  if (isTauri) {
    try {
      cachedCoinType = await invoke<number>("get_chain_id");
      return cachedCoinType;
    } catch (error) {
      console.warn(
        "Failed to get chain ID from backend, using default:",
        error
      );
      return DEFAULT_COIN_TYPE;
    }
  }
  return DEFAULT_COIN_TYPE;
}

export async function createMnemonic(strength?: 128 | 160 | 192 | 224 | 256) {
  return generateMnemonic(strength);
}

export async function isValidMnemonic(m: string) {
  return validateMnemonic(m);
}

export async function deriveAccount(
  mnemonic: string,
  passphrase: string,
  index = 0,
  change = 0
): Promise<DerivedAccount> {
  const seed = await mnemonicToSeed(mnemonic, passphrase);
  const root = await fromSeed(seed);
  const coinType = await getCoinType();
  const path = `m/44'/${coinType}'/0'/${change}/${index}`;
  const node = await derivePath(root, path);
  const pkHex = Array.from(node.privateKey)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  // Leverage backend to compute address from private key when running in Tauri
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  let address: string;
  if (isTauri) {
    const result = await invoke<{ address: string; private_key: string }>(
      "import_chiral_account",
      { privateKey: "0x" + pkHex }
    );
    address = result.address;
  } else {
    // Web/demo fallback: compute a pseudo address using SHA-256 (note: not Keccak) of the private key
    // This is ONLY for UI/demo usage outside Tauri and is not chain-valid.
    const enc = new TextEncoder().encode(pkHex);
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", enc));
    const last20 = digest.slice(-20);
    address =
      "0x" +
      Array.from(last20)
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");
  }
  return {
    index,
    change,
    path,
    privateKeyHex: pkHex,
    address,
  };
}

export async function deriveNext(
  mnemonic: string,
  passphrase: string,
  existing: Array<{ index: number }>,
  change = 0
) {
  const nextIndex =
    existing.length > 0 ? Math.max(...existing.map((a) => a.index)) + 1 : 0;
  return deriveAccount(mnemonic, passphrase, nextIndex, change);
}

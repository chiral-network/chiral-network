export type SeedRecord = {
  id: string;
  path: string;
  hash: string;
  name?: string;
  size?: number;
  addedAt?: string; // ISO
  manifest?: any;
};

const STORAGE_KEY = "chiral.seeds.v1";

async function tauriInvoke(cmd: string, args?: any): Promise<any> {
  const invoke =
    (globalThis as any).__tauri_invoke ?? (globalThis as any).invoke;
  if (!invoke) throw new Error("tauri invoke not available");
  return invoke(cmd, args);
}

export async function saveSeedList(seeds: SeedRecord[]): Promise<void> {
  // Only store data for small files (data field is already set by addSeedWithData)
  const payload = JSON.stringify({ version: 2, seeds });
  try {
    if ((globalThis as any).__tauri_invoke || (globalThis as any).invoke) {
      await tauriInvoke("write_seed_list", { payload });
      return;
    }
  } catch (e) {
    console.warn(
      "tauri write_seed_list failed, falling back to localStorage",
      e
    );
  }
  try {
    localStorage.setItem(STORAGE_KEY, payload);
  } catch (e) {
    console.error("Failed to persist seed list to localStorage", e);
    throw e;
  }
}

export async function loadSeedList(): Promise<SeedRecord[]> {
  try {
    console.log("from tauri");
    if ((globalThis as any).__tauri_invoke || (globalThis as any).invoke) {
      const res = await tauriInvoke("read_seed_list");
      if (!res) return [];
      if (typeof res === "string") {
        try {
          const parsed = JSON.parse(res);
          if (Array.isArray(parsed.seeds)) return parsed.seeds;
        } catch (e) {
          console.warn("Failed to parse tauri read_seed_list result", e);
        }
      } else if (res && Array.isArray(res.seeds)) {
        return res.seeds;
      }
    }
  } catch (e) {
    console.warn(
      "tauri read_seed_list failed, falling back to localStorage",
      e
    );
  }
  try {
    console.log("from localstorage");
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    console.log(parsed);
    if (parsed && Array.isArray(parsed.seeds)) return parsed.seeds;
  } catch (e) {
    console.error(
      "Failed to parse persisted seed list; ignoring and returning empty list",
      e
    );
  }
  return [];
}

export async function clearSeedList(): Promise<void> {
  try {
    if ((globalThis as any).__tauri_invoke || (globalThis as any).invoke) {
      await tauriInvoke("clear_seed_list");
    }
  } catch (e) {
    // ignore
  }
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch (_) {}
}

export default { saveSeedList, loadSeedList, clearSeedList };

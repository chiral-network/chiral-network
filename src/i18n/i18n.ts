import {
  register,
  init,
  getLocaleFromNavigator,
  locale as $locale,
} from "svelte-i18n";

register("en", () => import("../locales/en.json"));
register("ko", () => import("../locales/ko.json"));
register("es", () => import("../locales/es.json"));
register("zh", () => import("../locales/zh.json"));
register("ru", () => import("../locales/ru.json"));
register("pt", () => import("../locales/pt.json"));
register("hi", () => import("../locales/hi.json"));
register("fr", () => import("../locales/fr.json"));
register("bn", () => import("../locales/bn.json"));
register("ar", () => import("../locales/ar.json"));

let store: any = null;
async function ensureStore() {
  if (!store) {
    try {
      const { Store } = await import("@tauri-apps/plugin-store");
      store = await Store.load(".settings.dat");
    } catch {
      store = null;
    }
  }
}
export async function saveLocale(lang: string) {
  await ensureStore();
  if (store) {
    await store.set("locale", lang);
    await store.save();
  } else {
    localStorage.setItem("locale", lang);
  }
}
export async function loadLocale(): Promise<string | null> {
  await ensureStore();
  if (store) return (await store.get("locale")) as string | null;
  return localStorage.getItem("locale");
}

async function detectOsLocale(): Promise<string | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const l = (await invoke("detect_locale")) as string;
    return l || null;
  } catch {
    return null;
  }
}

export async function setupI18n(initial?: string) {
  const fallback = "en";
  const stored = await loadLocale();
  const os = await detectOsLocale(); // e.g., "ko-KR"
  const nav = getLocaleFromNavigator(); // e.g., "en-US"
  const pick = (initial || stored || os || nav || fallback).split("-")[0]; // "en"/"ko"

  await init({ fallbackLocale: fallback, initialLocale: pick });

  const rtl = new Set(["ar", "he", "fa", "ur"]);
  document.documentElement.setAttribute("dir", rtl.has(pick) ? "rtl" : "ltr");
}

export async function changeLocale(lang: string) {
  $locale.set(lang);
  await saveLocale(lang);
  const rtl = new Set(["ar", "he", "fa", "ur"]);
  document.documentElement.setAttribute("dir", rtl.has(lang) ? "rtl" : "ltr");
}

declare module '@tauri-apps/api/tauri' {
  export function invoke<T = unknown>(command: string, args?: any): Promise<T>;
  // helper used in tests mock
  export function __reset(): void;
  // Intentionally do not declare a default export here to avoid duplicate identifier
}

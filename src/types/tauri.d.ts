// Minimal type declarations for `@tauri-apps/api/tauri` used by the frontend shim and tests
// This avoids TypeScript errors when the package's module layout isn't picked up by the tooling.

declare module '@tauri-apps/api/tauri' {
  export function invoke<T = unknown>(command: string, args?: any): Promise<T>;
  // helper used in tests to reset mocked state
  export function __reset(): void;
  // Intentionally no default export to avoid duplicate identifier issues in different d.ts files
}

import type { SaveDialogOptions } from "@tauri-apps/plugin-dialog";

const EXTENSION_PATTERN = /\.([A-Za-z0-9]+)$/;

/**
 * Builds Tauri save dialog options that preserve the original file extension
 * so macOS doesn't append a stray `.*` suffix.
 */
export function buildSaveDialogOptions(fileName?: string): SaveDialogOptions {
  if (!fileName?.trim()) {
    return {};
  }

  const defaultPath = fileName.trim();
  const fileSegment = defaultPath.split(/[/\\]/).pop() ?? "";
  const extensionMatch = fileSegment.match(EXTENSION_PATTERN);
  const extension = extensionMatch?.[1]?.toLowerCase();

  const options: SaveDialogOptions = { defaultPath };

  if (extension && /^[a-z0-9]+$/i.test(extension)) {
    options.filters = [
      {
        name: `${extension.toUpperCase()} Files`,
        extensions: [extension],
      },
    ];
  }

  return options;
}

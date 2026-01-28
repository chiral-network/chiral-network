import { invoke } from "@tauri-apps/api/core";
import { join } from "@tauri-apps/api/path";

export async function resolveOutputPath(
  fileName: string,
  outputPath?: string,
): Promise<string> {
  if (outputPath) return outputPath;

  const downloadDir = await invoke<string>("get_download_directory");
  await invoke("ensure_directory_exists", { path: downloadDir });
  return await join(downloadDir, fileName);
}

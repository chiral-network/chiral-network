import { invoke } from "@tauri-apps/api/core";

export async function addEd2kSource(fileHash: string, ed2kLink: string) {
  return await invoke('add_ed2k_source', { fileHash, ed2kLink });
}

export async function listEd2kSources(fileHash: string) {
  return await invoke('list_ed2k_sources', { fileHash });
}

export async function removeEd2kSource(fileHash: string, serverUrl: string) {
  return await invoke('remove_ed2k_source', { fileHash, serverUrl });
}

export async function searchEd2k(query: string, serverUrl?: string) {
  return await invoke('search_ed2k_file', { query, serverUrl });
}

export async function testEd2kConnection(serverUrl: string) {
  return await invoke('test_ed2k_connection', { serverUrl });
}

export async function getEd2kDownloadStatus(fileHash: string) {
  return await invoke('get_ed2k_download_status', { fileHash });
}

export async function parseEd2kLink(ed2kLink: string) {
  return await invoke('parse_ed2k_link', { ed2kLink });
}

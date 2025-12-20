/**
 * Browser-native ED2K link parser
 * 
 * Parses ED2K (eDonkey2000) links without requiring the Tauri backend.
 * Supports both file and server link formats.
 * 
 * File format:  ed2k://|file|FileName.ext|FileSize|MD4Hash|/
 * Server format: ed2k://|server|IP|Port|/
 */

/** Information about an ED2K file extracted from a link */
export interface Ed2kFileInfo {
  file_hash: string;
  file_size: number;
  file_name: string | null;
}

/** Information about an ED2K server extracted from a link */
export interface Ed2kServerInfo {
  server_url: string;
  ip: string;
  port: string;
}

/** Union type for parsed ED2K link results */
export type Ed2kParseResult =
  | { type: 'file'; data: Ed2kFileInfo }
  | { type: 'server'; data: Ed2kServerInfo };

/** Custom error class for ED2K parsing errors */
export class Ed2kParseError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'Ed2kParseError';
  }
}

/**
 * Validates that a string is a valid MD4 hash (32 hexadecimal characters)
 */
function isValidMd4Hash(hash: string): boolean {
  return hash.length === 32 && /^[a-fA-F0-9]+$/.test(hash);
}

/**
 * Parses an ED2K link and returns the extracted information
 * 
 * @param link - The ED2K link to parse (e.g., "ed2k://|file|name|size|hash|/")
 * @returns Parsed result containing either file or server information
 * @throws Ed2kParseError if the link is invalid
 * 
 * @example
 * ```ts
 * const result = parseEd2kLink('ed2k://|file|ubuntu.iso|3654957056|31D6CFE0D16AE931B73C59D7E0C089C0|/');
 * if (result.type === 'file') {
 *   console.log(result.data.file_name); // "ubuntu.iso"
 * }
 * ```
 */
export function parseEd2kLink(link: string): Ed2kParseResult {
  // Validate prefix
  if (!link.startsWith('ed2k://|')) {
    throw new Ed2kParseError(`Invalid ED2K link: must start with "ed2k://|"`);
  }

  // Strip prefix and clean trailing characters
  const partsStr = link.slice('ed2k://|'.length);
  const cleanPartsStr = partsStr.replace(/[/|]+$/, '');
  const parts = cleanPartsStr.split('|');

  if (parts.length === 0) {
    throw new Ed2kParseError('Invalid ED2K link: empty content');
  }

  const linkType = parts[0];

  switch (linkType) {
    case 'file': {
      // Format: ed2k://|file|FileName.ext|FileSize|MD4Hash|/
      if (parts.length < 4) {
        throw new Ed2kParseError(
          'Invalid ED2K file link: requires name, size, and hash'
        );
      }

      const file_name = decodeURIComponent(parts[1]);
      const fileSizeStr = parts[2];
      const file_hash = parts[3];

      // Parse and validate file size
      const file_size = parseInt(fileSizeStr, 10);
      if (isNaN(file_size) || file_size < 0) {
        throw new Ed2kParseError(`Invalid file size: ${fileSizeStr}`);
      }

      // Validate MD4 hash format (32 hex characters)
      if (!isValidMd4Hash(file_hash)) {
        throw new Ed2kParseError(
          `Invalid MD4 hash format: expected 32 hex characters, got "${file_hash}"`
        );
      }

      return {
        type: 'file',
        data: {
          file_hash,
          file_size,
          file_name,
        },
      };
    }

    case 'server': {
      // Format: ed2k://|server|IP|Port|/
      if (parts.length < 3) {
        throw new Ed2kParseError(
          'Invalid ED2K server link: requires IP and port'
        );
      }

      const ip = parts[1];
      const port = parts[2];
      const server_url = `ed2k://|server|${ip}|${port}|/`;

      return {
        type: 'server',
        data: {
          server_url,
          ip,
          port,
        },
      };
    }

    default:
      throw new Ed2kParseError(`Unknown ED2K link type: ${linkType}`);
  }
}

/**
 * Convenience function to parse a file link and return only file info
 * 
 * @param link - The ED2K file link to parse
 * @returns The parsed file information
 * @throws Ed2kParseError if the link is invalid or not a file link
 */
export function parseEd2kFileLink(link: string): Ed2kFileInfo {
  const result = parseEd2kLink(link);
  if (result.type !== 'file') {
    throw new Ed2kParseError(`Expected file link, got ${result.type} link`);
  }
  return result.data;
}

/**
 * Convenience function to parse a server link and return only server info
 * 
 * @param link - The ED2K server link to parse
 * @returns The parsed server information
 * @throws Ed2kParseError if the link is invalid or not a server link
 */
export function parseEd2kServerLink(link: string): Ed2kServerInfo {
  const result = parseEd2kLink(link);
  if (result.type !== 'server') {
    throw new Ed2kParseError(`Expected server link, got ${result.type} link`);
  }
  return result.data;
}

/**
 * Generates an ED2K file link from components
 * 
 * @param fileName - The name of the file
 * @param fileSize - The size of the file in bytes
 * @param md4Hash - The MD4 hash of the file (32 hex characters)
 * @returns The generated ED2K link
 * @throws Ed2kParseError if the hash is invalid
 */
export function generateEd2kFileLink(
  fileName: string,
  fileSize: number,
  md4Hash: string
): string {
  if (!isValidMd4Hash(md4Hash)) {
    throw new Ed2kParseError(
      `Invalid MD4 hash format: expected 32 hex characters`
    );
  }
  const encodedName = encodeURIComponent(fileName);
  return `ed2k://|file|${encodedName}|${fileSize}|${md4Hash}|/`;
}

/**
 * Checks if a string is a valid ED2K link (file or server)
 * 
 * @param link - The string to check
 * @returns true if the string is a valid ED2K link
 */
export function isValidEd2kLink(link: string): boolean {
  try {
    parseEd2kLink(link);
    return true;
  } catch {
    return false;
  }
}


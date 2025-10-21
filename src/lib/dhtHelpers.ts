/**
 * Reset the tracked number of DHT connection attempts when a connection succeeds.
 * Keeps the prior attempt count when we stay disconnected so the UI can show retries.
 */
export function resetConnectionAttempts(
  attempts: number,
  connectionSuccessful: boolean
): number {
  return connectionSuccessful ? 0 : attempts;
}

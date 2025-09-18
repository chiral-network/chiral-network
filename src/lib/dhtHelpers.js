/**
 * Reset the tracked number of DHT connection attempts when a connection succeeds.
 * Keeps the prior attempt count when we stay disconnected so the UI can show retries.
 * @param {number} attempts
 * @param {boolean} connectionSuccessful
 * @returns {number}
 */
export function resetConnectionAttempts(attempts, connectionSuccessful) {
  return connectionSuccessful ? 0 : attempts;
}

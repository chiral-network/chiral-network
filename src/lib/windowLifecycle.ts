// Desktop window lifecycle (book Appendix A.1.10 / A.2; OpenSpec
// `desktop-window-lifecycle`): closing the main window minimizes it to the
// taskbar/Dock and keeps Chiral running. Only an explicit, confirmed Quit
// closes the native session and ends the process.

/** Emitted by the native tray's "Quit Chiral…" item (desktop.rs `QUIT_REQUEST_EVENT`). */
export const QUIT_REQUEST_EVENT = 'chiral://quit-requested';

export interface CloseRequest { preventDefault(): void }
export interface MinimizableWindow { minimize(): Promise<void> }
export interface DestroyableWindow { destroy(): Promise<void> }
export type CloseOutcome =
  | { outcome: 'minimized' }
  | { outcome: 'failed'; error: unknown }
  | { outcome: 'quitting' };

/**
 * Handle a native close request. Outside a confirmed quit the close is always
 * prevented before minimizing, so a minimize failure leaves the window open
 * instead of silently quitting. During a confirmed quit the request is left
 * unprevented so the window can close.
 */
export async function handleCloseRequest(event: CloseRequest, win: MinimizableWindow, quitting: boolean): Promise<CloseOutcome> {
  if (quitting) return { outcome: 'quitting' };
  event.preventDefault();
  try {
    await win.minimize();
    return { outcome: 'minimized' };
  } catch (error) {
    return { outcome: 'failed', error };
  }
}

/**
 * Confirmed quit: close the native session first (cancelling active streams);
 * destroy the window only if that succeeded. A rejection propagates and leaves
 * the window open.
 */
export async function quitApplication(win: DestroyableWindow, closeSession: () => Promise<void>): Promise<void> {
  await closeSession();
  await win.destroy();
}

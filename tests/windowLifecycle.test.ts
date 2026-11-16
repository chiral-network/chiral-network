import { describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { handleCloseRequest, quitApplication } from '$lib/windowLifecycle';

function closeEvent() {
  let prevented = false;
  return { preventDefault: vi.fn(() => { prevented = true; }), get prevented() { return prevented; } };
}

describe('closing the main window', () => {
  it('prevents the close and minimizes instead of quitting', async () => {
    const event = closeEvent();
    const win = { minimize: vi.fn(async () => {}) };
    await expect(handleCloseRequest(event, win, false)).resolves.toEqual({ outcome: 'minimized' });
    expect(event.prevented).toBe(true);
    expect(win.minimize).toHaveBeenCalledOnce();
  });

  it('prevents the close before minimizing so a failure cannot quit', async () => {
    const order: string[] = [];
    const event = { preventDefault: () => order.push('prevent') };
    const failure = new Error('minimize not permitted');
    const win = { minimize: vi.fn(async () => { order.push('minimize'); throw failure; }) };
    await expect(handleCloseRequest(event, win, false)).resolves.toEqual({ outcome: 'failed', error: failure });
    expect(order).toEqual(['prevent', 'minimize']);
  });

  it('lets a close through during a confirmed quit without minimizing', async () => {
    const event = closeEvent();
    const win = { minimize: vi.fn(async () => {}) };
    await expect(handleCloseRequest(event, win, true)).resolves.toEqual({ outcome: 'quitting' });
    expect(event.prevented).toBe(false);
    expect(win.minimize).not.toHaveBeenCalled();
  });
});

describe('confirmed quit', () => {
  it('closes the native session before destroying the window', async () => {
    const order: string[] = [];
    await quitApplication({ destroy: async () => { order.push('destroy'); } }, async () => { order.push('logout'); });
    expect(order).toEqual(['logout', 'destroy']);
  });

  it('keeps the window when closing the session fails', async () => {
    const destroy = vi.fn(async () => {});
    await expect(quitApplication({ destroy }, async () => { throw new Error('logout failed'); })).rejects.toThrow('logout failed');
    expect(destroy).not.toHaveBeenCalled();
  });
});

describe('window permissions', () => {
  it('grants only minimize and destroy for window control', () => {
    const capability = JSON.parse(readFileSync('src-tauri/capabilities/default.json', 'utf8')) as { permissions: string[] };
    const windowPermissions = capability.permissions.filter(p => p.startsWith('core:window:')).sort();
    expect(windowPermissions).toEqual(['core:window:allow-destroy', 'core:window:allow-minimize']);
    expect(capability.permissions.some(p => /shell|fs:|tray|global-shortcut/.test(p))).toBe(false);
  });
});

import { beforeEach } from 'vitest';

// jsdom supplies storage but not media-query observation. No native runtime or
// provider response is fabricated: browser-unavailable behavior is exercised.
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: (query: string) => ({
    matches: false, media: query, onchange: null,
    addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
  }),
});
beforeEach(() => { localStorage.clear(); });

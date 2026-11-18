import { describe, expect, it } from 'vitest';
import { existsSync, readFileSync } from 'node:fs';

// The webview loads nothing from a remote origin (A.1.1 native boundary, requirement 1):
// no stylesheet, font, script or image URL may point off the application bundle.
const remote = /^(?:[a-z][a-z0-9+.-]*:)?\/\//i;
function remoteResources(html: string): string[] {
  const document = new DOMParser().parseFromString(html, 'text/html');
  const urls = [
    ...[...document.querySelectorAll('link[href]')].map(element => element.getAttribute('href')!),
    ...[...document.querySelectorAll('script[src], img[src], iframe[src], source[src]')].map(element => element.getAttribute('src')!),
  ];
  return urls.filter(url => remote.test(url.trim()));
}

describe('the webview references no remote resource', () => {
  for (const page of ['index.html', 'dist/index.html']) {
    it.runIf(existsSync(page))(`${page} links, scripts and images are local`, () => {
      expect(remoteResources(readFileSync(page, 'utf8'))).toEqual([]);
    });
  }
  it('detects a remote stylesheet', () => {
    expect(remoteResources('<link rel="stylesheet" href="https://fonts.example/css"><script src="//cdn.example/x.js"></script><img src="/local.png">'))
      .toEqual(['https://fonts.example/css', '//cdn.example/x.js']);
  });
});

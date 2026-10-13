/**
 * Regression tests from the extension review: PopcornStash known by more than
 * one host name, and what happens to Chrome's own copy of a download.
 */
import {describe, expect, it} from 'vitest';
import {chromeCopyOutcome, shouldIntercept} from '../../src/lib/intercept';
import {buildDownloadPayload, cookiesAllowedFor, isOnServerHost, stashIdentity} from '../../src/lib/payload';

// Paired through the Tailscale name; the server's public URL (its pairing
// `server_name`) is stash.example.com.
const STASH = ['https://nas.tail1234.ts.net', 'stash.example.com'] as const;
const item = {state: 'in_progress', incognito: false};

describe('PopcornStash under its public host name', () => {
  it('builds the identity from the paired origin and the pairing server_name', () => {
    expect(stashIdentity('https://nas.tail1234.ts.net', 'stash.example.com')).toEqual([...STASH]);
    expect(stashIdentity('https://nas.tail1234.ts.net', '')).toEqual(['https://nas.tail1234.ts.net']);
    expect(stashIdentity('https://nas.tail1234.ts.net', undefined)).toEqual(['https://nas.tail1234.ts.net']);
  });

  it('is recognised by the paired origin and by the server name', () => {
    expect(isOnServerHost('https://nas.tail1234.ts.net/api/items/1/file', STASH)).toBe(true);
    expect(isOnServerHost('https://stash.example.com/api/items/1/file?download=1', STASH)).toBe(true);
    expect(isOnServerHost('https://STASH.example.com:8443/x', STASH)).toBe(true);
    expect(isOnServerHost('https://files.example.org/x', STASH)).toBe(false);
    // A server without a public URL reports a plain name; it must not match everything.
    expect(isOnServerHost('https://files.example.org/x', ['https://nas.tail1234.ts.net', 'PopcornStash'])).toBe(false);
    expect(isOnServerHost('https://files.example.org/x', ['https://nas.tail1234.ts.net', ''])).toBe(false);
  });

  it('leaves files saved from the public address in Chrome', () => {
    const url = 'https://stash.example.com/api/items/1/file?download=1';
    expect(shouldIntercept({...item, url}, STASH)).toEqual({intercept: false, reason: 'from PopcornStash'});
    expect(shouldIntercept({...item, url: 'https://files.example.org/a.zip'}, STASH).intercept).toBe(true);
  });

  it('never forwards the session cookie of the public address', () => {
    expect(cookiesAllowedFor('https://stash.example.com/api/items/1/file', true, STASH)).toBe(false);
    const payload = buildDownloadPayload({
      url: 'https://stash.example.com/api/items/1/file?download=1',
      cookies: [{name: '__Host-stash', value: 'session-token'}],
      allowCookies: true,
      serverOrigin: STASH,
    });
    expect(payload.headers?.cookie).toBeUndefined();
  });
});

describe('chromeCopyOutcome', () => {
  it('cancels a copy that is still downloading', () => {
    expect(chromeCopyOutcome('in_progress')).toEqual({cancel: true, note: 'Chrome stopped its own copy.'});
  });

  it('says a finished copy stays in the Downloads folder', () => {
    expect(chromeCopyOutcome('complete')).toEqual({
      cancel: false,
      note: 'Chrome had already finished its copy, so it stays in your Downloads folder too.',
    });
  });

  it('does not claim Chrome finished a copy that was cancelled or removed', () => {
    for (const state of ['interrupted', undefined]) {
      const outcome = chromeCopyOutcome(state);
      expect(outcome.cancel).toBe(false);
      expect(outcome.note).not.toMatch(/finished/);
    }
  });
});

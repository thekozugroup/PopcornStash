import {describe, expect, it} from 'vitest';
import {checkLink, isMagnet, linkName} from '../../src/lib/links';

const HASH = '0123456789abcdef0123456789abcdef01234567';

describe('isMagnet', () => {
  it('detects v1 and v2 torrent magnets', () => {
    expect(isMagnet(`magnet:?xt=urn:btih:${HASH}`)).toBe(true);
    expect(isMagnet(`MAGNET:?dn=Film&xt=urn:btih:${HASH.toUpperCase()}`)).toBe(true);
    expect(isMagnet('magnet:?xt=urn:btmh:1220abcdef')).toBe(true);
    expect(isMagnet(`  magnet:?xt=urn:btih:${HASH}&tr=udp://tracker.example:1337  `)).toBe(true);
  });

  it('rejects things that are not torrent magnets', () => {
    expect(isMagnet('magnet:?dn=Only+a+name')).toBe(false);
    expect(isMagnet('magnet:?xt=urn:sha1:ABCDEF')).toBe(false);
    expect(isMagnet(`magnet:xt=urn:btih:${HASH}`)).toBe(false);
    expect(isMagnet(`https://example.com/?xt=urn:btih:${HASH}`)).toBe(false);
    expect(isMagnet('')).toBe(false);
  });
});

describe('checkLink', () => {
  it('accepts web links and magnets', () => {
    expect(checkLink(' https://example.com/a.mkv ')).toEqual({ok: true, kind: 'web', url: 'https://example.com/a.mkv'});
    expect(checkLink(`magnet:?xt=urn:btih:${HASH}`)).toMatchObject({ok: true, kind: 'magnet'});
    expect(checkLink('www.example.com/a.mkv')).toEqual({ok: true, kind: 'web', url: 'https://www.example.com/a.mkv'});
  });

  it('explains links that only exist in the browser', () => {
    for (const url of ['blob:https://example.com/123', 'data:video/mp4;base64,AAAA', 'file:///tmp/a.mkv']) {
      const result = checkLink(url);
      expect(result.ok).toBe(false);
    }
    const blob = checkLink('blob:https://example.com/123');
    expect(!blob.ok && blob.reason).toMatch(/only exists inside this browser/);
  });

  it('refuses other schemes, text and credentials', () => {
    expect(checkLink('javascript:alert(1)').ok).toBe(false);
    expect(checkLink('ftp://example.com/a').ok).toBe(false);
    expect(checkLink('just some words').ok).toBe(false);
    expect(checkLink('').ok).toBe(false);
    expect(checkLink('https://me:pw@example.com/a').ok).toBe(false);
    expect(checkLink('magnet:?dn=nothing').ok).toBe(false);
  });
});

describe('linkName', () => {
  it('uses the magnet display name, else a short hash', () => {
    expect(linkName(`magnet:?xt=urn:btih:${HASH}&dn=Big+Buck+Bunny`)).toBe('Big Buck Bunny');
    expect(linkName(`magnet:?xt=urn:btih:${HASH}`)).toBe('Torrent 01234567');
  });

  it('uses the last path part of a web link', () => {
    expect(linkName('https://example.com/films/Night%20of%20the%20Living%20Dead.mp4?x=1')).toBe(
      'Night of the Living Dead.mp4',
    );
    expect(linkName('https://example.com/')).toBe('example.com');
  });
});

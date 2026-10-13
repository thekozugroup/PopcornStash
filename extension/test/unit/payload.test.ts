import {describe, expect, it} from 'vitest';
import {
  buildDownloadPayload,
  cleanFilename,
  cookieHeader,
  cookiesAllowedFor,
  isOnServerHost,
  refererFor,
} from '../../src/lib/payload';

const SERVER = 'https://stash.example.com';
const UA = 'Mozilla/5.0 (X11; Linux x86_64) Chrome/141.0.0.0 Safari/537.36';
const COOKIES = [
  {name: 'session', value: 'abc123'},
  {name: 'theme', value: 'dark'},
];

describe('buildDownloadPayload', () => {
  it('sends cookies, referer and user agent for a web link when cookies are allowed', () => {
    expect(
      buildDownloadPayload({
        url: 'https://files.example.org/video/clip.mp4',
        pageUrl: 'https://files.example.org/video/42#comments',
        userAgent: UA,
        cookies: COOKIES,
        allowCookies: true,
        serverOrigin: SERVER,
      }),
    ).toEqual({
      url: 'https://files.example.org/video/clip.mp4',
      headers: {
        referer: 'https://files.example.org/video/42',
        user_agent: UA,
        cookie: 'session=abc123; theme=dark',
      },
    });
  });

  it('never sends cookies when the option is off', () => {
    const payload = buildDownloadPayload({
      url: 'https://files.example.org/clip.mp4',
      userAgent: UA,
      cookies: COOKIES,
      allowCookies: false,
      serverOrigin: SERVER,
    });
    expect(payload.headers?.cookie).toBeUndefined();
    expect(payload.headers?.user_agent).toBe(UA);
  });

  it("never sends cookies for the stash's own origin", () => {
    const payload = buildDownloadPayload({
      url: 'https://stash.example.com/api/items/1/file?download=1',
      cookies: [{name: '__Host-stash', value: 'session-token'}],
      allowCookies: true,
      serverOrigin: SERVER,
    });
    expect(payload).toEqual({url: 'https://stash.example.com/api/items/1/file?download=1'});
  });

  it("never sends cookies for the stash's host on another port (cookies ignore ports)", () => {
    const payload = buildDownloadPayload({
      url: 'http://localhost:9000/file.mkv',
      cookies: [{name: 'stash', value: 'session-token'}],
      allowCookies: true,
      serverOrigin: 'http://localhost:8788',
    });
    expect(payload.headers?.cookie).toBeUndefined();
  });

  it('sends nothing but the link for magnets', () => {
    const magnet = 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Big+Buck+Bunny';
    expect(
      buildDownloadPayload({
        url: magnet,
        pageUrl: 'https://tracker.example.net/t/1',
        userAgent: UA,
        cookies: COOKIES,
        allowCookies: true,
        serverOrigin: SERVER,
      }),
    ).toEqual({url: magnet});
  });

  it('leaves headers out entirely when there are none', () => {
    expect(buildDownloadPayload({url: 'https://a.example/f.zip', allowCookies: false, serverOrigin: SERVER})).toEqual({
      url: 'https://a.example/f.zip',
    });
  });

  it('skips an empty cookie list', () => {
    const payload = buildDownloadPayload({
      url: 'https://a.example/f.zip',
      cookies: [],
      allowCookies: true,
      serverOrigin: SERVER,
    });
    expect(payload.headers).toBeUndefined();
  });

  it('passes a cleaned file name', () => {
    const payload = buildDownloadPayload({
      url: 'https://a.example/download?id=7',
      filename: '/home/me/Downloads/Holiday\u0007 2024.mov',
      allowCookies: false,
      serverOrigin: SERVER,
    });
    expect(payload.filename).toBe('Holiday 2024.mov');
  });
});

describe('cookiesAllowedFor', () => {
  it('needs the option, a web link and another host', () => {
    expect(cookiesAllowedFor('https://files.example.org/a', true, SERVER)).toBe(true);
    expect(cookiesAllowedFor('https://files.example.org/a', false, SERVER)).toBe(false);
    expect(cookiesAllowedFor('https://stash.example.com:8443/a', true, SERVER)).toBe(false);
    expect(cookiesAllowedFor('magnet:?xt=urn:btih:abc', true, SERVER)).toBe(false);
    expect(cookiesAllowedFor('blob:https://files.example.org/1', true, SERVER)).toBe(false);
  });
});

describe('refererFor', () => {
  it('follows strict-origin-when-cross-origin', () => {
    expect(refererFor('https://a.example/page?q=1#x', 'https://a.example/file')).toBe('https://a.example/page?q=1');
    expect(refererFor('https://a.example/page?q=1', 'https://cdn.example/file')).toBe('https://a.example/');
    expect(refererFor('https://a.example/page', 'http://a.example/file')).toBeUndefined();
    expect(refererFor('http://a.example/page', 'https://a.example/file')).toBe('http://a.example/');
  });

  it('ignores pages that are not web pages', () => {
    expect(refererFor('chrome://newtab/', 'https://a.example/file')).toBeUndefined();
    expect(refererFor('file:///home/me/page.html', 'https://a.example/file')).toBeUndefined();
    expect(refererFor(undefined, 'https://a.example/file')).toBeUndefined();
    expect(refererFor('not a url', 'https://a.example/file')).toBeUndefined();
  });
});

describe('helpers', () => {
  it('builds a cookie header', () => {
    expect(cookieHeader(COOKIES)).toBe('session=abc123; theme=dark');
    expect(cookieHeader([{name: '', value: 'x'}])).toBeUndefined();
  });

  it('matches the server host regardless of port and case', () => {
    expect(isOnServerHost('https://STASH.example.com:9/x', SERVER)).toBe(true);
    expect(isOnServerHost('https://example.com/x', SERVER)).toBe(false);
    expect(isOnServerHost('nonsense', SERVER)).toBe(false);
  });

  it('cleans file names', () => {
    expect(cleanFilename('C:\\Users\\me\\Downloads\\film.mkv')).toBe('film.mkv');
    expect(cleanFilename('..')).toBeUndefined();
    expect(cleanFilename('   ')).toBeUndefined();
    expect(cleanFilename(null)).toBeUndefined();
  });
});

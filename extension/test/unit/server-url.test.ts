import {describe, expect, it} from 'vitest';
import {displayHost, parseServerAddress, resolveVerifyUrl} from '../../src/lib/server-url';

function origin(input: string): string {
  const result = parseServerAddress(input);
  if (!result.ok) throw new Error(`rejected ${input}: ${result.reason}`);
  return result.origin;
}

function reason(input: string): string {
  const result = parseServerAddress(input);
  if (result.ok) throw new Error(`accepted ${input} as ${result.origin}`);
  return result.reason;
}

describe('parseServerAddress', () => {
  it('accepts https and strips the path, query and fragment', () => {
    const result = parseServerAddress('https://stash.example.com/library/movies?sort=title#top');
    expect(result).toEqual({
      ok: true,
      origin: 'https://stash.example.com',
      host: 'stash.example.com',
      hostname: 'stash.example.com',
      pattern: 'https://stash.example.com/*',
    });
  });

  it('keeps a non-default port and drops the default one', () => {
    expect(origin('https://stash.example.com:8443/')).toBe('https://stash.example.com:8443');
    expect(origin('https://stash.example.com:443')).toBe('https://stash.example.com');
  });

  it('assumes https for a bare host name, with or without a port', () => {
    expect(origin('stash.example.com')).toBe('https://stash.example.com');
    expect(origin('stash.example.com:8443/pair')).toBe('https://stash.example.com:8443');
    expect(origin('  Stash.Example.COM  ')).toBe('https://stash.example.com');
  });

  it('allows plain http only for localhost and 127.0.0.1', () => {
    expect(origin('http://localhost:8788/')).toBe('http://localhost:8788');
    expect(origin('http://127.0.0.1:8788/settings')).toBe('http://127.0.0.1:8788');
    expect(origin('localhost:8788')).toBe('http://localhost:8788');
    expect(origin('127.0.0.1:8788')).toBe('http://127.0.0.1:8788');
    expect(origin('https://localhost:8788')).toBe('https://localhost:8788');
  });

  it('rejects plain http anywhere else', () => {
    expect(reason('http://stash.example.com')).toMatch(/https/);
    expect(reason('http://192.168.1.20:8788')).toMatch(/localhost/);
    expect(reason('http://localhost.example.com')).toMatch(/https/);
  });

  it('rejects addresses with credentials', () => {
    expect(reason('https://owner:secret@stash.example.com')).toMatch(/user name and password/);
    expect(reason('https://owner@stash.example.com')).toMatch(/user name and password/);
    expect(reason('owner:secret@stash.example.com')).toMatch(/user name and password/);
  });

  it('rejects other schemes and things that are not addresses', () => {
    expect(reason('ftp://stash.example.com')).toMatch(/https/);
    expect(reason('javascript:alert(1)')).toMatch(/https/);
    expect(reason('chrome://extensions')).toMatch(/https/);
    expect(reason('')).toMatch(/Enter/);
    expect(reason('   ')).toMatch(/Enter/);
    expect(reason('stash example com')).toMatch(/spaces/);
    expect(reason('https://')).toBeTruthy();
  });
});

describe('resolveVerifyUrl', () => {
  it('resolves a path against the server origin', () => {
    expect(resolveVerifyUrl('/pair?code=ABCD-EFGH', 'https://stash.example.com', 'ABCD-EFGH')).toBe(
      'https://stash.example.com/pair?code=ABCD-EFGH',
    );
  });

  it('keeps an absolute https URL (the server public URL)', () => {
    expect(resolveVerifyUrl('https://stash.example.com/pair?code=X', 'https://192.168.1.2:8788', 'X')).toBe(
      'https://stash.example.com/pair?code=X',
    );
  });

  it('falls back to the pairing page for missing or unsafe values', () => {
    const fallback = 'https://stash.example.com/pair?code=ABCD-EFGH';
    expect(resolveVerifyUrl(null, 'https://stash.example.com', 'ABCD-EFGH')).toBe(fallback);
    expect(resolveVerifyUrl('', 'https://stash.example.com', 'ABCD-EFGH')).toBe(fallback);
    expect(resolveVerifyUrl('javascript:alert(1)', 'https://stash.example.com', 'ABCD-EFGH')).toBe(fallback);
    expect(resolveVerifyUrl('https://a:b@evil.example', 'https://stash.example.com', 'ABCD-EFGH')).toBe(fallback);
  });
});

describe('displayHost', () => {
  it('shows host and port', () => {
    expect(displayHost('http://localhost:8788')).toBe('localhost:8788');
    expect(displayHost('https://stash.example.com')).toBe('stash.example.com');
  });
});

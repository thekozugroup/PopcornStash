import {describe, expect, it} from 'vitest';
import {downloadRow, percent, showsProgress, stateWords} from '../../src/lib/downloads-view';
import {serverProgress, shouldIntercept} from '../../src/lib/intercept';
import {clientName} from '../../src/lib/platform';
import type {Download, VpnStatus} from '../../src/lib/types';
import {describeVpn} from '../../src/lib/vpn';

function download(overrides: Partial<Download>): Download {
  return {
    id: 'd1',
    kind: 'http',
    name: 'clip.mp4',
    source: 'https://example.com/clip.mp4',
    state: 'downloading',
    progress: 0.427,
    total_bytes: 1000,
    done_bytes: 427,
    down_bps: 100,
    up_bps: 0,
    peers: null,
    eta_s: 6,
    error: null,
    created_at: 1,
    completed_at: null,
    item_ids: [],
    ...overrides,
  };
}

const vpn = (overrides: Partial<VpnStatus>): VpnStatus => ({
  configured: true,
  reachable: true,
  state: 'connected',
  provider: 'nordvpn',
  protocol: 'wireguard',
  label: null,
  server: 'nl123',
  country: 'Netherlands',
  public_ip: '203.0.113.9',
  protected: true,
  since: 1,
  error: null,
  kill_switch: true,
  allow_no_vpn: false,
  ...overrides,
});

describe('download words', () => {
  it('describes each state', () => {
    expect(stateWords(download({}))).toBe('Downloading · 42%');
    expect(stateWords(download({kind: 'torrent', peers: 0}))).toBe('Looking for peers · 42%');
    expect(stateWords(download({kind: 'torrent', state: 'queued', error: 'Waiting for the VPN to connect.'}))).toBe(
      'Waiting for the VPN to connect.',
    );
    expect(stateWords(download({state: 'sorted'}))).toBe('In your library');
    expect(stateWords(download({state: 'failed', error: 'The server said 403.'}))).toBe('Failed: The server said 403.');
  });

  it('clamps progress and shows the bar only while moving or waiting', () => {
    expect(percent(download({progress: 1.4}))).toBe(100);
    expect(percent(download({progress: Number.NaN}))).toBe(0);
    expect(showsProgress(download({state: 'paused'}))).toBe(true);
    expect(showsProgress(download({state: 'sorted'}))).toBe(false);
  });
});

describe('describeVpn', () => {
  it('matches the web app wording', () => {
    expect(describeVpn(vpn({})).label).toBe('Protected · NordVPN · Netherlands');
    expect(describeVpn(vpn({state: 'connecting'})).label).toBe('Torrents paused: VPN connecting');
    expect(describeVpn(vpn({state: 'not_configured', configured: false})).label).toBe('Torrents blocked: no VPN');
    expect(describeVpn(vpn({protected: false})).tone).toBe('error');
    expect(describeVpn(null, true).label).toBe('VPN status unavailable');
  });

  it('names the VPN once, even when the server label already carries the country', () => {
    // The server labels a NordVPN setup "NordVPN · <country> · <server>" and also sends the country.
    expect(describeVpn(vpn({label: 'NordVPN · Netherlands · nl812'})).label).toBe('Protected · NordVPN · Netherlands');
    expect(describeVpn(vpn({label: 'NordVPN · nl812', country: null})).label).toBe('Protected · NordVPN');
    expect(
      describeVpn(vpn({provider: 'wireguard', label: 'WireGuard · home-router', country: null})).label,
    ).toBe('Protected · WireGuard · home-router');
    expect(describeVpn(vpn({provider: 'openvpn', label: null, country: 'Sweden'})).label).toBe('Protected · OpenVPN · Sweden');
  });

  it('says torrents are off when the server has them turned off on purpose', () => {
    const off = describeVpn(
      vpn({state: 'unreachable', reachable: false, configured: false, torrents_off: true, error: 'Torrents are turned off on this server.'}),
    );
    expect(off.label).toBe('Torrents off');
    expect(off.detail).toBe('Web links still download.');
    const reasoned = describeVpn(
      vpn({state: 'unreachable', reachable: false, torrents_off: true, error: 'Torrents need NET_ADMIN and /dev/net/tun.'}),
    );
    expect(reasoned.detail).toBe('Torrents need NET_ADMIN and /dev/net/tun. Web links still download.');
    expect(describeVpn(vpn({state: 'unreachable', reachable: false})).label).toBe('Torrents blocked: VPN service offline');
  });

  it('warns that torrents use this network whenever the server allows no VPN, whatever VPN is saved', () => {
    // STASH_ALLOW_NO_VPN: torrents run on this network. A saved VPN is unused, so the
    // popup must never say torrents are paused or waiting for it.
    for (const state of ['not_configured', 'connecting', 'disconnected', 'connected', 'error'] as const) {
      for (const configured of [true, false]) {
        const summary = describeVpn(vpn({state, configured, allow_no_vpn: true, kill_switch: false, protected: null}));
        expect(summary, `${state} configured=${configured}`).toEqual({
          tone: 'warning',
          label: 'No VPN: torrents use this network',
          detail: 'This server allows torrents without a VPN. Your router must protect the traffic.',
        });
      }
    }
  });
});

describe('shouldIntercept', () => {
  const server = 'https://stash.example.com';
  const item = {url: 'https://files.example.org/a.zip', state: 'in_progress', incognito: false};

  it('takes over ordinary web downloads', () => {
    expect(shouldIntercept(item, server)).toEqual({intercept: true, url: item.url});
  });

  it('leaves browser-only, incognito, extension and PopcornStash downloads alone', () => {
    expect(shouldIntercept({...item, url: 'blob:https://files.example.org/1'}, server).intercept).toBe(false);
    expect(shouldIntercept({...item, url: 'data:text/plain,hi'}, server).intercept).toBe(false);
    expect(shouldIntercept({...item, url: 'file:///tmp/x'}, server).intercept).toBe(false);
    expect(shouldIntercept({...item, incognito: true}, server).intercept).toBe(false);
    expect(shouldIntercept({...item, byExtensionId: 'abc'}, server).intercept).toBe(false);
    expect(shouldIntercept({...item, state: 'complete'}, server).intercept).toBe(false);
    expect(shouldIntercept({...item, url: 'https://stash.example.com/api/items/1/file?download=1'}, server).intercept).toBe(
      false,
    );
    expect(
      shouldIntercept({...item, finalUrl: 'https://stash.example.com/api/items/1/file'}, server).intercept,
    ).toBe(false);
  });
});

describe('serverProgress', () => {
  it('waits for bytes, accepts finished, reports failures', () => {
    expect(serverProgress(undefined)).toBe('waiting');
    expect(serverProgress(download({state: 'queued', done_bytes: 0}))).toBe('waiting');
    expect(serverProgress(download({done_bytes: 0}))).toBe('waiting');
    expect(serverProgress(download({}))).toBe('started');
    expect(serverProgress(download({state: 'completed'}))).toBe('started');
    expect(serverProgress(download({state: 'failed'}))).toBe('failed');
  });
});

describe('clientName', () => {
  it('names the platform', () => {
    expect(clientName('mac')).toBe('Chrome on macOS');
    expect(clientName('win')).toBe('Chrome on Windows');
    expect(clientName('cros')).toBe('Chrome on ChromeOS');
    expect(clientName(undefined)).toBe('Chrome');
  });
});

describe('downloadRow', () => {
  it('puts the percentage beside the name and a bar while bytes move', () => {
    expect(downloadRow(download({}))).toEqual({
      name: 'clip.mp4',
      meta: '42%',
      detail: null,
      bar: 42,
      error: false,
      description: 'Downloading · 42%',
    });
    expect(downloadRow(download({state: 'paused'}))).toMatchObject({meta: 'Paused 42%', bar: 42});
    expect(downloadRow(download({kind: 'torrent', peers: 0}))).toMatchObject({meta: 'Finding peers', bar: 42});
  });

  it('uses a short state and no bar otherwise', () => {
    expect(
      downloadRow(download({kind: 'torrent', state: 'queued', progress: 0, error: 'Waiting for the VPN to connect.'})),
    ).toMatchObject({meta: 'Waiting for VPN', bar: null, description: 'Waiting for the VPN to connect.'});
    expect(downloadRow(download({state: 'sorted'}))).toMatchObject({meta: 'In library', bar: null, detail: null});
    expect(downloadRow(download({state: 'failed', error: 'Gone.'}))).toMatchObject({meta: 'Failed', detail: 'Gone.', error: true});
    expect(downloadRow(download({name: '', source: 'https://example.com/x'})).name).toBe('https://example.com/x');
  });
});

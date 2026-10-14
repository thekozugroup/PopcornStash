import {describe, expect, it} from 'vitest';
import type {VpnStatus} from '../api/types';
import {describeVpn, showsVpnLine, torrentsAffected} from './vpn';

const connecting = {state: 'connecting', protected: null, torrents_off: false} as unknown as VpnStatus;

describe('describeVpn', () => {
  it('names each state once: the label says what is wrong, the detail what still works', () => {
    expect(describeVpn(null)).toEqual({tone: 'neutral', label: 'Checking VPN', detail: ''});
    expect(describeVpn(null, {isUnavailable: true})).toEqual({
      tone: 'neutral',
      label: 'VPN status unavailable',
      detail: 'Web links and uploads still work.',
    });
  });
});

describe('showsVpnLine', () => {
  it('leaves the line out while the server does not answer and nothing is known about the VPN', () => {
    // The page already says the downloads could not be loaded; "links still work" would be false.
    expect(showsVpnLine(null, true, true)).toBe(false);
    expect(showsVpnLine(null, false, true)).toBe(false);
  });

  it('keeps a status read before, and an unreadable status while the server answers', () => {
    expect(showsVpnLine(connecting, false, true)).toBe(true);
    expect(showsVpnLine(null, true, false)).toBe(true);
    expect(showsVpnLine(null, true, false)).toBe(torrentsAffected(null, true));
  });
});

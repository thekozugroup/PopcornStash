import {describe, expect, it} from 'vitest';
import type {Download} from '../../api/types';
import {activeStateWords, fetchingTorrentDetails} from './downloadText';

const magnet: Download = {
  id: 'fixture', kind: 'torrent', name: 'Metadata fixture', source: 'magnet:?xt=urn:btih:fixture',
  state: 'downloading', progress: 0, total_bytes: null, done_bytes: 0,
  down_bps: 0, up_bps: 0, peers: null, eta_s: null, error: null,
  created_at: 0, completed_at: null, item_ids: [], destinations: [],
};

describe('torrent waiting states', () => {
  it('explains missing metadata and respects pause and queue states', () => {
    expect(fetchingTorrentDetails(magnet)).toBe(true);
    expect(activeStateWords(magnet)).toBe('Fetching torrent details');
    expect(activeStateWords({...magnet, state: 'paused'})).toBe('Paused');
    expect(activeStateWords({...magnet, state: 'queued', error: 'Waiting for the VPN.'})).toBe('Waiting for the VPN.');
    expect(fetchingTorrentDetails({...magnet, kind: 'http'})).toBe(false);
  });

  it('shows peer discovery after metadata and transfer when bytes are arriving', () => {
    expect(activeStateWords({...magnet, total_bytes: 1024})).toBe('Looking for peers');
    expect(activeStateWords({...magnet, total_bytes: 1024, peers: 0, down_bps: 100})).toBe('Downloading');
    expect(activeStateWords({...magnet, done_bytes: 100, down_bps: 100})).toBe('Downloading');
  });
});

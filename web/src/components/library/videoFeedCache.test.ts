import {describe, expect, it} from 'vitest';
import {ApiError} from '../../api/client';
import type {Item, VideoFeed} from '../../api/types';
import {emptyFeedState, videoFeedCache} from './videoFeedCache';

const removed = {id: 'item-a', size: 100} as Item;
const retained = {id: 'item-b', size: 200} as Item;
function page(key: string): VideoFeed {
  const recent = [removed, retained];
  return {
    continue_watching: recent, suggested: recent, recent,
    collections: [{folder: {key, name: key, item_count: 4, size: 800, added_at: 1, custom: false}, recent}],
    next_cursor: 'more',
  };
}
function pagedFeed() {
  const first = videoFeedCache(emptyFeedState, {type: 'refreshed', page: page('first')});
  return videoFeedCache(first, {type: 'appended', page: page('second')});
}

describe('cached video shelves', () => {
  it('evicts a removed item from all shelves and loaded pages before refresh, including if refresh fails', () => {
    const cached = pagedFeed();
    const evicted = videoFeedCache(cached, {type: 'removed', id: removed.id});
    const result = videoFeedCache(evicted, {type: 'failed', during: 'refresh', error: new Error('offline')});
    for (const {data} of [evicted, result]) {
      expect([data!.continue_watching, data!.suggested, data!.recent]).toEqual([[retained], [retained], [retained]]);
      expect(data!.collections.map(row => row.recent)).toEqual([[retained], [retained]]);
      expect(data!.collections.map(row => row.folder.item_count)).toEqual([3, 3]);
      expect(data!.next_cursor).toBe('more');
    }
    expect(videoFeedCache(evicted, {type: 'removed', id: removed.id})).toEqual(evicted);
    expect(cached.data!.collections[0]!.recent).toEqual([removed, retained]);
  });

  describe.each(['refresh', 'pagination'] as const)('%s failures', during => {
    it.each([401, 403, 404])('clears all cached pages and the cursor on HTTP %i', status => {
      const fetching = videoFeedCache(pagedFeed(), {type: 'loadingMore'});
      const error = new ApiError('unknown', 'Unavailable', status);
      expect(videoFeedCache(fetching, {type: 'failed', during, error})).toEqual({...emptyFeedState, error});
    });

    it('preserves the cached pages and retry cursor after a temporary failure', () => {
      const cached = pagedFeed();
      const fetching = videoFeedCache(cached, {type: 'loadingMore'});
      const error = new ApiError('unknown', 'Unavailable', 503);
      const failed = videoFeedCache(fetching, {type: 'failed', during, error});
      expect(failed.data).toBe(cached.data);
      expect(failed.busy).toBe(false);
      expect(during === 'pagination' ? failed.moreError : failed.error).toBe(error);
    });
  });
});

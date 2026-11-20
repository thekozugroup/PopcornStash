import {ApiError} from '../../api/client';
import type {Item, VideoFeed} from '../../api/types';

interface FeedState {
  data: VideoFeed | null;
  error: unknown;
  moreError: unknown;
  busy: boolean;
}

export const emptyFeedState: FeedState = {data: null, error: null, moreError: null, busy: false};

type FeedAction =
  | {type: 'refreshed' | 'appended'; page: VideoFeed}
  | {type: 'loadingMore'}
  | {type: 'removed'; id: string}
  | {type: 'failed'; error: unknown; during: 'refresh' | 'pagination'};

export function appendCollections(feed: VideoFeed, page: VideoFeed): VideoFeed {
  const collections = new Map(feed.collections.map(row => [row.folder.key, row]));
  for (const row of page.collections) collections.set(row.folder.key, row);
  return {...feed, collections: [...collections.values()], next_cursor: page.next_cursor};
}

export function videoFeedCache(state: FeedState, action: FeedAction): FeedState {
  switch (action.type) {
    case 'refreshed': return {...emptyFeedState, data: action.page};
    case 'loadingMore': return {...state, busy: true, moreError: null};
    case 'appended': return {...state, data: state.data && appendCollections(state.data, action.page), busy: false, moreError: null};
    case 'failed': {
      const error = action.error;
      if (error instanceof ApiError && [401, 403, 404].includes(error.status)) {
        return {...emptyFeedState, error};
      }
      return action.during === 'pagination' ? {...state, moreError: error, busy: false} : {...state, error, busy: false};
    }
    case 'removed': {
      const feed = state.data;
      if (!feed) return state;
      const keep = (item: Item) => item.id !== action.id;
      const collections = feed.collections.map(row => {
        const item = row.recent.find(entry => entry.id === action.id);
        if (!item) return row;
        const folder = {...row.folder, item_count: Math.max(0, row.folder.item_count - 1), size: Math.max(0, row.folder.size - item.size)};
        return {folder, recent: row.recent.filter(keep)};
      });
      return {...state, data: {...feed, continue_watching: feed.continue_watching.filter(keep),
        suggested: feed.suggested.filter(keep), recent: feed.recent.filter(keep), collections}};
    }
  }
}

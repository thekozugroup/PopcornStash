import {afterEach, expect, it, vi} from 'vitest';
import {items, libraries} from './client';

afterEach(() => vi.unstubAllGlobals());
it('keeps folder/source scope and pagination when searching videos', async () => {
  const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({items: [], next_cursor: null}), {headers: {'content-type': 'application/json'}}));
  vi.stubGlobal('fetch', fetcher);
  await items.list({library_id: 'films', download_group: 'download:one & two', folder: 'A/B', q: 'Harbor', include_inbox: true, limit: 60, cursor: 'next'});
  const url = new URL(fetcher.mock.calls[0]![0] as string, 'https://stash.example');
  expect(Object.fromEntries(url.searchParams)).toEqual({library_id: 'films', download_group: 'download:one & two', folder: 'A/B', q: 'Harbor', include_inbox: 'true', limit: '60', cursor: 'next'});
});
it('sends only the selected item IDs when changing presentation folders', async () => {
  const fetcher = vi.fn().mockResolvedValue(new Response(null, {status: 204}));
  vi.stubGlobal('fetch', fetcher);
  await libraries.moveToDownloadFolder('films', 'custom:archive', ['one', 'three']);
  const [url, init] = fetcher.mock.calls[0] as [string, RequestInit];
  expect(url).toBe('/api/libraries/films/download-folders/move');
  expect(init.method).toBe('POST');
  expect(JSON.parse(init.body as string)).toEqual({key: 'custom:archive', item_ids: ['one', 'three']});
});

import {afterEach, expect, it, vi} from 'vitest';
import {finishCompletedFiles, getTransferFiles, skipTransferFile} from './downloadFiles';

afterEach(() => vi.unstubAllGlobals());

it('loads only the open torrent and passes the cancellation signal', async () => {
  const snapshot = {ready: true, review_required: false, files: []};
  const fetcher = vi.fn().mockResolvedValue(Response.json(snapshot));
  vi.stubGlobal('fetch', fetcher);
  const signal = new AbortController().signal;
  expect(await getTransferFiles('test/1', signal)).toEqual(snapshot);
  expect(fetcher).toHaveBeenCalledWith('/api/downloads/test%2F1/files', expect.objectContaining({method: 'GET', signal}));
});

it('skips exactly one index and lets the server choose verified files when finishing', async () => {
  const fetcher = vi.fn().mockResolvedValueOnce(new Response(null, {status: 204}))
    .mockResolvedValueOnce(Response.json({id: 'test/1', state: 'completed'}));
  vi.stubGlobal('fetch', fetcher);
  await skipTransferFile('test/1', 19);
  expect(await finishCompletedFiles('test/1')).toMatchObject({state: 'completed'});
  const post = expect.objectContaining({method: 'POST', body: '{}', credentials: 'same-origin',
    headers: expect.objectContaining({'Content-Type': 'application/json'})});
  expect(fetcher.mock.calls).toEqual([
    ['/api/downloads/test%2F1/files/19/skip', post],
    ['/api/downloads/test%2F1/finish-completed', post],
  ]);
});

it('posts the full reviewed selection and numerically sorted verified indices, including explicit empty sets', async () => {
  const fetcher = vi.fn<typeof fetch>().mockImplementation(async () => Response.json({state: 'completed'}));
  vi.stubGlobal('fetch', fetcher);
  await finishCompletedFiles('test', {expectedSelected: [2, 7, 19], expectedComplete: new Set([19, 2])});
  await finishCompletedFiles('test', {expectedSelected: [], expectedComplete: new Set()});
  expect(fetcher.mock.calls.map(([, init]) => JSON.parse(init?.body as string))).toEqual([
    {expected_selected: [2, 7, 19], expected_complete: [2, 19]}, {expected_selected: [], expected_complete: []},
  ]);
});

it('surfaces a state conflict without retrying the finish action', async () => {
  const fetcher = vi.fn().mockResolvedValue(Response.json({error: {code: 'conflict', message: 'A reviewed file is no longer complete.'}}, {status: 409}));
  vi.stubGlobal('fetch', fetcher);
  await expect(finishCompletedFiles('test', {expectedSelected: [7, 19], expectedComplete: new Set([7])})).rejects.toMatchObject({status: 409, code: 'conflict'});
  expect(fetcher).toHaveBeenCalledTimes(1);
});

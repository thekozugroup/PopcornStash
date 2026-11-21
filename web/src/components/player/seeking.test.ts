import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest';
import {PreviewFrames} from './previewFrames';
import {positionAt, previewPlacement, previewSecond} from './seeking';

describe('seek previews', () => {
  it('samples bounded frames, including the end of short and long videos', () => {
    expect(previewSecond(7.9, 60)).toBe(6);
    expect(previewSecond(999, 60)).toBe(58);
    expect(previewSecond(0, 0.5)).toBe(0);
    expect(previewSecond(3600, 7200)).toBe(3600);
    expect(previewSecond(-10, 60)).toBe(0);
    expect(previewSecond(Infinity, 60)).toBe(0);
    expect(previewSecond(10, NaN)).toBe(0);
  });
  it('matches the slider thumb inset and locale direction at both edges', () => {
    expect(positionAt(10, 0, 120, 60, false)).toBe(0);
    expect(positionAt(110, 0, 120, 60, false)).toBe(60);
    expect(positionAt(60, 0, 120, 60, false)).toBe(30);
    expect(positionAt(-20, 0, 120, 60, true)).toBe(60);
    expect(positionAt(999, 0, 120, 60, true)).toBe(0);
  });
  it('tracks fractional positions and clips shorter than one second', () => {
    expect(positionAt(65, 5, 120, 0.5, false)).toBe(0.25);
    expect(positionAt(120, 5, 120, 0.5, false)).toBe(0.5);
    expect(positionAt(5, 0, 20, 12, true)).toBe(9);
    expect(positionAt(10, 0, 0, 12, true)).toBe(0);
  });
  it('centers on the thumb and keeps both ends inside a narrow control strip', () => {
    const track = {left: 10, available: 300, width: 280, rtl: false};
    expect(previewPlacement(0, 0.5, track)).toEqual({left: 0, width: 96});
    expect(previewPlacement(0.25, 0.5, track)).toEqual({left: 102, width: 96});
    expect(previewPlacement(0.5, 0.5, track)).toEqual({left: 204, width: 96});
    expect(previewPlacement(0, 0.5, {...track, rtl: true})).toEqual({left: 204, width: 96});
    expect(previewPlacement(10, 20, {...track, available: 60})).toEqual({left: 0, width: 60});
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return {promise, resolve};
}

describe('the active player’s preview frames', () => {
  const fetchFrame = vi.fn<typeof fetch>();
  const decode = vi.fn<() => Promise<void>>();
  const remove = vi.fn();
  let frames: PreviewFrames;
  let sequence: number;

  beforeEach(() => {
    vi.useFakeTimers();
    const jpeg = new Response();
    vi.spyOn(jpeg, 'blob').mockResolvedValue(new Blob(['fixture']));
    fetchFrame.mockReset().mockResolvedValue(jpeg);
    decode.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal('fetch', fetchFrame);
    remove.mockClear();
    vi.stubGlobal('Image', class { src = ''; decode = decode; remove = remove; });
    sequence = 0;
    vi.spyOn(URL, 'createObjectURL').mockImplementation(() => `blob:fixture-${++sequence}`);
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    frames = new PreviewFrames('fixture/one');
  });
  afterEach(() => {
    frames.clear();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('starts the latest bucket by 40 ms even during continuous movement', async () => {
    for (const second of [0, 2, 4, 6]) {
      frames.select(second);
      await vi.advanceTimersByTimeAsync(10);
    }
    expect(fetchFrame).toHaveBeenCalledTimes(1);
    expect(fetchFrame).toHaveBeenCalledWith('/api/items/fixture%2Fone/preview?time_s=6',
      expect.objectContaining({credentials: 'same-origin', cache: 'no-store'}));
    expect(frames.read(6)).toMatchObject({src: 'blob:fixture-1'});
  });

  it('keeps up with a 95 ms sweep and replays all twelve frames from memory', async () => {
    const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));
    const jpeg = new Response();
    vi.spyOn(jpeg, 'blob').mockResolvedValue(new Blob(['fixture']));
    fetchFrame.mockImplementation(async () => { await delay(25); return jpeg; });
    decode.mockImplementation(() => delay(16));
    for (let i = 0; i < 12; i++) {
      frames.select(i * 2);
      await vi.advanceTimersByTimeAsync(95);
      expect(frames.read(i * 2)).toMatchObject({src: `blob:fixture-${i + 1}`});
    }
    frames.select(null);
    for (let i = 11; i >= 0; i--) {
      frames.select(i * 2);
      expect(frames.read(i * 2)).toMatchObject({src: `blob:fixture-${i + 1}`});
    }
    expect(fetchFrame).toHaveBeenCalledTimes(12);
    expect(decode).toHaveBeenCalledTimes(12);
  });

  it('finishes one extraction at a time and skips superseded positions', async () => {
    const slow = deferred<Response>();
    fetchFrame.mockReturnValueOnce(slow.promise);
    frames.select(2);
    await vi.advanceTimersByTimeAsync(40);
    for (const second of [4, 6, 8]) {
      frames.select(second);
      await vi.advanceTimersByTimeAsync(95);
    }
    expect(fetchFrame).toHaveBeenCalledTimes(1);
    slow.resolve(new Response('fixture'));
    await vi.advanceTimersByTimeAsync(0);
    expect(frames.read(8)).toBeNull();
    await vi.advanceTimersByTimeAsync(40);
    expect(fetchFrame).toHaveBeenCalledTimes(2);
    expect(fetchFrame.mock.calls[1]?.[0]).toContain('time_s=8');
    expect(frames.read(8)).toMatchObject({src: 'blob:fixture-2'});
  });

  it('reuses decoded frames across separate hovers and evicts the least recently viewed', async () => {
    for (let i = 0; i < 12; i++) {
      frames.select(i * 2);
      await vi.advanceTimersByTimeAsync(40);
    }
    frames.select(null);
    frames.select(0);
    expect(frames.read(0)).toMatchObject({src: 'blob:fixture-1'});
    expect(decode).toHaveBeenCalledTimes(12);
    frames.select(24);
    await vi.advanceTimersByTimeAsync(40);
    expect(fetchFrame).toHaveBeenCalledTimes(13);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:fixture-2');
    expect(frames.read(2)).toBeNull();
    expect(frames.read(0)).toBeTruthy();
  });

  it('cancels a delayed request on leave and abandons a busy retry', async () => {
    frames.select(0);
    frames.select(null);
    await vi.advanceTimersByTimeAsync(100);
    expect(fetchFrame).not.toHaveBeenCalled();
    fetchFrame.mockResolvedValueOnce(new Response(null, {status: 409}));
    frames.select(4);
    await vi.advanceTimersByTimeAsync(40);
    frames.select(null);
    await vi.advanceTimersByTimeAsync(500);
    expect(fetchFrame).toHaveBeenCalledTimes(1);
  });

  it('retries a busy frame once and permits a later revisit after failure', async () => {
    fetchFrame.mockResolvedValueOnce(new Response(null, {status: 409}))
      .mockResolvedValueOnce(new Response(null, {status: 409}));
    frames.select(2);
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetchFrame).toHaveBeenCalledTimes(2);
    expect(frames.read(2)).toBe('unavailable');
    frames.select(null);
    frames.select(2);
    await vi.advanceTimersByTimeAsync(40);
    expect(frames.read(2)).toMatchObject({src: 'blob:fixture-1'});
  });

  it('discards an in-flight decode after teardown and isolates the next item', async () => {
    frames.select(2);
    await vi.advanceTimersByTimeAsync(40);
    const slow = deferred<void>();
    decode.mockReturnValueOnce(slow.promise);
    frames.select(4);
    await vi.advanceTimersByTimeAsync(40);
    expect(frames.read(4)).toBeNull();
    frames.clear();
    expect(frames.read(2)).toBeNull();
    expect(remove).toHaveBeenCalledOnce();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:fixture-1');
    expect(fetchFrame.mock.calls[1]?.[1]?.signal?.aborted).toBe(true);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:fixture-2');

    const next = new PreviewFrames('fixture-two');
    try {
      next.select(4);
      await vi.advanceTimersByTimeAsync(40);
      slow.resolve();
      await vi.advanceTimersByTimeAsync(0);
      expect(frames.read(4)).toBeNull();
      expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2);
      expect(next.read(4)).toMatchObject({src: 'blob:fixture-3'});
    } finally {
      next.clear();
    }
  });

  it('drops every cached frame when the server denies access', async () => {
    frames.select(0);
    await vi.advanceTimersByTimeAsync(40);
    fetchFrame.mockResolvedValueOnce(new Response(null, {status: 403}));
    frames.select(4);
    await vi.advanceTimersByTimeAsync(40);
    expect(frames.read(4)).toBe('unavailable');
    expect(frames.read(0)).toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:fixture-1');
  });
});

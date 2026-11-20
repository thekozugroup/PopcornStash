import {afterEach, beforeEach, expect, it, vi} from 'vitest';
import * as api from '../../api/downloadFiles';
import {watchTransferFiles} from './useTransferFiles';

class Visibility extends EventTarget {
  hidden = false;
  setHidden(value: boolean) {
    this.hidden = value;
    this.dispatchEvent(new Event('visibilitychange'));
  }
}

const snapshot: api.TransferFiles = {ready: true, review_required: false, files: []};
let visibility: Visibility;

beforeEach(() => {
  visibility = new Visibility();
  vi.stubGlobal('document', visibility);
  vi.useFakeTimers();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it('does no work while hidden, refreshes immediately on return, and cancels both timers on close', async () => {
  visibility.hidden = true;
  const read = vi.spyOn(api, 'getTransferFiles').mockResolvedValue(snapshot);
  const observer = {received: vi.fn(), stale: vi.fn(), failed: vi.fn()};
  const close = watchTransferFiles('test', observer);
  expect(read).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
  visibility.setHidden(false);
  visibility.setHidden(false);
  expect(read).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(observer.received).toHaveBeenCalledWith(snapshot);

  visibility.setHidden(true);
  expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(read).toHaveBeenCalledTimes(1);
  visibility.setHidden(false);
  expect(read).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(3000);
  expect(read).toHaveBeenCalledTimes(3);
  close();
  expect(vi.getTimerCount()).toBe(0);
  visibility.setHidden(true);
  visibility.setHidden(false);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(read).toHaveBeenCalledTimes(3);
});

it('aborts outstanding reads on hide and close and ignores their late results', async () => {
  const finishRead: Array<(value: api.TransferFiles) => void> = [];
  const read = vi.spyOn(api, 'getTransferFiles').mockImplementation(() =>
    new Promise(resolve => finishRead.push(resolve)));
  const observer = {received: vi.fn(), stale: vi.fn(), failed: vi.fn()};
  const close = watchTransferFiles('test', observer);
  const first = read.mock.calls[0]![1]!;
  visibility.setHidden(true);
  expect(first.aborted).toBe(true);
  expect(observer.stale).toHaveBeenCalledOnce();
  visibility.setHidden(false);
  const second = read.mock.calls[1]![1]!;
  expect(second.aborted).toBe(false);

  finishRead[0]!(snapshot);
  await vi.advanceTimersByTimeAsync(0);
  expect(observer.received).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
  close();
  expect(second.aborted).toBe(true);
  finishRead[1]!(snapshot);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(observer.received).not.toHaveBeenCalled();
  expect(observer.failed).not.toHaveBeenCalled();
  expect(read).toHaveBeenCalledTimes(2);
});

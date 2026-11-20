import {describe, expect, it} from 'vitest';
import type {TransferFile} from '../../api/downloadFiles';
import {fileProgressText, finishPlan, isVerified, selectedFiles, selectionEdits, stillMatchesConfirmation} from './transferFiles';
import {emptyFileRules, selectionFromFilters} from './fileRules';

const sample: TransferFile = {
  index: 7, path: 'fixture.bin', size: 1000, kind: 'other', selected: true,
  complete: false, verified_bytes: 50,
};
const done = {...sample, complete: true, verified_bytes: sample.size};

describe('file progress', () => {
  it('distinguishes verified progress, a recheck, and an older server response', () => {
    expect(fileProgressText(sample, false, true)).toBe('5% · 50 B of 1 KB');
    expect(fileProgressText({...sample, verified_bytes: 999}, false, true)).toMatch(/^99%/);
    expect(fileProgressText({...sample, verified_bytes: null}, false, true)).toBe('Progress unavailable · 1 KB');
    expect(fileProgressText({...sample, verified_bytes: undefined, complete: undefined}, true, true)).toBe('Selected · 1 KB');
    expect(fileProgressText({...done, complete: false}, false, true)).toMatch(/^Verifying/);
    expect(fileProgressText(done, false, true)).toBe('Complete · 1 KB');
  });

  it('distinguishes an unsaved skip from a skipped file with shared pieces', () => {
    expect(fileProgressText(sample, false, false)).toBe('Will skip · 50 B of 1 KB');
    expect(fileProgressText({...sample, selected: false}, false, false)).toBe('Skipped · 50 B of 1 KB');
    expect(fileProgressText({...sample, selected: false}, false, true)).toMatch(/^Will download/);
  });

  it('requires both positive completion evidence and the exact byte count, including empty files', () => {
    expect(isVerified({...done, size: 0, verified_bytes: 0})).toBe(true);
    for (const verified_bytes of [null, undefined, -1, 50, 1001, NaN]) {
      expect(isVerified({...done, verified_bytes})).toBe(false);
    }
    expect(isVerified({...done, complete: false})).toBe(false);
  });
});

describe('live selection', () => {
  it('preserves manual overrides and combines them with untouched remote selections', () => {
    const second = {...sample, index: 17, selected: false};
    const edits = selectionEdits([sample, second], new Set());
    expect([...edits]).toEqual([[7, false]]);
    const polled = [{...sample, verified_bytes: 300}, {...second, selected: true}];
    expect([...selectedFiles(polled, edits)]).toEqual([17]);
    expect(selectedFiles([sample], new Map([[96, true]]))).toEqual(new Set([7]));
  });

  it('applies filters explicitly and retains subsequent individual choices during polls', () => {
    const second: TransferFile = {...sample, index: 19, kind: 'video', selected: false};
    const files = [sample, second];
    const result = selectionFromFilters(files, emptyFileRules, ['video']);
    expect([...selectedFiles(files, new Map())]).toEqual([7]);
    const edited = new Set(result.selected);
    edited.add(7);
    expect(selectedFiles([{...done}, second], selectionEdits(files, edited))).toEqual(new Set([7, 19]));
  });
});

describe('finish completed', () => {
  it('withholds finish if any selected file has unknown or inconsistent status', () => {
    for (const extra of [
      {...sample, verified_bytes: null}, {...sample, complete: undefined},
      {...sample, complete: true}, {...done, complete: false},
    ]) {
      expect(finishPlan([done, {...extra, index: 9}])).toBeNull();
    }
  });

  it('keeps the whole reviewed selection when another selected file finishes or rows reorder', () => {
    const incomplete = {...sample, index: 19};
    const reviewed = finishPlan([incomplete, done, {...done, index: 3, selected: false}])!;
    expect(reviewed.expectedSelected).toEqual([7, 19]);
    expect(reviewed.expectedComplete).toEqual(new Set([7]));
    expect(stillMatchesConfirmation(reviewed, finishPlan([done, {...done, index: 19}]))).toBe(true);
    expect(stillMatchesConfirmation(reviewed, finishPlan([done, {...incomplete, verified_bytes: 91}]))).toBe(true);
  });

  it('protects every completion displayed at confirmation and allows further gains during the final check', () => {
    const files = [done, {...sample, index: 10}, {...sample, index: 21}];
    const opened = finishPlan(files)!;
    const confirmed = finishPlan([done, {...done, index: 10}, files[2]!])!;
    expect(stillMatchesConfirmation(opened, confirmed)).toBe(true);
    expect(confirmed.expectedComplete).toEqual(new Set([7, 10]));
    // One previously unfinished file finishes while another loses verification.
    expect(stillMatchesConfirmation(confirmed, finishPlan([done, files[1]!, {...done, index: 21}]))).toBe(false);
    expect(stillMatchesConfirmation(confirmed, finishPlan([done, {...done, index: 10}, {...done, index: 21}]))).toBe(true);
    expect(confirmed.expectedComplete).toEqual(new Set([7, 10]));
  });

  it('requires a new confirmation after any membership change, lost completion, or missing status', () => {
    const reviewed = finishPlan([done, {...sample, index: 19}])!;
    expect(stillMatchesConfirmation(reviewed, finishPlan([done, {...sample, index: 20}]))).toBe(false);
    expect(stillMatchesConfirmation(reviewed, finishPlan([done]))).toBe(false);
    expect(stillMatchesConfirmation(reviewed, finishPlan([sample, {...done, index: 19}]))).toBe(false);
    expect(stillMatchesConfirmation(reviewed, null)).toBe(false);
  });

  it('counts only selected and fully verified files, across the whole torrent', () => {
    const skipped = {...done, selected: false};
    expect(finishPlan([skipped, sample, {...done, complete: false}])).toBeNull();
    const plan = finishPlan([done, {...sample, index: 10}, {...skipped, index: 12}]);
    expect(plan).toMatchObject({completed: 1, selected: 2});
    expect(plan?.description).toContain('discard unfinished and skipped files');
    expect(finishPlan([done])).toMatchObject({completed: 1, selected: 1});
  });
});

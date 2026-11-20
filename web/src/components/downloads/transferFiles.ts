import type {TransferFile} from '../../api/downloadFiles';
import {formatBytes, formatBytesOf, formatCount} from '../../lib/format';

/** A local choice only overrides that file. Polls still pick up other selection changes. */
export function selectedFiles(files: TransferFile[], edits?: ReadonlyMap<number, boolean>) {
  return new Set(files.filter(file => edits?.get(file.index) ?? file.selected).map(file => file.index));
}

export function selectionEdits(files: TransferFile[], selection: ReadonlySet<number>) {
  const edits = new Map<number, boolean>();
  for (const file of files) {
    const checked = selection.has(file.index);
    if (checked !== file.selected) edits.set(file.index, checked);
  }
  return edits;
}

function verifiedBytes(file: TransferFile) {
  const done = file.verified_bytes;
  if (done == null || !Number.isSafeInteger(done) || done < 0 || done > file.size) return null;
  return done;
}

export function isVerified(file: TransferFile) {
  return file.complete === true && verifiedBytes(file) === file.size;
}

export function fileProgressText(file: TransferFile, review: boolean, checked: boolean) {
  const done = verifiedBytes(file);
  let state: string;
  if (!checked) state = review || !file.selected ? 'Skipped' : 'Will skip';
  else if (!file.selected) state = review ? 'Selected' : 'Will download';
  else if (isVerified(file)) state = 'Complete';
  else if (review && (done === null || done === 0)) state = 'Selected';
  else if (done === null) state = 'Progress unavailable';
  else if (file.size === 0 || done === file.size) state = 'Verifying';
  else state = `${Math.floor(done / file.size * 100)}%`;
  const bytes = done === null || isVerified(file) || (review && done === 0) ? formatBytes(file.size) : formatBytesOf(done, file.size);
  return `${state} · ${bytes}`;
}

export function finishPlan(files: TransferFile[]) {
  const selected = files.filter(file => file.selected);
  for (const file of selected) {
    const bytes = verifiedBytes(file);
    if (bytes === null || typeof file.complete !== 'boolean' || file.complete !== (bytes === file.size)) return null;
  }
  const expectedComplete = new Set(selected.filter(isVerified).map(file => file.index));
  const completed = expectedComplete.size;
  if (!completed) return null;
  return {
    expectedSelected: selected.map(file => file.index).sort((a, b) => a - b),
    expectedComplete,
    completed,
    selected: selected.length,
    description: `${completed} of ${formatCount(selected.length, 'selected file')} ${selected.length === 1 ? 'is' : 'are'} complete. Finishing stops downloading and sharing. The server will send fully downloaded, verified selections for sorting and discard unfinished and skipped files.`,
  };
}

export type FinishPlan = NonNullable<ReturnType<typeof finishPlan>>;

/** More reviewed files may finish. Changing membership or losing verified data needs a new confirmation. */
export function stillMatchesConfirmation(reviewed: FinishPlan, current: FinishPlan | null) {
  return current !== null
    && reviewed.expectedSelected.length === current.expectedSelected.length
    && reviewed.expectedSelected.every((index, position) => index === current.expectedSelected[position])
    && [...reviewed.expectedComplete].every(index => current.expectedComplete.has(index));
}

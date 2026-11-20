import * as stylex from '@stylexjs/stylex';
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter, VStack} from '@astryxdesign/core/Layout';
import {Divider} from '@astryxdesign/core/Divider';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useEffect, useId, useRef, useState, type ReactNode} from 'react';
import {downloads as api} from '../../api/client';
import {finishCompletedFiles, getTransferFiles, skipTransferFile, type TransferFiles} from '../../api/downloadFiles';
import type {Download, TorrentFileKind} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {formatBytes, formatCount} from '../../lib/format';
import {FileRulesEditor} from './FileRulesEditor';
import {TorrentTypeChoices} from './TorrentTypeChoices';
import {draftFromRules, selectionFromFilters, type FileRuleDraft} from './fileRules';
import {fileProgressText, finishPlan, isVerified, selectedFiles, selectionEdits, stillMatchesConfirmation, type FinishPlan} from './transferFiles';
import {useTransferFiles} from './useTransferFiles';

const PAGE_SIZE = 100;
const styles = stylex.create({
  fileRow: {display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, paddingBlock: 6},
  fileInfo: {display: 'flex', flexDirection: 'column', gap: 4, minWidth: 0, overflowWrap: 'anywhere'},
  file: {minWidth: 0, overflowWrap: 'anywhere', fontVariantNumeric: 'tabular-nums'},
  list: {listStyle: 'none', padding: 0, margin: 0},
});

interface DialogProps {
  download: Download;
  onClose: () => void;
  onSaved: (message: string, updated?: Download) => void;
}

export function TorrentFilesDialog({download, onSaved, onClose}: DialogProps) {
  const [busy, setBusy] = useState(false);
  const {data, error, fresh, refresh, setData} = useTransferFiles(download.id, busy);
  const close = (open: boolean) => { if (!open && !busy) onClose(); };
  const review = data?.review_required === true;
  const header = <DialogHeader title={review ? 'Review torrent' : 'Torrent files'} subtitle={download.name} onOpenChange={close} />;
  const loadError = error && <Banner status="error" title="Files could not be refreshed" description={error}
    collapsible={false} endContent={<Button label="Try again" variant="secondary" onClick={refresh} />} />;

  return <Dialog isOpen onOpenChange={close} purpose="form" width={660} maxHeight="90dvh">
    {data?.ready ? <FilePicker key={review ? 'review' : 'transfer'} data={data} download={download} header={header}
      fresh={fresh} refresh={refresh} loadError={loadError} onSkip={index => setData(current => current && ({
        ...current, files: current.files.map(file => file.index === index ? {...file, selected: false} : file),
      }))} onStart={indices => setData(current => current && ({
        ...current, review_required: false, files: current.files.map(file => ({...file, selected: indices.has(file.index)})),
      }))} busy={busy} setBusy={setBusy} onSaved={onSaved} onClose={onClose} />
      : <Layout header={header} content={<LayoutContent>
        {loadError || <Text as="p" color="secondary" role="status">Fetching the file list. Waiting for a peer to share the torrent details.</Text>}
      </LayoutContent>} footer={<LayoutFooter><Button label="Close" variant="secondary" onClick={onClose} /></LayoutFooter>} />}
  </Dialog>;
}

interface PickerProps extends DialogProps {
  data: TransferFiles;
  fresh: boolean;
  refresh: () => void;
  header: ReactNode;
  loadError: ReactNode;
  onSkip: (index: number) => void;
  onStart: (indices: Set<number>) => void;
  busy: boolean;
  setBusy: (busy: boolean) => void;
}

function FilePicker({data, fresh, refresh, download, header, loadError, busy, setBusy, onSaved, onClose, onSkip, onStart}: PickerProps) {
  const review = data.review_required === true;
  const [draft, setDraft] = useState(() => ({...draftFromRules(data.rules), enabled: true}));
  const [types, setTypes] = useState<TorrentFileKind[]>(data.rules?.file_types ?? ['video', 'subtitle']);
  const [editingRequested, setEditing] = useState(false);
  const canEdit = download.state !== 'queued';
  const editing = review || (canEdit && editingRequested);
  const [pendingFilters, setPendingFilters] = useState(false);
  const [filterError, setFilterError] = useState<string>();
  const [actionError, setActionError] = useState<string | null>(null);
  const [edits, setEdits] = useState(new Map<number, boolean>());
  const [search, setSearch] = useState('');
  const [shown, setShown] = useState(PAGE_SIZE);
  const rowId = useId();
  const finishCheck = useRef<AbortController | null>(null);
  useEffect(() => () => finishCheck.current?.abort(), []);
  const [confirmation, setConfirmation] = useState<FinishPlan | null>(null);
  const [skipping, setSkipping] = useState<number | null>(null);
  const files = data.files;
  const selected = selectedFiles(files, editing ? edits : undefined);
  const dirty = files.some(file => selected.has(file.index) !== file.selected);
  const needle = search.trim().toLocaleLowerCase();
  const matches = needle ? files.filter(file => file.path.toLocaleLowerCase().includes(needle)) : files;
  const visible = matches.slice(0, shown);
  const selectedBytes = files.reduce((sum, file) => sum + (selected.has(file.index) ? file.size : 0), 0);
  const plan = fresh && !loadError && data.review_required === false && !editing ? finishPlan(files) : null;
  if (confirmation && !stillMatchesConfirmation(confirmation, plan)) {
    setConfirmation(null);
    if (fresh) setActionError('The file list changed. Review the latest selection and progress before finishing.');
  } else if (confirmation && plan && plan.completed > confirmation.completed) {
    // Every completion shown in the open confirmation must still be verified at Finish.
    setConfirmation(plan);
  }
  const canSave = fresh && !busy && !loadError && selected.size > 0 && !pendingFilters && !filterError && (review || dirty);
  const canFinish = !!plan && !busy && !loadError && !editing;

  function changeFilters(next: FileRuleDraft, nextTypes: TorrentFileKind[]) {
    setDraft(next);
    setTypes(nextTypes);
    setFilterError(undefined);
    setPendingFilters(true);
  }
  function applyFilters() {
    const result = selectionFromFilters(files, draft, types);
    setFilterError(result.error);
    if (result.selected) {
      setEdits(selectionEdits(files, result.selected));
      setPendingFilters(false);
    }
  }
  function choose(indices: number[], checked: boolean) {
    const next = new Set(selected);
    for (const index of indices) {
      if (checked) next.add(index);
      else next.delete(index);
    }
    setEdits(selectionEdits(files, next));
  }
  async function save() {
    if (!canSave) return;
    refresh();
    setActionError(null);
    setBusy(true);
    try {
      if (review) {
        const updated = await api.startTorrent(download.id, [...selected]);
        onStart(selected);
        onSaved(updated.state === 'queued' ? 'Download queued.' : 'Download started.', updated);
      } else {
        await api.selectFiles(download.id, [...selected]);
        onSaved('File selection saved.');
        onClose();
      }
    } catch (cause) {
      setActionError(errorMessage(cause, 'The selection could not be saved. Try again.'));
    } finally { setBusy(false); }
  }
  async function skip(index: number) {
    if (!fresh || busy || loadError || editing || selected.size <= 1) return;
    setSkipping(index);
    refresh();
    setActionError(null);
    setBusy(true);
    try {
      await skipTransferFile(download.id, index);
      onSkip(index);
      onSaved('File skipped.');
    } catch (cause) {
      setActionError(errorMessage(cause, 'The file could not be skipped. Try again.'));
    } finally {
      setSkipping(null);
      setBusy(false);
    }
  }

  async function finish() {
    if (!confirmation || !stillMatchesConfirmation(confirmation, plan) || !canFinish || finishCheck.current) return;
    const controller = new AbortController();
    finishCheck.current = controller;
    setBusy(true);
    setActionError(null);
    try {
      const signal = AbortSignal.any([controller.signal, AbortSignal.timeout(9000)]);
      const latest = await getTransferFiles(download.id, signal);
      if (signal.aborted) return;
      if (!latest.ready || latest.review_required !== false || !stillMatchesConfirmation(confirmation, finishPlan(latest.files))) {
        setActionError('The file status changed. Review the latest progress before finishing.');
        return;
      }
      const updated = await finishCompletedFiles(download.id, confirmation);
      onSaved('Completed files sent for sorting.', updated);
      onClose();
    } catch (cause) {
      if (!controller.signal.aborted) setActionError(errorMessage(cause, 'The torrent could not be finished. Check the file list and try again.'));
    } finally {
      finishCheck.current = null;
      refresh();
      setConfirmation(null);
      setBusy(false);
    }
  }

  return <>
    <Layout header={header} content={<LayoutContent><VStack gap={3}>
      {loadError}
      {actionError && <Banner status="error" title="Action could not be completed" description={actionError} collapsible={false} />}
      {review && <Text as="p" color="secondary">Choose which files to download, then start the torrent.</Text>}
      {!review && canEdit && <HStack><Button label={editing ? 'Cancel edits' : 'Edit selection'} variant="secondary"
        isDisabled={busy} aria-expanded={editing} onClick={() => {
          setEdits(new Map());
          setFilterError(undefined);
          setPendingFilters(false);
          setDraft({...draftFromRules(data.rules), enabled: true});
          setTypes(data.rules?.file_types ?? ['video', 'subtitle']);
          setEditing(!editing);
        }} /></HStack>}
      {editing && <VStack gap={2}>
        <TorrentTypeChoices showHelp={false} label="File types" value={types} onChange={next => changeFilters(draft, next)} disabled={busy} />
        <FileRulesEditor value={draft} onChange={next => changeFilters(next, types)} disabled={busy} error={filterError} />
        <HStack gap={2} wrap="wrap"><Button label="Filter" variant="secondary" isDisabled={busy} onClick={applyFilters} />
          <Text type="supporting" color="secondary" role="status">{pendingFilters ? 'Apply filters to update the selection.' : 'Filter selects matching files.'}</Text>
        </HStack>
      </VStack>}
      <TextInput label="Find files in this torrent" value={search} hasClear onChange={value => { setSearch(value); setShown(PAGE_SIZE); }} />
      {editing && <HStack gap={2} wrap="wrap">
        <Button label="Select shown" variant="ghost" isDisabled={busy || !visible.length} onClick={() => choose(visible.map(f => f.index), true)} />
        <Button label="Deselect shown" variant="ghost" isDisabled={busy || !visible.length} onClick={() => choose(visible.map(f => f.index), false)} />
      </HStack>}
      <ul aria-label="Torrent files" {...stylex.props(styles.list)}>{visible.map(file => <li key={file.index} {...stylex.props(styles.file)}>
        <Divider />
        {editing ? <CheckboxInput label={file.path} description={fileProgressText(file, review, selected.has(file.index))}
          value={selected.has(file.index)} isDisabled={busy} onChange={value => choose([file.index], value)} />
          : <div {...stylex.props(styles.fileRow)}>
            <div role="group" aria-labelledby={`${rowId}-name-${file.index}`} aria-describedby={`${rowId}-status-${file.index}`}
              {...stylex.props(styles.fileInfo)}>
              <Text as="p" display="block" type="label" id={`${rowId}-name-${file.index}`}>{file.path}</Text>
              <Text as="p" display="block" type="supporting" color="secondary" id={`${rowId}-status-${file.index}`}>{fileProgressText(file, false, file.selected)}</Text>
            </div>
            {file.selected && !isVerified(file) && file.complete !== undefined && <Button label="Skip" aria-label={`Skip ${file.path}`}
              variant="ghost" aria-describedby={`${rowId}-status-${file.index}`} isDisabled={!fresh || busy || !!loadError || selected.size <= 1} isLoading={skipping === file.index}
              tooltip={selected.size <= 1 ? 'Keep at least one file selected' : 'Skip this file and keep the others'}
              onClick={() => void skip(file.index)} />}
          </div>}
      </li>)}</ul>
      {visible.length === 0 && <Text as="p" color="secondary">No files match your search.</Text>}
      {matches.length > shown && <Button label="Show more" variant="ghost" onClick={() => setShown(n => n + PAGE_SIZE)} />}
    </VStack></LayoutContent>} footer={<LayoutFooter><VStack gap={2}>
      <Text as="p" type="supporting" role="status">
        {selected.size ? `${formatCount(selected.size, 'file')} selected · ${formatBytes(selectedBytes)}` : 'Select at least one file to continue.'}
        {!editing && plan ? ` · ${plan.completed} complete` : ''}
      </Text>
      <HStack gap={2} hAlign="end" wrap="wrap">
        {!editing && plan && <Button label="Finish completed files…" variant="secondary" isDisabled={!canFinish} onClick={() => { setActionError(null); setConfirmation(plan); }} />}
        <Button label="Close" variant="secondary" isDisabled={busy} onClick={onClose} />
        {editing && <Button label={review ? 'Start download' : 'Save selection'} variant="primary" isDisabled={!canSave} isLoading={busy && !confirmation} onClick={() => void save()} />}
      </HStack>
    </VStack></LayoutFooter>} />
    {confirmation && plan && <AlertDialog isOpen title="Finish completed files?" description={confirmation.description}
      cancelLabel="Keep downloading" actionLabel="Finish completed files" isActionLoading={busy}
      onOpenChange={open => { if (!open && !busy) setConfirmation(null); }} onAction={() => void finish()} />}
  </>;
}

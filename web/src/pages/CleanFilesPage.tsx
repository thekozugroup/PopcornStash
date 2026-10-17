/** Explicit owner cleanup, grouped by folder with no automatic destructive policy. */
import * as stylex from '@stylexjs/stylex';
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {Divider} from '@astryxdesign/core/Divider';
import {Heading} from '@astryxdesign/core/Heading';
import {HStack, VStack} from '@astryxdesign/core/Layout';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useToast} from '@astryxdesign/core/Toast';
import {useEffect, useState} from 'react';
import {useSearchParams} from 'react-router';
import {files as api, libraries as librariesApi} from '../api/client';
import type {CleanupFile} from '../api/types';
import {useLibraries} from '../app/LibrariesProvider';
import {cleanupKinds, groupedFiles, matchingFiles, selectNonVideos} from '../components/cleanup/selection';
import {OwnerOnly} from '../components/settings/OwnerOnly';
import {PageHeader} from '../components/PageHeader';
import {errorMessage} from '../lib/errors';
import {formatBytes, formatCount} from '../lib/format';

const styles = stylex.create({path: {overflowWrap: 'anywhere'}, row: {minWidth: 0}});

function CleanFiles({folder}: {folder: string}) {
  const [, setParams] = useSearchParams();
  const {libraries} = useLibraries();
  const [extraFolders, setExtraFolders] = useState<string[]>([]);
  const [data, setData] = useState<CleanupFile[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [kind, setKind] = useState('all');
  const [review, setReview] = useState('all');
  const [search, setSearch] = useState('');
  const [shown, setShown] = useState(100);
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const showToast = useToast();
  useEffect(() => {
    const controller = new AbortController();
    api.list(folder || undefined, controller.signal).then(result => {
      if (!controller.signal.aborted) { setData(result.files); setTruncated(result.truncated); }
    }).catch((e: unknown) => { if (!controller.signal.aborted) setError(errorMessage(e, 'Files could not be loaded. Try again.')); });
    return () => controller.abort();
  }, [folder, revision]);
  useEffect(() => {
    const controller = new AbortController();
    librariesApi.folders(controller.signal).then(result => { if (!controller.signal.aborted) setExtraFolders(result.folders.map(f => f.name)); }).catch(() => {});
    return () => controller.abort();
  }, []);
  const reload = () => { setData(null); setSelected(new Set()); setError(null); setShown(100); setRevision(v => v + 1); };
  const changeFilter = (set: (v: string) => void, value: string) => { set(value); setShown(100); setSelected(new Set()); };
  const visible = matchingFiles(data ?? [], kind, review, search);
  const chosen = (data ?? []).filter(f => selected.has(f.path));
  const size = chosen.reduce((sum, f) => sum + f.size, 0);
  const folderOptions = new Map<string, string>([
    ['', 'Every folder'], ['Inbox', 'Inbox'], ['Downloads/Completed', 'Completed downloads'], ['Other', 'Other files'],
    ...(libraries ?? []).filter(l => l.folder).map(l => [l.folder ?? '', l.name] as [string, string]), ...extraFolders.map(f => [f, f] as [string, string]),
  ]);
  if (folder && !folderOptions.has(folder)) folderOptions.set(folder, folder);
  const remove = async () => {
    setBusy(true); setError(null);
    const removed: string[] = [];
    const failed: string[] = [];
    let interrupted: unknown;
    try {
      for (let offset = 0; offset < chosen.length; offset += 500) {
        const result = await api.remove(chosen.slice(offset, offset + 500).map(({path, fingerprint}) => ({path, fingerprint})));
        removed.push(...result.removed); failed.push(...result.failed.map(f => `${f.path}: ${f.message}`));
      }
    } catch (e) { interrupted = e; }
    setData(current => current?.filter(f => !removed.includes(f.path)) ?? null);
    setSelected(new Set(chosen.filter(f => !removed.includes(f.path)).map(f => f.path)));
    setBusy(false); setConfirm(false);
    if (failed.length || interrupted) setError(`${formatCount(removed.length, 'file')} moved to Trash. ${failed.slice(0, 3).join(' ')} ${interrupted ? errorMessage(interrupted, 'The connection was interrupted. Refresh before trying again.') : 'Refresh before trying again.'}`);
    else showToast({body: `${formatCount(removed.length, 'file')} moved to Trash.`});
  };
  return <>
    <PageHeader title="Clean files" subtitle="Choose what to remove, by folder or file type. Owners can restore removed files from Trash."
      actions={<Button label="Open trash" variant="secondary" href="/settings/libraries#trash" />} />
    <VStack gap={6}>
      <HStack gap={4} wrap="wrap" vAlign="end">
        <Selector label="Folder" value={folder} onChange={value => { setParams({folder: value}); setSearch(''); setReview('all'); setKind('all'); }}
          options={[...folderOptions].map(([value, label]) => ({value, label}))} isDisabled={busy} width={280} />
        <Selector label="File type" value={kind} onChange={value => changeFilter(setKind, value)} options={[{value: 'all', label: 'All types'}, ...cleanupKinds]} isDisabled={busy} width={180} />
        <Selector label="Review" value={review} onChange={value => changeFilter(setReview, value)} options={[{value: 'all', label: 'All files'}, {value: 'waiting', label: 'Waiting to import'}, {value: 'ignored', label: 'Ignored files'}]} isDisabled={busy} width={180} />
        <TextInput label="Find a file or folder" value={search} onChange={value => changeFilter(setSearch, value)} isDisabled={busy} width={280} />
      </HStack>
      {truncated ? <Banner status="warning" title="Showing the first 20,000 files" description="Choose a smaller folder to see the rest. Selection applies only to the files listed here." collapsible={false} /> : null}
      {error ? <Banner status="error" title="Cleanup needs attention" description={error} collapsible={false}
        endContent={<Button label="Refresh" variant="secondary" isDisabled={busy} onClick={reload} />} /> : null}
      <HStack gap={3} wrap="wrap">
        <Button label="Select matching files" variant="secondary" isDisabled={busy || !visible.length} onClick={() => setSelected(new Set(visible.map(f => f.path)))} />
        <Button label="Keep videos only" variant="secondary" isDisabled={busy || !visible.some(f => f.kind !== 'video')} onClick={() => setSelected(selectNonVideos(visible))} />
        <Button label="Clear selection" variant="ghost" isDisabled={busy || !selected.size} onClick={() => setSelected(new Set())} />
        <Button label="Refresh" variant="ghost" isDisabled={busy} onClick={reload} />
      </HStack>
      <Text as="p" type="supporting" color="secondary">Keep videos only selects the non-video files in this view for removal. Subtitles are separate files; embedded subtitle tracks stay in the video.</Text>
      <HStack gap={4} wrap="wrap" vAlign="center">
        <Text>{formatCount(chosen.length, 'file')} selected · {formatBytes(size)}</Text>
        <Button label="Move selected to Trash" variant="destructive" isDisabled={busy || !chosen.length} onClick={() => setConfirm(true)} />
      </HStack>
      {data === null && !error ? <Text as="p" role="status">Reading files…</Text> : null}
      {data !== null && !visible.length ? <Text as="p" color="secondary">No files match. Choose another folder or clear the filters.</Text> : null}
      <VStack gap={6}>{groupedFiles(visible.slice(0, shown)).map(([path, rows]) => <VStack gap={2} key={path}>
        <Heading level={2} xstyle={styles.path}>{path}</Heading>
        <VStack gap={0}>{rows.map(file => <VStack gap={0} key={file.path}>
          <HStack gap={3} paddingBlock={3} vAlign="center">
            <VStack gap={1} xstyle={styles.row}>
              <CheckboxInput label={file.path.split('/').at(-1) ?? file.path} value={selected.has(file.path)} isDisabled={busy}
                onChange={value => setSelected(current => { const next = new Set(current); if (value) next.add(file.path); else next.delete(file.path); return next; })} />
              <Text as="p" type="supporting" color="secondary">{cleanupKinds.find(k => k.value === file.kind)?.label} · {formatBytes(file.size)}{file.review === 'ignored' ? ' · Ignored' : file.review === 'waiting' ? ' · Waiting to import' : ''}</Text>
            </VStack>
          </HStack><Divider />
        </VStack>)}</VStack>
      </VStack>)}</VStack>
      {visible.length > shown ? <Button label={`Show more (${formatCount(visible.length - shown, 'file')} remaining)`} variant="secondary" onClick={() => setShown(v => v + 100)} /> : null}
    </VStack>
    <AlertDialog isOpen={confirm} onOpenChange={open => { if (!busy) setConfirm(open); }} title={`Remove ${formatCount(chosen.length, 'file')}?`}
      description={`${formatBytes(size)} will move to Trash. Only the selected files move; their videos and other companions stay unless selected. Owners can restore them from Settings.`}
      actionLabel="Move to Trash" isActionLoading={busy} onAction={() => void remove()} />
  </>;
}
export default function CleanFilesPage() {
  const [params] = useSearchParams();
  const folder = params.get('folder') ?? 'Inbox';
  return <OwnerOnly><CleanFiles key={folder} folder={folder} /></OwnerOnly>;
}

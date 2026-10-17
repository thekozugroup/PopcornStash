import * as stylex from '@stylexjs/stylex';
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {Divider} from '@astryxdesign/core/Divider';
import {HStack, VStack} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useToast} from '@astryxdesign/core/Toast';
import {Folder} from 'lucide-react';
import {useEffect, useState} from 'react';
import {useParams, useSearchParams} from 'react-router';
import {items as itemsApi, libraries as api} from '../api/client';
import type {Item} from '../api/types';
import {useLibraries} from '../app/LibrariesProvider';
import {useAuth} from '../auth/AuthProvider';
import {PageHeader} from '../components/PageHeader';
import {errorMessage} from '../lib/errors';
import {formatBytes} from '../lib/format';
import {LibraryGone} from './LibraryPage';

const styles = stylex.create({row: {minWidth: 0, flex: 1}, path: {overflowWrap: 'anywhere'}});

function FolderPicker({id, folder, onChange, disabled, version = 0}: {id: string; folder: string; onChange: (folder: string) => void; disabled: boolean; version?: number}) {
  const [children, setChildren] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    api.subfolders(id, folder, controller.signal).then(result => { if (!controller.signal.aborted) { setChildren(result.folders); setError(null); } })
      .catch((e: unknown) => { if (!controller.signal.aborted) setError(errorMessage(e, 'Folders could not be loaded.')); });
    return () => controller.abort();
  }, [id, folder, version]);
  return <VStack gap={2}>
    <Text weight="semibold" xstyle={styles.path}>{folder || 'Library root'}</Text>
    <HStack gap={2} wrap="wrap">
      {folder ? <Button label="Up one folder" variant="ghost" isDisabled={disabled} onClick={() => onChange(folder.split('/').slice(0, -1).join('/'))} /> : null}
      {children?.map(child => <Button key={child} label={child.split('/').at(-1) ?? child} icon={<Folder size={18} />} variant="secondary" isDisabled={disabled} onClick={() => onChange(child)} />)}
    </HStack>
    {error ? <Text role="alert">{error}</Text> : children === null ? <Text role="status">Reading folders…</Text> : children.length === 0 ? <Text type="supporting" color="secondary">No subfolders here.</Text> : null}
  </VStack>;
}

function FolderContents({id, folder, libraryName}: {id: string; folder: string; libraryName: string}) {
  const {isOwner} = useAuth();
  const [, setParams] = useSearchParams();
  const toast = useToast();
  const [draft, setDraft] = useState('');
  const [query, setQuery] = useState('');
  const [files, setFiles] = useState<Item[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [target, setTarget] = useState(folder);
  const [newName, setNewName] = useState('');
  const [version, setVersion] = useState(0);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  useEffect(() => { const t = setTimeout(() => { const value = draft.trim(); if (value !== query) { setLoading(true); setQuery(value); } }, 300); return () => clearTimeout(t); }, [draft, query]);
  const pending = loading || draft.trim() !== query;
  useEffect(() => {
    const controller = new AbortController();
    itemsApi.list({library_id: id, folder, q: query, sort: 'title', order: 'asc', limit: 200}, controller.signal).then(result => {
      if (!controller.signal.aborted) { setFiles(result.items); setCursor(result.next_cursor); setLoading(false); }
    }).catch((e: unknown) => { if (!controller.signal.aborted) { setFiles([]); setCursor(null); setLoading(false); setError(errorMessage(e, 'Files could not be loaded.')); } });
    return () => controller.abort();
  }, [id, folder, query, version]);
  const changeFolder = (value: string) => setParams(value ? {folder: value} : {});
  const create = async () => {
    setBusy(true); setError(null);
    try { await api.createFolder(id, target, newName.trim()); setNewName(''); setLoading(true); setVersion(v => v + 1); toast({body: 'Folder created.'}); }
    catch (e) { setError(errorMessage(e, 'Folder could not be created.')); }
    finally { setBusy(false); }
  };
  const more = async () => {
    if (!cursor) return;
    setBusy(true); setError(null);
    try {
      const page = await itemsApi.list({library_id: id, folder, q: query, sort: 'title', order: 'asc', limit: Math.min(200, 500 - files.length), cursor});
      setFiles(current => [...current, ...page.items.filter(item => !current.some(existing => existing.id === item.id))]); setCursor(page.next_cursor);
    } catch (e) { setError(errorMessage(e, 'More files could not be loaded.')); }
    finally { setBusy(false); }
  };
  const move = async () => {
    setBusy(true); setError(null);
    try {
      const result = await api.moveItems(id, [...selected], target);
      setSelected(new Set(result.failed.map(f => f.id)));
      if (result.completed.length) {
        toast({body: `${result.completed.length} files moved. Watch history is preserved.`});
      }
      if (result.failed.length) setError(`${result.failed.length} files did not move. ${result.failed.slice(0, 3).map(f => f.message).join(' ')}`);
      // Refresh from the server only after a complete batch; failed selections stay available.
      setLoading(true); setVersion(v => v + 1);
    } catch (e) { setError(errorMessage(e, 'Files did not move. Refresh before trying again.')); }
    finally { setBusy(false); setConfirm(false); }
  };
  return <>
    <PageHeader title={libraryName} subtitle="Browse subfolders or search their contents. Select files to move them together."
      actions={<Button label="Back to library" variant="ghost" href={`/libraries/${encodeURIComponent(id)}`} />} />
    <VStack gap={6}>
      <FolderPicker key={folder} id={id} folder={folder} onChange={changeFolder} disabled={busy} version={version} />
      <TextInput label="Search this folder and its subfolders" value={draft} onChange={value => { setDraft(value); setSelected(new Set()); }} isDisabled={busy} hasClear />
      {error ? <Banner status="error" title="Check these files" description={error} collapsible={false} endContent={<Button label="Refresh" variant="secondary" onClick={() => { setError(null); setLoading(true); setVersion(v => v + 1); }} />} /> : null}
      {isOwner ? <>
        <HStack gap={2} wrap="wrap">
          <Button label={`Select shown (${files.length})`} variant="secondary" isDisabled={busy || pending || !files.length} onClick={() => setSelected(new Set(files.map(f => f.id)))} />
          <Button label="Clear selection" variant="ghost" isDisabled={busy || !selected.size} onClick={() => setSelected(new Set())} />
          <Text role="status">{selected.size} selected</Text>
        </HStack>
        <Divider />
        <Text weight="semibold">Move selected to</Text>
        <FolderPicker key={`target-${target}`} id={id} folder={target} onChange={setTarget} disabled={busy} version={version} />
        <HStack gap={2} wrap="wrap" vAlign="end">
          <TextInput label="New subfolder in this destination" value={newName} onChange={setNewName} isDisabled={busy} width={280} onEnter={() => { if (newName.trim()) void create(); }} />
          <Button label="Create folder" variant="secondary" isDisabled={busy || !newName.trim()} onClick={() => void create()} />
          <Button label={`Move selected (${selected.size})`} variant="primary" isDisabled={busy || pending || !selected.size} onClick={() => setConfirm(true)} />
        </HStack>
      </> : null}
      {pending ? <Text role="status">Loading files…</Text> : files.length === 0 ? <Text>No files match this folder or search.</Text> : <VStack gap={0}>{files.map(file => <VStack key={file.id} gap={0}>
        <HStack gap={3} paddingBlock={3}>
          <VStack gap={1} xstyle={styles.row}>
            {isOwner ? <CheckboxInput label={file.title} value={selected.has(file.id)} isDisabled={busy} onChange={checked => setSelected(current => { const next = new Set(current); if (checked) next.add(file.id); else next.delete(file.id); return next; })} /> : <Button label={file.title} variant="ghost" href={`/item/${encodeURIComponent(file.id)}`} />}
            <Text type="supporting" color="secondary" xstyle={styles.path}>{file.rel_path} · {formatBytes(file.size)}</Text>
          </VStack>
          {isOwner ? <Button label="Open" variant="ghost" href={`/item/${encodeURIComponent(file.id)}`} /> : null}
        </HStack><Divider />
      </VStack>)}</VStack>}
      {cursor && files.length < 500 ? <Button label="Load more files" variant="secondary" isDisabled={busy || pending} onClick={() => void more()} /> : null}
      {cursor && files.length >= 500 ? <Text type="supporting">Showing 500 files. Narrow the search or move this selection before continuing.</Text> : null}
    </VStack>
    <AlertDialog isOpen={confirm} onOpenChange={open => { if (!busy) setConfirm(open); }} title={`Move ${selected.size} files?`}
      description={`Destination: ${target || 'Library root'}. Existing files are never overwritten. Only selected files move; companions stay unless selected.`}
      actionLabel="Move files" actionVariant="primary" isActionLoading={busy} onAction={() => void move()} />
  </>;
}

export default function LibraryFoldersPage() {
  const {id = ''} = useParams();
  const [params] = useSearchParams();
  const folder = params.get('folder') ?? '';
  const {find, status} = useLibraries();
  const library = find(id);
  if (!library) return status === 'ready' ? <LibraryGone /> : <Text role="status">Loading library…</Text>;
  return <FolderContents key={`${id}:${folder}`} id={id} folder={folder} libraryName={library.name} />;
}

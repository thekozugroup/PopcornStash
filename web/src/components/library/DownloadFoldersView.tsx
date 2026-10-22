import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {List, ListItem} from '@astryxdesign/core/List';
import {HStack, VStack} from '@astryxdesign/core/Layout';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {ChevronRight, Folder} from 'lucide-react';
import {useEffect, useState} from 'react';
import {useSearchParams} from 'react-router';
import {libraries as api} from '../../api/client';
import type {DownloadFolder, ItemSort} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {formatBytes, formatCount, watchedFraction} from '../../lib/format';
import {MediaGrid, MediaGridSkeleton} from '../MediaGrid';
import {MediaTile} from '../MediaTile';
import {FlattenDownloadFiles} from './FlattenDownloadFiles';
import {LibraryToolbar} from './LibraryToolbar';
import {displayTitle, itemHref, itemMeta, thumbSources} from './libraryFormat';
import {useLibraryEvents} from './useLibraryEvents';
import {usePagedItems} from './usePagedItems';

const styles = stylex.create({
  wrap: {overflowWrap: 'anywhere'},
});

export function DownloadFoldersView({id, isOwner}: {id: string; isOwner: boolean}) {
  const [params, setParams] = useSearchParams();
  const groupKey = params.get('download') ?? '';
  const [groups, setGroups] = useState<DownloadFolder[] | null>(null);
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [search, setSearch] = useState('');
  const [sort, setSort] = useState('name');
  const [editing, setEditing] = useState<'create' | 'rename' | null>(null);
  const [name, setName] = useState('');
  useEffect(() => {
    const abort = new AbortController();
    api.downloadFolders(id, abort.signal).then(value => { if (!abort.signal.aborted) { setGroups(value); setError(null); } })
      .catch((e: unknown) => { if (!abort.signal.aborted) setError(errorMessage(e, 'Download folders could not be loaded.')); });
    return () => abort.abort();
  }, [id, revision]);
  useLibraryEvents({onAdded: () => setRevision(v => v + 1), onRemoved: () => setRevision(v => v + 1), onUpdated: () => setRevision(v => v + 1)});
  const group = groups?.find(g => g.key === groupKey);
  const open = (key: string) => {
    setEditing(null); setError(null);
    setParams(current => { const next = new URLSearchParams(current); if (key) next.set('download', key); else next.delete('download'); next.delete('q'); next.delete('sort'); return next; });
  };
  const saveName = async () => {
    setBusy(true); setError(null);
    try {
      const result = await api.nameDownloadFolder(id, name.trim(), editing === 'rename' ? groupKey : undefined);
      setEditing(null); setRevision(v => v + 1); open(result.key);
    } catch (e) { setError(errorMessage(e, 'Folder could not be saved.')); }
    finally { setBusy(false); }
  };
  const visible = groups?.filter(g => g.name.toLocaleLowerCase().includes(search.toLocaleLowerCase())).sort((a, b) =>
    (sort === 'recent' ? b.added_at - a.added_at : sort === 'size' ? b.size - a.size : a.name.localeCompare(b.name)) || a.key.localeCompare(b.key));
  return <VStack gap={4}>
    <HStack gap={2} wrap="wrap">
      {groupKey ? <Button label="Download folders" variant="ghost" icon={<Folder size={18} />} onClick={() => open('')} /> : null}
      {group ? <Text weight="semibold" xstyle={styles.wrap}>{group.name}</Text> : null}
      {isOwner ? <>
        <Button label="New folder" variant="secondary" isDisabled={busy} onClick={() => { setEditing('create'); setName(''); }} />
        {group ? <Button label="Rename folder" variant="ghost" isDisabled={busy} onClick={() => { setEditing('rename'); setName(group.name); }} /> : null}
      </> : null}
    </HStack>
    <Text type="supporting" color="secondary">Folders group videos from each download, including nested source folders. Files stay in place.</Text>
    {isOwner && group ? <FlattenDownloadFiles key={`${id}:${group.key}`} id={id} groupKey={group.key} onMoved={() => setRevision(v => v + 1)} /> : null}
    {editing ? <VStack gap={2}>
      <TextInput label={editing === 'create' ? 'New folder name' : 'Folder name'} value={name} onChange={setName} isDisabled={busy} onEnter={() => { if (name.trim()) void saveName(); }} />
      <HStack gap={2}><Button label="Save folder" variant="primary" isDisabled={!name.trim()} isLoading={busy} onClick={() => void saveName()} /><Button label="Cancel" variant="ghost" isDisabled={busy} onClick={() => setEditing(null)} /></HStack>
    </VStack> : null}
    {error ? <Banner status="error" title="Folders unavailable" description={error} collapsible={false} endContent={<Button label="Try again" variant="secondary" onClick={() => setRevision(v => v + 1)} />} /> : null}
    {groups === null ? <Text role="status">Loading download folders…</Text> : groupKey ? group ?
      <DownloadVideos key={`${id}:${groupKey}`} id={id} group={group} groups={groups} isOwner={isOwner} onMoved={() => setRevision(v => v + 1)} /> :
      <Text>This folder is empty or no longer available. Choose Download folders to continue.</Text> : <>
      <HStack gap={2} wrap="wrap"><TextInput label="Find a download folder" value={search} onChange={setSearch} hasClear />
        <Selector label="Sort folders" value={sort} onChange={setSort} options={[{value: 'name', label: 'Name'}, {value: 'recent', label: 'Recently added'}, {value: 'size', label: 'Largest first'}]} /></HStack>
      <List hasDividers>{visible?.map(g => <ListItem key={g.key} label={g.name} description={`${formatCount(g.item_count, 'video')} · ${formatBytes(g.size)}`}
        startContent={<Folder size={24} aria-hidden="true" />} endContent={<ChevronRight size={18} aria-hidden="true" />} onClick={() => open(g.key)} />)}</List>
      {!visible?.length ? <Text>{groups.length ? 'No download folders match your search.' : 'No videos here yet. Downloads will appear in their own folders.'}</Text> : null}
    </>}
  </VStack>;
}

function DownloadVideos({id, group, groups, isOwner, onMoved}: {id: string; group: DownloadFolder; groups: DownloadFolder[]; isOwner: boolean; onMoved: () => void}) {
  const [query, setQuery] = useState('');
  const [sort, setSort] = useState<ItemSort>('title');
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [target, setTarget] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const list = usePagedItems({library_id: id, download_group: group.key, q: query, sort, order: sort === 'title' ? 'asc' : 'desc'});
  const move = async () => {
    setBusy(true); setError(null);
    try { await api.moveToDownloadFolder(id, target, [...selected]); setSelected(new Set()); setSelecting(false); setTarget(''); list.retry(); onMoved(); }
    catch (e) { setError(errorMessage(e, 'Videos stayed in this folder. Try again.')); }
    finally { setBusy(false); }
  };
  return <>
    <LibraryToolbar searchLabel={`Search ${group.name}`} query={query} onQueryChange={value => { setQuery(value); setSelected(new Set()); }} sort={sort} onSortChange={setSort}
      sortOptions={[{value: 'title', label: 'Title'}, {value: 'added', label: 'Recently added'}, {value: 'size', label: 'Largest first'}]} />
    {isOwner ? <VStack gap={2}>
      <HStack gap={2} wrap="wrap"><Button label={selecting ? 'Done selecting' : 'Select videos'} variant="secondary" isDisabled={busy} onClick={() => { setSelecting(v => !v); setSelected(new Set()); }} />
        {selecting ? <><Button label="Select shown" variant="ghost" isDisabled={busy || list.isStale || list.isLoading || list.items.length > 500} onClick={() => setSelected(new Set(list.items.map(i => i.id)))} /><Text role="status">{selected.size} selected</Text></> : null}</HStack>
      {selecting ? <HStack gap={2} wrap="wrap" vAlign="end"><Selector label="Move to folder" placeholder="Choose a folder" value={target} onChange={setTarget} hasSearch isDisabled={busy}
        options={groups.filter(g => g.key !== group.key).map(g => ({value: g.key, label: g.name}))} />
        <Button label="Move selected" variant="primary" isDisabled={!target || !selected.size || selected.size > 500 || list.isStale} isLoading={busy} onClick={() => void move()} /></HStack> : null}
    </VStack> : null}
    {error || list.error ? <Banner status="error" title="Check these videos" description={error ?? errorMessage(list.error, 'Videos could not be loaded.')} collapsible={false} endContent={<Button label="Try again" onClick={list.retry} />} /> : null}
    {list.isLoading ? <MediaGridSkeleton shape="frame" count={6} /> : <MediaGrid shape="frame" label={group.name} isBusy={list.isStale}>{list.items.map(item => {
      const thumb = thumbSources(item, list.versionOf(item.id));
      return <VStack key={item.id} gap={2}>
        {selecting ? <CheckboxInput label={`Select ${displayTitle(item)}`} value={selected.has(item.id)} isDisabled={busy || list.isStale || (selected.size >= 500 && !selected.has(item.id))}
          onChange={value => setSelected(current => { const next = new Set(current); if (value) next.add(item.id); else next.delete(item.id); return next; })} /> : null}
        <MediaTile href={itemHref(item)} title={displayTitle(item)} meta={itemMeta(item)} imageSrc={thumb?.src} imageSrcSet={thumb?.srcSet} shape="frame" progress={watchedFraction(item.progress)} />
      </VStack>;
    })}</MediaGrid>}
    {!list.isLoading && !list.items.length && !list.error ? <Text>{query ? 'No videos match your search.' : 'This folder is empty. Select videos in another folder to move them here.'}</Text> : null}
    {list.hasMore ? <Button label="Load more videos" variant="secondary" isLoading={list.isLoadingMore} onClick={list.loadMore} /> : null}
    {list.loadMoreError ? <Text role="alert">More videos could not be loaded. Try again.</Text> : null}
  </>;
}

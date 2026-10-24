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
import type {DownloadFolder, DownloadTree, RenameFolderPreview, ItemSort} from '../../api/types';
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
  const folder = params.get('subfolder') ?? '';
  const collapsed = params.get('subfolders') === 'collapsed';
  const [treeState,setTree] = useState<{key:string; value:DownloadTree} | null>(null);
  const [preview,setPreview] = useState<RenameFolderPreview | null>(null);
  const [groups, setGroups] = useState<DownloadFolder[] | null>(null);
  const [revision, setRevision] = useState(0);
  const treeKey = JSON.stringify([id,groupKey,folder,revision]);
  const tree = treeState?.key === treeKey ? treeState.value : null;
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [search, setSearch] = useState('');
  const [sort, setSort] = useState('name');
  const [organizing,setOrganizing] = useState(false);
  const [editing, setEditing] = useState<'create' | 'rename' | null>(null);
  const [name, setName] = useState('');
  useEffect(() => {
    const abort = new AbortController();
    api.downloadFolders(id, abort.signal).then(value => { if (!abort.signal.aborted) { setGroups(value); setError(null); } })
      .catch((e: unknown) => { if (!abort.signal.aborted) setError(errorMessage(e, 'Download folders could not be loaded.')); });
    return () => abort.abort();
  }, [id, revision]);
  useLibraryEvents({onAdded: () => setRevision(v => v + 1), onRemoved: () => setRevision(v => v + 1), onUpdated: () => setRevision(v => v + 1)});
  useEffect(() => {
    if (!groupKey || collapsed) return;
    const abort = new AbortController();
    api.downloadTree(id,groupKey,folder,abort.signal).then(value => {if(!abort.signal.aborted)setTree({key:treeKey,value});})
      .catch((e:unknown) => {if(!abort.signal.aborted)setError(errorMessage(e,'Subfolders could not be loaded.'));});
    return () => abort.abort();
  },[id,groupKey,folder,collapsed,revision,treeKey]);
  const group = groups?.find(g => g.key === groupKey);
  const browse = (path:string) => {setEditing(null);setPreview(null);setError(null);setParams(current=>{const next=new URLSearchParams(current);if(path)next.set('subfolder',path);else next.delete('subfolder');return next;});};
  const open = (key: string) => {
    setEditing(null); setPreview(null); setError(null);
    setParams(current => { const next = new URLSearchParams(current); if (key) next.set('download', key); else next.delete('download'); next.delete('q'); next.delete('sort'); next.delete('subfolder'); return next; });
  };
  const saveName = async () => {
    setBusy(true); setError(null);
    try {
      if(editing === 'rename') {
        if(!preview) {setPreview(await api.previewFolderRename(id,groupKey,folder,name.trim()));return;}
        await api.renameDirectory(id,groupKey,folder,name.trim(),preview.token);
        const parent=folder.includes('/')?folder.slice(0,folder.lastIndexOf('/')):'';
        if(folder)browse(parent?`${parent}/${name.trim()}`:name.trim());
      } else {
        const result = await api.createDownloadDirectory(id,name.trim(),groupKey || undefined,folder);
        if(!groupKey)open(result.key);
      }
      setEditing(null); setPreview(null); setRevision(v => v + 1);
    } catch (e) { setPreview(null); setError(errorMessage(e, 'Folder could not be saved.')); }
    finally { setBusy(false); }
  };
  const visible = groups?.filter(g => g.name.toLocaleLowerCase().includes(search.toLocaleLowerCase())).sort((a, b) =>
    (sort === 'recent' ? b.added_at - a.added_at : sort === 'size' ? b.size - a.size : a.name.localeCompare(b.name)) || a.key.localeCompare(b.key));
  return <VStack gap={4}>
    <HStack gap={2} wrap="wrap">
      {groupKey ? <Button label="Download folders" variant="ghost" icon={<Folder size={18} />} onClick={() => open('')} /> : null}
      {isOwner && group ? <Button label={organizing ? "Close folder actions" : "Folder actions"} variant="secondary" isDisabled={busy} onClick={()=>{setOrganizing(v=>!v);setEditing(null);setPreview(null);}} /> : null}
      {isOwner && (!group || organizing) ? <>
        <Button label={group ? "New subfolder" : "New folder"} variant="secondary" isDisabled={busy} onClick={() => { setEditing('create'); setName(''); setPreview(null); }} />
        {group ? <Button label="Rename folder" variant="ghost" isDisabled={busy} onClick={() => { setEditing('rename'); setName(folder.split('/').at(-1) || group.name); setPreview(null); }} /> : null}
      </> : null}
    </HStack>
    <Text type="supporting" color="secondary">Each download has its own folder. Browse its subfolders or collapse them to see every video together.</Text>
    {group ? <VStack gap={2}>
      <HStack gap={2} wrap="wrap">
        <Button label={group.name} variant="ghost" isDisabled={busy || !folder} onClick={() => browse('')} />
        {folder.split('/').filter(Boolean).map((part,index,parts)=><Button key={parts.slice(0,index+1).join('/')} label={part} variant="ghost" isDisabled={busy || index===parts.length-1} onClick={()=>browse(parts.slice(0,index+1).join('/'))} />)}
      </HStack>
      <CheckboxInput label="Collapse subfolders" size="sm" description="Show all videos together without moving files." value={collapsed} isDisabled={busy} onChange={value=>{setEditing(null);setPreview(null);setParams(current=>{const next=new URLSearchParams(current);if(value)next.set('subfolders','collapsed');else next.delete('subfolders');next.delete('subfolder');return next;});}} />
    </VStack> : null}
    {isOwner && group && organizing ? <FlattenDownloadFiles key={`${id}:${group.key}`} id={id} groupKey={group.key} onMoved={() => setRevision(v => v + 1)} /> : null}
    {editing ? <VStack gap={2}>
      <TextInput label={editing === 'create' ? 'New folder name' : 'Folder name'} value={name} onChange={value=>{setName(value);setPreview(null);}} isDisabled={busy} onEnter={() => { if (name.trim()) void saveName(); }} />
      <Text type="supporting" color="secondary">{editing === 'rename' ? 'Renames the actual folder on Atlas, including its subfolders and other files. Watch history and video details stay with each video.' : 'Creates a folder on Atlas inside this location.'}</Text>
      {preview ? <VStack gap={2}>
        <Text type="supporting" xstyle={styles.wrap}>From: {preview.from}</Text><Text type="supporting" xstyle={styles.wrap}>To: {preview.to}</Text>
        <Text role="status">{formatCount(preview.file_count,'file')} in {formatCount(preview.folder_count,'folder')}.</Text>
        {preview.blocked.map(message=><Text key={message} role="alert">{message}</Text>)}
      </VStack> : null}
      <HStack gap={2}><Button label={editing === 'rename' ? preview ? 'Rename on Atlas' : 'Preview rename' : 'Create folder'} variant="primary" isDisabled={!name.trim() || !!preview?.blocked.length} isLoading={busy} onClick={() => void saveName()} /><Button label="Cancel" variant="ghost" isDisabled={busy} onClick={() => {setEditing(null);setPreview(null);}} /></HStack>
    </VStack> : null}
    {error ? <Banner status="error" title="Folders unavailable" description={error} collapsible={false} endContent={<Button label="Try again" variant="secondary" onClick={() => setRevision(v => v + 1)} />} /> : null}
    {groups === null ? <Text role="status">Loading download folders…</Text> : groupKey ? group ?
      <>
      {!collapsed ? tree ? <List hasDividers>{tree.folders.map(child=><ListItem key={child.folder} label={child.name} description={`${formatCount(child.item_count,'video')} · ${formatBytes(child.size)}`} startContent={<Folder size={24} aria-hidden="true" />} endContent={<ChevronRight size={18} aria-hidden="true" />} onClick={()=>browse(child.folder)} />)}</List> : <Text role="status">Loading subfolders…</Text> : null}
      <DownloadVideos key={`${id}:${groupKey}:${folder}:${collapsed}`} folder={collapsed ? undefined : folder} id={id} group={group} groups={groups} isOwner={isOwner} onMoved={() => setRevision(v => v + 1)} /></> :
      <Text>This folder is empty or no longer available. Choose Download folders to continue.</Text> : <>
      <HStack gap={2} wrap="wrap"><TextInput label="Find a download folder" value={search} onChange={setSearch} hasClear />
        <Selector label="Sort folders" value={sort} onChange={setSort} options={[{value: 'name', label: 'Name'}, {value: 'recent', label: 'Recently added'}, {value: 'size', label: 'Largest first'}]} /></HStack>
      <List hasDividers>{visible?.map(g => <ListItem key={g.key} label={g.name} description={`${formatCount(g.item_count, 'video')} · ${formatBytes(g.size)}`}
        startContent={<Folder size={24} aria-hidden="true" />} endContent={<ChevronRight size={18} aria-hidden="true" />} onClick={() => open(g.key)} />)}</List>
      {!visible?.length ? <Text>{groups.length ? 'No download folders match your search.' : 'No videos here yet. Downloads will appear in their own folders.'}</Text> : null}
    </>}
  </VStack>;
}

function DownloadVideos({id, group, groups, isOwner, onMoved, folder}: {folder?: string; id: string; group: DownloadFolder; groups: DownloadFolder[]; isOwner: boolean; onMoved: () => void}) {
  const [query, setQuery] = useState('');
  const [sort, setSort] = useState<ItemSort>('title');
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [target, setTarget] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const list = usePagedItems({library_id: id, download_group: group.key, download_folder: folder, q: query, sort, order: sort === 'title' ? 'asc' : 'desc'});
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
    {!list.isLoading && !list.items.length && !list.error ? <Text>{query ? 'No videos match your search.' : folder !== undefined ? 'No videos directly in this folder. Open a subfolder or turn on Collapse subfolders.' : 'This download folder has no videos yet.'}</Text> : null}
    {list.hasMore ? <Button label="Load more videos" variant="secondary" isLoading={list.isLoadingMore} onClick={list.loadMore} /> : null}
    {list.loadMoreError ? <Text role="alert">More videos could not be loaded. Try again.</Text> : null}
  </>;
}

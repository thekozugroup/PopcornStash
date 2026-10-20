import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter, VStack} from '@astryxdesign/core/Layout';
import {Selector} from '@astryxdesign/core/Selector';
import {Divider} from '@astryxdesign/core/Divider';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useEffect, useState} from 'react';
import {downloads as api} from '../../api/client';
import type {Download, TorrentFiles, TorrentFileKind} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {formatBytes, formatCount} from '../../lib/format';
import {fileKinds} from './fileSelection';
import {FileRulesEditor} from './FileRulesEditor';
import {TorrentTypeChoices} from './TorrentTypeChoices';
import {draftFromRules, emptyFileRules, matchesFileRules, parseFileRules} from './fileRules';

const styles = stylex.create({
  path: {overflowWrap: 'anywhere'},
});

export function TorrentFilesDialog({download, onClose, onSaved}: {
  download: Download; onClose: () => void; onSaved: () => void;
}) {
  const [data, setData] = useState<TorrentFiles | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const [filter, setFilter] = useState('all');
  const [search, setSearch] = useState('');
  const [shown, setShown] = useState(100);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [draft, setDraft] = useState({...emptyFileRules});
  const [ruleTypes, setRuleTypes] = useState<TorrentFileKind[]>(['video', 'subtitle']);
  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = async () => {
      try {
        const next = await api.files(download.id, controller.signal);
        if (cancelled) return;
        setData(next);
        if (next.ready) { setDraft(draftFromRules(next.rules)); setRuleTypes(next.rules?.file_types ?? ['video', 'subtitle']); }
        setError(null);
        if (next.ready) setSelected(new Set(next.files.filter(file => file.selected).map(file => file.index)));
        else timer = setTimeout(() => void load(), 3000);
      } catch (e) { if (!cancelled) setError(errorMessage(e, 'Files could not be loaded. Try again.')); }
    };
    void load();
    return () => { cancelled = true; controller.abort(); clearTimeout(timer); };
  }, [download.id, revision]);
  const files = data?.files ?? [];
  const parsed = parseFileRules(draft, ruleTypes);
  const preview = selected;
  const applyFilters = () => {
    if (parsed.error || ruleTypes.length === 0) return;
    setSelected(new Set(files.filter(file => draft.enabled && parsed.rules
      ? matchesFileRules(file, parsed.rules) : ruleTypes.includes(file.kind)).map(file => file.index)));
  };
  const visible = files.filter(file => (filter === 'all' || file.kind === filter) && file.path.toLocaleLowerCase().includes(search.toLocaleLowerCase()));
  const save = async () => {
    setBusy(true); setError(null);
    try {
      if (data?.review_required) await api.startTorrent(download.id, [...selected]);
      else await api.selectFiles(download.id, [...selected]);
      onSaved(); onClose();
    }
    catch (e) { setError(errorMessage(e, 'The selection was not saved. Try again.')); }
    finally { setBusy(false); }
  };
  return <Dialog isOpen onOpenChange={open => { if (!open && !busy) onClose(); }} purpose="form" width={660}>
    <Layout header={<DialogHeader title={data?.review_required ? "Review torrent" : "Choose files"} subtitle={download.name} onOpenChange={open => { if (!open && !busy) onClose(); }} />}
      content={<LayoutContent><VStack gap={4}>
        {error ? <Banner status="error" title="Files unavailable" description={error} collapsible={false} /> : null}
        {!data?.ready ? <VStack gap={2}>
          <Text as="p" color="secondary">{error ? 'Retry when the torrent is available.' : 'Fetching the file list. This needs a peer with the torrent’s details.'}</Text>
          <Button label="Try again" variant="secondary" onClick={() => setRevision(v => v + 1)} />
        </VStack> : <>
          <Text as="p" color="secondary">{data?.review_required ? 'Choose what to download. File contents stay stopped until you start.' : 'Adjust this torrent’s selection. Files already downloaded are not deleted.'}</Text>
          <Button label={filtersOpen ? 'Hide filters' : 'Filters'} variant="ghost" isDisabled={busy} onClick={() => setFiltersOpen(value => !value)} />
          {filtersOpen ? <VStack gap={4}>
          <FileRulesEditor value={draft} onChange={setDraft} disabled={busy} error={parsed.error} />
          <TorrentTypeChoices showHelp={false} label="File types to match" value={ruleTypes} onChange={setRuleTypes} disabled={busy} />
          <Button label="Apply filters to selection" variant="secondary" isDisabled={busy || !!parsed.error || ruleTypes.length === 0} onClick={applyFilters} />
          <Text as="p" type="supporting" color="secondary">Filters select matching files. Then adjust any checkbox below. Applying filters again replaces your selection.</Text>
          </VStack> : null}
          <TextInput label="Find files" value={search} onChange={value => { setSearch(value); setShown(100); }} />
          <Selector label="Show" value={filter} onChange={value => { setFilter(value); setShown(100); }}
            options={[{value: 'all', label: 'All types'}, ...fileKinds]} width="100%" />
          <HStack gap={2} wrap="wrap">
            <Button label="Select shown" variant="ghost" isDisabled={busy || visible.length === 0} onClick={() => setSelected(current => new Set([...current, ...visible.map(f => f.index)]))} />
            <Button label="Deselect shown" variant="ghost" isDisabled={busy || visible.length === 0} onClick={() => setSelected(current => new Set([...current].filter(index => !visible.some(f => f.index === index))))} />
          </HStack>
          <VStack gap={0}>{visible.slice(0, shown).map(file => <VStack key={file.index} gap={0}>
            <VStack gap={0} paddingBlock={3}>
            <CheckboxInput label={file.path.split('/').at(-1) ?? file.path} value={preview.has(file.index)}
              isDisabled={busy} onChange={value => setSelected(current => { const next = new Set(current); if (value) next.add(file.index); else next.delete(file.index); return next; })} />
            <Text as="p" type="supporting" color="secondary" xstyle={styles.path}>{file.path} · {formatBytes(file.size)}</Text>
            </VStack>
            <Divider />
          </VStack>)}</VStack>
          {visible.length === 0 ? <Text as="p" color="secondary">No files match this filter.</Text> : null}
          {visible.length > shown ? <Button label="Show more" variant="ghost" onClick={() => setShown(n => n + 100)} /> : null}

          {preview.size === 0 ? <Text as="p" type="supporting" color="secondary">Select at least one file to continue.</Text> : null}
        </>}
      </VStack></LayoutContent>}
      footer={<LayoutFooter><VStack gap={2}>
        {data?.ready ? <Text as="p" type="supporting" aria-live="polite">{formatCount(preview.size, 'file')} selected · {formatBytes(files.reduce((sum, f) => sum + (preview.has(f.index) ? f.size : 0), 0))}</Text> : null}
        <HStack gap={2} hAlign="end" wrap="wrap">
        <Button label="Close" variant="secondary" isDisabled={busy} onClick={onClose} />
        <Button label={data?.review_required ? 'Start download' : 'Save selection'} variant="primary" isDisabled={!data?.ready || selected.size === 0} isLoading={busy} onClick={() => void save()} />
      </HStack></VStack></LayoutFooter>} />
  </Dialog>;
}

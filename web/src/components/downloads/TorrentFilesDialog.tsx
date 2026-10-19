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
import {fileKinds, groupValue, selectGroup} from './fileSelection';
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
  const preview = draft.enabled ? new Set(files.filter(f => parsed.rules && matchesFileRules(f, parsed.rules)).map(f => f.index)) : selected;
  const visible = files.filter(file => (filter === 'all' || file.kind === filter) && file.path.toLocaleLowerCase().includes(search.toLocaleLowerCase()));
  const save = async () => {
    setBusy(true); setError(null);
    try {
      if (draft.enabled && parsed.rules) await api.setRules(download.id, parsed.rules);
      else await api.selectFiles(download.id, [...selected]);
      onSaved(); onClose();
    }
    catch (e) { setError(errorMessage(e, 'The selection was not saved. Try again.')); }
    finally { setBusy(false); }
  };
  return <Dialog isOpen onOpenChange={open => { if (!open && !busy) onClose(); }} purpose="form" width={660}>
    <Layout header={<DialogHeader title="Choose files" subtitle={download.name} onOpenChange={open => { if (!open && !busy) onClose(); }} />}
      content={<LayoutContent><VStack gap={4}>
        {error ? <Banner status="error" title="Files unavailable" description={error} collapsible={false} /> : null}
        {!data?.ready ? <VStack gap={2}>
          <Text as="p" color="secondary">{error ? 'Retry when the torrent is available.' : 'Fetching the file list. This needs a peer with the torrent’s details.'}</Text>
          <Button label="Try again" variant="secondary" onClick={() => setRevision(v => v + 1)} />
        </VStack> : <>
          <FileRulesEditor value={draft} onChange={setDraft} disabled={busy} error={parsed.error} />
          {draft.enabled ? <TorrentTypeChoices showHelp={false} label="File types to match" value={ruleTypes} onChange={setRuleTypes} disabled={busy} /> : <HStack gap={4} wrap="wrap">{fileKinds.map(kind => <CheckboxInput key={kind.value} label={kind.label}
            value={groupValue(files, selected, kind.value)} isDisabled={busy || !files.some(f => f.kind === kind.value)}
            onChange={value => setSelected(selectGroup(files, selected, kind.value, value))} />)}</HStack>}
          <Text as="p" type="supporting" color="secondary">{draft.enabled ? 'The list previews your rules. Turn rules off to choose individual files. Already downloaded files are not deleted by rule changes.' : 'Only selected files are kept. Deselect instructions, artwork or subtitles you don’t need. This changes this torrent only.'}</Text>
          <TextInput label="Find files" value={search} onChange={value => { setSearch(value); setShown(100); }} />
          <Selector label="Show" value={filter} onChange={value => { setFilter(value); setShown(100); }}
            options={[{value: 'all', label: 'All types'}, ...fileKinds]} width="100%" />
          <VStack gap={0}>{visible.slice(0, shown).map(file => <VStack key={file.index} gap={0}>
            <VStack gap={0} paddingBlock={3}>
            <CheckboxInput label={file.path.split('/').at(-1) ?? file.path} value={preview.has(file.index)}
              isDisabled={busy || draft.enabled} onChange={value => setSelected(current => { const next = new Set(current); if (value) next.add(file.index); else next.delete(file.index); return next; })} />
            <Text as="p" type="supporting" color="secondary" xstyle={styles.path}>{file.path} · {formatBytes(file.size)}</Text>
            </VStack>
            <Divider />
          </VStack>)}</VStack>
          {visible.length === 0 ? <Text as="p" color="secondary">No files match this filter.</Text> : null}
          {visible.length > shown ? <Button label="Show more" variant="ghost" onClick={() => setShown(n => n + 100)} /> : null}

          {draft.enabled && preview.size === 0 && !parsed.error ? <Text as="p" type="supporting" color="secondary">No files match. Applying these rules pauses this torrent until you change the rules or choose files.</Text> : null}
        </>}
      </VStack></LayoutContent>}
      footer={<LayoutFooter><VStack gap={2}>
        {data?.ready ? <Text as="p" type="supporting" aria-live="polite">{formatCount(preview.size, 'file')} {draft.enabled ? (preview.size === 1 ? 'matches' : 'match') : 'selected'} · {formatBytes(files.reduce((sum, f) => sum + (preview.has(f.index) ? f.size : 0), 0))}</Text> : null}
        <HStack gap={2} hAlign="end" wrap="wrap">
        <Button label="Cancel" variant="secondary" isDisabled={busy} onClick={onClose} />
        <Button label={draft.enabled ? 'Apply rules' : 'Save selection'} variant="primary" isDisabled={!data?.ready || (draft.enabled ? !!parsed.error : selected.size === 0)} isLoading={busy} onClick={() => void save()} />
      </HStack></VStack></LayoutFooter>} />
  </Dialog>;
}

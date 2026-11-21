import * as stylex from '@stylexjs/stylex';
import {useEffect, useState} from 'react';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {VStack, HStack} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import {libraries as api} from '../../api/client';
import type {FlattenPreview} from '../../api/types';
import {errorMessage} from '../../lib/errors';
import {formatCount} from '../../lib/format';

const styles = stylex.create({path: {overflowWrap: 'anywhere'}});

/** The preview itself is the confirmation. No disk operation happens when opening it. */
export function FlattenDownloadFiles({id, groupKey, onMoved, onBusy, onClose}: {
  id: string;
  groupKey: string;
  onMoved: () => void;
  onBusy: (busy: boolean) => void;
  onClose: () => void;
}) {
  const [revision, setRevision] = useState(0);
  const [loaded, setLoaded] = useState<{revision: number; value: FlattenPreview | null}>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const preview = loaded?.revision === revision ? loaded.value : null;
  useEffect(() => {
    let current = true;
    api.previewFlatten(id, groupKey).then(
      value => { if (current) setLoaded({revision, value}); },
      cause => {
        if (!current) return;
        setLoaded({revision, value: null});
        setError(errorMessage(cause, 'File moves could not be previewed. Try again.'));
      },
    );
    return () => { current = false; };
  }, [id, groupKey, revision]);
  const load = () => {
    setRevision(value => value + 1); setError(null); setResult(null); setExpanded(false);
  };
  const move = async () => {
    if (!preview) return;
    setBusy(true); onBusy(true); setError(null);
    try {
      const value = await api.flattenDownload(id, groupKey, preview.token);
      setLoaded({revision, value: null});
      setResult(`${formatCount(value.completed.length, 'video')} organized.`);
      if (value.failed.length) setError(value.failed.map(f => f.message).join(' '));
      onMoved();
    } catch (e) { setLoaded({revision, value: null}); setError(errorMessage(e, 'Files could not be moved. Review a fresh preview.')); }
    finally { setBusy(false); onBusy(false); }
  };
  const moves = preview?.files.filter(file => file.from !== file.to) ?? [];
  return <VStack gap={3}>
    {!preview ? <HStack gap={2} wrap="wrap">
      {loaded?.revision !== revision ? <Text role="status">Preparing preview…</Text> : null}
      {error && !result ? <Button label="Retry preview" variant="secondary" onClick={load} /> : null}
      <Button label={result ? 'Done' : 'Cancel'} variant="ghost" onClick={onClose} />
    </HStack> : <VStack gap={3}>
      <Text weight="semibold">Move files into this download folder</Text>
      <Text type="supporting" color="secondary" xstyle={styles.path}>Destination: {preview.folder}</Text>
      <Text type="supporting" color="secondary">Videos and their matching sidecars move here, with filenames unchanged. Other files and folders stay in place. Nothing is overwritten.</Text>
      <Text role="status">{moves.length ? `${formatCount(moves.length, 'file')} to move.` : preview.blocked.length ? 'No files are ready to move.' : 'Files are already in this folder.'}</Text>
      {preview.blocked.map(message => <Text key={message} role="alert" xstyle={styles.path}>{message}</Text>)}
      {(expanded ? moves : moves.slice(0, 12)).map(file => <VStack key={`${file.item_id}:${file.from}`} gap={1}>
        <Text type="supporting" color="secondary" xstyle={styles.path}>From: {file.from}</Text>
        <Text type="supporting" xstyle={styles.path}>To: {file.to}</Text>
      </VStack>)}
      {!expanded && moves.length > 12 ? <Button label={`Show all ${moves.length} moves`} variant="ghost" onClick={() => setExpanded(true)} /> : null}
      <HStack gap={2} wrap="wrap">
        <Button label="Move files" variant="primary" isDisabled={!moves.length || !!preview.blocked.length} isLoading={busy} onClick={() => void move()} />
        <Button label="Refresh preview" variant="secondary" isDisabled={busy} onClick={load} />
        <Button label="Cancel" variant="ghost" isDisabled={busy} onClick={onClose} />
      </HStack>
    </VStack>}
    {result ? <Text role="status">{result}</Text> : null}
    {error ? <Banner status="error" title="Files need attention" description={error} collapsible={false} /> : null}
  </VStack>;
}

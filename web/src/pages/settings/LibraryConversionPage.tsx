import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Banner} from '@astryxdesign/core/Banner';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {Heading} from '@astryxdesign/core/Heading';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {ChevronLeft} from 'lucide-react';
import {useEffect, useState, type FormEvent} from 'react';
import {useParams} from 'react-router';
import {libraries} from '../../api/client';
import type {ConversionPolicy, ConversionView} from '../../api/types';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {StorageSavings} from './ConversionSavings';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {useDocumentTitle} from '../../components/PageHeader';
import {errorMessage} from '../../lib/errors';
import {formatBytes} from '../../lib/format';

const styles = stylex.create({
  page: {minWidth: 0},
  back: {marginInlineStart: -12, marginBottom: 8},
  field: {display: 'flex', flexDirection: 'column', gap: 20, maxWidth: 440, minWidth: 0},
  row: {display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 12},
  jobs: {display: 'flex', flexDirection: 'column', gap: 20, minWidth: 0},
  job: {display: 'flex', flexDirection: 'column', gap: 4, minWidth: 0, overflowWrap: 'anywhere'},
});
const stateLabel: Record<string, string> = {encoding: 'Converting', verifying: 'Checking video', publishing: 'Replacing original', complete: 'Complete', skipped: 'Original kept', failed: 'Needs attention', cancelled: 'Stopped'};

export default function Component() {
  return <OwnerOnly><ConversionSettings /></OwnerOnly>;
}
function ConversionSettings() {
  const {id = ''} = useParams();
  const [view, setView] = useState<ConversionView | null>(null);
  const [draft, setDraft] = useState<ConversionPolicy | null>(null);
  const [name, setName] = useState('Library');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useDocumentTitle(`${name} video conversion`);
  useEffect(() => {
    const controller = new AbortController();
    let running = false;
    const refresh = async () => {
      if (running) return;
      running = true;
      try {
        const [next, library] = await Promise.all([libraries.conversion(id, controller.signal), libraries.get(id, controller.signal)]);
        if (!controller.signal.aborted) { setView(next); setDraft(current => current ?? next.policy); setName(library.name); setError(''); }
      } catch (e) { if (!controller.signal.aborted) setError(errorMessage(e)); }
      finally { running = false; }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => { controller.abort(); clearInterval(timer); };
  }, [id, attempt]);
  const change = (patch: Partial<ConversionPolicy>) => { setDraft(current => current && {...current, ...patch}); setNotice(''); };
  async function save(event: FormEvent) {
    event.preventDefault();
    if (!draft || busy) return;
    setBusy(true); setError(''); setNotice('');
    try { const next = await libraries.saveConversion(id, draft); setView(next); setDraft(next.policy); setNotice(next.policy.enabled ? 'Saved. This library will be checked automatically.' : 'Saved. Automatic conversion is off.'); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }
  async function retry() {
    setBusy(true);
    try { setView(await libraries.retryConversion(id)); setNotice('Failed and stopped videos will be checked again.'); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }
  const matching = view?.encoders.filter(e => e.codec === draft?.codec && (draft.compute === 'auto' || e.hardware === (draft.compute === 'hardware'))) ?? [];
  return <div {...stylex.props(styles.page)}>
    <Button label="Transcoding" variant="ghost" icon={<ChevronLeft size={18} aria-hidden="true" />} href="/settings/transcoding" xstyle={styles.back} />
    <Heading level={1}>Video conversion</Heading>
    {error ? <Banner status="error" title="Could not update conversion" description={error} endContent={<Button label="Try again" onClick={() => setAttempt(x => x + 1)} />} /> : null}
    {!view || !draft ? <Text role="status">Loading conversion settings…</Text> : <>
      <form onSubmit={event => void save(event)}>
        <SettingsGroup title="Automatic conversion" description="Applies to existing and newly imported videos in this library. Downloads finish and import before conversion starts.">
          <CheckboxInput label="Convert this library automatically" value={draft.enabled} onChange={enabled => change({enabled})} isDisabled={busy} description="Enabling this allows originals to be replaced. Only smaller files that pass full-video, frame, audio and subtitle checks are kept." />
          <Text type="supporting" color="secondary">Item titles, folders, favourites and watch progress stay with the video. HDR, rotated videos and formats that cannot be preserved are kept unchanged.</Text>
        </SettingsGroup>
        <SettingsGroup title="Format and quality" description="AV1 saves space on modern devices. H.265 works with a wider range of players.">
          <div {...stylex.props(styles.field)}>
            <Selector label="Video format" value={draft.codec} onChange={codec => change({codec: codec as ConversionPolicy['codec']})} options={[{value: 'av1', label: 'AV1'}, {value: 'hevc', label: 'H.265 (HEVC)'}]} presentation="adaptive" width="100%" isDisabled={busy} />
            <Selector label="Quality" value={draft.quality} onChange={quality => change({quality: quality as ConversionPolicy['quality']})} options={[{value: 'high', label: 'Higher quality'}, {value: 'balanced', label: 'Balanced'}, {value: 'small', label: 'Smaller files'}]} description="Conversion is lossy. Higher quality keeps more detail; a file is replaced only if it also saves space." presentation="adaptive" width="100%" isDisabled={busy} />
          </div>
        </SettingsGroup>
        <SettingsGroup title="Processing" description="Runs on this server. Automatic prefers a tested GPU and uses the CPU when no compatible GPU is available.">
          <div {...stylex.props(styles.field)}>
            <Selector label="Use" value={draft.compute} onChange={compute => change({compute: compute as ConversionPolicy['compute']})} options={[{value: 'auto', label: 'Automatic'}, {value: 'hardware', label: 'GPU only'}, {value: 'cpu', label: 'CPU only'}]} presentation="adaptive" width="100%" isDisabled={busy} />
            <Text type="supporting" color="secondary">{matching[0] ? `Available: ${matching[0].label}` : 'No working encoder for this choice. Originals will be kept.'}</Text>
            <Text type="supporting" color="secondary">Turning conversion off stops active conversions safely. Completed replacements remain.</Text>
          </div>
        </SettingsGroup>
        <div {...stylex.props(styles.row)}><Button type="submit" variant="primary" label="Save changes" isLoading={busy} />{notice ? <Text role="status">{notice}</Text> : null}</div>
      </form>
      <SettingsGroup title="Storage savings" description="Verified replacements for videos still in this library."><StorageSavings savings={view.savings} /></SettingsGroup>
      <SettingsGroup title="Activity" description={view.policy.enabled ? `${view.pending} videos waiting to be checked.` : 'Automatic conversion is off.'}>
        <div {...stylex.props(styles.jobs)} aria-live="polite">
          {view.jobs.length === 0 ? <Text color="secondary">No conversions yet.</Text> : view.jobs.map(job => <div key={job.id} {...stylex.props(styles.job)}>
            <Text weight="medium">{job.title}</Text>
            <Text type="supporting" color="secondary">{stateLabel[job.state] ?? job.state}{job.state === 'encoding' || job.state === 'verifying' ? ` · ${Math.round(job.progress * 100)}%` : ''}{job.saved_bytes > 0 ? ` · ${formatBytes(job.saved_bytes)} saved` : ''}</Text>
            {job.encoder ? <Text type="supporting" color="secondary">{job.encoder}</Text> : null}
            {job.message ? <Text type="supporting" color="secondary">{job.message}</Text> : null}
          </div>)}
          {view.jobs.some(j => j.state === 'failed' || j.state === 'cancelled') ? <Button label="Retry stopped conversions" onClick={() => void retry()} isDisabled={busy || !view.policy.enabled} /> : null}
        </div>
      </SettingsGroup>
    </>}
  </div>;
}

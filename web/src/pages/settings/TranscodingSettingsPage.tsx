import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {ProgressBar} from '@astryxdesign/core/ProgressBar';
import {Switch} from '@astryxdesign/core/Switch';
import {Text} from '@astryxdesign/core/Text';
import {useState} from 'react';
import {libraries} from '../../api/client';
import type {TranscodingOverview} from '../../api/types';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {formatBytes} from '../../lib/format';
import {SettingsSectionHeader} from './SettingsSectionHeader';
import {useLiveTranscoding} from './useLiveTranscoding';
import {StorageSavings, percent} from './ConversionSavings';

const styles = stylex.create({
  list: {display: 'flex', flexDirection: 'column', minWidth: 0},
  row: {paddingBlock: 20, borderBottomWidth: {default: 1, ':last-child': 0}, borderBottomStyle: 'solid', borderBottomColor: colorVars['--color-border'], display: 'flex', flexDirection: 'column', gap: 10, minWidth: 0},
  top: {display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 20, minWidth: 0},
  title: {minWidth: 0, overflowWrap: 'anywhere'},
  banner: {marginBlock: 12},
  link: {alignSelf: 'flex-start'},
  actions: {display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 12},
});
const states: Record<string, string> = {encoding: 'Converting', verifying: 'Checking video', publishing: 'Replacing original', complete: 'Complete', skipped: 'Original kept', failed: 'Needs attention', cancelled: 'Stopped'};
export default function Component() { return <OwnerOnly><TranscodingSettings /></OwnerOnly>; }
function TranscodingSettings() {
  const live = useLiveTranscoding();
  const [saving, setSaving] = useState<Record<string, boolean>>({});
  const [saveError, setSaveError] = useState('');
  const [notice, setNotice] = useState('');
  const [clearing, setClearing] = useState(false);
  async function clearActivity() {
    if (clearing) return;
    setClearing(true); setSaveError('');
    try { live.acceptOverview(await libraries.clearTranscodingActivity()); setNotice('Finished activity cleared. Storage savings are preserved.'); }
    catch { setSaveError('Could not clear activity. Try again.'); }
    finally { setClearing(false); }
  }
  async function toggle(library: TranscodingOverview['libraries'][number], enabled: boolean) {
    if (saving[library.id]) return;
    setSaving(current => ({...current, [library.id]: true})); setSaveError(''); setNotice('');
    try {
      const view = await libraries.saveConversion(library.id, {...library.policy, enabled});
      live.accept(library.id, view);
      setNotice(`${library.name}: automatic transcoding ${enabled ? 'on' : 'off'}.`);
    } catch { setSaveError(`Could not confirm changes for ${library.name}. Check the latest switch state before trying again.`); }
    finally { setSaving(current => ({...current, [library.id]: false})); }
  }
  const data = live.data;
  return <>
    <SettingsSectionHeader title="Transcoding" subtitle="Make library videos smaller, with full-video checks before originals are replaced." />
    <div {...stylex.props(styles.actions)}>
      <Text type="supporting" color="secondary">{live.error || live.stale ? 'Updates paused. Showing the last available data.' : live.updated ? live.stream === 'open' ? 'Live updates' : 'Updating every 2 seconds' : 'Connecting…'}{live.updated ? ` · Updated ${new Date(live.updated).toLocaleTimeString()}` : ''}</Text>
      <Button label="Refresh" variant="ghost" onClick={live.reload} />
    </div>
    {live.error ? <Banner xstyle={styles.banner} status="warning" title="Could not refresh activity" description="Transcoding continues on the server. This page will reconnect automatically." /> : null}
    {saveError ? <Banner xstyle={styles.banner} status="error" title="Could not confirm settings" description={saveError} /> : null}
    {notice ? <Text role="status">{notice}</Text> : null}
    {!data ? <Text role="status">{live.error ? 'Waiting for a connection. Use Refresh to try again.' : 'Loading transcoding…'}</Text> : <>
      <SettingsGroup title="Storage savings" description="Verified replacements for videos still in your libraries. Original size includes unchanged videos; edited or deleted files no longer contribute past savings.">
        <StorageSavings savings={data.savings} />
      </SettingsGroup>
      <SettingsGroup title="Libraries" description="Each switch saves immediately. Enabling allows verified, smaller replacements of existing videos and new imports. Turning it off safely stops that library's active conversions.">
        <div {...stylex.props(styles.list)}>
          {data.libraries.length === 0 ? <Text>No video libraries yet. Create one in Libraries to begin.</Text> : data.libraries.map(library => {
            return <div key={library.id} {...stylex.props(styles.row)}>
              <Switch label={library.name} value={library.policy.enabled} onChange={enabled => void toggle(library, enabled)} labelPosition="start" labelSpacing="spread" width="100%" isLoading={saving[library.id]} isDisabled={saving[library.id] || live.error || live.stale} />
              <Text type="supporting" color="secondary">{library.policy.enabled ? 'On' : 'Off'} · {library.policy.codec === 'av1' ? 'AV1' : 'H.265'} · {library.pending} waiting to be checked</Text>
              <StorageSavings savings={library.savings} />
              <Button xstyle={styles.link} label={`Options for ${library.name}`} variant="ghost" href={`/settings/transcoding/${library.id}`} />
            </div>;
          })}
        </div>
      </SettingsGroup>
      <SettingsGroup title="Activity" description="Current tasks first, followed by recent results. Checking the video can take time after encoding reaches 100%.">
        <Button xstyle={styles.link} label="Clear finished activity" variant="ghost" onClick={() => void clearActivity()} isLoading={clearing} isDisabled={clearing || !data.jobs.some(job => ['complete', 'skipped', 'failed', 'cancelled'].includes(job.state))} />
        <Text type="supporting" color="secondary">Clears finished entries. Active tasks and storage savings stay.</Text>
        <div {...stylex.props(styles.list)}>
          {data.jobs.length === 0 ? <Text>No recent activity. Enabled libraries continue to be checked automatically.</Text> : data.jobs.map(job => <div key={job.id} {...stylex.props(styles.row)}>
            <div {...stylex.props(styles.top)}><Heading level={4} xstyle={styles.title}>{job.title}</Heading><Text type="supporting">{states[job.state] ?? job.state}</Text></div>
            <Text type="supporting" color="secondary">{job.library_name}{job.encoder ? ` · ${job.encoder}` : ''}</Text>
            {job.state === 'encoding' || job.state === 'verifying' ? <ProgressBar label={job.state === 'encoding' ? 'Encoding progress' : 'Video check progress'} value={Math.round(job.progress * 100)} hasValueLabel /> : null}
            {job.state === 'publishing' ? <ProgressBar label={`${states[job.state]}: ${job.title}`} isIndeterminate /> : null}
            {job.result_bytes !== null ? <Text type="supporting">{formatBytes(job.original_bytes)} → {formatBytes(job.result_bytes)} · {formatBytes(job.saved_bytes)} saved ({percent(job.original_bytes > 0 ? job.saved_bytes * 100 / job.original_bytes : 0)})</Text> : <Text type="supporting" color="secondary">Original: {formatBytes(job.original_bytes)}</Text>}
            {job.message ? <Text type="supporting" color="secondary">{job.message}</Text> : null}
            {job.state === 'failed' || job.state === 'cancelled' ? <Button label={`Review ${job.library_name} options`} variant="ghost" href={`/settings/transcoding/${job.library_id}`} /> : null}
          </div>)}
        </div>
      </SettingsGroup>
    </>}
  </>;
}

/**
 * Settings, System (owner): version and health, disk space, background
 * work, hardware acceleration and streaming (transcode) settings.
 */
import * as stylex from '@stylexjs/stylex';
import {typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Code} from '@astryxdesign/core/Code';
import {MetadataList, MetadataListItem} from '@astryxdesign/core/MetadataList';
import {NumberInput} from '@astryxdesign/core/NumberInput';
import {ProgressBar} from '@astryxdesign/core/ProgressBar';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {useToast} from '@astryxdesign/core/Toast';
import {useMediaQuery} from '@astryxdesign/core/hooks';
import {useEffect, useRef, useState} from 'react';
import {system} from '../../api/client';
import {useStashEvent} from '../../api/events';
import type {HwAccel, Settings, SystemInfo} from '../../api/types';
import {StatusTag} from '../../components/StatusTag';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {LoadError, LoadingRows, RowsPanel} from '../../components/settings/SectionStates';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {useResource} from '../../components/settings/useResource';
import {useSettingsDraft} from '../../components/settings/useSettingsDraft';
import {errorMessage} from '../../lib/errors';
import {formatBytes, formatCount} from '../../lib/format';
import {SettingsSectionHeader} from './SettingsSectionHeader';

const styles = stylex.create({
  groups: {
    display: 'flex',
    flexDirection: 'column',
  },
  facts: {
    paddingBlock: 8,
    paddingInline: 20,
  },
  disk: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
    maxWidth: 560,
  },
  note: {
    lineHeight: 1.55,
    maxWidth: '64ch',
  },
  path: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: '0.875rem',
    overflowWrap: 'anywhere',
  },
  form: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
    gap: 20,
    maxWidth: 560,
  },
  footer: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 12,
  },
});

const HWACCEL_NAMES: Record<string, string> = {
  none: 'Off (software)',
  vaapi: 'VAAPI (Intel and AMD)',
  qsv: 'Intel Quick Sync',
  nvenc: 'NVIDIA NVENC',
};

function hwaccelName(value: string): string {
  return HWACCEL_NAMES[value] ?? value.toUpperCase();
}

const HEIGHTS = [
  {value: '480', label: '480p'},
  {value: '720', label: '720p'},
  {value: '1080', label: '1080p (Full HD)'},
  {value: '1440', label: '1440p'},
  {value: '2160', label: '2160p (4K)'},
];

const QUALITY = [
  {value: '19', label: 'Higher quality', description: 'Sharper, uses more bandwidth'},
  {value: '23', label: 'Balanced', description: 'Recommended for most homes'},
  {value: '28', label: 'Smaller streams', description: 'For slow connections'},
];

/** Reloads system info shortly after background work changes, at most every few seconds. */
function useJobRefresh(reload: () => void) {
  const timer = useRef<number | null>(null);
  useStashEvent('job', () => {
    if (timer.current !== null) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      reload();
    }, 3000);
  });
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );
}

function useFactLabels() {
  const isNarrow = useMediaQuery('(max-width: 639px)');
  return isNarrow ? ({position: 'top'} as const) : ({position: 'start', width: 150} as const);
}

function ServerGroup({info}: {info: SystemInfo}) {
  const labels = useFactLabels();
  return (
    <SettingsGroup title="This server" description="What is running and whether it is ready.">
      <RowsPanel>
        <div {...stylex.props(styles.facts)}>
          <MetadataList label={labels}>
            <MetadataListItem label="Version">{info.version}</MetadataListItem>
            <MetadataListItem label="Address">
              {info.public_url ?? (
                <Text color="secondary">
                  Not set. Set <Code>STASH_PUBLIC_URL</Code> to the HTTPS address you open PopcornStash at.
                </Text>
              )}
            </MetadataListItem>
            <MetadataListItem label="Passkeys">
              {info.passkeys_ready ? (
                <StatusTag tone="connected">Ready</StatusTag>
              ) : (
                <StatusTag tone="error" wrap>
                  Not ready: needs an HTTPS address in STASH_PUBLIC_URL
                </StatusTag>
              )}
            </MetadataListItem>
            <MetadataListItem label="Torrent engine">
              {info.torrentd.reachable ? (
                <StatusTag tone="connected">Running</StatusTag>
              ) : (
                <StatusTag tone="error" wrap>
                  Not reachable. Torrents are stopped.
                </StatusTag>
              )}
            </MetadataListItem>
            <MetadataListItem label="ffmpeg">
              {info.ffmpeg_version ?? (
                <StatusTag tone="error" wrap>
                  Not found. Videos that need converting will not play.
                </StatusTag>
              )}
            </MetadataListItem>
          </MetadataList>
        </div>
      </RowsPanel>
    </SettingsGroup>
  );
}

function StorageGroup({info}: {info: SystemInfo}) {
  const {total, free} = info.disk;
  const used = Math.max(0, total - free);
  const freeShare = total > 0 ? free / total : 1;
  const isLow = total > 0 && freeShare < 0.1;
  const labels = useFactLabels();
  return (
    <SettingsGroup title="Storage" description="Space on the disk that holds your PopcornStash folder.">
      <div {...stylex.props(styles.disk)}>
        <ProgressBar
          label="Disk space"
          value={used}
          max={total > 0 ? total : 1}
          hasValueLabel
          formatValueLabel={() => `${formatBytes(used)} of ${formatBytes(total)} used`}
          variant={isLow ? 'warning' : 'neutral'}
        />
        <Text as="p" type="supporting" color="secondary" xstyle={styles.note}>
          {isLow
            ? `Only ${formatBytes(free)} free. Downloads stop when the disk is full, so empty the trash or add space.`
            : `${formatBytes(free)} free.`}
        </Text>
      </div>
      <RowsPanel>
        <div {...stylex.props(styles.facts)}>
          <MetadataList label={labels}>
            <MetadataListItem label="Library folder">
              <span {...stylex.props(styles.path)}>{info.stash_root}</span>
            </MetadataListItem>
            <MetadataListItem label="App data">
              <span {...stylex.props(styles.path)}>{info.config_dir}</span>
            </MetadataListItem>
          </MetadataList>
        </div>
      </RowsPanel>
    </SettingsGroup>
  );
}

function JobsGroup({info}: {info: SystemInfo}) {
  const {queued, running, failed} = info.jobs;
  return (
    <SettingsGroup
      title="Background work"
      description="Scanning, sorting, reading video details and making thumbnails. A job that fails is tried up to three times.">
      <RowsPanel>
        <div {...stylex.props(styles.facts)}>
          <MetadataList orientation="horizontal">
            <MetadataListItem label="Waiting">{formatCount(queued, 'job')}</MetadataListItem>
            <MetadataListItem label="Running">{formatCount(running, 'job')}</MetadataListItem>
            <MetadataListItem label="Failed">
              {failed > 0 ? <StatusTag tone="warning">{formatCount(failed, 'job')}</StatusTag> : 'None'}
            </MetadataListItem>
          </MetadataList>
        </div>
      </RowsPanel>
      {failed > 0 ? (
        <Text as="p" type="supporting" color="secondary" xstyle={styles.note}>
          A failed job usually means a file could not be read. The container log names the file.
        </Text>
      ) : null}
    </SettingsGroup>
  );
}

interface TranscodeDraft {
  hwaccel: HwAccel;
  max_height: number;
  crf: number;
  max_sessions: number | null;
}

function pickTranscode(settings: Settings): TranscodeDraft {
  return {...settings.transcode};
}

function PlaybackGroup({info}: {info: SystemInfo | undefined}) {
  const showToast = useToast();
  const form = useSettingsDraft(pickTranscode);
  const [sessionsError, setSessionsError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const draft = form.draft;
  const available = new Set(info?.hwaccel.available ?? []);

  const found = info
    ? ` Hardware acceleration found on this server: ${
        info.hwaccel.available.length > 0 ? info.hwaccel.available.map(hwaccelName).join(', ') : 'none'
      }.`
    : '';
  const description = `Videos a device cannot play directly are converted while you watch.${found}`;

  if (form.isLoading) {
    return (
      <SettingsGroup title="Streaming" description={description}>
        <LoadingRows rows={2} label="Loading streaming settings" />
      </SettingsGroup>
    );
  }
  if (!draft) {
    return (
      <SettingsGroup title="Streaming" description={description}>
        <LoadError title="Streaming settings did not load" error={form.error} onRetry={form.reload} />
      </SettingsGroup>
    );
  }

  const hwOptions = (['none', 'vaapi', 'qsv', 'nvenc'] as const).map(value => ({
    value,
    label: hwaccelName(value),
    description:
      value === 'none'
        ? 'Works everywhere, uses the most CPU'
        : available.has(value)
          ? 'Found on this server'
          : 'Not found on this server',
    disabled: value !== 'none' && info !== undefined && !available.has(value) && value !== draft.hwaccel,
  }));
  const qualityOptions = QUALITY.some(option => Number(option.value) === draft.crf)
    ? QUALITY
    : [
        ...QUALITY,
        {value: String(draft.crf), label: `Custom (CRF ${draft.crf})`, description: 'Set outside this page'},
      ];
  const heightOptions = HEIGHTS.some(option => Number(option.value) === draft.max_height)
    ? HEIGHTS
    : [...HEIGHTS, {value: String(draft.max_height), label: `${draft.max_height}p`}];

  const save = async () => {
    const sessions = draft.max_sessions;
    if (sessions === null || !Number.isInteger(sessions) || sessions < 1 || sessions > 8) {
      setSessionsError('Enter a whole number from 1 to 8.');
      return;
    }
    setSessionsError(null);
    setIsSaving(true);
    try {
      await form.save({transcode: {...draft, max_sessions: sessions}});
      showToast({body: 'Streaming settings saved. New streams use them.'});
    } catch (cause) {
      showToast({type: 'error', body: errorMessage(cause, 'The settings were not saved. Try again.')});
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <SettingsGroup title="Streaming" description={description}>
      <div {...stylex.props(styles.form)}>
        <Selector
          label="Hardware acceleration"
          options={hwOptions}
          value={draft.hwaccel}
          onChange={value => form.edit(current => ({...current, hwaccel: (value ?? 'none') as HwAccel}))}
          width="100%"
        />
        <Selector
          label="Highest resolution"
          options={heightOptions}
          value={String(draft.max_height)}
          onChange={value => form.edit(current => ({...current, max_height: Number(value ?? current.max_height)}))}
          width="100%"
        />
        <Selector
          label="Quality"
          options={qualityOptions}
          value={String(draft.crf)}
          onChange={value => form.edit(current => ({...current, crf: Number(value ?? current.crf)}))}
          width="100%"
        />
        <NumberInput
          label="Streams at once"
          value={draft.max_sessions}
          onChange={value => {
            form.edit(current => ({...current, max_sessions: value}));
            setSessionsError(null);
          }}
          min={1}
          max={8}
          isIntegerOnly
          hasNumberSteppers
          status={sessionsError ? {type: 'error', message: sessionsError} : undefined}
          statusVariant="detached"
          width="100%"
        />
      </div>
      <div {...stylex.props(styles.footer)}>
        <Button
          label="Save changes"
          variant={form.isDirty ? 'primary' : 'secondary'}
          onClick={() => void save()}
          isLoading={isSaving}
          isDisabled={!form.isDirty}
        />
        {form.isDirty ? <Button label="Discard" variant="ghost" onClick={form.reset} /> : null}
      </div>
    </SettingsGroup>
  );
}

function SystemSettings() {
  const info = useResource(signal => system.get(signal), {key: 'system'});
  useJobRefresh(info.reload);

  return (
    <>
      <SettingsSectionHeader title="System" subtitle="Storage, version and server health." />
      {info.isLoading ? (
        <div {...stylex.props(styles.groups)}>
          <SettingsGroup title="This server" description="What is running and whether it is ready.">
            <LoadingRows rows={4} label="Loading system information" />
          </SettingsGroup>
        </div>
      ) : info.data ? (
        <div {...stylex.props(styles.groups)}>
          <ServerGroup info={info.data} />
          <StorageGroup info={info.data} />
          <JobsGroup info={info.data} />
          <PlaybackGroup info={info.data} />
        </div>
      ) : (
        <div {...stylex.props(styles.groups)}>
          <SettingsGroup title="This server" description="What is running and whether it is ready.">
            <LoadError title="System information did not load" error={info.error} onRetry={info.reload} />
          </SettingsGroup>
          <PlaybackGroup info={undefined} />
        </div>
      )}
    </>
  );
}

export default function SystemSettingsPage() {
  return (
    <OwnerOnly>
      <SystemSettings />
    </OwnerOnly>
  );
}

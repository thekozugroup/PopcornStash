/**
 * Torrent and web download settings on the VPN page: seeding, how many
 * torrents run at once, speed limits, and whether web links use the VPN.
 * Saved together with one partial settings update; invalid values are
 * explained next to the field.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {NumberInput} from '@astryxdesign/core/NumberInput';
import {RadioList, RadioListItem} from '@astryxdesign/core/RadioList';
import {Switch} from '@astryxdesign/core/Switch';
import {Text} from '@astryxdesign/core/Text';
import {useToast} from '@astryxdesign/core/Toast';
import {useState} from 'react';
import type {HttpViaVpn, Settings} from '../../../api/types';
import {SettingsGroup} from '../../../components/settings/SettingsGroup';
import {LoadError, LoadingRows} from '../../../components/settings/SectionStates';
import {useSettingsDraft} from '../../../components/settings/useSettingsDraft';
import {errorMessage} from '../../../lib/errors';

const styles = stylex.create({
  pair: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
    gap: 16,
    maxWidth: 560,
  },
  seedFields: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    paddingInlineStart: {default: 52, '@media (max-width: 520px)': 0},
  },
  field: {
    maxWidth: 272,
  },
  footer: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 12,
    paddingTop: 4,
  },
});

interface TorrentDraft {
  seed_after_complete: boolean;
  seed_ratio: number | null;
  seed_hours: number | null;
  max_active: number | null;
  down_limit_kbps: number | null;
  up_limit_kbps: number | null;
  http_via_vpn: HttpViaVpn;
}

type Errors = Partial<Record<keyof TorrentDraft, string>>;

function pick(settings: Settings): TorrentDraft {
  return {...settings.torrent, http_via_vpn: settings.http_via_vpn};
}

const LIMIT_MAX = 1_000_000;
/** The torrent engine moves 16 KB blocks; a lower limit would stop transfers instead of slowing them. */
const LIMIT_MIN = 16;

function validate(draft: TorrentDraft): Errors {
  const errors: Errors = {};
  if (draft.seed_after_complete) {
    if (draft.seed_ratio === null || draft.seed_ratio < 0 || draft.seed_ratio > 100)
      errors.seed_ratio = 'Enter a ratio from 0 to 100.';
    if (
      draft.seed_hours === null ||
      draft.seed_hours < 1 ||
      draft.seed_hours > 720 ||
      !Number.isInteger(draft.seed_hours)
    )
      errors.seed_hours = 'Enter a whole number of hours from 1 to 720.';
  }
  if (draft.max_active === null || draft.max_active < 1 || draft.max_active > 50 || !Number.isInteger(draft.max_active))
    errors.max_active = 'Enter a whole number from 1 to 50.';
  for (const key of ['down_limit_kbps', 'up_limit_kbps'] as const) {
    const value = draft[key];
    if (value === null || value < 0 || value > LIMIT_MAX || !Number.isInteger(value))
      errors[key] = 'Enter a whole number. 0 means no limit.';
    else if (value > 0 && value < LIMIT_MIN) errors[key] = `Enter 0 for no limit, or ${LIMIT_MIN} KB/s or more.`;
  }
  return errors;
}

function errorStatus(message: string | undefined) {
  return message ? {type: 'error' as const, message} : undefined;
}

export interface TorrentSettingsProps {
  isPrimary: boolean;
  /** STASH_ALLOW_NO_VPN is on: there is no VPN for torrents or web downloads to use. */
  isNoVpn?: boolean;
}

export function TorrentSettings({isPrimary, isNoVpn = false}: TorrentSettingsProps) {
  const showToast = useToast();
  const form = useSettingsDraft(pick);
  const [errors, setErrors] = useState<Errors>({});
  const [isSaving, setIsSaving] = useState(false);
  const {draft} = form;

  if (form.isLoading || (!draft && !form.error)) {
    return (
      <SettingsGroup title="Torrents" description="Seeding, how many run at once, and speed limits.">
        <LoadingRows rows={3} label="Loading torrent settings" />
      </SettingsGroup>
    );
  }
  if (!draft) {
    return (
      <SettingsGroup title="Torrents" description="Seeding, how many run at once, and speed limits.">
        <LoadError title="Torrent settings did not load" error={form.error} onRetry={form.reload} />
      </SettingsGroup>
    );
  }

  const set = <K extends keyof TorrentDraft>(key: K, value: TorrentDraft[K]) => {
    form.edit(current => ({...current, [key]: value}));
    if (errors[key]) setErrors(current => ({...current, [key]: undefined}));
  };

  const save = async () => {
    const found = validate(draft);
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    setIsSaving(true);
    try {
      await form.save({
        http_via_vpn: draft.http_via_vpn,
        torrent: {
          seed_after_complete: draft.seed_after_complete,
          seed_ratio: draft.seed_ratio ?? undefined,
          seed_hours: draft.seed_hours ?? undefined,
          max_active: draft.max_active ?? undefined,
          down_limit_kbps: draft.down_limit_kbps ?? undefined,
          up_limit_kbps: draft.up_limit_kbps ?? undefined,
        },
      });
      showToast({body: 'Torrent settings saved.'});
    } catch (cause) {
      showToast({type: 'error', body: errorMessage(cause, 'The settings were not saved. Try again.')});
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <>
      <SettingsGroup
        title="Torrents"
        description="Finished torrents move into your library. Seeding keeps sharing them for a while first.">
        <Switch
          label="Keep seeding after a download finishes"
          description="Seeding stops at the ratio or the time limit, whichever comes first."
          value={draft.seed_after_complete}
          onChange={value => set('seed_after_complete', value)}
        />
        {draft.seed_after_complete ? (
          <div {...stylex.props(styles.seedFields)}>
            <div {...stylex.props(styles.pair)}>
              <NumberInput
                label="Seed ratio"
                description="Uploaded divided by the torrent’s size."
                value={draft.seed_ratio}
                onChange={value => set('seed_ratio', value)}
                min={0}
                max={100}
                step={0.1}
                status={errorStatus(errors.seed_ratio)}
                statusVariant="detached"
                width="100%"
              />
              <NumberInput
                label="Seed time"
                description="At most this long."
                value={draft.seed_hours}
                onChange={value => set('seed_hours', value)}
                min={1}
                max={720}
                isIntegerOnly
                units="hours"
                status={errorStatus(errors.seed_hours)}
                statusVariant="detached"
                width="100%"
              />
            </div>
          </div>
        ) : null}
        <div {...stylex.props(styles.field)}>
          <NumberInput
            label="Torrents at once"
            description="Others wait in line. Seeding and paused torrents do not count."
            value={draft.max_active}
            onChange={value => set('max_active', value)}
            min={1}
            max={50}
            isIntegerOnly
            status={errorStatus(errors.max_active)}
            statusVariant="detached"
            width="100%"
          />
        </div>
        <div {...stylex.props(styles.pair)}>
          <NumberInput
            label="Download limit"
            description={`0 means no limit. The lowest limit is ${LIMIT_MIN} KB/s.`}
            value={draft.down_limit_kbps}
            onChange={value => set('down_limit_kbps', value)}
            min={0}
            max={LIMIT_MAX}
            step={100}
            isIntegerOnly
            units="KB/s"
            status={errorStatus(errors.down_limit_kbps)}
            statusVariant="detached"
            width="100%"
          />
          <NumberInput
            label="Upload limit"
            description={`0 means no limit. The lowest limit is ${LIMIT_MIN} KB/s.`}
            value={draft.up_limit_kbps}
            onChange={value => set('up_limit_kbps', value)}
            min={0}
            max={LIMIT_MAX}
            step={100}
            isIntegerOnly
            units="KB/s"
            status={errorStatus(errors.up_limit_kbps)}
            statusVariant="detached"
            width="100%"
          />
        </div>
      </SettingsGroup>

      <SettingsGroup
        title="Web downloads"
        description={
          isNoVpn
            ? 'Links you add from a browser or the Chrome extension. There is no VPN on this server, so they use this network.'
            : 'Links you add from a browser or the Chrome extension. Torrents always use the VPN.'
        }>
        <RadioList
          label="Web downloads use the VPN"
          isDisabled={isNoVpn}
          disabledMessage={isNoVpn ? 'Applies once the server uses a VPN.' : undefined}
          value={draft.http_via_vpn}
          onChange={value => set('http_via_vpn', value as HttpViaVpn)}>
          <RadioListItem
            value="when_available"
            label="When it is connected"
            description="Use the VPN when it is up, and your home connection when it is not."
          />
          <RadioListItem
            value="always"
            label="Always"
            description="Web downloads only run through the VPN, and fail while it is down."
          />
          <RadioListItem value="never" label="Never" description="Web downloads use your home connection." />
        </RadioList>
        <div {...stylex.props(styles.footer)}>
          {/* Blue only when there is something to save: a disabled primary reads as broken. */}
          <Button
            label="Save changes"
            variant={isPrimary && form.isDirty ? 'primary' : 'secondary'}
            onClick={() => void save()}
            isLoading={isSaving}
            isDisabled={!form.isDirty}
          />
          {form.isDirty ? (
            <>
              <Button
                label="Discard"
                variant="ghost"
                onClick={() => {
                  form.reset();
                  setErrors({});
                }}
              />
              <Text type="supporting" color="secondary">
                Unsaved changes
              </Text>
            </>
          ) : null}
        </div>
      </SettingsGroup>
    </>
  );
}

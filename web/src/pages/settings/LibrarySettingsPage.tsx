/**
 * Settings, Library (owner): auto-sort, how the folder is laid out (with the
 * real folder from system info), a rescan, and the trash with restore,
 * retention and Empty trash.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Code} from '@astryxdesign/core/Code';
import {ListItem} from '@astryxdesign/core/List';
import {MetadataList, MetadataListItem} from '@astryxdesign/core/MetadataList';
import {NumberInput} from '@astryxdesign/core/NumberInput';
import {Switch} from '@astryxdesign/core/Switch';
import {Text} from '@astryxdesign/core/Text';
import {Thumbnail} from '@astryxdesign/core/Thumbnail';
import {useToast} from '@astryxdesign/core/Toast';
import {useMediaQuery} from '@astryxdesign/core/hooks';
import {useState} from 'react';
import {library, system, trash} from '../../api/client';
import {useStashEvent} from '../../api/events';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {EmptyRows, LoadError, LoadingRows, RowsPanel, PanelList} from '../../components/settings/SectionStates';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {listTrash, type TrashRow} from '../../components/settings/settingsApi';
import {useConfirm} from '../../components/settings/useConfirm';
import {useResource} from '../../components/settings/useResource';
import {useSettingsDraft} from '../../components/settings/useSettingsDraft';
import {errorMessage} from '../../lib/errors';
import {formatCount, formatRelative} from '../../lib/format';
import {SettingsSectionHeader} from './SettingsSectionHeader';

const styles = stylex.create({
  groups: {
    display: 'flex',
    flexDirection: 'column',
  },
  root: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  path: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: '0.875rem',
    color: colorVars['--color-text-primary'],
    overflowWrap: 'anywhere',
  },
  example: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: '0.8125rem',
    color: colorVars['--color-text-secondary'],
    overflowWrap: 'anywhere',
  },
  folderValue: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    minWidth: 0,
  },
  note: {
    lineHeight: 1.55,
    maxWidth: '64ch',
  },
  facts: {
    paddingBlock: 12,
    paddingInline: 20,
  },
  trashMeta: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    minWidth: 0,
  },
  inline: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 12,
  },
  retention: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-end',
    gap: 12,
  },
  thumb: {
    width: {default: 64, '@media (max-width: 639px)': 48},
    height: {default: 40, '@media (max-width: 639px)': 32},
    borderRadius: 6,
    flexShrink: 0,
  },
  thumbEmpty: {
    width: {default: 64, '@media (max-width: 639px)': 48},
    height: {default: 40, '@media (max-width: 639px)': 32},
    borderRadius: 6,
    flexShrink: 0,
    backgroundColor: colorVars['--color-background-muted'],
  },
});

const FOLDERS: {name: string; what: string; example: string}[] = [
  {name: 'Movies', what: 'One folder per film.', example: 'Movies/Harbor Lights (2019)/Harbor Lights (2019).mkv'},
  {name: 'Shows', what: 'By show and season.', example: 'Shows/Garden Tales/Season 02/Garden Tales - S02E04.mkv'},
  {name: 'Home Videos', what: 'By the month they were filmed.', example: 'Home Videos/2025/07/IMG_4410.MOV'},
  {name: 'Photos', what: 'By the month they were taken.', example: 'Photos/2025/06/IMG_20250614_183022.jpg'},
  {name: 'Other', what: 'Anything that is not one of the above.', example: 'Other/recipe-cards.pdf'},
  {name: 'Inbox', what: 'Copy files here and they are sorted for you.', example: 'Inbox/'},
  {
    name: 'Downloads',
    what: 'In Progress while downloading, then Completed until sorted.',
    example: 'Downloads/Completed/',
  },
];

/** The folder part of a relative path ("Home Videos/2025/07"). */
function folderOf(path: string): string {
  const parts = path.split('/').filter(Boolean);
  return parts.length > 1 ? parts.slice(0, -1).join('/') : (parts[0] ?? path);
}

function SortingGroup() {
  const showToast = useToast();
  const form = useSettingsDraft(settings => settings.auto_sort);
  const value = form.draft;
  return (
    <SettingsGroup
      title="Sorting"
      description="New files from Downloads and Inbox go into the right folder on their own.">
      {form.isLoading ? (
        <LoadingRows rows={1} label="Loading sorting settings" />
      ) : value === null ? (
        <LoadError title="Sorting settings did not load" error={form.error} onRetry={form.reload} />
      ) : (
        <Switch
          label="Sort new files automatically"
          description="Files that clearly are a movie, an episode, a home video or a photo move straight into the library. Anything uncertain waits in Inbox for you to check."
          value={value}
          changeAction={async next => {
            try {
              await form.save({auto_sort: next});
              showToast({body: next ? 'Auto-sort is on.' : 'Auto-sort is off. New files wait in Inbox.'});
            } catch (cause) {
              showToast({type: 'error', body: errorMessage(cause, 'The setting was not saved. Try again.')});
            }
          }}
        />
      )}
    </SettingsGroup>
  );
}

function FoldersGroup() {
  const isNarrow = useMediaQuery('(max-width: 639px)');
  const info = useResource(signal => system.get(signal), {key: 'system'});
  const showToast = useToast();
  const [scan, setScan] = useState<'idle' | 'starting' | 'running' | 'done'>('idle');

  useStashEvent('job', event => {
    if (event.kind !== 'scan') return;
    if (event.state === 'running' || event.state === 'queued') setScan('running');
    else if (event.state === 'done' || event.state === 'failed')
      setScan(current => (current === 'idle' ? current : 'done'));
  });

  const rescan = async () => {
    setScan('starting');
    try {
      await library.rescan();
      setScan('running');
    } catch (cause) {
      setScan('idle');
      showToast({type: 'error', body: errorMessage(cause, 'The scan did not start. Try again.')});
    }
  };

  const root = info.data?.stash_root ?? null;

  return (
    <>
      <SettingsGroup
        title="Folder layout"
        description="Everything lives in one folder. PopcornStash only moves files that arrive through Inbox or Downloads. Files you put in the other folders yourself stay exactly where they are.">
        <div {...stylex.props(styles.root)}>
          <Text type="label">Your PopcornStash folder</Text>
          {info.isLoading ? (
            <Text type="supporting" color="secondary">
              Looking it up
            </Text>
          ) : root ? (
            <span {...stylex.props(styles.path)}>{root}</span>
          ) : (
            <Text type="supporting" color="secondary">
              The folder set by <Code>STASH_ROOT</Code> in the container (<Code>/stash</Code> unless you changed it).
            </Text>
          )}
        </div>
        <RowsPanel>
          <div {...stylex.props(styles.facts)}>
            <MetadataList label={isNarrow ? {position: 'top'} : {position: 'start', width: 132}}>
              {FOLDERS.map(folder => (
                <MetadataListItem key={folder.name} label={folder.name}>
                  <span {...stylex.props(styles.folderValue)}>
                    <Text>{folder.what}</Text>
                    <span {...stylex.props(styles.example)}>{folder.example}</span>
                  </span>
                </MetadataListItem>
              ))}
            </MetadataList>
          </div>
        </RowsPanel>
      </SettingsGroup>

      <SettingsGroup
        title="Scan the folder"
        description="PopcornStash notices new files on its own and checks everything every 30 minutes. Scan now if something you copied in is missing.">
        <div {...stylex.props(styles.inline)}>
          <Button
            label={scan === 'running' || scan === 'starting' ? 'Scanning' : 'Scan now'}
            variant="secondary"
            onClick={() => void rescan()}
            isLoading={scan === 'starting'}
            isDisabled={scan === 'running'}
          />
          <span aria-live="polite">
            <Text type="supporting" color="secondary">
              {scan === 'running'
                ? 'Looking for new and moved files. They appear in the library as they are found.'
                : scan === 'done'
                  ? 'Scan finished.'
                  : ''}
            </Text>
          </span>
        </div>
      </SettingsGroup>
    </>
  );
}

function TrashRowItem({row, onRestore}: {row: TrashRow; onRestore: (row: TrashRow) => void}) {
  return (
    <ListItem
      label={row.title}
      startContent={
        row.thumb ? (
          <Thumbnail src={row.thumb} alt="" xstyle={styles.thumb} />
        ) : (
          <span aria-hidden="true" {...stylex.props(styles.thumbEmpty)} />
        )
      }
      description={
        <span {...stylex.props(styles.trashMeta)}>
          <span>Moved to trash {formatRelative(row.trashedAt).toLowerCase()}</span>
          <span {...stylex.props(styles.example)}>From {folderOf(row.originalPath)}</span>
        </span>
      }
      endContent={<Button label="Restore" variant="secondary" clickAction={async () => onRestore(row)} />}
    />
  );
}

function TrashGroup() {
  const showToast = useToast();
  const items = useResource(signal => listTrash(signal), {key: 'trash'});
  const form = useSettingsDraft(settings => settings.trash_retention_days);
  const [retentionError, setRetentionError] = useState<string | null>(null);
  const {ask, dialog} = useConfirm();
  const rows = items.data ?? [];

  const restore = async (row: TrashRow) => {
    try {
      await trash.restore(row.id);
      items.mutate(list => list.filter(item => item.id !== row.id));
      showToast({body: `${row.title} is back in ${folderOf(row.originalPath)}.`});
    } catch (cause) {
      showToast({type: 'error', body: errorMessage(cause, 'It was not restored. Try again.')});
    }
  };

  const emptyTrash = () =>
    ask({
      title: `Delete ${formatCount(rows.length, 'item')} for good?`,
      description: 'The files are deleted from the disk and cannot be restored.',
      actionLabel: 'Empty trash',
      failure: 'The trash was not emptied. Try again.',
      run: async () => {
        await trash.empty();
        items.mutate(() => []);
        showToast({body: 'Trash emptied.'});
      },
    });

  // NumberInput commits on blur or Enter. No steppers or wheel here, so one
  // edit is one save (and one toast), not one per click.
  const saveRetention = async (days: number | null) => {
    if (days === null || !Number.isInteger(days) || days < 1 || days > 365) {
      setRetentionError('Enter a whole number of days from 1 to 365.');
      return;
    }
    setRetentionError(null);
    if (days === form.saved?.trash_retention_days) return;
    form.edit(() => days);
    try {
      await form.save({trash_retention_days: days});
      showToast({body: `Trash now keeps items for ${formatCount(days, 'day')}.`});
    } catch (cause) {
      form.reset();
      showToast({type: 'error', body: errorMessage(cause, 'The setting was not saved. Try again.')});
    }
  };

  return (
    <SettingsGroup
      title="Trash"
      description={
        form.saved
          ? `Deleted items wait here for ${formatCount(form.saved.trash_retention_days, 'day')}, then are removed from the disk for good.`
          : 'Deleted items wait here before they are removed from the disk for good.'
      }
      actions={rows.length > 0 ? <Button label="Empty trash" variant="secondary" onClick={emptyTrash} /> : undefined}>
      {items.isLoading ? (
        <LoadingRows rows={3} label="Loading the trash" />
      ) : items.error !== undefined && !items.data ? (
        <LoadError title="The trash did not load" error={items.error} onRetry={items.reload} />
      ) : rows.length === 0 ? (
        <EmptyRows>The trash is empty. Items you delete from the library wait here first.</EmptyRows>
      ) : (
        <PanelList density="spacious">
          {rows.map(row => (
            <TrashRowItem key={row.id} row={row} onRestore={restore} />
          ))}
        </PanelList>
      )}
      <div {...stylex.props(styles.retention)}>
        <NumberInput
          label="Keep items in the trash for"
          value={form.draft}
          onChange={value => void saveRetention(value)}
          min={1}
          max={365}
          isIntegerOnly
          isWheelEnabled={false}
          units="days"
          isDisabled={form.draft === null}
          status={retentionError ? {type: 'error', message: retentionError} : undefined}
          statusVariant="detached"
          width={240}
        />
      </div>
      {dialog}
    </SettingsGroup>
  );
}

function LibrarySettings() {
  return (
    <>
      <SettingsSectionHeader title="Library" subtitle="Sorting, the folder layout and the trash." />
      <div {...stylex.props(styles.groups)}>
        <SortingGroup />
        <FoldersGroup />
        <TrashGroup />
      </div>
    </>
  );
}

export default function LibrarySettingsPage() {
  return (
    <OwnerOnly>
      <LibrarySettings />
    </OwnerOnly>
  );
}

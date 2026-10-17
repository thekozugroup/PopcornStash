/**
 * Settings, Libraries (owner): the libraries in the sidebar's order (open
 * one to edit it; move it up or down, or delete it, from its More menu),
 * the top-level folders that are not a library yet with "Make a library",
 * then sorting, the folder layout, a rescan and the trash.
 *
 * Folder removal is explicit, optional and recoverable in Trash.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Code} from '@astryxdesign/core/Code';
import {ListItem} from '@astryxdesign/core/List';
import {MetadataList, MetadataListItem} from '@astryxdesign/core/MetadataList';
import {MoreMenu} from '@astryxdesign/core/MoreMenu';
import {NumberInput} from '@astryxdesign/core/NumberInput';
import {Switch} from '@astryxdesign/core/Switch';
import {Text} from '@astryxdesign/core/Text';
import {Thumbnail} from '@astryxdesign/core/Thumbnail';
import {useToast} from '@astryxdesign/core/Toast';
import {useMediaQuery} from '@astryxdesign/core/hooks';
import {Folder} from 'lucide-react';
import {useEffect, useRef, useState} from 'react';
import {files as filesApi, libraries as librariesApi, library, system, trash, users as usersApi} from '../../api/client';
import {useStashEvent} from '../../api/events';
import type {Library, UnassignedFolder, User} from '../../api/types';
import {useLibraries} from '../../app/LibrariesProvider';
import {BreakableAddress} from '../../components/BreakableName';
import {DeleteLibraryDialog} from '../../components/settings/DeleteLibraryDialog';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {EmptyRows, LoadError, LoadingRows, PanelList, RowsPanel} from '../../components/settings/SectionStates';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {listTrash, type TrashRow} from '../../components/settings/settingsApi';
import {useConfirm} from '../../components/settings/useConfirm';
import {useResource} from '../../components/settings/useResource';
import {useSettingsDraft} from '../../components/settings/useSettingsDraft';
import {StatusTag} from '../../components/StatusTag';
import {errorMessage} from '../../lib/errors';
import {formatCount, formatRelative} from '../../lib/format';
import {kindLine, takesLine, whoSeesLine} from '../../lib/libraries';
import {LibraryGlyph} from '../../lib/libraryIcons';
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
    overflowWrap: 'break-word',
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
  refusal: {
    overflowWrap: 'anywhere',
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
    borderRadius: 'var(--popcorn-radius-tag)',
    flexShrink: 0,
  },
  thumbEmpty: {
    width: {default: 64, '@media (max-width: 639px)': 48},
    height: {default: 40, '@media (max-width: 639px)': 32},
    borderRadius: 'var(--popcorn-radius-tag)',
    flexShrink: 0,
    backgroundColor: colorVars['--color-background-muted'],
  },
  // A library's icon at 20px on a 40px quiet square, as in the sidebar's family.
  iconSquare: {
    display: 'grid',
    placeItems: 'center',
    width: 40,
    height: 40,
    flexShrink: 0,
    borderRadius: 'var(--radius-element)',
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-icon-primary'],
  },
  // Facts spaced 16px apart, wrapping on narrow screens.
  strip: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    columnGap: 16,
    rowGap: 4,
    minWidth: 0,
  },
  // A folder's name is shown whole, on phones and between 1100 and 1200px too, where
  // a tooltip would never be seen: a short one ("Grandma's Slides/") fits its line, and a
  // long one wraps at its spaces and separators instead of being cut off.
  folderName: {
    display: 'block',
    minWidth: 0,
    overflowWrap: 'break-word',
    fontFamily: typographyVars['--font-family-code'],
    fontSize: '0.875rem',
  },
  // On phones the button sits under the name and counts: the icon stays with the name
  // at the top of the row instead of centring on all three lines.
  folderRowTop: {
    alignItems: 'flex-start',
  },
  folderText: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'flex-start',
    gap: 12,
    minWidth: 0,
  },
});

/** How a sorted file is laid out inside any library's folder, and the fixed places. */
const LAYOUT: {name: string; what: string; example: string}[] = [
  {name: 'Movies', what: 'One folder per film, in the library’s folder.', example: 'Harbor Lights (2019)/Harbor Lights (2019).mkv'},
  {name: 'TV episodes', what: 'By show and season.', example: 'Garden Tales/Season 02/Garden Tales - S02E04.mkv'},
  {name: 'Camera videos', what: 'By the month they were filmed.', example: '2025/07/IMG_4410.MOV'},
  {name: 'Photos', what: 'By the month they were taken.', example: '2025/06/IMG_20250614_183022.jpg'},
  {name: 'Other', what: 'Music, documents and anything else.', example: 'Other/recipe-cards.pdf'},
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

function editorHref(id: string): string {
  return `/settings/libraries/${encodeURIComponent(id)}`;
}

/** "124 videos  12 photos  3 other files" for a folder that is not a library. */
export function folderCounts(folder: UnassignedFolder): string[] {
  const counts = [
    folder.video_count ? formatCount(folder.video_count, 'video') : null,
    folder.photo_count ? formatCount(folder.photo_count, 'photo') : null,
    folder.other_count ? formatCount(folder.other_count, 'other file') : null,
  ].filter((entry): entry is string => Boolean(entry));
  return counts.length > 0 ? counts : ['Nothing indexed yet'];
}

function LibraryRow({
  library,
  people,
  index,
  total,
  onMove,
  onDelete,
}: {
  library: Library;
  people: User[];
  index: number;
  total: number;
  onMove: (library: Library, step: -1 | 1) => void;
  onDelete: (library: Library) => void;
}) {
  const folderProblem =
    library.folder_state === 'missing' ? 'Folder missing' : library.folder_state === 'read_only' ? 'Folder read-only' : null;
  return (
    <ListItem
      label={library.name}
      href={editorHref(library.id)}
      startContent={
        <span aria-hidden="true" {...stylex.props(styles.iconSquare)}>
          <LibraryGlyph name={library.icon} size={20} />
        </span>
      }
      description={
        <span {...stylex.props(styles.strip)}>
          <span>{kindLine(library)}</span>
          <span>{takesLine(library.claims)}</span>
          <span>{whoSeesLine(library.visibility, library.member_ids, people)}</span>
          {folderProblem ? <StatusTag tone="warning">{folderProblem}</StatusTag> : null}
        </span>
      }
      endContent={
        <span data-library-more={library.id}>
          <MoreMenu
            label={`More for ${library.name}`}
            alignment="end"
            items={[
              {label: 'Move up', isDisabled: index === 0, onClick: () => onMove(library, -1)},
              {label: 'Move down', isDisabled: index === total - 1, onClick: () => onMove(library, 1)},
              {type: 'divider'},
              {label: 'Delete…', variant: 'destructive', onClick: () => onDelete(library)},
            ]}
          />
        </span>
      }
    />
  );
}

function LibrariesGroup({people}: {people: User[]}) {
  const showToast = useToast();
  const {libraries, status, error, reload} = useLibraries();
  const [deleting, setDeleting] = useState<Library | null>(null);
  const [order, setOrder] = useState<Library[] | null>(null);
  /** The row whose More button keeps focus once the moved list has re-rendered. */
  const focusId = useRef<string | null>(null);
  const list = order ?? libraries;

  useEffect(() => {
    const id = focusId.current;
    if (!id) return;
    document.querySelector<HTMLElement>(`[data-library-more="${CSS.escape(id)}"] button`)?.focus();
  }, [list]);

  const move = async (target: Library, step: -1 | 1) => {
    const ids = (list ?? []).map(entry => entry.id);
    const from = ids.indexOf(target.id);
    const to = from + step;
    if (from < 0 || to < 0 || to >= ids.length) return;
    ids.splice(from, 1);
    ids.splice(to, 0, target.id);
    const optimistic = ids
      .map(id => list?.find(entry => entry.id === id))
      .filter((entry): entry is Library => Boolean(entry));
    focusId.current = target.id;
    setOrder(optimistic);
    try {
      const result = await librariesApi.reorder(ids);
      setOrder(result.libraries);
      await reload();
      setOrder(null);
    } catch (cause) {
      setOrder(null);
      showToast({type: 'error', body: errorMessage(cause, 'The order was not saved. Try again.')});
    }
  };


  return (
    <SettingsGroup
      title="Libraries"
      description="What the sidebar and Home show, in this order."
      actions={<Button label="Make a library" variant="primary" href="/settings/libraries/new" />}>
      {status === 'loading' ? (
        <LoadingRows rows={4} label="Loading libraries" />
      ) : status === 'error' && !libraries ? (
        <LoadError title="Libraries did not load" error={error} onRetry={() => void reload()} />
      ) : !list || list.length === 0 ? (
        <EmptyRows>No libraries yet. Make one to give the family something to browse.</EmptyRows>
      ) : (
        <PanelList density="spacious">
          {list.map((entry, index) => (
            <LibraryRow
              key={entry.id}
              library={entry}
              people={people}
              index={index}
              total={list.length}
              onMove={(target, step) => void move(target, step)}
              onDelete={setDeleting}
            />
          ))}
        </PanelList>
      )}
      {deleting ? <DeleteLibraryDialog library={deleting} onClose={() => setDeleting(null)} onDeleted={removed => {
        const name = deleting.name; setDeleting(null); void reload();
        showToast({body: removed ? `${name} and its folder moved to Trash.` : `${name} is no longer in the app. Its files stay where they are.`});
      }} /> : null}
    </SettingsGroup>
  );
}

function UnassignedFoldersGroup() {
  const {libraries} = useLibraries();
  // On a phone the button goes under the name, so the name keeps the row's whole width.
  const isNarrow = useMediaQuery('(max-width: 639px)');
  const folders = useResource(signal => librariesApi.folders(signal), {key: `folders:${libraries?.length ?? 0}`});
  const rows = folders.data?.folders ?? [];
  if (folders.isLoading || rows.length === 0) return null;
  return (
    <SettingsGroup
      title="Folders not in a library"
      description="Only owners see what is in them. Make one a library to share it.">
      <PanelList density="spacious">
        {rows.map(folder => {
          const make = (
            <Button
              label="Make a library"
              variant="secondary"
              href={`/settings/libraries/new?folder=${encodeURIComponent(folder.name)}`}
              aria-label={`Make a library from ${folder.name}`}
            />
          );
          const counts = (
            <span {...stylex.props(styles.strip)}>
              {folderCounts(folder).map(count => (
                <span key={count}>{count}</span>
              ))}
            </span>
          );
          return (
            <ListItem
              key={folder.name}
              label={
                <span {...stylex.props(styles.folderName)}>
                  <BreakableAddress address={`${folder.name}/`} />
                </span>
              }
              xstyle={isNarrow ? styles.folderRowTop : undefined}
              startContent={
                <span aria-hidden="true" {...stylex.props(styles.iconSquare)}>
                  <Folder size={20} strokeWidth={1.5} />
                </span>
              }
              description={
                isNarrow ? (
                  <span {...stylex.props(styles.folderText)}>
                    {counts}
                    {make}
                  </span>
                ) : (
                  counts
                )
              }
              endContent={isNarrow ? undefined : make}
            />
          );
        })}
      </PanelList>
    </SettingsGroup>
  );
}

function SortingGroup() {
  const showToast = useToast();
  const form = useSettingsDraft(settings => settings.auto_sort);
  const value = form.draft;
  return (
    <SettingsGroup
      title="Sorting"
      description="New files from Downloads and Inbox go to the library that takes their kind.">
      {form.isLoading ? (
        <LoadingRows rows={1} label="Loading sorting settings" />
      ) : value === null ? (
        <LoadError title="Sorting settings did not load" error={form.error} onRetry={form.reload} />
      ) : (
        <Switch
          label="Sort new files automatically"
          description="Files that clearly are a movie, an episode, a camera video or a photo move straight into the library that takes them. Anything uncertain, or a kind no library takes, waits in Inbox for you to check."
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
        description="Everything lives in one folder, with a folder for each library. A sorted file is laid out by what it is. PopcornStash only moves files that arrive through Inbox or Downloads; files you put anywhere else stay exactly where they are.">
        <div {...stylex.props(styles.root)}>
          <Text type="label">Your PopcornStash folder</Text>
          {info.isLoading ? (
            <Text type="supporting" color="secondary">
              Looking it up
            </Text>
          ) : root ? (
            <span {...stylex.props(styles.path)}>
              <BreakableAddress address={root} />
            </span>
          ) : (
            <Text type="supporting" color="secondary">
              The folder set by <Code>STASH_ROOT</Code> in the container (<Code>/stash</Code> unless you changed it).
            </Text>
          )}
        </div>
        <RowsPanel>
          <div {...stylex.props(styles.facts)}>
            <MetadataList label={isNarrow ? {position: 'top'} : {position: 'start', width: 132}}>
              {LAYOUT.map(folder => (
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
                ? 'Looking for new and moved files. They appear in their libraries as they are found.'
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
          <span {...stylex.props(styles.example)}>From {row.originalPath}</span>
          {row.detail ? <Text type="supporting">{row.detail}</Text> : null}
          {/* Its library would no longer show it: no Restore, and the server says why. */}
          {row.restoreRefusal ? <span {...stylex.props(styles.refusal)}>{row.restoreRefusal}</span> : null}
        </span>
      }
      endContent={
        row.restoreRefusal ? undefined : (
          <Button label="Restore" variant="secondary" clickAction={async () => onRestore(row)} />
        )
      }
    />
  );
}

function TrashGroup() {
  const showToast = useToast();
  const items = useResource(signal => listTrash(signal), {key: 'trash'});
  const form = useSettingsDraft(settings => settings.trash_retention_days);
  const [retentionError, setRetentionError] = useState<string | null>(null);
  const {ask, dialog} = useConfirm();
  const fileTrash = useResource(signal => filesApi.trash(signal), {key: 'file-trash'});
  const extraRows: TrashRow[] = (fileTrash.data ?? []).map(entry => ({
    id: entry.id, title: entry.original_rel_path.split('/').at(-1) ?? entry.original_rel_path,
    kind: null, category: null, thumb: null, originalPath: entry.original_rel_path,
    trashedAt: entry.trashed_at, restoreRefusal: null, fileEntry: true,
    detail: entry.is_dir ? `Folder with ${formatCount(entry.file_count, 'file')}. Restoring keeps it private to owners until made a library.` : 'Selected file only; companion files stayed in place.',
  }));
  const rows = [...(items.data ?? []), ...extraRows].sort((a, b) => b.trashedAt - a.trashedAt);

  const restore = async (row: TrashRow) => {
    try {
      if (row.fileEntry) {
        await filesApi.restore(row.id);
        fileTrash.mutate(list => list.filter(item => item.id !== row.id));
      } else {
        await trash.restore(row.id);
        items.mutate(list => list.filter(item => item.id !== row.id));
      }
      showToast({body: `${row.title} is back in ${folderOf(row.originalPath)}.`});
    } catch (cause) {
      showToast({type: 'error', body: errorMessage(cause, 'It was not restored. Try again.')});
      items.reload();
      fileTrash.reload();
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
        fileTrash.mutate(() => []);
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
      {items.isLoading || fileTrash.isLoading ? (
        <LoadingRows rows={3} label="Loading the trash" />
      ) : (items.error !== undefined && !items.data) || (fileTrash.error !== undefined && !fileTrash.data) ? (
        <LoadError title="The trash did not load" error={items.error ?? fileTrash.error} onRetry={() => { items.reload(); fileTrash.reload(); }} />
      ) : rows.length === 0 ? (
        <EmptyRows>The trash is empty.</EmptyRows>
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

function LibrariesSettings() {
  const people = useResource(signal => usersApi.list(signal), {key: 'users'});
  return (
    <>
      <SettingsSectionHeader
        title="Libraries"
        subtitle="Your libraries and who sees each one, folders that are not a library yet, sorting and the trash."
      />
      <div {...stylex.props(styles.groups)}>
        <LibrariesGroup people={people.data ?? []} />
        <UnassignedFoldersGroup />
        <SortingGroup />
        <FoldersGroup />
        <section id="trash" aria-label="Trash"><TrashGroup /></section>
      </div>
    </>
  );
}

export default function LibrariesSettingsPage() {
  return (
    <OwnerOnly>
      <LibrariesSettings />
    </OwnerOnly>
  );
}

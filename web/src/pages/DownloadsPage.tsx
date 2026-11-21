/**
 * Downloads: everything on its way into the library.
 *
 * Top: one field for a web link or magnet (the primary action), plus upload
 * and .torrent pickers, and the VPN line when torrents are affected. Then
 * In progress (torrents, web downloads and browser uploads together, live
 * from `download` events and the upload queue) and Finished (where each
 * download's files went). Files dropped anywhere in the app arrive here as
 * `location.state.droppedFiles` and start uploading straight away.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {AlertDialog} from '@astryxdesign/core/AlertDialog';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {ToggleButton} from '@astryxdesign/core/ToggleButton';
import {Layers} from 'lucide-react';
import {useToast} from '@astryxdesign/core/Toast';
import {useCallback, useEffect, useMemo, useRef, useState, type ReactNode} from 'react';
import {useLocation, useNavigate, useSearchParams} from 'react-router';
import {ApiError, downloads as downloadsApi} from '../api/client';
import type {Download, Library} from '../api/types';
import {useLibraries} from '../app/LibrariesProvider';
import {useAuth} from '../auth/AuthProvider';
import {AddPanel} from '../components/downloads/AddPanel';
import {TorrentFilesDialog} from '../components/downloads/TorrentFilesDialog';
import {DownloadRow} from '../components/downloads/DownloadRow';
import {DownloadsEmpty} from '../components/downloads/DownloadsEmpty';
import {isActive} from '../components/downloads/downloadText';
import {FinishedRow} from '../components/downloads/FinishedRow';
import {ListSkeleton} from '../components/downloads/ListSkeleton';
import {RemoveDownloadDialog} from '../components/downloads/RemoveDownloadDialog';
import {rowStyles} from '../components/downloads/TransferRow';
import {useDownloadItems} from '../components/downloads/useDownloadItems';
import {useDownloads} from '../components/downloads/useDownloads';
import {PageHeader} from '../components/PageHeader';
import {UploadRow} from '../components/uploads/UploadRow';
import {
  addFiles,
  cancelUpload,
  onUploaded,
  restoreInterrupted,
  useUploadQueue,
  type UploadEntry,
} from '../components/uploads/uploadQueue';
import {useVpnStatus} from '../hooks/useVpnStatus';
import {errorMessage} from '../lib/errors';
import {formatCount} from '../lib/format';

const TORRENT_MAX_BYTES = 10 * 1024 * 1024;
const FINISHED_PAGE = 15;

const styles = stylex.create({
  sections: {
    display: 'flex',
    flexDirection: 'column',
    gap: 40,
  },
  section: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    minWidth: 0,
  },
  sectionHeader: {
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: 10,
  },
  count: {
    fontVariantNumeric: 'tabular-nums',
  },
  layoutOptions: {marginInlineStart: 'auto'},
  group: {
    borderBottomWidth: 1, borderBottomStyle: 'solid', borderBottomColor: colorVars['--color-border'],
    ':last-child': {borderBottomWidth: 0},
  },
  groupHeading: {paddingBlock: 8, paddingInline: 16, overflowWrap: 'anywhere'},
  panel: {
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 'var(--radius-container)',
    backgroundColor: colorVars['--color-background-surface'],
    overflow: 'clip',
  },
  quiet: {
    paddingBlock: 20,
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
    lineHeight: 1.6,
  },
  more: {
    display: 'flex',
    justifyContent: 'center',
    paddingBlock: 8,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
});

/** Dropped-file batches already handled (StrictMode runs effects twice). */
const handledDrops = new WeakSet<object>();

function isTorrentFile(file: File): boolean {
  return file.name.toLowerCase().endsWith('.torrent');
}

function byNewest(a: Download, b: Download): number {
  return b.created_at - a.created_at;
}

function byFinished(a: Download, b: Download): number {
  return (b.completed_at ?? b.created_at) - (a.completed_at ?? a.created_at);
}

interface TransferEntry {
  key: string;
  libraryId?: string | null;
  row: ReactNode;
}

function TransferList({entries, libraries, grouped, label}: {
  entries: TransferEntry[];
  libraries: Library[];
  grouped: boolean;
  label: string;
}) {
  const rows = (items: TransferEntry[], labelledBy?: string) =>
    <ul {...stylex.props(rowStyles.list)} aria-labelledby={labelledBy}>
      {items.map(entry => <li key={entry.key} {...stylex.props(rowStyles.item)}>{entry.row}</li>)}
    </ul>;
  if (!grouped) return rows(entries, label);

  const names = new Map(libraries.map(library => [library.id, library.name]));
  const groups = new Map<string | null, TransferEntry[]>();
  for (const entry of entries) {
    const key = entry.libraryId ? (names.has(entry.libraryId) ? entry.libraryId : '') : null;
    const group = groups.get(key);
    if (group) group.push(entry);
    else groups.set(key, [entry]);
  }
  const order = [...names.keys(), null, ''];
  return order.map((key, index) => {
    const list = groups.get(key);
    if (!list?.length) return null;
    const heading = `${label}-library-${index}`;
    return <div key={heading} {...stylex.props(styles.group)}>
      <Text as="h3" type="label" id={heading} weight="semibold" xstyle={styles.groupHeading}>
        {key === null ? 'Automatic sorting' : names.get(key) ?? 'Library unavailable'}
      </Text>
      {rows(list, heading)}
    </div>;
  });
}

export default function DownloadsPage() {
  const {isOwner, user} = useAuth();
  const userId = user?.id ?? null;
  const location = useLocation();
  const [params, setParams] = useSearchParams();
  const grouped = params.get('group') === 'library';
  const {libraries} = useLibraries();
  const navigate = useNavigate();
  const showToast = useToast();
  const vpn = useVpnStatus();
  const data = useDownloads();
  const {upsert, remove: removeLocal} = data;
  const queue = useUploadQueue();
  const uploadInputRef = useRef<HTMLInputElement | null>(null);

  const [filesTargetId, setFilesTargetId] = useState<string | null>(null);
  const filesTarget = data.items.find(item => item.id === filesTargetId && isActive(item));
  const [libraryId, setLibraryId] = useState('');
  const [removeTarget, setRemoveTarget] = useState<Download | null>(null);
  const [cancelTarget, setCancelTarget] = useState<UploadEntry | null>(null);
  const [isCancelling, setIsCancelling] = useState(false);
  const [retrying, setRetrying] = useState<ReadonlySet<string>>(new Set());
  const [isAddingTorrents, setIsAddingTorrents] = useState(false);
  const [finishedShown, setFinishedShown] = useState(FINISHED_PAGE);

  const active = useMemo(() => data.items.filter(isActive).sort(byNewest), [data.items]);
  const finished = useMemo(() => data.items.filter(item => !isActive(item)).sort(byFinished), [data.items]);
  const items = useDownloadItems(finished);

  const toastError = useCallback(
    (message: string) => showToast({body: message, type: 'error'}),
    [showToast],
  );

  // Interrupted uploads from an earlier visit, and finished uploads joining the list.
  useEffect(() => {
    if (userId) restoreInterrupted(userId);
    return onUploaded(download => upsert(download));
  }, [upsert, userId]);

  const addTorrents = useCallback(
    async (files: File[], destination = libraryId) => {
      setIsAddingTorrents(true);
      let added = 0;
      try {
        for (const file of files) {
          if (file.size > TORRENT_MAX_BYTES) {
            toastError(`${file.name} is larger than 10 MB, so it is not a .torrent file PopcornStash can read.`);
            continue;
          }
          try {
            const next = await downloadsApi.addTorrent(file, destination || undefined);
            upsert(next);
            if (added === 0) setFilesTargetId(next.id);
            added += 1;
          } catch (error) {
            const fallback =
              error instanceof ApiError && error.code === 'vpn_unavailable'
                ? 'Torrents are turned off on this server. Web links and uploads still work.'
                : `${file.name} could not be added.`;
            toastError(errorMessage(error, fallback));
          }
        }
      } finally {
        setIsAddingTorrents(false);
      }
      if (added > 0) showToast({body: added === 1 ? 'Torrent added.' : `${formatCount(added, 'torrent')} added.`});
    },
    [showToast, toastError, upsert, libraryId],
  );

  const handleFiles = useCallback(
    (files: File[], destination = libraryId) => {
      const torrents = files.filter(isTorrentFile);
      const others = files.filter(file => !isTorrentFile(file));
      if (others.length > 0 && userId) {
        const result = addFiles(others, userId, destination || undefined);
        const messages: string[] = [];
        if (result.started > 0) {
          messages.push(
            result.started === 1 && others.length === 1
              ? `Uploading ${others[0]?.name ?? 'the file'}.`
              : `Uploading ${formatCount(result.started, 'file')}.`,
          );
        }
        if (result.resumed > 0) {
          messages.push(
            result.resumed === 1
              ? 'One upload continues from where it stopped.'
              : `${formatCount(result.resumed, 'upload')} continue from where they stopped.`,
          );
        }
        if (result.duplicates > 0) {
          messages.push(
            result.duplicates === 1 ? 'One file was already uploading.' : `${formatCount(result.duplicates, 'file')} were already uploading.`,
          );
        }
        if (messages.length > 0) showToast({body: messages.join(' ')});
      }
      if (torrents.length > 0) void addTorrents(torrents, destination);
    },
    [addTorrents, showToast, userId, libraryId],
  );

  // Files dropped anywhere in the app arrive through router state.
  useEffect(() => {
    const state = location.state as {droppedFiles?: unknown; libraryId?: string} | null;
    if (!state || !Array.isArray(state.droppedFiles) || handledDrops.has(state)) return;
    handledDrops.add(state);
    const files = state.droppedFiles.filter((file): file is File => file instanceof File);
    // Start the uploads after this render settles.
    if (files.length > 0) queueMicrotask(() => handleFiles(files, state.libraryId));
    navigate(`${location.pathname}${location.search}`, {replace: true, state: null});
  }, [location.state, location.pathname, location.search, navigate, handleFiles]);

  const retry = async (download: Download) => {
    setRetrying(current => new Set(current).add(download.id));
    try {
      upsert(await downloadsApi.resume(download.id));
    } catch (error) {
      const cannotResume = error instanceof ApiError && [400, 404, 409].includes(error.status);
      if (!cannotResume) {
        toastError(errorMessage(error, 'The download could not be retried. Try again.'));
      } else {
        // The server cannot restart this one in place: add the same link again.
        try {
          upsert(await downloadsApi.create({url: download.source, ...(download.library_id ? {library_id: download.library_id} : {})}));
          await downloadsApi.remove(download.id, false).catch(() => undefined);
          removeLocal(download.id);
        } catch (second) {
          toastError(errorMessage(second, 'The download could not be retried. Try again.'));
        }
      }
    } finally {
      setRetrying(current => {
        const next = new Set(current);
        next.delete(download.id);
        return next;
      });
    }
  };

  const confirmCancelUpload = async () => {
    if (!cancelTarget) return;
    setIsCancelling(true);
    try {
      await cancelUpload(cancelTarget.key);
    } finally {
      setIsCancelling(false);
      setCancelTarget(null);
    }
  };

  const inProgressCount = active.length + queue.length;
  const hasAnything = inProgressCount > 0 || finished.length > 0;
  const isReady = data.status === 'ready';
  const visibleFinished = finished.slice(0, finishedShown);

  return (
    <>
      <PageHeader
        title="Downloads"
        actions={isOwner ? <Button label="Clean completed files" variant="secondary" href="/clean?folder=Downloads%2FCompleted" /> : undefined}
        subtitle="New files land here first, then move into your library."
      />

      {filesTarget && <TorrentFilesDialog
        key={filesTarget.id}
        download={filesTarget}
        onClose={() => setFilesTargetId(null)}
        onSaved={(message, updated) => {
          if (updated) upsert(updated);
          else data.reload();
          showToast({body: message});
        }}
      />}
      <AddPanel
        libraryId={libraryId} onLibrary={setLibraryId}
        isOwner={isOwner}
        vpnStatus={vpn.status}
        vpnUnavailable={vpn.isUnavailable}
        isServerQuiet={data.status === 'error'}
        uploadInputRef={uploadInputRef}
        isAddingTorrents={isAddingTorrents}
        onAdded={download => {
          upsert(download);
          if (download.kind === 'torrent') setFilesTargetId(download.id);
          showToast({body: `Added ${download.name}.`});
        }}
        onPickFiles={handleFiles}
        onPickTorrents={files => void addTorrents(files)}
      />

      <div {...stylex.props(styles.sections)}>
        {data.status === 'error' ? (
          <Banner
            status="error"
            title="Downloads could not be loaded"
            description={data.error ?? 'PopcornStash did not answer. Check the connection and try again.'}
            collapsible={false}
            endContent={<Button label="Try again" variant="secondary" onClick={data.reload} />}
          />
        ) : null}

        {isReady && !hasAnything ? <DownloadsEmpty isOwner={isOwner} /> : null}

        {data.status === 'loading' || inProgressCount > 0 || (isReady && hasAnything) ? (
          <section aria-labelledby="in-progress-title" {...stylex.props(styles.section)}>
            <div {...stylex.props(styles.sectionHeader)}>
              <Heading level={2} id="in-progress-title">
                In progress
              </Heading>
              {inProgressCount > 0 ? (
                <Text type="supporting" color="secondary" xstyle={styles.count}>
                  {inProgressCount}
                </Text>
              ) : null}
              {hasAnything ? <div {...stylex.props(styles.layoutOptions)}>
                <ToggleButton label="Group by library" icon={<Layers size={18} aria-hidden="true" />} isPressed={grouped}
                  onPressedChange={pressed => setParams(current => {
                    const next = new URLSearchParams(current);
                    if (pressed) next.set('group', 'library');
                    else next.delete('group');
                    return next;
                  }, {replace: true, preventScrollReset: true})}>Group by library</ToggleButton>
              </div> : null}
            </div>
            <div {...stylex.props(styles.panel)}>
              {inProgressCount > 0 ? (
                <TransferList grouped={grouped} libraries={libraries ?? []} label="in-progress-title" entries={[
                  ...queue.map(entry => ({
                    key: `upload-${entry.key}`, libraryId: entry.libraryId,
                    row: <UploadRow entry={entry} onChooseAgain={() => uploadInputRef.current?.click()} onCancel={setCancelTarget} />,
                  })),
                  ...active.map(download => ({
                    key: download.id, libraryId: download.library_id,
                    row: <DownloadRow onFiles={item => setFilesTargetId(item.id)} download={download} onChanged={upsert} onRemove={setRemoveTarget} onError={toastError} />,
                  })),
                ]} />
              ) : data.status === 'loading' ? (
                <ListSkeleton label="Loading downloads" rows={3} />
              ) : (
                <Text as="p" display="block" color="secondary" xstyle={styles.quiet}>
                  Nothing is downloading right now. Paste a link above or drop files anywhere to start.
                </Text>
              )}
            </div>
          </section>
        ) : null}

        {isReady && finished.length > 0 ? (
          <section aria-labelledby="finished-title" {...stylex.props(styles.section)}>
            <div {...stylex.props(styles.sectionHeader)}>
              <Heading level={2} id="finished-title">
                Finished
              </Heading>
              <Text type="supporting" color="secondary" xstyle={styles.count}>
                {finished.length}
              </Text>
            </div>
            <div {...stylex.props(styles.panel)}>
              <TransferList grouped={grouped} libraries={libraries ?? []} label="finished-title" entries={visibleFinished.map(download => ({
                key: download.id, libraryId: download.library_id,
                row: <FinishedRow download={download} items={items} isOwner={isOwner} isRetrying={retrying.has(download.id)}
                  onRetry={item => void retry(item)} onUploadAgain={() => uploadInputRef.current?.click()} onRemove={setRemoveTarget} />,
              }))} />
              {finished.length > visibleFinished.length ? (
                <div {...stylex.props(styles.more)}>
                  <Button
                    label={`Show ${Math.min(FINISHED_PAGE, finished.length - visibleFinished.length)} more`}
                    variant="ghost"
                    onClick={() => setFinishedShown(shown => shown + FINISHED_PAGE)}
                  />
                </div>
              ) : null}
            </div>
          </section>
        ) : null}
      </div>

      <RemoveDownloadDialog
        download={removeTarget}
        onClose={() => setRemoveTarget(null)}
        onRemoved={download => {
          removeLocal(download.id);
          showToast({body: 'Download removed.'});
        }}
      />
      <AlertDialog
        isOpen={cancelTarget !== null}
        onOpenChange={open => {
          if (!open && !isCancelling) setCancelTarget(null);
        }}
        title={cancelTarget?.state === 'interrupted' ? 'Discard this upload?' : 'Cancel this upload?'}
        description={`The part of ${cancelTarget?.name ?? 'this file'} already sent is deleted from the server. To add the file later, upload it again.`}
        actionLabel={cancelTarget?.state === 'interrupted' ? 'Discard upload' : 'Cancel upload'}
        cancelLabel="Keep it"
        actionVariant="primary"
        isActionLoading={isCancelling}
        onAction={() => void confirmCancelUpload()}
      />
    </>
  );
}

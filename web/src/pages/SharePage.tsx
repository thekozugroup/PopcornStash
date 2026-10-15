/**
 * Share target. Android's share sheet posts what another app shared to
 * /share (the manifest's `share_target`), and the service worker
 * (public/sw.js) answers with one of these:
 *
 * - `/share?url&text&title`: a link or text. Takes the first web link or
 *   magnet out of it, asks once, then adds it to Downloads.
 * - `/share?files=<id>`: photos, videos or other files, kept on this device
 *   by the service worker. The page lists them and, once the person
 *   confirms, sends them to Downloads exactly like dropped files (the upload
 *   queue). Cancel deletes the kept copies.
 * - `/share?failed=files`: the files could not be kept (usually no room).
 *
 * The session gate in front of this route sends signed-out visitors to sign
 * in and back here with the same query; shared files wait on the device.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Text} from '@astryxdesign/core/Text';
import {
  CircleCheck,
  FileArchive,
  FileIcon,
  FilesIcon,
  FileText,
  FileUp,
  FileX,
  Film,
  Globe,
  ImageIcon,
  ImagesIcon,
  Link2Off,
  Magnet,
  Music,
  type LucideIcon,
} from 'lucide-react';
import {useEffect, useId, useState, type ReactNode} from 'react';
import {useNavigate, useSearchParams} from 'react-router';
import {ApiError, downloads as downloadsApi} from '../api/client';
import type {Download} from '../api/types';
import {useAuth} from '../auth/AuthProvider';
import {BreakableAddress, BreakableName} from '../components/BreakableName';
import {findLink, isTorrentLink, type DetectedLink} from '../components/downloads/linkDetect';
import {MediaThumb} from '../components/downloads/MediaThumb';
import {DestinationSelect} from '../components/downloads/DestinationSelect';
import {VpnLine} from '../components/downloads/VpnLine';
import {isNoVpnMode, torrentsAffected, torrentsOff, torrentsWait} from '../lib/vpn';
import {PageHeader} from '../components/PageHeader';
import {
  deleteOldShares,
  discardShare,
  isShareId,
  loadShare,
  releaseSharedCopy,
  type Share,
} from '../components/uploads/sharedFiles';
import {useVpnStatus} from '../hooks/useVpnStatus';
import {errorMessage} from '../lib/errors';
import {formatBytes, formatCount} from '../lib/format';

const styles = stylex.create({
  card: {
    display: 'flex',
    flexDirection: 'column',
    gap: 24,
    maxWidth: 600,
    paddingBlock: {default: 28, '@media (max-width: 559px)': 20},
    paddingInline: {default: 28, '@media (max-width: 559px)': 16},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 'var(--radius-container)',
    backgroundColor: colorVars['--color-background-surface'],
  },
  summary: {
    display: 'grid',
    gridTemplateColumns: {default: 'auto minmax(0, 1fr)', '@media (max-width: 559px)': 'minmax(0, 1fr)'},
    columnGap: 16,
    alignItems: 'start',
  },
  icon: {
    display: {default: 'grid', '@media (max-width: 559px)': 'none'},
    placeItems: 'center',
    width: 44,
    height: 44,
    borderRadius: 'var(--popcorn-radius-media)',
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-icon-secondary'],
  },
  iconDone: {
    backgroundColor: colorVars['--color-success-muted'],
    color: colorVars['--color-success'],
  },
  text: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    minWidth: 0,
  },
  title: {
    overflowWrap: 'anywhere',
  },
  url: {
    display: '-webkit-box',
    WebkitBoxOrient: 'vertical',
    WebkitLineClamp: 3,
    overflow: 'hidden',
    margin: 0,
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    // Breaks come from the address's own separators (BreakableAddress); only a long
    // run without any (a magnet's hash) breaks where it must.
    overflowWrap: 'break-word',
  },
  shared: {
    margin: 0,
    paddingBlock: 10,
    paddingInline: 12,
    borderRadius: 'var(--radius-element)',
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    overflowWrap: 'anywhere',
    whiteSpace: 'pre-wrap',
    maxHeight: 160,
    overflow: 'auto',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
  vpn: {
    paddingTop: 16,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  note: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    paddingTop: 20,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  noteText: {
    lineHeight: 1.55,
    maxWidth: '62ch',
    textWrap: 'pretty',
  },
  files: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  fileList: {
    margin: 0,
    padding: 0,
    listStyle: 'none',
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
    borderBottomWidth: 1,
    borderBottomStyle: 'solid',
    borderBottomColor: colorVars['--color-border'],
  },
  fileRow: {
    display: 'grid',
    gridTemplateColumns: 'auto minmax(0, 1fr)',
    columnGap: 14,
    alignItems: 'center',
    paddingBlock: 10,
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  fileText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    minWidth: 0,
  },
  fileName: {
    overflowWrap: 'break-word',
  },
  meta: {
    fontVariantNumeric: 'tabular-nums',
  },
  loading: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
});

/** `route` is ", only through the VPN", or empty when the server runs torrents without one. */
const KIND: Record<DetectedLink['kind'], {icon: LucideIcon; words: (link: DetectedLink, route: string) => string}> = {
  magnet: {icon: Magnet, words: (_link, route) => `Magnet link. Downloads as a torrent${route}.`},
  'torrent-url': {
    icon: FileUp,
    words: (link, route) => `Link to a .torrent file on ${link.host ?? 'the web'}. Downloads as a torrent${route}.`,
  },
  http: {icon: Globe, words: link => `Web link from ${link.host ?? 'the web'}.`},
};

function cleanTitle(value: string | null, link: DetectedLink | null): string | null {
  if (!value) return null;
  const trimmed = value.trim();
  if (!trimmed || (link && trimmed.includes(link.url))) return null;
  return trimmed.length > 160 ? `${trimmed.slice(0, 157)}...` : trimmed;
}

/** How a shared file is described in the list. */
interface FileKind {
  label: string;
  icon: LucideIcon;
  group: 'photo' | 'video' | 'other';
}

const PHOTO_EXTENSIONS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic', 'heif', 'avif', 'dng', 'tif', 'tiff']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'm4v', 'mov', 'mkv', 'webm', 'avi', '3gp', 'mts', 'm2ts', 'wmv']);
const AUDIO_EXTENSIONS = new Set(['mp3', 'm4a', 'aac', 'flac', 'wav', 'ogg', 'opus']);
const ARCHIVE_EXTENSIONS = new Set(['zip', 'rar', '7z', 'tar', 'gz']);
/** Shared files listed by name; the rest are counted. */
const SHOWN_FILES = 6;

function extensionOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

function isTorrentFile(file: File): boolean {
  return extensionOf(file.name) === 'torrent';
}

function kindOf(file: File): FileKind {
  const ext = extensionOf(file.name);
  const type = file.type.toLowerCase();
  if (ext === 'torrent') return {label: 'Torrent file', icon: FileUp, group: 'other'};
  if (type.startsWith('image/') || PHOTO_EXTENSIONS.has(ext)) return {label: 'Photo', icon: ImageIcon, group: 'photo'};
  if (type.startsWith('video/') || VIDEO_EXTENSIONS.has(ext)) return {label: 'Video', icon: Film, group: 'video'};
  if (type.startsWith('audio/') || AUDIO_EXTENSIONS.has(ext)) return {label: 'Audio', icon: Music, group: 'other'};
  if (type === 'application/pdf' || ext === 'pdf') return {label: 'PDF', icon: FileText, group: 'other'};
  if (ARCHIVE_EXTENSIONS.has(ext)) return {label: 'Archive', icon: FileArchive, group: 'other'};
  return {label: 'File', icon: FileIcon, group: 'other'};
}

/** Pictures the browser can draw (not HEIC or RAW) get a thumbnail. */
function canPreview(file: File): boolean {
  return /^image\/(jpeg|png|gif|webp|avif|bmp)$/.test(file.type);
}

/**
 * "2 photos", "1 video", "3 photos and 1 video", or "4 files", plus the word
 * for the ones not listed ("And 2 more photos").
 */
function describeFiles(files: readonly File[]): {title: string; icon: LucideIcon; noun: string} {
  const groups = files.map(file => kindOf(file).group);
  const photos = groups.filter(group => group === 'photo').length;
  const videos = groups.filter(group => group === 'video').length;
  if (photos + videos < files.length) return {title: formatCount(files.length, 'file'), icon: FilesIcon, noun: 'file'};
  if (videos === 0) {
    return {title: formatCount(photos, 'photo'), icon: photos === 1 ? ImageIcon : ImagesIcon, noun: 'photo'};
  }
  if (photos === 0) return {title: formatCount(videos, 'video'), icon: Film, noun: 'video'};
  return {title: `${formatCount(photos, 'photo')} and ${formatCount(videos, 'video')}`, icon: FilesIcon, noun: 'file'};
}

export default function SharePage() {
  const [params] = useSearchParams();
  const filesId = params.get('files');
  if (params.get('failed') === 'files') return <SharedFilesFailed />;
  if (filesId !== null) return <SharedFiles key={filesId} id={filesId} />;
  return <SharedLink />;
}

/** How to get PopcornStash into Android's share sheet. */
function AndroidNote() {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} {...stylex.props(styles.note)}>
      <Heading level={3} id={headingId}>
        On Android
      </Heading>
      <Text as="p" type="supporting" color="secondary" xstyle={styles.noteText}>
        The Android app adds PopcornStash to the share sheet. Without it, install PopcornStash from Chrome’s menu
        (Install app or Add to Home screen). Then, in any app, choose Share, then PopcornStash, to send links, photos
        and videos.
      </Text>
    </section>
  );
}

/** A card that names a state in one heading and a sentence or two, with one way on. */
function ShareNotice({
  icon: Icon,
  title,
  children,
  action,
}: {
  icon: LucideIcon;
  title: string;
  children: ReactNode;
  action: ReactNode;
}) {
  const headingId = useId();
  return (
    <section {...stylex.props(styles.card)} aria-labelledby={headingId}>
      <div {...stylex.props(styles.summary)}>
        <span {...stylex.props(styles.icon)} aria-hidden="true">
          <Icon size={22} strokeWidth={1.75} />
        </span>
        <div {...stylex.props(styles.text)}>
          <Heading level={2} id={headingId}>
            {title}
          </Heading>
          <Text as="p" color="secondary">
            {children}
          </Text>
        </div>
      </div>
      <div {...stylex.props(styles.actions)}>{action}</div>
    </section>
  );
}

function SharedFilesFailed() {
  return (
    <>
      <PageHeader title="Add shared files" subtitle="Shared from another app." />
      <ShareNotice
        icon={FileX}
        title="The shared files did not arrive"
        action={<Button label="Open Downloads" variant="secondary" href="/downloads" />}>
        This phone could not keep them for PopcornStash, usually because it is short of space. Free some space and
        share them again, or upload them from Downloads.
      </ShareNotice>
    </>
  );
}

interface LoadedShare {
  share: Share | null;
  /** Object URLs of the pictures shown in the list. */
  previews: ReadonlyMap<File, string>;
}

function SharedFiles({id}: {id: string}) {
  const [libraryId, setLibraryId] = useState('');
  const navigate = useNavigate();
  const [loaded, setLoaded] = useState<LoadedShare | null>(null);
  const [isSending, setIsSending] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);

  useEffect(() => {
    let isCurrent = true;
    const made: string[] = [];
    void (async () => {
      await deleteOldShares();
      const share = isShareId(id) ? await loadShare(id) : null;
      if (!isCurrent) return;
      const previews = new Map<File, string>();
      for (const {file} of share?.files.slice(0, SHOWN_FILES) ?? []) {
        if (!canPreview(file)) continue;
        const url = URL.createObjectURL(file);
        made.push(url);
        previews.set(file, url);
      }
      setLoaded({share, previews});
    })();
    return () => {
      isCurrent = false;
      for (const url of made) URL.revokeObjectURL(url);
    };
  }, [id]);

  if (!loaded) {
    return (
      <>
        <PageHeader title="Add shared files" subtitle="Check the files, then upload them to Downloads." />
        <section {...stylex.props(styles.card, styles.loading)} aria-busy="true" aria-label="Loading shared files">
          <Skeleton width="45%" height={24} />
          <Skeleton height={60} index={1} />
          <Skeleton height={60} index={2} />
        </section>
      </>
    );
  }

  const {share, previews} = loaded;
  if (!share) {
    return (
      <>
        <PageHeader title="Add shared files" subtitle="Shared from another app." />
        <ShareNotice
          icon={CircleCheck}
          title="Nothing left to upload"
          action={<Button label="Open Downloads" variant="secondary" href="/downloads" />}>
          These files were already uploaded or cancelled. Uploads still on their way are in Downloads.
        </ShareNotice>
      </>
    );
  }

  const files = share.files.map(shared => shared.file);
  const total = files.reduce((sum, file) => sum + file.size, 0);
  const summary = describeFiles(files);
  const SummaryIcon = summary.icon;
  const hidden = files.length - SHOWN_FILES;

  const upload = async () => {
    if (isSending) return;
    setIsSending(true);
    const handOff: File[] = [];
    for (const {record, file} of share.files) {
      if (!isTorrentFile(file)) {
        handOff.push(file);
        continue;
      }
      // Added as a torrent, not through the upload queue: read it now, so its kept copy can go.
      try {
        const bytes = await file.arrayBuffer();
        handOff.push(new File([bytes], file.name, {type: file.type, lastModified: file.lastModified}));
        void releaseSharedCopy(record);
      } catch {
        handOff.push(file);
      }
    }
    // Downloads uploads them exactly like dropped files; each kept copy goes once its upload is done.
    navigate('/downloads', {replace: true, state: {droppedFiles: handOff, libraryId}});
  };

  const cancel = async () => {
    setIsCancelling(true);
    await discardShare(share.id);
    navigate('/', {replace: true});
  };

  return (
    <>
      <PageHeader title="Add shared files" subtitle="Check the files, then upload them to Downloads." />
      <section {...stylex.props(styles.card)} aria-labelledby="share-files-title">
        <div {...stylex.props(styles.summary)}>
          <span {...stylex.props(styles.icon)} aria-hidden="true">
            <SummaryIcon size={22} strokeWidth={1.75} />
          </span>
          <div {...stylex.props(styles.text)}>
            <Heading level={2} id="share-files-title">
              {summary.title}
            </Heading>
            <Text as="p" color="secondary">
              {files.length === 1
                ? `${formatBytes(total)}. Once it is uploaded, it is sorted into your library.`
                : `${formatBytes(total)} in all. Once each one is uploaded, it is sorted into your library.`}
            </Text>
          </div>
        </div>

        <div {...stylex.props(styles.files)}>
          <ul aria-label="Shared files" {...stylex.props(styles.fileList)}>
            {share.files.slice(0, SHOWN_FILES).map(({record, file}) => {
              const kind = kindOf(file);
              return (
                <li key={record.key} {...stylex.props(styles.fileRow)}>
                  <MediaThumb src={previews.get(file)} icon={kind.icon} width="40px" height="40px" iconSize={18} />
                  <div {...stylex.props(styles.fileText)}>
                    <Text weight="medium" xstyle={styles.fileName}>
                      <BreakableName name={file.name} />
                    </Text>
                    <Text type="supporting" color="secondary" xstyle={styles.meta}>
                      {kind.label} · {formatBytes(file.size)}
                    </Text>
                  </div>
                </li>
              );
            })}
          </ul>
          {hidden > 0 ? (
            <Text as="p" type="supporting" color="secondary">
              And {formatCount(hidden, `more ${summary.noun}`, `more ${summary.noun}s`)}.
            </Text>
          ) : null}
        </div>

        <DestinationSelect value={libraryId} onChange={setLibraryId} disabled={isSending || isCancelling} />
        <div {...stylex.props(styles.actions)}>
          <Button
            label="Upload to Downloads"
            variant="primary"
            isLoading={isSending}
            isDisabled={isCancelling}
            onClick={() => void upload()}
          />
          <Button
            label="Cancel"
            variant="secondary"
            isLoading={isCancelling}
            isDisabled={isSending}
            onClick={() => void cancel()}
          />
        </div>
      </section>
    </>
  );
}

function SharedLink() {
  const [libraryId, setLibraryId] = useState('');
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const {isOwner} = useAuth();
  const vpn = useVpnStatus();
  const [isSending, setIsSending] = useState(false);
  const [error, setError] = useState<{message: string; isVpn: boolean} | null>(null);
  const [sent, setSent] = useState<Download | null>(null);

  const sharedUrl = params.get('url');
  const sharedText = params.get('text');
  const sharedTitle = params.get('title');
  const link = findLink(sharedUrl) ?? findLink(sharedText) ?? findLink(sharedTitle);
  const title = cleanTitle(sharedTitle, link) ?? link?.name ?? link?.host ?? 'Shared link';
  const received = [sharedTitle, sharedText, sharedUrl].filter(Boolean).join('\n');

  const send = async () => {
    if (!link || isSending) return;
    setIsSending(true);
    setError(null);
    try {
      setSent(await downloadsApi.create({url: link.url, library_id: libraryId || undefined}));
    } catch (cause) {
      if (cause instanceof ApiError && cause.code === 'vpn_unavailable') {
        setError({
          message:
            cause.message ||
            'Magnet links need the VPN, and torrents are turned off on this server. Web links still work.',
          isVpn: true,
        });
      } else {
        setError({message: errorMessage(cause, 'The link was not added. Try again.'), isVpn: false});
      }
    } finally {
      setIsSending(false);
    }
  };

  if (sent) {
    const waiting = sent.state === 'queued';
    return (
      <>
        <PageHeader title="Added to Downloads" documentTitle="Add shared link" />
        <section {...stylex.props(styles.card)} aria-labelledby="share-done-title">
          <div {...stylex.props(styles.summary)}>
            <span {...stylex.props(styles.icon, styles.iconDone)} aria-hidden="true">
              <CircleCheck size={22} strokeWidth={1.75} />
            </span>
            <div {...stylex.props(styles.text)}>
              <Heading level={2} id="share-done-title">
                <span {...stylex.props(styles.title)}>{sent.name}</span>
              </Heading>
              <Text as="p" color="secondary">
                {waiting
                  ? sent.error ?? 'It is waiting to start. It appears under In progress in Downloads.'
                  : 'It is downloading now and appears under In progress in Downloads. When it finishes, it is sorted into your library.'}
              </Text>
            </div>
          </div>
          <div {...stylex.props(styles.actions)}>
            <Button label="Open Downloads" variant="primary" href="/downloads" />
            <Button label="Done" variant="secondary" onClick={() => navigate('/', {replace: true})} />
          </div>
        </section>
      </>
    );
  }

  if (!link) {
    return (
      <>
        <PageHeader title="Add shared link" subtitle="Shared from another app." />
        <section {...stylex.props(styles.card)} aria-labelledby="share-none-title">
          <div {...stylex.props(styles.summary)}>
            <span {...stylex.props(styles.icon)} aria-hidden="true">
              <Link2Off size={22} strokeWidth={1.75} />
            </span>
            <div {...stylex.props(styles.text)}>
              <Heading level={2} id="share-none-title">
                Nothing to download in what was shared
              </Heading>
              <Text as="p" color="secondary">
                PopcornStash takes web links, magnet links, photos, videos and other files. What arrived did not
                include any of them.
              </Text>
            </div>
          </div>
          {received ? (
            <div>
              <Text as="p" type="supporting" color="secondary">
                What arrived
              </Text>
              <p {...stylex.props(styles.shared)}>{received}</p>
            </div>
          ) : null}
          <div {...stylex.props(styles.actions)}>
            <Button label="Open Downloads" variant="secondary" href="/downloads" />
          </div>
          <AndroidNote />
        </section>
      </>
    );
  }

  const kind = KIND[link.kind];
  const KindIcon = kind.icon;
  const showVpn = isTorrentLink(link) && torrentsAffected(vpn.status, vpn.isUnavailable);
  // A torrent cannot be added at all while torrents are off on this server.
  const isBlocked = isTorrentLink(link) && torrentsOff(vpn.status);
  // Torrents run only through the VPN, unless the server allows them without one.
  const route = isNoVpnMode(vpn.status) ? '' : ', only through the VPN';
  const words = isBlocked
    ? 'A torrent. Torrents are off on this server, so it cannot be added.'
    : isTorrentLink(link) && torrentsWait(vpn.status, vpn.isUnavailable)
      ? `${kind.words(link, route)} It waits until the VPN is ready.`
      : kind.words(link, route);

  return (
    <>
      <PageHeader title="Add shared link" subtitle="Check the link, then add it to Downloads." />
      <section {...stylex.props(styles.card)} aria-labelledby="share-title">
        <div {...stylex.props(styles.summary)}>
          <span {...stylex.props(styles.icon)} aria-hidden="true">
            <KindIcon size={22} strokeWidth={1.75} />
          </span>
          <div {...stylex.props(styles.text)}>
            <Heading level={2} id="share-title">
              <span {...stylex.props(styles.title)}>{title}</span>
            </Heading>
            <Text as="p" color="secondary">
              {words}
            </Text>
            <p {...stylex.props(styles.url)} title={link.url}>
              <BreakableAddress address={link.url} />
            </p>
          </div>
        </div>

        {showVpn ? (
          <div {...stylex.props(styles.vpn)}>
            <VpnLine status={vpn.status} isUnavailable={vpn.isUnavailable} isOwner={isOwner} />
          </div>
        ) : null}

        {error ? (
          <Banner
            status="error"
            title="Not added"
            description={error.message}
            collapsible={false}
            endContent={
              // The VPN line above already links to the settings when it is shown.
              error.isVpn && isOwner && !showVpn ? (
                <Button label="Open VPN settings" variant="secondary" href="/settings/vpn" />
              ) : undefined
            }
          />
        ) : null}

        <DestinationSelect value={libraryId} onChange={setLibraryId} disabled={isSending} />
        <div {...stylex.props(styles.actions)}>
          <Button
            label="Add to Downloads"
            variant="primary"
            isLoading={isSending}
            isDisabled={isBlocked}
            onClick={() => void send()}
          />
          <Button label="Cancel" variant="secondary" isDisabled={isSending} onClick={() => navigate('/', {replace: true})} />
        </div>
      </section>
    </>
  );
}

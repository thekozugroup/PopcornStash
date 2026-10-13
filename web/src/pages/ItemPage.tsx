/**
 * One item (`/item/:id`): the player (or photo, or file) on a black stage,
 * then the title, a quiet metadata line and the actions: favourite,
 * download the original, edit details, organize files that sit outside the
 * library folders, and (owner only) move to Trash. Technical details and
 * the file's location appear only on request.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {DropdownMenu} from '@astryxdesign/core/DropdownMenu';
import {Heading} from '@astryxdesign/core/Heading';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Text} from '@astryxdesign/core/Text';
import {useToast} from '@astryxdesign/core/Toast';
import {ToggleButton} from '@astryxdesign/core/ToggleButton';
import {ChevronLeft, Download, FolderInput, MoreHorizontal, Pencil, Star, Trash2} from 'lucide-react';
import {useEffect, useRef, useState} from 'react';
import {useLocation, useNavigate, useParams} from 'react-router';
import {ApiError, items as itemsApi} from '../api/client';
import type {ItemDetail} from '../api/types';
import {useAuth} from '../auth/AuthProvider';
import {DeleteItemDialog} from '../components/library/DeleteItemDialog';
import {EditDetailsDialog} from '../components/library/EditDetailsDialog';
import {ItemFacts} from '../components/library/ItemFacts';
import {FileStage, PhotoStage} from '../components/library/ItemStages';
import {LoadError} from '../components/library/LoadError';
import {OrganizeDialog} from '../components/library/OrganizeDialog';
import {
  CATEGORY_FOLDER,
  displayTitle,
  fileType,
  formatTime,
  hasFileNameTitle,
  isInLibraryFolder,
  itemHref,
  itemParent,
} from '../components/library/libraryFormat';
import {useLibraryEvents} from '../components/library/useLibraryEvents';
import {PageHeader, useDocumentTitle} from '../components/PageHeader';
import {VideoPlayer} from '../components/player/VideoPlayer';
import {errorMessage} from '../lib/errors';
import {formatBytes, formatDate, formatDateTime, formatDuration} from '../lib/format';

const styles = stylex.create({
  back: {
    marginInlineStart: -12,
    marginBottom: 12,
  },
  header: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    columnGap: 24,
    rowGap: 16,
    marginTop: 28,
    marginBottom: 24,
  },
  titleBlock: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    minWidth: 0,
    flex: '1 1 320px',
    // Camera file names have no spaces; let them wrap instead of overflowing.
    overflowWrap: 'anywhere',
  },
  meta: {
    lineHeight: 1.5,
    fontVariantNumeric: 'tabular-nums',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 8,
  },
  banners: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    marginTop: 20,
  },
  facts: {
    paddingTop: 20,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  stageSkeleton: {
    aspectRatio: '16 / 9',
    maxHeight: 'calc(100dvh - 200px)',
    marginInline: {default: 0, '@media (max-width: 799px)': -20},
    borderRadius: {default: 16, '@media (max-width: 799px)': 0},
    overflow: 'hidden',
  },
  skeletonText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    marginTop: 28,
  },
  starFilled: {
    fill: 'currentColor',
  },
  // The error banner carries the visible title; the page still needs its h1.
  visuallyHidden: {
    position: 'absolute',
    width: 1,
    height: 1,
    margin: -1,
    padding: 0,
    overflow: 'hidden',
    clipPath: 'inset(50%)',
    whiteSpace: 'nowrap',
    borderWidth: 0,
  },
});

interface ItemState {
  id: string;
  item: ItemDetail | null;
  error: unknown;
}

function metaLine(item: ItemDetail): string {
  const duration = formatDuration(item.duration_s);
  switch (item.category) {
    case 'movie':
      return [item.year ? String(item.year) : null, duration].filter(Boolean).join(' · ');
    case 'episode': {
      const where =
        item.season !== null && item.episode !== null
          ? item.season === 0
            ? `Special ${item.episode}`
            : `Season ${item.season}, episode ${item.episode}`
          : null;
      return [where, duration].filter(Boolean).join(' · ');
    }
    case 'home_video':
      return [item.taken_at ? `Recorded ${formatDateTime(item.taken_at)}` : `Added ${formatDate(item.added_at)}`, duration]
        .filter(Boolean)
        .join(' · ');
    case 'photo':
      if (!item.taken_at) return `Added ${formatDate(item.added_at)}`;
      // A camera-named photo's heading is already its date.
      return hasFileNameTitle(item) ? `Taken at ${formatTime(item.taken_at)}` : `Taken ${formatDateTime(item.taken_at)}`;
    default:
      return [fileType(item.rel_path), duration || formatBytes(item.size)].filter(Boolean).join(' · ');
  }
}

function StatusBanners({item}: {item: ItemDetail}) {
  if (item.status === 'missing') {
    return (
      <Banner
        status="warning"
        title="This file is missing"
        description="PopcornStash cannot find it on the server right now. If it was moved or renamed outside PopcornStash, it shows up again after the next scan."
        collapsible={false}
      />
    );
  }
  if (item.status === 'trashed') {
    return (
      <Banner
        status="warning"
        title="This file is in Trash"
        description="It is deleted for good when its time in Trash runs out. The owner can restore it before then."
        collapsible={false}
      />
    );
  }
  if (item.status === 'needs_review') {
    return (
      <Banner
        status="info"
        title="Waiting for a quick look in Inbox"
        description="PopcornStash was not sure where this belongs, so it has not been sorted yet."
        collapsible={false}
        endContent={<Button label="Open Inbox" variant="secondary" href="/inbox" />}
      />
    );
  }
  return null;
}

export default function ItemPage() {
  const {id = ''} = useParams();
  const location = useLocation();
  const navigate = useNavigate();
  const showToast = useToast();
  const {user} = useAuth();
  const [nonce, setNonce] = useState(0);
  const [state, setState] = useState<ItemState>({id: '', item: null, error: null});
  const [dialog, setDialog] = useState<{kind: 'edit' | 'organize' | 'delete'; key: number} | null>(null);
  const [isSavingFavorite, setIsSavingFavorite] = useState(false);
  const autoPlay = (location.state as {autoplay?: boolean} | null)?.autoplay === true;

  useEffect(() => {
    const controller = new AbortController();
    itemsApi.get(id, controller.signal).then(
      item => setState({id, item, error: null}),
      (error: unknown) => {
        if (!controller.signal.aborted) setState(s => ({id, item: s.id === id ? s.item : null, error}));
      },
    );
    return () => controller.abort();
  }, [id, nonce]);

  const item = state.id === id ? state.item : null;
  const error = state.id === id ? state.error : null;
  useDocumentTitle(item ? displayTitle(item) : 'Loading');

  // Set when this page moved the item to Trash: the page is on its way out, so the
  // server's "removed" event (which can arrive before the answer, while the dialog is
  // still open) must not reload an item that is now gone (a 404).
  const trashedHere = useRef(false);
  const isLeaving = () => trashedHere.current || dialog?.kind === 'delete';

  useLibraryEvents({
    onChanged: ids => {
      if (ids.includes(id) && !isLeaving()) setNonce(n => n + 1);
    },
    onRemoved: removed => {
      if (removed === id && !isLeaving()) setNonce(n => n + 1);
    },
  });

  const parent = item ? itemParent(item) : null;
  const back = (
    <div {...stylex.props(styles.back)}>
      <Button
        label={parent?.label ?? 'Home'}
        variant="ghost"
        href={parent?.href ?? '/'}
        icon={<ChevronLeft size={18} strokeWidth={1.75} aria-hidden="true" />}
      />
    </div>
  );

  if (!item) {
    if (error instanceof ApiError && error.code === 'not_found') {
      return (
        <PageHeader
          title="This file is not in your library"
          subtitle="It may have been moved to Trash, or the link is out of date."
          actions={<Button label="Go to Home" variant="primary" href="/" />}
          isActionBelow
        />
      );
    }
    if (error) {
      return (
        <>
          {back}
          <div {...stylex.props(styles.visuallyHidden)}>
            <Heading level={1}>This file could not be opened</Heading>
          </div>
          <LoadError title="This file could not be opened" error={error} onRetry={() => setNonce(n => n + 1)} />
        </>
      );
    }
    return (
      <div role="status" aria-busy="true" aria-label="Loading">
        {back}
        <div {...stylex.props(styles.stageSkeleton)}>
          <Skeleton width="100%" height="100%" radius="none" />
        </div>
        <div {...stylex.props(styles.skeletonText)}>
          <Skeleton width="45%" height={30} radius={1} />
          <Skeleton width="25%" height={16} radius={1} />
        </div>
      </div>
    );
  }

  const isOwner = user?.role === 'owner';
  const canOrganize = !isInLibraryFolder(item.rel_path) && item.status !== 'trashed';
  const isVideo = item.kind === 'video';
  const isPhoto = item.kind === 'photo';
  const hasStageDownload = !isVideo && !isPhoto && item.kind !== 'audio';
  const title = displayTitle(item);

  const replaceItem = (next: ItemDetail) => setState({id, item: next, error: null});

  const toggleFavorite = async (favorite: boolean) => {
    const previous = item;
    replaceItem({...item, favorite});
    setIsSavingFavorite(true);
    try {
      replaceItem(await itemsApi.update(item.id, {favorite}));
    } catch (failure) {
      replaceItem(previous);
      showToast({body: `Favourite was not saved. ${errorMessage(failure)}`, type: 'error'});
    } finally {
      setIsSavingFavorite(false);
    }
  };

  const menuItems = [
    {label: 'Edit details', icon: Pencil, onClick: () => setDialog({kind: 'edit', key: Date.now()})},
    ...(canOrganize
      ? [{label: 'Organize into library', icon: FolderInput, onClick: () => setDialog({kind: 'organize', key: Date.now()})}]
      : []),
    ...(isOwner && item.status !== 'trashed'
      ? [
          {type: 'divider' as const},
          {
            label: 'Move to Trash',
            icon: Trash2,
            onClick: () => setDialog({kind: 'delete', key: Date.now()}),
          },
        ]
      : []),
  ];

  let stage;
  if (isVideo) {
    stage = (
      <VideoPlayer
        key={item.id}
        item={item}
        autoPlay={autoPlay}
        onNext={
          item.next_episode_id
            ? () => navigate(itemHref({id: item.next_episode_id as string}), {state: {autoplay: true}})
            : null
        }
      />
    );
  } else if (isPhoto) {
    stage = <PhotoStage key={item.id} item={item} />;
  } else {
    stage = <FileStage key={item.id} item={item} />;
  }

  return (
    <>
      {back}
      {stage}

      {item.status !== 'ready' ? (
        <div {...stylex.props(styles.banners)}>
          <StatusBanners item={item} />
        </div>
      ) : null}

      <div {...stylex.props(styles.header)}>
        <div {...stylex.props(styles.titleBlock)}>
          <Heading level={1} textWrap="balance">
            {title}
          </Heading>
          <Text as="p" color="secondary" xstyle={styles.meta}>
            {metaLine(item)}
          </Text>
        </div>
        <div {...stylex.props(styles.actions)}>
          <ToggleButton
            label="Favourite"
            isPressed={item.favorite}
            isDisabled={isSavingFavorite}
            onPressedChange={pressed => void toggleFavorite(pressed)}
            icon={<Star size={16} strokeWidth={1.75} aria-hidden="true" />}
            pressedIcon={<Star size={16} strokeWidth={1.75} aria-hidden="true" {...stylex.props(styles.starFilled)} />}>
            Favourite
          </ToggleButton>
          {!hasStageDownload ? (
            <Button
              label="Download"
              variant="secondary"
              href={itemsApi.fileUrl(item.id, true)}
              icon={<Download size={16} strokeWidth={1.75} aria-hidden="true" />}
              tooltip="Download the original file"
            />
          ) : null}
          <DropdownMenu
            button={{
              label: 'More actions',
              variant: 'ghost',
              isIconOnly: true,
              icon: <MoreHorizontal size={18} strokeWidth={1.75} aria-hidden="true" />,
            }}
            hasChevron={false}
            alignment="end"
            menuWidth={220}
            items={menuItems}
          />
        </div>
      </div>

      <div {...stylex.props(styles.facts)}>
        <ItemFacts item={item} />
      </div>

      {dialog?.kind === 'edit' ? (
        <EditDetailsDialog
          key={dialog.key}
          item={item}
          isOpen
          onOpenChange={open => !open && setDialog(null)}
          onSaved={saved => {
            replaceItem(saved);
            showToast({body: 'Details saved.'});
          }}
        />
      ) : null}
      {dialog?.kind === 'organize' ? (
        <OrganizeDialog
          key={dialog.key}
          item={item}
          isOpen
          onOpenChange={open => !open && setDialog(null)}
          onOrganized={moved => {
            replaceItem(moved);
            showToast({body: `Moved into ${CATEGORY_FOLDER[moved.category]}.`});
          }}
        />
      ) : null}
      {dialog?.kind === 'delete' ? (
        <DeleteItemDialog
          key={dialog.key}
          item={item}
          isOpen
          onOpenChange={open => !open && setDialog(null)}
          onDeleted={() => {
            trashedHere.current = true;
            showToast({body: `“${title}” moved to Trash.`});
            navigate(parent?.href ?? '/', {replace: true});
          }}
        />
      ) : null}
    </>
  );
}

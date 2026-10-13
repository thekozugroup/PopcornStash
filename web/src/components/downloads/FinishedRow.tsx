/**
 * A finished download and where its files went: "Sorted into Movies" with a
 * link to the film, "Waiting for a decision" with a link to Inbox, or the
 * failure in words with Retry when the source can be fetched again (Upload
 * again for a failed browser upload).
 *
 * Under 560px the text column keeps its place beside the thumbnail and the
 * recovery button moves under the outcome, so the file name keeps its width.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Link} from '@astryxdesign/core/Link';
import {MoreMenu} from '@astryxdesign/core/MoreMenu';
import {Text} from '@astryxdesign/core/Text';
import {CircleCheck, CircleDashed, CircleX, Inbox, LoaderCircle, CircleMinus, type LucideIcon} from 'lucide-react';
import type {Download, ItemDetail} from '../../api/types';
import {formatBytes, formatRelative} from '../../lib/format';
import {KIND_ICON} from './DownloadRow';
import {canRetry, canUploadAgain, finishedOutcome, KIND_LABEL, type FinishedOutcome} from './downloadText';
import {MediaThumb} from './MediaThumb';

const styles = stylex.create({
  row: {
    display: 'grid',
    gridTemplateColumns: 'auto minmax(0, 1fr) auto',
    gridTemplateAreas: {
      default: '"thumb title actions" "thumb body actions"',
      '@media (max-width: 559px)': '"thumb title actions" "thumb body body"',
    },
    columnGap: {default: 16, '@media (max-width: 559px)': 12},
    rowGap: 4,
    alignItems: 'start',
    paddingBlock: 16,
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
  },
  thumb: {
    gridArea: 'thumb',
    display: 'flex',
    justifyContent: 'center',
    width: 72,
  },
  title: {
    gridArea: 'title',
    minWidth: 0,
    paddingTop: 2,
    overflowWrap: 'anywhere',
  },
  body: {
    gridArea: 'body',
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    minWidth: 0,
  },
  meta: {
    display: 'flex',
    flexWrap: 'wrap',
    columnGap: 16,
    rowGap: 2,
    margin: 0,
    padding: 0,
    listStyle: 'none',
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    fontVariantNumeric: 'tabular-nums',
  },
  outcome: {
    margin: 0,
    display: 'flex',
    alignItems: 'flex-start',
    gap: 8,
    minWidth: 0,
    fontSize: 14,
    lineHeight: 1.45,
    color: colorVars['--color-text-primary'],
  },
  outcomeIcon: {
    flexShrink: 0,
    marginTop: 1,
    display: 'inline-flex',
  },
  outcomeText: {
    minWidth: 0,
    overflowWrap: 'anywhere',
  },
  success: {color: colorVars['--color-success']},
  warning: {color: colorVars['--color-warning']},
  error: {color: colorVars['--color-error']},
  neutral: {color: colorVars['--color-icon-secondary']},
  actions: {
    gridArea: 'actions',
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    marginBlock: -2,
  },
  recoverEnd: {
    display: {default: 'flex', '@media (max-width: 559px)': 'none'},
  },
  recoverInline: {
    display: {default: 'none', '@media (max-width: 559px)': 'flex'},
    marginTop: 4,
  },
});

const OUTCOME_ICON: Record<FinishedOutcome['kind'], {icon: LucideIcon; tone: 'success' | 'warning' | 'error' | 'neutral'}> = {
  sorted: {icon: CircleCheck, tone: 'success'},
  review: {icon: Inbox, tone: 'warning'},
  waiting: {icon: CircleDashed, tone: 'neutral'},
  sorting: {icon: LoaderCircle, tone: 'neutral'},
  nothing: {icon: CircleMinus, tone: 'neutral'},
  failed: {icon: CircleX, tone: 'error'},
  cancelled: {icon: CircleMinus, tone: 'neutral'},
  unknown: {icon: CircleCheck, tone: 'success'},
};

export interface FinishedRowProps {
  download: Download;
  items: Map<string, ItemDetail>;
  isOwner: boolean;
  isRetrying: boolean;
  onRetry: (download: Download) => void;
  /** Opens the file picker so a failed upload can be sent again. */
  onUploadAgain: () => void;
  onRemove: (download: Download) => void;
}

function OutcomeLine({outcome}: {outcome: FinishedOutcome}) {
  const {icon: Icon, tone} = OUTCOME_ICON[outcome.kind];
  let content;
  switch (outcome.kind) {
    case 'sorted':
      content = (
        <>
          {outcome.text}
          {outcome.linkLabel && outcome.href ? (
            <>
              {': '}
              <Link href={outcome.href} size="base">
                {outcome.linkLabel}
              </Link>
            </>
          ) : null}
        </>
      );
      break;
    case 'review':
      content = (
        <>
          {outcome.text}.{' '}
          <Link href={outcome.href} size="base">
            Review in Inbox
          </Link>
        </>
      );
      break;
    case 'failed':
      content = <>Failed. {outcome.text}</>;
      break;
    default:
      content = outcome.text;
  }
  return (
    <p {...stylex.props(styles.outcome)}>
      <span {...stylex.props(styles.outcomeIcon, styles[tone])} aria-hidden="true">
        <Icon size={16} strokeWidth={2} />
      </span>
      <span {...stylex.props(styles.outcomeText)}>{content}</span>
    </p>
  );
}

export function FinishedRow({download, items, isOwner, isRetrying, onRetry, onUploadAgain, onRemove}: FinishedRowProps) {
  const loaded = download.item_ids.map(id => items.get(id));
  const outcome = finishedOutcome(download, loaded, isOwner);
  const thumbItem = loaded.find(item => item?.thumb);
  const isPoster = thumbItem?.category === 'movie';
  const finishedAt = download.completed_at ?? download.created_at;
  const meta = [
    KIND_LABEL[download.kind],
    download.state === 'failed' ? `Started ${formatRelative(download.created_at).toLowerCase()}` : `Finished ${formatRelative(finishedAt).toLowerCase()}`,
    download.total_bytes ? formatBytes(download.total_bytes) : null,
  ].filter((entry): entry is string => Boolean(entry));

  // One recovery action at most; rendered twice (trailing edge, or under the
  // outcome on narrow screens) with only one ever displayed.
  const recover = canRetry(download) ? (
    <Button label="Retry" variant="secondary" isLoading={isRetrying} onClick={() => onRetry(download)} />
  ) : canUploadAgain(download) ? (
    <Button label="Upload again" variant="secondary" onClick={onUploadAgain} />
  ) : null;

  return (
    <div {...stylex.props(styles.row)}>
      <span {...stylex.props(styles.thumb)}>
        <MediaThumb
          src={thumbItem?.thumb ?? null}
          icon={KIND_ICON[download.kind]}
          width={isPoster ? '48px' : '72px'}
          height={isPoster ? '72px' : '48px'}
        />
      </span>
      <Text type="label" as="p" maxLines={2} wordBreak="break-word" xstyle={styles.title}>
        {download.name}
      </Text>
      <div {...stylex.props(styles.body)}>
        <ul {...stylex.props(styles.meta)} aria-label="Details">
          {meta.map(entry => (
            <li key={entry}>{entry}</li>
          ))}
        </ul>
        <OutcomeLine outcome={outcome} />
        {recover ? <span {...stylex.props(styles.recoverInline)}>{recover}</span> : null}
      </div>
      <div {...stylex.props(styles.actions)}>
        {recover ? <span {...stylex.props(styles.recoverEnd)}>{recover}</span> : null}
        <MoreMenu
          label={`More for ${download.name}`}
          alignment="end"
          items={[{label: 'Remove from list…', variant: 'destructive', onClick: () => onRemove(download)}]}
        />
      </div>
    </div>
  );
}

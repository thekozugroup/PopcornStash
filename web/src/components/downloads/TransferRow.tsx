/**
 * One row of the In progress list: torrents, web downloads and browser
 * uploads share it. One truncated name, one status, inline telemetry and a
 * real progress bar. The icon and trailing actions share the row's center.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {ProgressBar} from '@astryxdesign/core/ProgressBar';
import {Text} from '@astryxdesign/core/Text';
import {Badge} from '@astryxdesign/core/Badge';
import type {LucideIcon} from 'lucide-react';
import type {ReactNode} from 'react';
import {formatPercent} from '../../lib/format';

export const rowStyles = stylex.create({
  list: {
    margin: 0,
    padding: 0,
    listStyle: 'none',
  },
  item: {
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
});

const styles = stylex.create({
  row: {
    display: 'grid',
    gridTemplateColumns: 'auto minmax(0, 1fr) auto',
    columnGap: {default: 12, '@media (max-width: 559px)': 8},
    rowGap: 4,
    alignItems: 'center',
    paddingBlock: 12,
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
  },
  icon: {
    gridRow: {default: 'auto', '@media (max-width: 559px)': '1 / 4'},
    display: 'grid',
    placeItems: 'center',
    width: {default: 64, '@media (max-width: 559px)': 40},
    height: {default: 64, '@media (max-width: 559px)': 40},
    borderRadius: 'var(--popcorn-radius-glyph)',
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-icon-secondary'],
  },
  title: {
    minWidth: 0,
    overflow: 'hidden',
  },
  body: {
    display: {default: 'flex', '@media (max-width: 559px)': 'contents'},
    flexDirection: 'column',
    gap: 6,
    minWidth: 0,
  },
  stateLine: {
    gridColumn: {default: 'auto', '@media (max-width: 559px)': '2 / 4'},
    gridRow: {default: 'auto', '@media (max-width: 559px)': '2'},
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'wrap',
    columnGap: 12,
    rowGap: 4,
    minWidth: 0,
  },
  stateText: {
    minWidth: 0,
  },
  percent: {
    flexShrink: 0,
    fontVariantNumeric: 'tabular-nums',
  },
  titleLine: {
    gridColumn: {default: 'auto', '@media (max-width: 559px)': '2'},
    gridRow: {default: 'auto', '@media (max-width: 559px)': '1'},
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 8,
    minWidth: 0,
  },
  telemetry: {
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    flexWrap: 'wrap',
    fontVariantNumeric: 'tabular-nums',
  },
  meta: {
    display: 'flex',
    flexWrap: 'wrap',
    columnGap: 12,
    rowGap: 2,
    margin: 0,
    padding: 0,
    listStyle: 'none',
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    fontVariantNumeric: 'tabular-nums',
  },
  actions: {
    gridColumn: {default: 'auto', '@media (max-width: 559px)': '3'},
    gridRow: {default: 'auto', '@media (max-width: 559px)': '1'},
    display: 'flex',
    alignItems: 'center',
    gap: 4,
  },
  error: {
    color: colorVars['--color-error'],
  },
  progress: {
    gridColumn: {default: 'auto', '@media (max-width: 559px)': '2 / 4'},
    gridRow: {default: 'auto', '@media (max-width: 559px)': '3'},
  },
  note: {
    gridColumn: {default: 'auto', '@media (max-width: 559px)': '2 / 4'},
    gridRow: {default: 'auto', '@media (max-width: 559px)': '4'},
  },
});

export type TransferTone = 'active' | 'paused' | 'failed';

export interface TransferRowProps {
  icon: LucideIcon;
  name: string;
  /** One changing status, for example "Downloading". */
  stateText: ReactNode;
  /** 0..1, or null when the size is unknown. */
  progress: number | null;
  tone?: TransferTone;
  /** Short metadata entries: size, speed, time left, peers. */
  meta?: (string | null | undefined | false)[];
  telemetry?: ReactNode;
  /** Full file size, ETA or waiting explanation, available on hover. */
  detail?: string;
  /** Extra line under the bar, for example an error message. */
  note?: ReactNode;
  actions?: ReactNode;
}

const BAR_VARIANT = {active: 'accent', paused: 'neutral', failed: 'error'} as const;

export function TransferRow({icon: Icon, name, stateText, progress, tone = 'active', meta = [], telemetry, detail, note, actions}: TransferRowProps) {
  const entries = meta.filter((entry): entry is string => Boolean(entry));
  const hasSize = progress !== null && Number.isFinite(progress);
  return (
    <div {...stylex.props(styles.row)}>
      <span {...stylex.props(styles.icon)} aria-hidden="true">
        <Icon size={24} strokeWidth={1.75} />
      </span>
      <div {...stylex.props(styles.body)} title={detail}>
        <div {...stylex.props(styles.titleLine)} title={name}>
          <Text type="label" as="p" maxLines={1} wordBreak="break-all" xstyle={styles.title}>{name}</Text>
        </div>
        <div {...stylex.props(styles.stateLine)}>
          <Text type="supporting" color="secondary" xstyle={styles.stateText}>
            {stateText}
          </Text>
          {hasSize || telemetry ? <div {...stylex.props(styles.telemetry)}>
            {hasSize ? <span {...stylex.props(styles.percent)} aria-label={`${formatPercent(progress)} complete`}><Badge variant="neutral" label={formatPercent(progress)} /></span> : null}
            {telemetry}
          </div> : null}
          {entries.length > 0 ? <ul {...stylex.props(styles.meta)}>{entries.map(entry => <li key={entry}>{entry}</li>)}</ul> : null}
        </div>
        <div {...stylex.props(styles.progress)}><ProgressBar
          label={`${name} progress`}
          isLabelHidden
          value={hasSize ? Math.round((progress ?? 0) * 1000) / 10 : 0}
          max={100}
          isIndeterminate={!hasSize && tone === 'active'}
          variant={BAR_VARIANT[tone]}
        /></div>
        {note ? (
          <Text type="supporting" as="p" xstyle={[styles.note, tone === 'failed' && styles.error]}>
            {note}
          </Text>
        ) : null}
      </div>
      {actions ? <div {...stylex.props(styles.actions)}>{actions}</div> : null}
    </div>
  );
}

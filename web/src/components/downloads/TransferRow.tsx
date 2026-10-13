/**
 * One row of the In progress list: torrents, web downloads and browser
 * uploads share it. Name, kind and state in words, a real progress bar,
 * then size, speed, time left and peers in the metadata style. Actions sit
 * at the trailing edge: one quiet pause or resume control and a menu.
 *
 * Under 560px the progress block takes the full row width below the name.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {ProgressBar} from '@astryxdesign/core/ProgressBar';
import {Text} from '@astryxdesign/core/Text';
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
    gridTemplateAreas: {
      default: '"icon title actions" "icon body actions"',
      '@media (max-width: 559px)': '"title actions" "body body"',
    },
    gridTemplateColumns: {default: 'auto minmax(0, 1fr) auto', '@media (max-width: 559px)': 'minmax(0, 1fr) auto'},
    columnGap: {default: 16, '@media (max-width: 559px)': 8},
    rowGap: 6,
    alignItems: 'start',
    paddingBlock: 16,
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
  },
  icon: {
    gridArea: 'icon',
    display: {default: 'grid', '@media (max-width: 559px)': 'none'},
    placeItems: 'center',
    width: 40,
    height: 40,
    borderRadius: 10,
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-icon-secondary'],
  },
  title: {
    gridArea: 'title',
    minWidth: 0,
    alignSelf: {default: 'start', '@media (max-width: 559px)': 'center'},
    paddingTop: {default: 2, '@media (max-width: 559px)': 0},
    overflowWrap: 'anywhere',
  },
  body: {
    gridArea: 'body',
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    minWidth: 0,
    marginTop: {default: 0, '@media (max-width: 559px)': 6},
  },
  stateLine: {
    display: 'flex',
    alignItems: 'baseline',
    justifyContent: 'space-between',
    gap: 12,
    minWidth: 0,
  },
  stateText: {
    minWidth: 0,
  },
  percent: {
    flexShrink: 0,
    fontVariantNumeric: 'tabular-nums',
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
  actions: {
    gridArea: 'actions',
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    marginBlock: {default: -2, '@media (max-width: 559px)': 0},
  },
  error: {
    color: colorVars['--color-error'],
  },
});

export type TransferTone = 'active' | 'paused' | 'failed';

export interface TransferRowProps {
  icon: LucideIcon;
  name: string;
  /** "Torrent · Downloading". */
  stateText: ReactNode;
  /** 0..1, or null when the size is unknown. */
  progress: number | null;
  tone?: TransferTone;
  /** Short metadata entries: size, speed, time left, peers. */
  meta?: (string | null | undefined | false)[];
  /** Extra line under the bar, for example an error message. */
  note?: ReactNode;
  actions?: ReactNode;
}

const BAR_VARIANT = {active: 'accent', paused: 'neutral', failed: 'error'} as const;

export function TransferRow({icon: Icon, name, stateText, progress, tone = 'active', meta = [], note, actions}: TransferRowProps) {
  const entries = meta.filter((entry): entry is string => Boolean(entry));
  const hasSize = progress !== null && Number.isFinite(progress);
  return (
    <div {...stylex.props(styles.row)}>
      <span {...stylex.props(styles.icon)} aria-hidden="true">
        <Icon size={20} strokeWidth={1.75} />
      </span>
      <Text type="label" as="p" maxLines={2} wordBreak="break-word" xstyle={styles.title}>
        {name}
      </Text>
      <div {...stylex.props(styles.body)}>
        <div {...stylex.props(styles.stateLine)}>
          <Text type="supporting" color="secondary" xstyle={styles.stateText}>
            {stateText}
          </Text>
          {hasSize ? (
            <Text type="supporting" color="secondary" xstyle={styles.percent}>
              {formatPercent(progress)}
            </Text>
          ) : null}
        </div>
        <ProgressBar
          label={`${name} progress`}
          isLabelHidden
          value={hasSize ? Math.round((progress ?? 0) * 1000) / 10 : 0}
          max={100}
          isIndeterminate={!hasSize && tone === 'active'}
          variant={BAR_VARIANT[tone]}
        />
        {entries.length > 0 ? (
          <ul {...stylex.props(styles.meta)}>
            {entries.map(entry => (
              <li key={entry}>{entry}</li>
            ))}
          </ul>
        ) : null}
        {note ? (
          <Text type="supporting" as="p" xstyle={tone === 'failed' ? styles.error : undefined}>
            {note}
          </Text>
        ) : null}
      </div>
      {actions ? <div {...stylex.props(styles.actions)}>{actions}</div> : null}
    </div>
  );
}

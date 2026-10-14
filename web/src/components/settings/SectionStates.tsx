/**
 * Loading, error and empty states shared by the settings sections, plus the
 * flat panel that holds a list of rows.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {List} from '@astryxdesign/core/List';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Text} from '@astryxdesign/core/Text';
import {VisuallyHidden} from '@astryxdesign/core/VisuallyHidden';
import type {ReactNode} from 'react';
import {errorMessage} from '../../lib/errors';

const styles = stylex.create({
  panel: {
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 16,
    backgroundColor: colorVars['--color-background-surface'],
    minWidth: 0,
    overflow: 'hidden',
  },
  skeletonRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 16,
    paddingBlock: 18,
    paddingInline: 20,
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  skeletonText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    flex: '1 1 auto',
    minWidth: 0,
  },
  list: {
    paddingInline: 8,
    paddingBlock: 2,
  },
  wrap: {
    overflowWrap: 'anywhere',
  },
  keep: {
    whiteSpace: 'nowrap',
  },
  quiet: {
    paddingBlock: 20,
    paddingInline: 20,
    lineHeight: 1.55,
  },
});

/** Flat surface with a thin border that holds rows. */
export function RowsPanel({children, label}: {children: ReactNode; label?: string}) {
  return (
    <div {...stylex.props(styles.panel)} aria-label={label} role={label ? 'region' : undefined}>
      {children}
    </div>
  );
}

/** Rows (Astryx ListItem) with dividers inside a flat panel. */
export function PanelList({children, density = 'spacious'}: {children: ReactNode; density?: 'balanced' | 'spacious'}) {
  return (
    <RowsPanel>
      <List hasDividers density={density} xstyle={styles.list}>
        {children}
      </List>
    </RowsPanel>
  );
}

/** Skeleton rows in the shape of the list that is loading. */
export function LoadingRows({rows = 3, label = 'Loading'}: {rows?: number; label?: string}) {
  return (
    <div {...stylex.props(styles.panel)} aria-busy="true">
      <VisuallyHidden>
        <span role="status">{label}</span>
      </VisuallyHidden>
      {Array.from({length: rows}, (_, index) => (
        <div key={index} {...stylex.props(styles.skeletonRow)} aria-hidden="true">
          <div {...stylex.props(styles.skeletonText)}>
            <Skeleton width={index % 2 === 0 ? '42%' : '34%'} height={14} index={index} />
            <Skeleton width={index % 2 === 0 ? '64%' : '52%'} height={12} index={index} />
          </div>
          <Skeleton width={72} height={32} radius={2} index={index} />
        </div>
      ))}
    </div>
  );
}

/** Says what failed and offers a retry. */
export function LoadError({title, error, onRetry}: {title: string; error: unknown; onRetry?: () => void}) {
  return (
    <Banner
      status="error"
      title={title}
      description={errorMessage(error, 'PopcornStash did not answer. Try again in a moment.')}
      collapsible={false}
      endContent={onRetry ? <Button label="Try again" variant="secondary" onClick={onRetry} /> : undefined}
    />
  );
}

/**
 * A row description that wraps instead of truncating (Astryx truncates plain
 * string descriptions to one line, which hides paths and explanations on
 * phones).
 */
export function Wrap({children}: {children: ReactNode}) {
  return <span {...stylex.props(styles.wrap)}>{children}</span>;
}

/**
 * A short run inside a wrapping description that must not split, such as a date:
 * "Added Oct 14, 2026." wraps before the date or after it, never inside it.
 */
export function KeepTogether({children}: {children: ReactNode}) {
  return <span {...stylex.props(styles.keep)}>{children}</span>;
}

/** A quiet sentence inside a panel when a list has nothing in it yet. */
export function EmptyRows({children}: {children: ReactNode}) {
  return (
    <div {...stylex.props(styles.panel)}>
      <Text as="p" display="block" type="supporting" color="secondary" xstyle={styles.quiet}>
        {children}
      </Text>
    </div>
  );
}

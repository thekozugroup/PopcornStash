/**
 * Quiet one-line notices on Home: files waiting for review in Inbox and
 * downloads still in progress. Each row is a single link to where the work
 * happens. Renders nothing when there is nothing to say.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, fontWeightVars} from '@astryxdesign/core/theme/tokens.stylex';
import {ArrowDownToLine, ChevronRight, Inbox, type LucideIcon} from 'lucide-react';
import {Link} from 'react-router';
import {formatCount} from '../../lib/format';

const styles = stylex.create({
  list: {
    listStyle: 'none',
    margin: 0,
    padding: 0,
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 16,
    backgroundColor: colorVars['--color-background-surface'],
    overflow: 'hidden',
  },
  item: {
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  row: {
    display: 'grid',
    gridTemplateColumns: '20px minmax(0, 1fr) auto',
    alignItems: 'center',
    columnGap: 14,
    minHeight: 56,
    paddingBlock: 10,
    paddingInline: 18,
    color: colorVars['--color-text-primary'],
    textDecoration: 'none',
    backgroundColor: {default: 'transparent', ':hover': colorVars['--color-overlay-hover']},
    outlineStyle: {default: 'none', ':focus-visible': 'solid'},
    outlineWidth: 2,
    outlineColor: colorVars['--color-accent'],
    outlineOffset: -2,
    borderRadius: 'inherit',
  },
  icon: {
    color: colorVars['--color-icon-secondary'],
    display: 'flex',
  },
  text: {
    display: 'flex',
    flexWrap: 'wrap',
    columnGap: 6,
    fontSize: 15,
    lineHeight: 1.45,
    minWidth: 0,
  },
  strong: {
    fontWeight: fontWeightVars['--font-weight-semibold'],
  },
  muted: {
    color: colorVars['--color-text-secondary'],
  },
  action: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 2,
    color: colorVars['--color-text-secondary'],
    fontSize: 14,
    fontWeight: fontWeightVars['--font-weight-medium'],
    whiteSpace: 'nowrap',
  },
  actionLabel: {
    display: {default: 'inline', '@media (max-width: 520px)': 'none'},
  },
});

interface Notice {
  key: string;
  href: string;
  icon: LucideIcon;
  lead: string;
  rest: string;
  action: string;
}

export function HomeNotices({needsReview, downloadsInProgress}: {needsReview: number; downloadsInProgress: number}) {
  const notices: Notice[] = [];
  if (needsReview > 0) {
    notices.push({
      key: 'inbox',
      href: '/inbox',
      icon: Inbox,
      lead: needsReview === 1 ? '1 file needs a quick look' : `${formatCount(needsReview, 'file')} need a quick look`,
      rest: 'before they are sorted.',
      action: 'Review in Inbox',
    });
  }
  if (downloadsInProgress > 0) {
    notices.push({
      key: 'downloads',
      href: '/downloads',
      icon: ArrowDownToLine,
      lead: `${formatCount(downloadsInProgress, 'download')} in progress.`,
      rest: 'New files appear here when they finish.',
      action: 'Open Downloads',
    });
  }
  if (notices.length === 0) return null;

  return (
    <ul aria-label="Needs attention" {...stylex.props(styles.list)}>
      {notices.map(notice => {
        const Icon = notice.icon;
        return (
          <li key={notice.key} {...stylex.props(styles.item)}>
            <Link to={notice.href} {...stylex.props(styles.row)}>
              <span aria-hidden="true" {...stylex.props(styles.icon)}>
                <Icon size={20} strokeWidth={1.75} />
              </span>
              <span {...stylex.props(styles.text)}>
                <span {...stylex.props(styles.strong)}>{notice.lead}</span>
                <span {...stylex.props(styles.muted)}>{notice.rest}</span>
              </span>
              <span {...stylex.props(styles.action)}>
                <span {...stylex.props(styles.actionLabel)}>{notice.action}</span>
                <ChevronRight size={18} strokeWidth={1.75} aria-hidden="true" />
              </span>
            </Link>
          </li>
        );
      })}
    </ul>
  );
}

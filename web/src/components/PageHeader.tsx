/**
 * Page heading block: the page's h1, a one-line subtitle, and at most one
 * primary action plus quiet secondary actions on the trailing edge (or
 * under the subtitle when the action is the page's only way forward, as on
 * "not found" pages). Also sets the document title.
 */
import * as stylex from '@stylexjs/stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {useEffect, type ReactNode} from 'react';

const styles = stylex.create({
  header: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    columnGap: 24,
    rowGap: 16,
    marginBottom: 32,
  },
  copy: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
    minWidth: 0,
    flex: '1 1 320px',
  },
  // "pretty" keeps a lone word off the last line without narrowing the block
  // (balance would, and would break paths like /family-photos early).
  subtitle: {
    maxWidth: '58ch',
    lineHeight: 1.6,
    textWrap: 'pretty',
  },
  actionsBelow: {
    marginTop: 14,
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 8,
  },
});

export interface PageHeaderProps {
  title: string;
  subtitle?: ReactNode;
  /** Trailing actions. Keep to one primary action per view. */
  actions?: ReactNode;
  /** Overrides the document title (defaults to `title`). */
  documentTitle?: string;
  /** Content between the subtitle and the actions column, such as a status tag. */
  children?: ReactNode;
  /** Put the actions under the subtitle instead of on the trailing edge. */
  isActionBelow?: boolean;
}

export function useDocumentTitle(title: string | null | undefined) {
  useEffect(() => {
    document.title = title ? `${title} · PopcornStash` : 'PopcornStash';
  }, [title]);
}

export function PageHeader({title, subtitle, actions, documentTitle, children, isActionBelow = false}: PageHeaderProps) {
  useDocumentTitle(documentTitle ?? title);
  const actionBlock = actions ? (
    <div {...stylex.props(styles.actions, isActionBelow && styles.actionsBelow)}>{actions}</div>
  ) : null;
  return (
    <div {...stylex.props(styles.header)}>
      <div {...stylex.props(styles.copy)}>
        <Heading level={1} textWrap="balance">
          {title}
        </Heading>
        {subtitle ? (
          <Text as="p" type="body" color="secondary" xstyle={styles.subtitle}>
            {subtitle}
          </Text>
        ) : null}
        {children}
        {isActionBelow ? actionBlock : null}
      </div>
      {isActionBelow ? null : actionBlock}
    </div>
  );
}

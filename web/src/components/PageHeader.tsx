/**
 * Page heading block: the page's h1, a one-line subtitle, and at most one
 * primary action plus quiet secondary actions on the trailing edge.
 * Also sets the document title.
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
  subtitle: {
    maxWidth: '58ch',
    lineHeight: 1.6,
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
}

export function useDocumentTitle(title: string | null | undefined) {
  useEffect(() => {
    document.title = title ? `${title} · PopcornStash` : 'PopcornStash';
  }, [title]);
}

export function PageHeader({title, subtitle, actions, documentTitle, children}: PageHeaderProps) {
  useDocumentTitle(documentTitle ?? title);
  return (
    <header {...stylex.props(styles.header)}>
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
      </div>
      {actions ? <div {...stylex.props(styles.actions)}>{actions}</div> : null}
    </header>
  );
}

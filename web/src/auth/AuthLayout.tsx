/**
 * Centered single-column layout for setup, sign-in, invites and recovery,
 * matching Popcorn SR's sign-in page: brand, heading, one task, a quiet note.
 */
import * as stylex from '@stylexjs/stylex';
import {Text} from '@astryxdesign/core/Text';
import type {ReactNode} from 'react';
import {Brand} from '../components/Brand';
import {useDocumentTitle} from '../components/PageHeader';
import {Heading} from '@astryxdesign/core/Heading';

const styles = stylex.create({
  page: {
    minHeight: '100dvh',
    display: 'grid',
    placeItems: 'center',
    paddingBlock: {default: 48, '@media (max-width: 520px)': 32},
    paddingInline: {default: 24, '@media (max-width: 520px)': 16},
  },
  column: {
    width: '100%',
    maxWidth: 440,
    display: 'flex',
    flexDirection: 'column',
  },
  brand: {
    marginBottom: {default: 44, '@media (max-width: 520px)': 32},
  },
  intro: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    marginBottom: 32,
  },
  subtitle: {
    lineHeight: 1.6,
  },
  body: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
  },
  footnote: {
    marginTop: 32,
    lineHeight: 1.6,
  },
});

export interface AuthLayoutProps {
  title: string;
  subtitle?: ReactNode;
  children?: ReactNode;
  footnote?: ReactNode;
}

export function AuthLayout({title, subtitle, children, footnote}: AuthLayoutProps) {
  useDocumentTitle(title);
  return (
    <main id="main" tabIndex={-1} {...stylex.props(styles.page)}>
      <div {...stylex.props(styles.column)}>
        <div {...stylex.props(styles.brand)}>
          <Brand href={null} />
        </div>
        <div {...stylex.props(styles.intro)}>
          <Heading level={1} textWrap="balance">
            {title}
          </Heading>
          {subtitle ? (
            <Text as="p" color="secondary" xstyle={styles.subtitle}>
              {subtitle}
            </Text>
          ) : null}
        </div>
        {children ? <div {...stylex.props(styles.body)}>{children}</div> : null}
        {footnote ? (
          <Text as="p" type="supporting" color="secondary" xstyle={styles.footnote}>
            {footnote}
          </Text>
        ) : null}
      </div>
    </main>
  );
}

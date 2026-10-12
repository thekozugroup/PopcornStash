/**
 * Heading for one settings section (an h2 under the Settings h1).
 */
import * as stylex from '@stylexjs/stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import type {ReactNode} from 'react';
import {useDocumentTitle} from '../../components/PageHeader';

const styles = stylex.create({
  header: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    columnGap: 24,
    rowGap: 12,
    marginBottom: 24,
  },
  copy: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    minWidth: 0,
    flex: '1 1 320px',
  },
  subtitle: {
    maxWidth: '68ch',
    lineHeight: 1.6,
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
});

export interface SettingsSectionHeaderProps {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
}

export function SettingsSectionHeader({title, subtitle, actions}: SettingsSectionHeaderProps) {
  useDocumentTitle(`${title} settings`);
  return (
    <header {...stylex.props(styles.header)}>
      <div {...stylex.props(styles.copy)}>
        <Heading level={2}>{title}</Heading>
        {subtitle ? (
          <Text as="p" color="secondary" xstyle={styles.subtitle}>
            {subtitle}
          </Text>
        ) : null}
      </div>
      {actions ? <div {...stylex.props(styles.actions)}>{actions}</div> : null}
    </header>
  );
}

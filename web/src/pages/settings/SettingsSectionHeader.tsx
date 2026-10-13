/**
 * Opening of one settings section: its h2 under the Settings h1, and one
 * sentence on what the section covers. The selected tab already shows the
 * section's name right above, so the heading is for the document outline
 * (screen readers, heading navigation) and the sentence is what people see.
 * A plain block, not a <header>: inside the app's main region a <header>
 * would be read as a second page banner.
 */
import * as stylex from '@stylexjs/stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import type {ReactNode} from 'react';
import {useDocumentTitle} from '../../components/PageHeader';

const styles = stylex.create({
  header: {
    marginBottom: 24,
  },
  heading: {
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
  subtitle: {
    maxWidth: '68ch',
    lineHeight: 1.6,
    textWrap: 'pretty',
  },
});

export interface SettingsSectionHeaderProps {
  title: string;
  subtitle?: ReactNode;
}

export function SettingsSectionHeader({title, subtitle}: SettingsSectionHeaderProps) {
  useDocumentTitle(`${title} settings`);
  return (
    <div {...stylex.props(styles.header)}>
      <div {...stylex.props(styles.heading)}>
        <Heading level={2}>{title}</Heading>
      </div>
      {subtitle ? (
        <Text as="p" color="secondary" xstyle={styles.subtitle}>
          {subtitle}
        </Text>
      ) : null}
    </div>
  );
}

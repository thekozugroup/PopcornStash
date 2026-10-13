/**
 * One group of related settings: a short heading and description beside (on
 * wide screens) or above (on narrow screens) the controls. Groups are
 * separated by a thin divider, never by cards stacked on cards.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {useId, type ReactNode} from 'react';

const WIDE = '@media (min-width: 1100px)';

const styles = stylex.create({
  group: {
    display: 'grid',
    gridTemplateColumns: {default: 'minmax(0, 1fr)', [WIDE]: 'minmax(200px, 260px) minmax(0, 1fr)'},
    columnGap: 48,
    rowGap: 20,
    paddingBlock: {default: 32, '@media (max-width: 520px)': 28},
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
    paddingBlockStart: {default: 32, ':first-child': 8},
    minWidth: 0,
  },
  intro: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    minWidth: 0,
  },
  description: {
    lineHeight: 1.55,
    maxWidth: '62ch',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
    marginTop: 10,
  },
  stacked: {
    gridTemplateColumns: 'minmax(0, 1fr)',
  },
  body: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    minWidth: 0,
  },
});

export interface SettingsGroupProps {
  title: string;
  description?: ReactNode;
  /** Actions under the description (for example "Create a token"). */
  actions?: ReactNode;
  children?: ReactNode;
  /** Put the heading above the content even on wide screens (wide tables). */
  isStacked?: boolean;
}

export function SettingsGroup({title, description, actions, children, isStacked = false}: SettingsGroupProps) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} {...stylex.props(styles.group, isStacked && styles.stacked)}>
      <div {...stylex.props(styles.intro)}>
        <Heading level={3} id={headingId}>
          {title}
        </Heading>
        {description ? (
          <Text as="p" type="supporting" color="secondary" xstyle={styles.description}>
            {description}
          </Text>
        ) : null}
        {actions ? <div {...stylex.props(styles.actions)}>{actions}</div> : null}
      </div>
      {children ? <div {...stylex.props(styles.body)}>{children}</div> : null}
    </section>
  );
}

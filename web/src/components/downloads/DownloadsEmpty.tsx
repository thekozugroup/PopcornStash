/**
 * Empty Downloads: explains the path every file takes, so the first
 * download is not a mystery. The owner sees the folder names; family
 * members see the same path in plain words.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {ArrowDown, ArrowRight} from 'lucide-react';
import {Fragment} from 'react';

const styles = stylex.create({
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 28,
    paddingBlock: {default: 36, '@media (max-width: 559px)': 24},
    paddingInline: {default: 32, '@media (max-width: 559px)': 16},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 'var(--radius-container)',
    backgroundColor: colorVars['--color-background-surface'],
  },
  intro: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    maxWidth: '60ch',
  },
  lead: {
    lineHeight: 1.6,
  },
  flow: {
    display: 'grid',
    gridTemplateColumns: {
      default: 'minmax(0, 1fr) auto minmax(0, 1fr) auto minmax(0, 1fr)',
      '@media (max-width: 719px)': 'minmax(0, 1fr)',
    },
    alignItems: 'start',
    columnGap: 24,
    rowGap: 12,
    margin: 0,
    padding: 0,
    // One flat path under a thin divider, not boxes inside the panel.
    paddingTop: 24,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
    listStyle: 'none',
  },
  step: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    minWidth: 0,
  },
  folder: {
    overflowWrap: 'anywhere',
    color: colorVars['--color-text-primary'],
    fontWeight: 500,
  },
  // Beside the step titles on wide screens; between the steps when they stack.
  arrow: {
    display: 'grid',
    placeItems: {default: 'center', '@media (max-width: 719px)': 'start'},
    alignSelf: 'start',
    paddingTop: {default: 1, '@media (max-width: 719px)': 0},
    color: colorVars['--color-icon-secondary'],
  },
  arrowWide: {
    display: {default: 'grid', '@media (max-width: 719px)': 'none'},
  },
  arrowNarrow: {
    display: {default: 'none', '@media (max-width: 719px)': 'grid'},
  },
});

interface Step {
  title: string;
  folder: string | null;
  text: string;
}

const OWNER_STEPS: Step[] = [
  {title: 'Arriving', folder: 'Downloads/In Progress', text: 'Torrents, web links and uploads land here while they come in.'},
  {title: 'Finished', folder: 'Downloads/Completed', text: 'Complete files wait here for a moment before sorting.'},
  {title: 'Sorted', folder: 'A library’s folder, or Other', text: 'Each file moves to the library that takes its kind. Anything unclear waits in Inbox.'},
];

const MEMBER_STEPS: Step[] = [
  {title: 'Arriving', folder: null, text: 'Torrents, web links and uploads show up under In progress while they come in.'},
  {title: 'Finished', folder: null, text: 'Complete files move under Finished and wait a moment before sorting.'},
  {title: 'Sorted', folder: null, text: 'Each file moves to the library that takes its kind. Anything unclear waits in Inbox.'},
];

export function DownloadsEmpty({isOwner}: {isOwner: boolean}) {
  const steps = isOwner ? OWNER_STEPS : MEMBER_STEPS;
  return (
    <section aria-labelledby="downloads-empty-title" {...stylex.props(styles.panel)}>
      <div {...stylex.props(styles.intro)}>
        <Heading level={2} id="downloads-empty-title">
          Nothing downloading yet
        </Heading>
        <Text as="p" color="secondary" xstyle={styles.lead}>
          Every file takes the same path into your library.
        </Text>
      </div>
      <ol {...stylex.props(styles.flow)} aria-label="How files come in">
        {steps.map((step, index) => (
          <Fragment key={step.title}>
            <li {...stylex.props(styles.step)}>
              <Text type="label">{step.title}</Text>
              {step.folder ? (
                <Text type="supporting" color="secondary" xstyle={styles.folder}>
                  {step.folder}
                </Text>
              ) : null}
              <Text type="supporting" color="secondary">
                {step.text}
              </Text>
            </li>
            {index < steps.length - 1 ? (
              <li aria-hidden="true" {...stylex.props(styles.arrow)}>
                <span {...stylex.props(styles.arrowWide)}>
                  <ArrowRight size={18} strokeWidth={1.75} />
                </span>
                <span {...stylex.props(styles.arrowNarrow)}>
                  <ArrowDown size={18} strokeWidth={1.75} />
                </span>
              </li>
            ) : null}
          </Fragment>
        ))}
      </ol>
    </section>
  );
}

/**
 * First run on Home: the library is empty, so explain the three ways files
 * come in and offer one primary action. Says where files go in plain words.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, fontWeightVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {FolderInput, Link2, Upload, type LucideIcon} from 'lucide-react';

const styles = stylex.create({
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 24,
    maxWidth: 760,
    paddingBlock: {default: 32, '@media (max-width: 520px)': 24},
    paddingInline: {default: 32, '@media (max-width: 520px)': 20},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 16,
    backgroundColor: colorVars['--color-background-surface'],
  },
  intro: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  lead: {
    maxWidth: '60ch',
    lineHeight: 1.6,
  },
  ways: {
    listStyle: 'none',
    margin: 0,
    padding: 0,
  },
  way: {
    display: 'grid',
    gridTemplateColumns: '40px minmax(0, 1fr)',
    columnGap: 16,
    alignItems: 'start',
    paddingBlock: 16,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  icon: {
    display: 'grid',
    placeItems: 'center',
    width: 40,
    height: 40,
    borderRadius: 10,
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-icon-primary'],
  },
  wayText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    paddingTop: 1,
  },
  wayTitle: {
    fontSize: 15,
    lineHeight: 1.45,
    fontWeight: fontWeightVars['--font-weight-semibold'],
  },
  wayBody: {
    lineHeight: 1.55,
    maxWidth: '62ch',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 12,
  },
});

const WAYS: {icon: LucideIcon; title: string; body: string}[] = [
  {
    icon: Upload,
    title: 'Drop files anywhere in this window',
    body: 'They upload to Downloads first, then move into the library that takes them.',
  },
  {
    icon: FolderInput,
    title: 'Copy files into the Inbox folder on the server',
    body: 'Inbox sits inside your PopcornStash folder. Files there are sorted once they finish copying.',
  },
  {
    icon: Link2,
    title: 'Add a link or magnet in Downloads',
    body: 'Paste a web link, a magnet link or a .torrent file. Torrents only run through the VPN.',
  },
];

export function FirstRunPanel() {
  return (
    <section aria-labelledby="first-run-title" {...stylex.props(styles.panel)}>
      <div {...stylex.props(styles.intro)}>
        <Heading level={2} id="first-run-title">
          Bring in your first videos and photos
        </Heading>
        <Text as="p" color="secondary" xstyle={styles.lead}>
          PopcornStash sorts everything for you. Anything it is unsure about waits in Inbox for a quick look.
        </Text>
      </div>
      <ul {...stylex.props(styles.ways)}>
        {WAYS.map(way => {
          const Icon = way.icon;
          return (
            <li key={way.title} {...stylex.props(styles.way)}>
              <span aria-hidden="true" {...stylex.props(styles.icon)}>
                <Icon size={20} strokeWidth={1.75} />
              </span>
              <span {...stylex.props(styles.wayText)}>
                <span {...stylex.props(styles.wayTitle)}>{way.title}</span>
                <Text as="span" type="supporting" color="secondary" xstyle={styles.wayBody}>
                  {way.body}
                </Text>
              </span>
            </li>
          );
        })}
      </ul>
      <div {...stylex.props(styles.actions)}>
        <Button label="Add a link" variant="primary" href="/downloads" />
      </div>
    </section>
  );
}

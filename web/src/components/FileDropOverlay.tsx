/**
 * Full-window overlay shown while files are dragged over the app. It says
 * where dropped files go before anything moves: they upload to Downloads,
 * then the sorter files them into the library.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {Upload} from 'lucide-react';

const fadeIn = stylex.keyframes({
  from: {opacity: 0},
  to: {opacity: 1},
});

const styles = stylex.create({
  overlay: {
    position: 'fixed',
    inset: 0,
    zIndex: 50,
    display: 'grid',
    placeItems: 'center',
    padding: 16,
    backgroundColor: colorVars['--color-overlay'],
    pointerEvents: 'none',
    animationName: {default: fadeIn, '@media (prefers-reduced-motion: reduce)': 'none'},
    animationDuration: '160ms',
    animationTimingFunction: 'ease-out',
  },
  card: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 12,
    width: '100%',
    maxWidth: 420,
    paddingBlock: {default: 36, '@media (max-width: 520px)': 28},
    paddingInline: {default: 32, '@media (max-width: 520px)': 20},
    textAlign: 'center',
    borderRadius: 'var(--radius-container)',
    borderWidth: 2,
    borderStyle: 'dashed',
    borderColor: colorVars['--color-accent'],
    backgroundColor: colorVars['--color-background-surface'],
  },
  icon: {
    display: 'grid',
    placeItems: 'center',
    width: 48,
    height: 48,
    borderRadius: 'var(--popcorn-radius-glyph)',
    backgroundColor: colorVars['--color-accent-muted'],
    color: colorVars['--color-text-primary'],
  },
  copy: {
    textWrap: 'balance',
    lineHeight: 1.6,
  },
});

export function FileDropOverlay({isVisible}: {isVisible: boolean}) {
  if (!isVisible) return null;
  return (
    <div {...stylex.props(styles.overlay)} role="presentation">
      <div {...stylex.props(styles.card)} role="status" aria-live="polite">
        <span {...stylex.props(styles.icon)} aria-hidden="true">
          <Upload size={22} strokeWidth={1.75} />
        </span>
        <Heading level={2}>Drop to upload</Heading>
        <Text as="p" color="secondary" xstyle={styles.copy}>
          Files upload to Downloads, then move into your library.
        </Text>
        <Text as="p" type="supporting" color="secondary" xstyle={styles.copy}>
          Folders upload everything inside. A{'\u00a0'}.torrent file starts a torrent instead.
        </Text>
      </div>
    </div>
  );
}

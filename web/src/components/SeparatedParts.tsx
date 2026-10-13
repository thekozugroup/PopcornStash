/**
 * A short status line made of parts ("Protected · WireGuard · lantern-vpn")
 * that wraps only between parts, never inside one, and never leaves a
 * separator dangling at either end of a line.
 *
 * Each part carries the separator in front of it, so the dot always travels
 * with the part it introduces. The row is shifted back by one separator's
 * width inside a clipping box: whichever part starts a line (the first one,
 * or one that wrapped) has its separator in the clipped strip, so a wrapped
 * line starts with the name itself. Screen readers get the plain sentence.
 */
import * as stylex from '@stylexjs/stylex';
import {VisuallyHidden} from '@astryxdesign/core/VisuallyHidden';

/** Room for " · " (the dot with a space each side). */
const SEPARATOR_WIDTH = '0.8em';

const styles = stylex.create({
  clip: {
    display: 'block',
    overflow: 'hidden',
  },
  row: {
    display: 'flex',
    flexWrap: 'wrap',
    marginInlineStart: `calc(-1 * ${SEPARATOR_WIDTH})`,
  },
  part: {
    minWidth: 0,
    overflowWrap: 'break-word',
  },
  separator: {
    display: 'inline-block',
    width: SEPARATOR_WIDTH,
    textAlign: 'center',
  },
});

export const PART_SEPARATOR = ' · ';

export function SeparatedParts({parts}: {parts: string[]}) {
  return (
    <>
      <VisuallyHidden>{parts.join(PART_SEPARATOR)}</VisuallyHidden>
      <span aria-hidden="true" {...stylex.props(styles.clip)}>
        <span {...stylex.props(styles.row)}>
          {parts.map((part, index) => (
            // Parts are positional; a name can repeat ("WireGuard · WireGuard").
            <span key={`${index}:${part}`} {...stylex.props(styles.part)}>
              <span {...stylex.props(styles.separator)}>·</span>
              {part}
            </span>
          ))}
        </span>
      </span>
    </>
  );
}

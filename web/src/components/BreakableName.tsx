/**
 * Text without spaces that wraps at its own separators.
 *
 * A file or release name such as "Saltwater.Choir.2022.1080p.WEB.webm" has no
 * spaces, so a narrow row would otherwise cut it mid-word ("…1080p.WE / B.webm").
 * A web address or a path does the same ("…live-at-the / -pier" with
 * `word-break: break-all`). A <wbr> after each separator gives the browser
 * better places to wrap; the text and what is copied or read aloud stay
 * exactly the same. A long run without separators still wraps through the
 * element's `overflow-wrap: break-word` instead of overflowing.
 */
import * as stylex from '@stylexjs/stylex';
import {Fragment} from 'react';

const styles = stylex.create({
  // One unit in a sentence: the whole address moves to the next line when it does not
  // fit where it starts, and breaks at its separators only when it is longer than a line.
  unit: {
    display: 'inline-block',
    maxWidth: '100%',
    overflowWrap: 'break-word',
  },
});

/** Names: after each dot, underscore and hyphen, keeping it on the part before the break. */
const AFTER_NAME_SEPARATOR = /(?<=[._-])(?=.)/u;

/**
 * Addresses and paths: after each slash, dot, hyphen, underscore, colon, `?`, `&`, `=` and
 * `#`, but never inside a run of them, so "https://" stays whole and a line never starts
 * with a separator.
 */
const AFTER_ADDRESS_SEPARATOR = /(?<=[/._\-:?&=#])(?=[^/._\-:?&=#])/u;

/** Where `text` may wrap: the pieces between break opportunities, in order. */
export function breakableParts(text: string, kind: 'name' | 'address'): string[] {
  return text.split(kind === 'name' ? AFTER_NAME_SEPARATOR : AFTER_ADDRESS_SEPARATOR);
}

function Breakable({text, kind}: {text: string; kind: 'name' | 'address'}) {
  return (
    <>
      {breakableParts(text, kind).map((part, index) => (
        // Parts are positional slices of one string, so the index is a stable key.
        <Fragment key={index}>
          {index > 0 ? <wbr /> : null}
          {part}
        </Fragment>
      ))}
    </>
  );
}

/** A file or release name that wraps after its dots, underscores and hyphens. */
export function BreakableName({name}: {name: string}) {
  return <Breakable text={name} kind="name" />;
}

/**
 * A web address or folder path that wraps after its separators ("https://video.example.org/
 * paper-moons/"). `isUnit` keeps a short address inside a sentence whole: it moves to the
 * next line instead of leaving "http://" at the end of this one.
 */
export function BreakableAddress({address, isUnit = false}: {address: string; isUnit?: boolean}) {
  if (!isUnit) return <Breakable text={address} kind="address" />;
  return (
    <span {...stylex.props(styles.unit)}>
      <Breakable text={address} kind="address" />
    </span>
  );
}

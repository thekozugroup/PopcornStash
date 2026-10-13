/**
 * A file or release name that wraps at its own separators. Names such as
 * "Saltwater.Choir.2022.1080p.WEB.webm" have no spaces, so a narrow row would
 * otherwise cut them mid-word ("…1080p.WE / B.webm"). A <wbr> after each dot,
 * underscore and hyphen gives the browser better places to wrap; the text and
 * what is copied or read aloud stay exactly the name. A long run without
 * separators still wraps through the row's `overflow-wrap: break-word`
 * instead of overflowing.
 */
import {Fragment} from 'react';

/** Splits after each separator, keeping it on the part before the break. */
const AFTER_SEPARATOR = /(?<=[._-])(?=.)/u;

export function BreakableName({name}: {name: string}) {
  const parts = name.split(AFTER_SEPARATOR);
  return (
    <>
      {parts.map((part, index) => (
        // Parts are positional slices of one string, so the index is a stable key.
        <Fragment key={index}>
          {index > 0 ? <wbr /> : null}
          {part}
        </Fragment>
      ))}
    </>
  );
}

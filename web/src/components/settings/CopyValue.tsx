/**
 * A value shown once (a device token, an invite link) with a Copy button.
 * The value is selectable text too, for browsers that block the clipboard.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Icon} from '@astryxdesign/core/Icon';
import {useClipboard} from '@astryxdesign/core/hooks';
import {Text} from '@astryxdesign/core/Text';
import {useId} from 'react';

const styles = stylex.create({
  wrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    minWidth: 0,
  },
  row: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'stretch',
    gap: 8,
  },
  value: {
    flex: '1 1 240px',
    minWidth: 0,
    display: 'flex',
    alignItems: 'center',
    minHeight: 44,
    paddingBlock: 10,
    paddingInline: 14,
    borderRadius: 8,
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-text-primary'],
    fontFamily: typographyVars['--font-family-code'],
    fontSize: 14,
    lineHeight: 1.5,
    overflowWrap: 'anywhere',
    userSelect: 'all',
  },
});

export interface CopyValueProps {
  label: string;
  value: string;
  /** Button text, for example "Copy link". */
  copyLabel?: string;
}

export function CopyValue({label, value, copyLabel = 'Copy'}: CopyValueProps) {
  const labelId = useId();
  const {copy, isCopied} = useClipboard({announce: 'Copied'});
  return (
    <div {...stylex.props(styles.wrap)}>
      <Text as="span" type="label" id={labelId}>
        {label}
      </Text>
      <div {...stylex.props(styles.row)}>
        <output aria-labelledby={labelId} {...stylex.props(styles.value)}>
          {value}
        </output>
        <Button
          label={isCopied ? 'Copied' : copyLabel}
          variant="secondary"
          icon={<Icon icon={isCopied ? 'check' : 'copy'} />}
          onClick={() => void copy(value)}
        />
      </div>
    </div>
  );
}

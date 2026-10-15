/**
 * The library icon picker (DESIGN.md, Icon picker): an Astryx
 * ToggleButtonGroup of icon-only buttons, one per name in the curated list,
 * in the server's order. Each is a 44px square with the 20px glyph; the
 * chosen one sits on the selected navigation fill with an ink glyph and a
 * 1px ink inner ring, so it reads as chosen without blue. Every button's
 * label is the icon's plain name, shown as its tooltip and read by screen
 * readers. Arrow keys move between buttons; Space or Enter chooses.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Text} from '@astryxdesign/core/Text';
import {ToggleButton, ToggleButtonGroup} from '@astryxdesign/core/ToggleButton';
import type {KeyboardEvent} from 'react';
import type {LibraryIcon} from '../../api/types';
import {libraryIcon, libraryIconLabel} from '../../lib/libraryIcons';

const styles = stylex.create({
  field: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    minWidth: 0,
  },
  group: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 4,
    // Eight to a row in the 400px form column; as many as fit on a phone.
    maxWidth: 380,
  },
  button: {
    width: 44,
    height: 44,
    minWidth: 44,
    paddingInline: 0,
    borderRadius: 'var(--radius-element)',
    color: colorVars['--color-icon-secondary'],
  },
  chosen: {
    backgroundColor: colorVars['--color-neutral'],
    color: colorVars['--color-text-primary'],
    boxShadow: `inset 0 0 0 1px ${colorVars['--color-text-primary']}`,
  },
  error: {
    color: colorVars['--color-error'],
  },
});

const ARROWS: Record<string, number> = {ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1};

export interface IconPickerProps {
  icons: LibraryIcon[];
  value: LibraryIcon;
  onChange: (icon: LibraryIcon) => void;
  isDisabled?: boolean;
  error?: string;
}

export function IconPicker({icons, value, onChange, isDisabled = false, error}: IconPickerProps) {
  // Arrow keys move focus along the buttons (they wrap in rows, so up and left go back).
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = ARROWS[event.key];
    if (!step) return;
    const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not([disabled])'));
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (index < 0) return;
    event.preventDefault();
    buttons[(index + step + buttons.length) % buttons.length]?.focus();
  };

  return (
    <div {...stylex.props(styles.field)}>
      <Text type="label">Icon</Text>
      <div onKeyDown={onKeyDown}>
        <ToggleButtonGroup
          label="Icon"
          type="single"
          value={value}
          onChange={next => {
            // Pressing the chosen icon again keeps it: a library always has one.
            if (typeof next === 'string' && next) onChange(next);
          }}
          isDisabled={isDisabled}
          xstyle={styles.group}>
          {icons.map(name => {
            const Icon = libraryIcon(name);
            return (
              <ToggleButton
                key={name}
                value={name}
                label={libraryIconLabel(name)}
                isIconOnly
                icon={<Icon size={20} strokeWidth={1.5} aria-hidden="true" />}
                xstyle={[styles.button, name === value && styles.chosen]}
              />
            );
          })}
        </ToggleButtonGroup>
      </div>
      {error ? (
        <Text type="supporting" xstyle={styles.error} role="alert">
          {error}
        </Text>
      ) : null}
    </div>
  );
}

/**
 * One Inbox item: the file, why it waits (when that adds something), and
 * where it goes: a Library selector that lists only the libraries this
 * person can see that fit the file, then Other files, with the suggestion
 * preselected. The selector is the row's one statement of where the file
 * goes; Accept sends exactly what it shows. Edit opens an inline form with
 * the details and a live preview; Ignore leaves the file where it is.
 *
 * Only the next item in the list (`isNext`) shows its main action in the
 * accent colour, so the page keeps one primary action however many files
 * wait. Closing the edit form returns focus to the button that opened it.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {FileQuestion, Film, ImageIcon, Info} from 'lucide-react';
import {useEffect, useRef} from 'react';
import type {InboxDecision, ItemDetail, Library} from '../../api/types';
import {formatBytes, formatDuration} from '../../lib/format';
import {BreakableName} from '../BreakableName';
import {MediaThumb} from '../downloads/MediaThumb';
import {InboxEditForm} from './InboxEditForm';
import {targetSelectorOptions, type MoveFieldErrors} from './MoveFields';
import {baseName, folderOf, needsDetails, resolveTarget, waitingReason, type Target, type TargetValue} from './places';

const styles = stylex.create({
  row: {
    display: 'grid',
    gridTemplateColumns: 'auto minmax(0, 1fr)',
    gridTemplateAreas: {
      default: '"thumb heading" "thumb body"',
      '@media (max-width: 719px)': '"thumb heading" "body body"',
    },
    alignItems: 'start',
    columnGap: {default: 20, '@media (max-width: 559px)': 14},
    rowGap: {default: 10, '@media (max-width: 719px)': 14},
    paddingBlock: {default: 20, '@media (max-width: 559px)': 16},
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
  },
  rowEditing: {
    gridTemplateAreas: {
      default: '"thumb heading" "thumb body" ". form"',
      '@media (max-width: 719px)': '"thumb heading" "body body" "form form"',
    },
  },
  thumb: {
    gridArea: 'thumb',
  },
  main: {
    gridArea: 'body',
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    minWidth: 0,
  },
  heading: {
    gridArea: 'heading',
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    minWidth: 0,
    alignSelf: {default: 'start', '@media (max-width: 719px)': 'center'},
  },
  meta: {
    display: 'flex',
    flexWrap: 'wrap',
    columnGap: 16,
    rowGap: 2,
    margin: 0,
    padding: 0,
    listStyle: 'none',
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    fontVariantNumeric: 'tabular-nums',
  },
  path: {
    overflowWrap: 'anywhere',
  },
  line: {
    display: 'flex',
    alignItems: 'flex-start',
    gap: 8,
    margin: 0,
    minWidth: 0,
    fontSize: 14,
    lineHeight: 1.45,
  },
  lineIcon: {
    display: 'inline-flex',
    flexShrink: 0,
    marginTop: 2,
    color: colorVars['--color-icon-secondary'],
  },
  reason: {
    color: colorVars['--color-text-secondary'],
    overflowWrap: 'anywhere',
  },
  // Where it goes, then what to do: one wrapping row on wide screens.
  decide: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    columnGap: 12,
    rowGap: 10,
  },
  picker: {
    flex: {default: '0 1 280px', '@media (max-width: 559px)': '1 1 100%'},
    minWidth: 0,
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
  form: {
    gridArea: 'form',
    marginTop: 6,
  },
  contents: {
    display: 'contents',
  },
});

export interface InboxRowProps {
  item: ItemDetail;
  libraries: Library[];
  /** Where this person may send the file. */
  choices: Target[];
  /** What the row's selector shows now. */
  target: TargetValue | null;
  onTargetChange: (item: ItemDetail, target: TargetValue | null) => void;
  isOwner: boolean;
  /** The first undecided item: its main action is the page's primary action. */
  isNext: boolean;
  /** Ignored before: it has no Ignore, only Accept and Edit. */
  isIgnored: boolean;
  isEditing: boolean;
  busy: 'accept' | 'ignore' | 'edit' | null;
  error: string | null;
  fieldErrors: MoveFieldErrors;
  onClearServerErrors: () => void;
  isLocked: boolean;
  onAccept: (item: ItemDetail, target: TargetValue) => void;
  onIgnore: (item: ItemDetail) => void;
  onEdit: (item: ItemDetail | null) => void;
  onSubmitEdit: (item: ItemDetail, decision: InboxDecision, destination: string, target: TargetValue) => void;
}

export function InboxRow({
  item,
  libraries,
  choices,
  target,
  onTargetChange,
  isOwner,
  isNext,
  isIgnored,
  isEditing,
  busy,
  error,
  fieldErrors,
  onClearServerErrors,
  isLocked,
  onAccept,
  onIgnore,
  onEdit,
  onSubmitEdit,
}: InboxRowProps) {
  const fileName = baseName(item.rel_path);
  const isPhoto = item.kind === 'photo';
  const reason = isIgnored ? null : waitingReason(item.suggestion, isOwner);
  const meta = [formatBytes(item.size), item.duration_s ? formatDuration(item.duration_s) : null].filter(
    (entry): entry is string => Boolean(entry),
  );
  const folder = folderOf(item.rel_path);
  const editRef = useRef<HTMLSpanElement | null>(null);
  const wasEditing = useRef(isEditing);
  const resolved = resolveTarget(target, libraries);
  // A series needs a show and an episode; without them Accept would be refused, so it opens the form.
  const wantsDetails = resolved ? needsDetails(resolved, item) : false;

  // Back from the edit form (cancelled): focus the button that opened it.
  useEffect(() => {
    if (wasEditing.current && !isEditing) editRef.current?.querySelector('button')?.focus();
    wasEditing.current = isEditing;
  }, [isEditing]);

  return (
    <article aria-label={fileName} data-inbox-id={item.id} {...stylex.props(styles.row, isEditing && styles.rowEditing)}>
      <span {...stylex.props(styles.thumb)}>
        <MediaThumb
          src={item.thumb}
          icon={isPhoto ? ImageIcon : item.kind === 'video' ? Film : FileQuestion}
          width={isPhoto ? 'clamp(64px, 18vw, 96px)' : 'clamp(96px, 24vw, 160px)'}
          height={isPhoto ? 'clamp(64px, 18vw, 96px)' : 'calc(clamp(96px, 24vw, 160px) * 9 / 16)'}
          iconSize={22}
        />
      </span>
      <div {...stylex.props(styles.heading)}>
        <Text type="label" as="p" maxLines={2} wordBreak="break-word">
          <BreakableName name={fileName} />
        </Text>
        <ul {...stylex.props(styles.meta)} aria-label="Details">
          {meta.map(entry => (
            <li key={entry}>{entry}</li>
          ))}
          {isOwner && folder ? <li {...stylex.props(styles.path)}>{`Now in ${folder}`}</li> : null}
        </ul>
      </div>
      <div {...stylex.props(styles.main)}>
        {reason ? (
          <p {...stylex.props(styles.line)}>
            <span {...stylex.props(styles.lineIcon)} aria-hidden="true">
              <Info size={16} strokeWidth={1.75} />
            </span>
            <span {...stylex.props(styles.reason)}>{reason}</span>
          </p>
        ) : null}

        {!isEditing ? (
          <div {...stylex.props(styles.decide)}>
            <div {...stylex.props(styles.picker)}>
              <Selector
                label="Library"
                isLabelHidden
                options={targetSelectorOptions(choices)}
                value={target ?? ''}
                placeholder="Choose a library"
                onChange={value => onTargetChange(item, (value || null) as TargetValue | null)}
                isDisabled={isLocked}
                // One place to go: shown, not offered.
                isReadOnly={choices.length <= 1}
                width="100%"
              />
            </div>
            <div data-inbox-actions="" {...stylex.props(styles.actions)}>
              <Button
                label={wantsDetails ? 'Add details' : 'Accept'}
                variant={isNext ? 'primary' : 'secondary'}
                isLoading={busy === 'accept'}
                isDisabled={(isLocked && busy !== 'accept') || target === null}
                tooltip={target === null ? 'Choose a library first' : undefined}
                onClick={() => {
                  if (!target) return;
                  if (wantsDetails) onEdit(item);
                  else onAccept(item, target);
                }}
              />
              <span ref={editRef} {...stylex.props(styles.contents)}>
                <Button label="Edit" variant="secondary" isDisabled={isLocked} onClick={() => onEdit(item)} />
              </span>
              {!isIgnored ? (
                <Button
                  label="Ignore"
                  variant="ghost"
                  tooltip="Leaves the file where it is and takes it out of Inbox"
                  isLoading={busy === 'ignore'}
                  isDisabled={isLocked && busy !== 'ignore'}
                  onClick={() => onIgnore(item)}
                />
              ) : null}
            </div>
          </div>
        ) : null}
      </div>

      {isEditing ? (
        <div {...stylex.props(styles.form)}>
          <InboxEditForm
            item={item}
            libraries={libraries}
            choices={choices}
            target={target}
            isOwner={isOwner}
            isBusy={busy === 'edit'}
            error={error}
            fieldErrors={fieldErrors}
            onClearServerErrors={onClearServerErrors}
            onCancel={() => onEdit(null)}
            onSubmit={(decision, destination, chosen) => onSubmitEdit(item, decision, destination, chosen)}
          />
        </div>
      ) : null}
    </article>
  );
}

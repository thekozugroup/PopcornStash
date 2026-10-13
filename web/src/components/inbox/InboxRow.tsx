/**
 * One Inbox item: the file, why it was not sorted, and where it will go.
 * Accept moves it to the suggested place; Edit opens an inline form with a
 * live destination preview; Ignore leaves the file where it is and takes it
 * out of Inbox.
 *
 * Only the next item in the list (`isNext`) shows its main action in the
 * accent colour, so the page keeps one primary action however many files
 * wait. Closing the edit form returns focus to the button that opened it.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Text} from '@astryxdesign/core/Text';
import {FileQuestion, Film, FolderInput, ImageIcon, Info} from 'lucide-react';
import {useEffect, useRef} from 'react';
import type {InboxDecision, ItemDetail} from '../../api/types';
import {formatBytes, formatDuration} from '../../lib/format';
import {MediaThumb} from '../downloads/MediaThumb';
import {InboxEditForm} from './InboxEditForm';
import {baseName, describeDestination, folderOf} from './places';

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
    gap: 10,
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
  name: {
    overflowWrap: 'anywhere',
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
  destination: {
    color: colorVars['--color-text-primary'],
    overflowWrap: 'anywhere',
  },
  place: {
    fontWeight: 600,
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
    marginTop: 4,
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
  isOwner: boolean;
  /** The first undecided item: its main action is the page's primary action. */
  isNext: boolean;
  isEditing: boolean;
  busy: 'accept' | 'ignore' | 'edit' | null;
  error: string | null;
  isLocked: boolean;
  onAccept: (item: ItemDetail) => void;
  onIgnore: (item: ItemDetail) => void;
  onEdit: (item: ItemDetail | null) => void;
  onSubmitEdit: (item: ItemDetail, decision: InboxDecision, destination: string) => void;
}

export function InboxRow({
  item,
  isOwner,
  isNext,
  isEditing,
  busy,
  error,
  isLocked,
  onAccept,
  onIgnore,
  onEdit,
  onSubmitEdit,
}: InboxRowProps) {
  const fileName = baseName(item.rel_path);
  const isPhoto = item.kind === 'photo';
  const suggestion = item.suggestion;
  const meta = [
    formatBytes(item.size),
    item.duration_s ? formatDuration(item.duration_s) : null,
  ].filter((entry): entry is string => Boolean(entry));
  const folder = folderOf(item.rel_path);
  const editRef = useRef<HTMLSpanElement | null>(null);
  const wasEditing = useRef(isEditing);

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
        <Text type="label" as="p" maxLines={2} wordBreak="break-word" xstyle={styles.name}>
          {fileName}
        </Text>
        <ul {...stylex.props(styles.meta)} aria-label="Details">
          {meta.map(entry => (
            <li key={entry}>{entry}</li>
          ))}
          {isOwner && folder ? <li {...stylex.props(styles.path)}>{`Now in ${folder}`}</li> : null}
        </ul>
      </div>
      <div {...stylex.props(styles.main)}>

        <p {...stylex.props(styles.line)}>
          <span {...stylex.props(styles.lineIcon)} aria-hidden="true">
            <Info size={16} strokeWidth={1.75} />
          </span>
          <span {...stylex.props(styles.reason)}>
            {suggestion?.reason ?? 'PopcornStash could not tell what this file is.'}
          </span>
        </p>

        {suggestion && !isEditing ? (
          <p {...stylex.props(styles.line)}>
            <span {...stylex.props(styles.lineIcon)} aria-hidden="true">
              <FolderInput size={16} strokeWidth={1.75} />
            </span>
            <span {...stylex.props(styles.destination)}>
              Will move to <span {...stylex.props(styles.place)}>{describeDestination(suggestion.destination)}</span>
            </span>
          </p>
        ) : null}

        {!isEditing ? (
          <div data-inbox-actions="" {...stylex.props(styles.actions)}>
            {suggestion ? (
              <Button
                label="Accept"
                variant={isNext ? 'primary' : 'secondary'}
                isLoading={busy === 'accept'}
                isDisabled={isLocked && busy !== 'accept'}
                onClick={() => onAccept(item)}
              />
            ) : null}
            <span ref={editRef} {...stylex.props(styles.contents)}>
              <Button
                label={suggestion ? 'Edit' : 'Choose where it goes'}
                variant={!suggestion && isNext ? 'primary' : 'secondary'}
                isDisabled={isLocked}
                onClick={() => onEdit(item)}
              />
            </span>
            <Button
              label="Ignore"
              variant="ghost"
              tooltip="Leaves the file where it is and takes it out of Inbox"
              isLoading={busy === 'ignore'}
              isDisabled={isLocked && busy !== 'ignore'}
              onClick={() => onIgnore(item)}
            />
          </div>
        ) : null}
      </div>

      {isEditing ? (
        <div {...stylex.props(styles.form)}>
          <InboxEditForm
            item={item}
            isBusy={busy === 'edit'}
            error={error}
            onCancel={() => onEdit(null)}
            onSubmit={(decision, destination) => onSubmitEdit(item, decision, destination)}
          />
        </div>
      ) : null}
    </article>
  );
}

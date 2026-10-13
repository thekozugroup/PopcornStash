/**
 * Inline edit for one Inbox item: choose what the file is, fill in the
 * fields that kind of file needs, and see where it will go before it moves.
 * The preview updates on every keystroke. Opening the form moves focus to
 * its first field.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {FolderInput} from 'lucide-react';
import {useEffect, useId, useRef, useState, type FormEvent} from 'react';
import type {InboxDecision, ItemCategory, ItemDetail} from '../../api/types';
import {CATEGORY_OPTIONS, draftFromItem, previewDestination, type DestinationDraft} from './places';

const styles = stylex.create({
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    paddingBlock: 20,
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
    borderRadius: 10,
    backgroundColor: colorVars['--color-background-muted'],
  },
  grid: {
    display: 'grid',
    gridTemplateColumns: {
      default: 'minmax(0, 2fr) minmax(0, 1fr) minmax(0, 1fr)',
      '@media (max-width: 719px)': 'minmax(0, 1fr) minmax(0, 1fr)',
    },
    gap: 16,
    alignItems: 'start',
  },
  wide: {
    gridColumn: {default: 'span 1', '@media (max-width: 719px)': '1 / -1'},
  },
  full: {
    gridColumn: '1 / -1',
  },
  preview: {
    display: 'flex',
    alignItems: 'flex-start',
    gap: 8,
    margin: 0,
    paddingTop: 16,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border-emphasized'],
  },
  previewIcon: {
    display: 'inline-flex',
    flexShrink: 0,
    marginTop: 2,
    color: colorVars['--color-icon-secondary'],
  },
  previewText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    minWidth: 0,
    overflowWrap: 'anywhere',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
});

type FieldErrors = Partial<Record<'title' | 'year' | 'show' | 'season' | 'episode', string>>;

function parseNumber(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  return /^\d+$/.test(trimmed) ? Number(trimmed) : Number.NaN;
}

export interface InboxEditFormProps {
  item: ItemDetail;
  isBusy: boolean;
  error: string | null;
  onCancel: () => void;
  onSubmit: (decision: InboxDecision, destination: string) => void;
}

export function InboxEditForm({item, isBusy, error, onCancel, onSubmit}: InboxEditFormProps) {
  const initial = draftFromItem(item);
  const [category, setCategory] = useState<ItemCategory>(initial.category);
  const [title, setTitle] = useState(initial.title);
  const [year, setYear] = useState(initial.year ? String(initial.year) : '');
  const [show, setShow] = useState(initial.showName);
  const [season, setSeason] = useState(initial.season !== null ? String(initial.season) : '');
  const [episode, setEpisode] = useState(initial.episode !== null ? String(initial.episode) : '');
  const [errors, setErrors] = useState<FieldErrors>({});
  const previewId = useId();
  const formRef = useRef<HTMLFormElement | null>(null);

  useEffect(() => {
    formRef.current?.querySelector<HTMLElement>('[role="combobox"], button, input')?.focus();
  }, []);

  const yearValue = parseNumber(year);
  const seasonValue = parseNumber(season);
  const episodeValue = parseNumber(episode);
  const draft: DestinationDraft = {
    category,
    title,
    year: yearValue !== null && Number.isFinite(yearValue) ? yearValue : null,
    showName: show,
    season: seasonValue !== null && Number.isFinite(seasonValue) ? seasonValue : null,
    episode: episodeValue !== null && Number.isFinite(episodeValue) ? episodeValue : null,
  };
  const preview = previewDestination(draft, item);

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const next: FieldErrors = {};
    if (category === 'movie') {
      if (!title.trim()) next.title = 'Enter the movie title.';
      if (yearValue !== null && (!Number.isFinite(yearValue) || yearValue < 1900 || yearValue > 2099)) {
        next.year = 'Use a four-digit year, like 1995.';
      }
    }
    if (category === 'episode') {
      if (!show.trim()) next.show = 'Enter the show name.';
      if (seasonValue === null || !Number.isFinite(seasonValue)) next.season = 'Enter the season number.';
      if (episodeValue === null || !Number.isFinite(episodeValue)) next.episode = 'Enter the episode number.';
    }
    setErrors(next);
    if (Object.keys(next).length > 0) return;

    const decision: InboxDecision = {action: 'accept', category};
    if (category === 'movie') {
      decision.title = title.trim();
      decision.year = draft.year;
    } else if (category === 'episode') {
      decision.show_name = show.trim();
      decision.season = draft.season;
      decision.episode = draft.episode;
    }
    onSubmit(decision, preview.place);
  };

  const status = (message: string | undefined) => (message ? {type: 'error' as const, message} : undefined);

  return (
    <form ref={formRef} onSubmit={submit} noValidate aria-label={`Edit where ${item.title} goes`} {...stylex.props(styles.form)}>
      <div {...stylex.props(styles.grid)}>
        <div {...stylex.props(styles.full)}>
          <Selector
            label="What is this file?"
            options={CATEGORY_OPTIONS.map(option => ({value: option.value, label: option.label}))}
            value={category}
            onChange={value => {
              if (value) setCategory(value as ItemCategory);
              setErrors({});
            }}
            isDisabled={isBusy}
            width="100%"
          />
        </div>

        {category === 'movie' ? (
          <>
            <div {...stylex.props(styles.wide)}>
              <TextInput
                label="Title"
                value={title}
                onChange={setTitle}
                isRequired
                isDisabled={isBusy}
                status={status(errors.title)}
                statusVariant="detached"
                width="100%"
              />
            </div>
            <TextInput
              label="Year"
              value={year}
              onChange={setYear}
              isOptional
              placeholder="1995"
              isDisabled={isBusy}
              status={status(errors.year)}
              statusVariant="detached"
              width="100%"
            />
          </>
        ) : null}

        {category === 'episode' ? (
          <>
            <div {...stylex.props(styles.wide)}>
              <TextInput
                label="Show"
                value={show}
                onChange={setShow}
                isRequired
                isDisabled={isBusy}
                status={status(errors.show)}
                statusVariant="detached"
                width="100%"
              />
            </div>
            <TextInput
              label="Season"
              value={season}
              onChange={setSeason}
              isRequired
              isDisabled={isBusy}
              status={status(errors.season)}
              statusVariant="detached"
              width="100%"
            />
            <TextInput
              label="Episode"
              value={episode}
              onChange={setEpisode}
              isRequired
              isDisabled={isBusy}
              status={status(errors.episode)}
              statusVariant="detached"
              width="100%"
            />
          </>
        ) : null}
      </div>

      <p {...stylex.props(styles.preview)} id={previewId} role="status" aria-live="polite">
        <span {...stylex.props(styles.previewIcon)} aria-hidden="true">
          <FolderInput size={16} strokeWidth={1.75} />
        </span>
        <span {...stylex.props(styles.previewText)}>
          <Text type="label">{`Will move to ${preview.place}`}</Text>
          {preview.hint ? (
            <Text type="supporting" color="secondary">
              {preview.hint}
            </Text>
          ) : preview.fileName ? (
            <Text type="supporting" color="secondary">
              {`Renamed to ${preview.fileName}`}
            </Text>
          ) : null}
        </span>
      </p>

      {error ? <Banner status="error" title="The file did not move" description={error} collapsible={false} /> : null}

      <div {...stylex.props(styles.actions)}>
        <Button type="submit" variant="primary" label="Move it there" isLoading={isBusy} />
        <Button variant="secondary" label="Cancel" onClick={onCancel} isDisabled={isBusy} />
      </div>
    </form>
  );
}

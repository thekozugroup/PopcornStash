/**
 * The fields of "where does this file go": the Library selector (only the
 * libraries this person can see that fit the file, then Other files), the
 * details that library needs (a title for a movie; show, season and episode
 * for a series), and the live preview, which is the one place that says the
 * destination in words ("Will move to Kids"). Used by the Inbox edit form and
 * the Organize dialog; the caller owns the buttons and the request.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {FileText, FolderInput} from 'lucide-react';
import type {InboxDecision, ItemCategory, ItemDetail, Library} from '../../api/types';
import {libraryIcon} from '../../lib/libraryIcons';
import {
  categoryAt,
  previewMove,
  resolveTarget,
  targetName,
  targetValue,
  type MoveDraft,
  type Target,
  type TargetValue,
} from './places';

const styles = stylex.create({
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
  folder: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: '0.8125rem',
    color: colorVars['--color-text-secondary'],
    overflowWrap: 'anywhere',
  },
});

export type MoveFieldErrors = Partial<Record<'target' | 'title' | 'year' | 'show' | 'season' | 'episode', string>>;

export interface MoveFormState {
  target: TargetValue | null;
  category: ItemCategory;
  title: string;
  year: string;
  show: string;
  season: string;
  episode: string;
}

function parseNumber(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  return /^\d+$/.test(trimmed) ? Number(trimmed) : Number.NaN;
}

function finite(value: number | null): number | null {
  return value !== null && Number.isFinite(value) ? value : null;
}

export function draftOf(form: MoveFormState): MoveDraft {
  return {
    category: form.category,
    title: form.title,
    year: finite(parseNumber(form.year)),
    showName: form.show,
    season: finite(parseNumber(form.season)),
    episode: finite(parseNumber(form.episode)),
  };
}

/** Checks the form; returns the request fields, or the errors to show. */
export function validateMove(
  form: MoveFormState,
  libraries: Library[],
): {errors: MoveFieldErrors; target: Target | null; body: Omit<InboxDecision, 'action'> | null} {
  const errors: MoveFieldErrors = {};
  const target = resolveTarget(form.target, libraries);
  if (!target) {
    errors.target = 'Choose a library for this file.';
    return {errors, target: null, body: null};
  }
  const draft = draftOf(form);
  const category = categoryAt(target, draft);
  const year = parseNumber(form.year);
  const season = parseNumber(form.season);
  const episode = parseNumber(form.episode);
  if (category === 'movie') {
    if (!form.title.trim()) errors.title = 'Enter the movie title.';
    if (year !== null && (!Number.isFinite(year) || year < 1900 || year > 2099)) errors.year = 'Use a four-digit year, like 1995.';
  }
  if (category === 'episode') {
    if (!form.show.trim()) errors.show = 'Enter the show name.';
    if (season !== null && !Number.isFinite(season)) errors.season = 'Enter the season as a number.';
    if (episode === null || !Number.isFinite(episode)) errors.episode = 'Enter the episode number.';
  }
  if (Object.keys(errors).length > 0) return {errors, target, body: null};

  const body: Omit<InboxDecision, 'action'> = {place: target.place};
  if (target.library) body.library_id = target.library.id;
  if (category === 'movie') {
    body.category = 'movie';
    body.title = form.title.trim();
    body.year = draft.year;
  } else if (category === 'episode') {
    body.category = 'episode';
    body.show_name = form.show.trim();
    body.season = draft.season ?? 1;
    body.episode = draft.episode;
  } else if (category === 'home_video') {
    body.category = 'home_video';
  }
  return {errors, target, body};
}

/** Maps a server error's `field` onto the form, when it names one. */
export function fieldOfServerError(field: string | null): keyof MoveFieldErrors | null {
  switch (field) {
    case 'library_id':
    case 'place':
      return 'target';
    case 'title':
      return 'title';
    case 'year':
      return 'year';
    case 'show_name':
      return 'show';
    case 'season':
      return 'season';
    case 'episode':
      return 'episode';
    default:
      return null;
  }
}

/** Selector options: the libraries (each with its icon), then Other files after a divider. */
export function targetSelectorOptions(choices: Target[]) {
  const hasOther = choices.some(choice => choice.place === 'other_files');
  const libraryChoices = choices.filter(choice => choice.library);
  return [
    ...libraryChoices.map(choice => {
      const Icon = libraryIcon(choice.library?.icon);
      return {value: targetValue(choice), label: targetName(choice), icon: <Icon size={16} strokeWidth={1.5} />};
    }),
    ...(hasOther && libraryChoices.length > 0 ? [{type: 'divider' as const}] : []),
    ...(hasOther ? [{value: 'other_files', label: 'Other files', icon: <FileText size={16} strokeWidth={1.5} />}] : []),
  ];
}

export interface MoveFieldsProps {
  item: ItemDetail;
  form: MoveFormState;
  onChange: (form: MoveFormState) => void;
  choices: Target[];
  libraries: Library[];
  errors: MoveFieldErrors;
  /** Owners see the folder a file lands in. */
  showFolder: boolean;
  isDisabled: boolean;
}

export function MoveFields({item, form, onChange, choices, libraries, errors, showFolder, isDisabled}: MoveFieldsProps) {
  const target = resolveTarget(form.target, libraries);
  const draft = draftOf(form);
  const category = target ? categoryAt(target, draft) : null;
  const preview = target ? previewMove(target, draft, item, showFolder) : null;
  const status = (message: string | undefined) => (message ? {type: 'error' as const, message} : undefined);
  const set = (change: Partial<MoveFormState>) => onChange({...form, ...change});
  const isOneOffs = target?.library?.structure === 'one_offs';
  const options = targetSelectorOptions(choices);

  return (
    <>
      <div {...stylex.props(styles.grid)}>
        <div {...stylex.props(styles.full)}>
          <Selector
            label="Library"
            options={options}
            value={form.target ?? ''}
            placeholder="Choose a library"
            onChange={value => set({target: (value || null) as TargetValue | null})}
            isDisabled={isDisabled}
            // One place to go (a file someone else added, or a document): shown, not offered.
            isReadOnly={choices.length <= 1}
            status={status(errors.target)}
            statusVariant="detached"
            width="100%"
          />
        </div>

        {isOneOffs ? (
          <div {...stylex.props(styles.full)}>
            <Selector
              label="What is this file?"
              options={[
                {value: 'movie', label: 'A movie'},
                {value: 'home_video', label: 'A camera video'},
              ]}
              value={category === 'home_video' ? 'home_video' : 'movie'}
              onChange={value => set({category: value as ItemCategory})}
              isDisabled={isDisabled}
              width="100%"
            />
          </div>
        ) : null}

        {category === 'movie' ? (
          <>
            <div {...stylex.props(styles.wide)}>
              <TextInput
                label="Title"
                value={form.title}
                onChange={title => set({title})}
                isRequired
                isDisabled={isDisabled}
                status={status(errors.title)}
                statusVariant="detached"
                width="100%"
              />
            </div>
            <TextInput
              label="Year"
              value={form.year}
              onChange={year => set({year})}
              isOptional
              placeholder="1995"
              isDisabled={isDisabled}
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
                value={form.show}
                onChange={show => set({show})}
                isRequired
                isDisabled={isDisabled}
                status={status(errors.show)}
                statusVariant="detached"
                width="100%"
              />
            </div>
            <TextInput
              label="Season"
              value={form.season}
              onChange={season => set({season})}
              placeholder="1"
              isDisabled={isDisabled}
              status={status(errors.season)}
              statusVariant="detached"
              width="100%"
            />
            <TextInput
              label="Episode"
              value={form.episode}
              onChange={episode => set({episode})}
              isRequired
              isDisabled={isDisabled}
              status={status(errors.episode)}
              statusVariant="detached"
              width="100%"
            />
          </>
        ) : null}
      </div>

      <p {...stylex.props(styles.preview)} role="status" aria-live="polite">
        <span {...stylex.props(styles.previewIcon)} aria-hidden="true">
          <FolderInput size={16} strokeWidth={1.75} />
        </span>
        <span {...stylex.props(styles.previewText)}>
          <Text type="label">{preview ? preview.title : 'Choose where it goes'}</Text>
          {preview?.folder ? <span {...stylex.props(styles.folder)}>{preview.folder}</span> : null}
          {preview?.hint ? (
            <Text type="supporting" color="secondary">
              {preview.hint}
            </Text>
          ) : preview?.fileName ? (
            <Text type="supporting" color="secondary">
              {`Renamed to ${preview.fileName}`}
            </Text>
          ) : null}
        </span>
      </p>
    </>
  );
}

/** The form's first values: the target the row shows, and the suggestion's details. */
export function initialMoveForm(item: ItemDetail, target: TargetValue | null): MoveFormState {
  const source = item.suggestion ?? item;
  return {
    target,
    category: source.category,
    title: source.title ?? '',
    year: source.year ? String(source.year) : '',
    show: source.show_name ?? '',
    season: source.season !== null && source.season !== undefined ? String(source.season) : '',
    episode: source.episode !== null && source.episode !== undefined ? String(source.episode) : '',
  };
}

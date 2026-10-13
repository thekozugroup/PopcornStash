/**
 * Search, sort and the favourites filter above a library grid. Search waits
 * for a short pause in typing before it asks the server.
 */
import * as stylex from '@stylexjs/stylex';
import {Selector} from '@astryxdesign/core/Selector';
import {TextInput} from '@astryxdesign/core/TextInput';
import {ToggleButton} from '@astryxdesign/core/ToggleButton';
import {Search, Star} from 'lucide-react';
import {useEffect, useState} from 'react';

const styles = stylex.create({
  bar: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 8,
    marginBottom: 28,
  },
  search: {
    flex: '1 1 260px',
    maxWidth: {default: 400, '@media (max-width: 640px)': 'none'},
    minWidth: 0,
  },
  controls: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    flexWrap: 'wrap',
  },
  starFilled: {
    fill: 'currentColor',
  },
});

const SEARCH_DELAY_MS = 300;

export interface SortOption<T extends string> {
  value: T;
  label: string;
}

export interface LibraryToolbarProps<T extends string> {
  searchLabel: string;
  query: string;
  onQueryChange: (query: string) => void;
  sort: T;
  sortOptions: SortOption<T>[];
  onSortChange: (sort: T) => void;
  /** Omit to hide the favourites filter. */
  isFavoritesOnly?: boolean;
  onFavoritesOnlyChange?: (value: boolean) => void;
}

export function LibraryToolbar<T extends string>({
  searchLabel,
  query,
  onQueryChange,
  sort,
  sortOptions,
  onSortChange,
  isFavoritesOnly,
  onFavoritesOnlyChange,
}: LibraryToolbarProps<T>) {
  const [draft, setDraft] = useState(query);
  const [sent, setSent] = useState(query);
  const [seenQuery, setSeenQuery] = useState(query);

  // The address changed from outside (back button, a cleared filter).
  if (query !== seenQuery) {
    setSeenQuery(query);
    if (query !== sent) {
      setDraft(query);
      setSent(query);
    }
  }

  useEffect(() => {
    if (draft.trim() === sent.trim()) return;
    const timer = setTimeout(() => {
      setSent(draft);
      onQueryChange(draft.trim());
    }, SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [draft, sent, onQueryChange]);

  return (
    <div role="search" {...stylex.props(styles.bar)}>
      <div {...stylex.props(styles.search)}>
        <TextInput
          label={searchLabel}
          isLabelHidden
          placeholder={searchLabel}
          value={draft}
          onChange={setDraft}
          onEnter={() => {
            setSent(draft);
            onQueryChange(draft.trim());
          }}
          startIcon={Search}
          hasClear
          width="100%"
          autoComplete="off"
        />
      </div>
      <div {...stylex.props(styles.controls)}>
        <Selector
          label="Sort by"
          isLabelHidden
          options={sortOptions.map(option => ({value: option.value, label: option.label}))}
          value={sort}
          onChange={value => {
            const next = sortOptions.find(option => option.value === value);
            if (next) onSortChange(next.value);
          }}
        />
        {onFavoritesOnlyChange ? (
          <ToggleButton
            label="Favourites"
            isPressed={Boolean(isFavoritesOnly)}
            onPressedChange={pressed => onFavoritesOnlyChange(pressed)}
            icon={<Star size={16} strokeWidth={1.75} aria-hidden="true" />}
            pressedIcon={<Star size={16} strokeWidth={1.75} aria-hidden="true" {...stylex.props(styles.starFilled)} />}>
            Favourites
          </ToggleButton>
        ) : null}
      </div>
    </div>
  );
}

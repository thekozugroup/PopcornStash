/**
 * One library category as a searchable, sortable grid with infinite scroll
 * (Movies, Home Videos, Other). Search, sort and the favourites filter live
 * in the address so the back button returns to the same view.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Text} from '@astryxdesign/core/Text';
import {type LucideIcon} from 'lucide-react';
import {useCallback, type ReactNode} from 'react';
import {useSearchParams} from 'react-router';
import type {Item, ItemSort, SortOrder} from '../../api/types';
import {watchedFraction} from '../../lib/format';
import {EmptyPanel} from '../EmptyPanel';
import {MediaGrid, MediaGridSkeleton} from '../MediaGrid';
import {MediaTile, type MediaShape} from '../MediaTile';
import {PageHeader} from '../PageHeader';
import {LibraryToolbar, type SortOption} from './LibraryToolbar';
import {LoadError} from './LoadError';
import {displayTitle, itemHref, itemMeta, thumbSources} from './libraryFormat';
import {useInfiniteScroll} from './useInfiniteScroll';
import {usePagedItems} from './usePagedItems';

const styles = stylex.create({
  more: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 12,
    marginTop: 28,
  },
  sentinel: {
    height: 1,
  },
  moreSkeleton: {
    marginTop: 28,
  },
});

/** The order each sort reads most naturally in. */
const SORT_ORDER: Record<ItemSort, SortOrder> = {
  added: 'desc',
  title: 'asc',
  taken: 'desc',
  size: 'desc',
};

export interface CategoryLibraryConfig {
  category: 'movie' | 'home_video' | 'other';
  title: string;
  subtitle: string;
  shape: MediaShape;
  /** Plural noun in sentences: "movies", "home videos", "files". */
  noun: string;
  sortOptions: SortOption<ItemSort>[];
  defaultSort: ItemSort;
  /** Empty library: the same quiet icon as the category's tile placeholder. */
  empty: {title: string; description: string; icon: LucideIcon};
  /** Placeholder icon for an item without a thumbnail. */
  placeholderFor?: (item: Item) => LucideIcon | undefined;
  /** Primary action when the category is empty. */
  emptyAction?: ReactNode;
}

export function CategoryLibraryPage({config}: {config: CategoryLibraryConfig}) {
  const [params, setParams] = useSearchParams();
  const q = params.get('q') ?? '';
  const sortParam = params.get('sort') as ItemSort | null;
  const sort = config.sortOptions.some(option => option.value === sortParam) ? (sortParam as ItemSort) : config.defaultSort;
  const favoritesOnly = params.get('fav') === '1';

  const update = useCallback(
    (change: Record<string, string | null>) => {
      setParams(
        current => {
          const next = new URLSearchParams(current);
          for (const [key, value] of Object.entries(change)) {
            if (value === null || value === '') next.delete(key);
            else next.set(key, value);
          }
          return next;
        },
        {replace: true},
      );
    },
    [setParams],
  );

  const onQueryChange = useCallback((value: string) => update({q: value || null}), [update]);

  const list = usePagedItems({category: config.category, q, sort, order: SORT_ORDER[sort], favorite: favoritesOnly});
  const sentinel = useInfiniteScroll(list.loadMore, {
    enabled: list.hasMore && !list.isLoadingMore && !list.loadMoreError,
    resetKey: list.items.length,
  });

  const isFiltered = Boolean(q) || favoritesOnly;

  let body: ReactNode;
  if (list.isLoading) {
    body = <MediaGridSkeleton shape={config.shape} count={config.shape === 'poster' ? 18 : 12} />;
  } else if (list.error) {
    body = <LoadError title={`${config.title} could not be loaded`} error={list.error} onRetry={list.retry} />;
  } else if (list.items.length === 0 && !list.isStale) {
    body = q ? (
      <EmptyPanel
        title={`No ${config.noun} match “${q}”`}
        description="Check the spelling, or search for part of the name."
        actions={<Button label="Clear search" variant="secondary" onClick={() => update({q: null})} />}
      />
    ) : favoritesOnly ? (
      <EmptyPanel
        title={`No favourite ${config.noun} yet`}
        description={`Open one of your ${config.noun} and choose Favourite to keep it here.`}
        actions={<Button label={`Show all ${config.noun}`} variant="secondary" onClick={() => update({fav: null})} />}
      />
    ) : (
      <EmptyPanel
        title={config.empty.title}
        description={config.empty.description}
        icon={<config.empty.icon size={28} strokeWidth={1.5} />}
        actions={config.emptyAction}
      />
    );
  } else {
    body = (
      <>
        <MediaGrid shape={config.shape} label={config.title} isBusy={list.isStale}>
          {list.items.map(item => {
            const thumb = thumbSources(item, list.versionOf(item.id));
            return (
              <MediaTile
                key={item.id}
                href={itemHref(item)}
                title={displayTitle(item)}
                meta={itemMeta(item)}
                imageSrc={thumb?.src}
                imageSrcSet={thumb?.srcSet}
                shape={config.shape}
                progress={watchedFraction(item.progress)}
                placeholderIcon={config.placeholderFor?.(item)}
              />
            );
          })}
        </MediaGrid>
        <div ref={sentinel} aria-hidden="true" {...stylex.props(styles.sentinel)} />
        {list.isLoadingMore ? (
          <div {...stylex.props(styles.moreSkeleton)}>
            <MediaGridSkeleton shape={config.shape} count={6} />
          </div>
        ) : null}
        {list.loadMoreError ? (
          <div {...stylex.props(styles.more)}>
            <Text as="p" type="supporting" color="secondary">
              {`More ${config.noun} could not be loaded.`}
            </Text>
            <Button label="Try again" variant="secondary" onClick={list.loadMore} />
          </div>
        ) : null}
      </>
    );
  }

  const hasLibrary = !(list.items.length === 0 && !isFiltered && !list.isLoading && !list.isStale && !list.error);

  return (
    <>
      <PageHeader title={config.title} subtitle={config.subtitle} />
      {hasLibrary ? (
        <LibraryToolbar
          searchLabel={`Search ${config.noun}`}
          query={q}
          onQueryChange={onQueryChange}
          sort={sort}
          sortOptions={config.sortOptions}
          onSortChange={value => update({sort: value === config.defaultSort ? null : value})}
          isFavoritesOnly={favoritesOnly}
          onFavoritesOnlyChange={value => update({fav: value ? '1' : null})}
        />
      ) : null}
      {body}
    </>
  );
}

/**
 * Movies: every film in the library as posters, most recently added first, with search,
 * sort, the favourites filter and infinite scroll.
 */
import {Button} from '@astryxdesign/core/Button';
import {CategoryLibraryPage, type CategoryLibraryConfig} from '../components/library/CategoryLibraryPage';

const config: CategoryLibraryConfig = {
  category: 'movie',
  title: 'Movies',
  subtitle: 'Every film in your library.',
  shape: 'poster',
  noun: 'movies',
  // Films carry a release year, not a date taken, so that sort is left out.
  sortOptions: [
    {value: 'added', label: 'Recently added'},
    {value: 'title', label: 'Title'},
    {value: 'size', label: 'Largest first'},
  ],
  defaultSort: 'added',
  empty: {
    title: 'No movies yet',
    description:
      'Films you add are sorted into Movies automatically. Add a link or magnet in Downloads, or drop video files anywhere in this window.',
  },
  emptyAction: <Button label="Add a link" variant="primary" href="/downloads" />,
};

export default function MoviesPage() {
  return <CategoryLibraryPage config={config} />;
}

/**
 * Home Videos: family clips as 16:9 frames, most recently filmed first, with
 * search, sort, the favourites filter and infinite scroll.
 */
import {Button} from '@astryxdesign/core/Button';
import {CategoryLibraryPage, type CategoryLibraryConfig} from '../components/library/CategoryLibraryPage';

const config: CategoryLibraryConfig = {
  category: 'home_video',
  title: 'Home Videos',
  subtitle: 'Family clips from your phones and cameras, most recently filmed first.',
  shape: 'frame',
  noun: 'home videos',
  // Tiles show the date each clip was filmed, so that is the default order:
  // a phone backup imported in one go still reads as a timeline.
  sortOptions: [
    {value: 'taken', label: 'Date filmed'},
    {value: 'added', label: 'Recently added'},
    {value: 'title', label: 'Title'},
    {value: 'size', label: 'Largest first'},
  ],
  defaultSort: 'taken',
  empty: {
    title: 'No home videos yet',
    description:
      'Clips from phones and cameras are sorted here by the month they were filmed. Drop them anywhere in this window, or copy them into the Inbox folder on the server.',
  },
  emptyAction: <Button label="Add files" variant="primary" href="/downloads" />,
};

export default function HomeVideosPage() {
  return <CategoryLibraryPage config={config} />;
}

/**
 * Home Videos: family clips as 16:9 frames, newest first, with search, sort
 * by date filmed, the favourites filter and infinite scroll.
 */
import {Button} from '@astryxdesign/core/Button';
import {CategoryLibraryPage, type CategoryLibraryConfig} from '../components/library/CategoryLibraryPage';

const config: CategoryLibraryConfig = {
  category: 'home_video',
  title: 'Home Videos',
  subtitle: 'Family clips from your phones and cameras, newest first.',
  shape: 'frame',
  noun: 'home videos',
  sortOptions: [
    {value: 'added', label: 'Recently added'},
    {value: 'taken', label: 'Date filmed'},
    {value: 'title', label: 'Title'},
    {value: 'size', label: 'Largest first'},
  ],
  defaultSort: 'added',
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

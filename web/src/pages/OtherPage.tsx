/**
 * Other: files that are not movies, shows, home videos or photos, such as
 * music, documents and archives. Same grid, search and sort as the rest of
 * the library.
 */
import {Button} from '@astryxdesign/core/Button';
import {FileText} from 'lucide-react';
import {CategoryLibraryPage, type CategoryLibraryConfig} from '../components/library/CategoryLibraryPage';
import {placeholderIcon} from '../components/library/placeholderIcon';

const config: CategoryLibraryConfig = {
  category: 'other',
  title: 'Other',
  subtitle: 'Music, documents and anything else.',
  shape: 'frame',
  noun: 'files',
  sortOptions: [
    {value: 'added', label: 'Recently added'},
    // Music and documents carry no date taken, so that sort is left out.
    {value: 'title', label: 'Name'},
    {value: 'size', label: 'Largest first'},
  ],
  defaultSort: 'added',
  empty: {
    title: 'Nothing in Other yet',
    icon: FileText,
    description:
      'Files that are not videos or photos, like music and documents, are kept here. Drop them anywhere in this window to add them.',
  },
  emptyAction: <Button label="Add files" variant="primary" href="/downloads" />,
  placeholderFor: placeholderIcon,
};

export default function OtherPage() {
  return <CategoryLibraryPage config={config} />;
}

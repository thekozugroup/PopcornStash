/**
 * Other: files that are not movies, shows, home videos or photos, such as
 * music, documents and archives. Same grid, search and sort as the rest of
 * the library.
 */
import {Button} from '@astryxdesign/core/Button';
import {FileArchive, FileText, Music} from 'lucide-react';
import {CategoryLibraryPage, type CategoryLibraryConfig} from '../components/library/CategoryLibraryPage';
import {fileType} from '../components/library/libraryFormat';

const ARCHIVES = new Set(['ZIP', 'RAR', '7Z', 'TAR', 'GZ', 'ISO']);

const config: CategoryLibraryConfig = {
  category: 'other',
  title: 'Other',
  subtitle: 'Music, documents and anything else that is not a film, show, home video or photo.',
  shape: 'frame',
  noun: 'files',
  sortOptions: [
    {value: 'added', label: 'Recently added'},
    {value: 'title', label: 'Name'},
    {value: 'taken', label: 'Date taken'},
    {value: 'size', label: 'Largest first'},
  ],
  defaultSort: 'added',
  empty: {
    title: 'Nothing in Other yet',
    description:
      'Files that are not videos or photos, like music and documents, are kept here. Drop them anywhere in this window to add them.',
  },
  emptyAction: <Button label="Add files" variant="primary" href="/downloads" />,
  placeholderFor: item => {
    if (item.kind === 'audio') return Music;
    if (ARCHIVES.has(fileType(item.rel_path) ?? '')) return FileArchive;
    if (item.kind === 'other') return FileText;
    return undefined;
  },
};

export default function OtherPage() {
  return <CategoryLibraryPage config={config} />;
}

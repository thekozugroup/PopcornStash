/**
 * Other files (`/other`): music, documents and anything else that is not a
 * video or a photo. It is a built-in place, not a library, so it has no
 * settings. Each file is seen by the person who added it and the owners; the
 * sentence under the title says so in the reader's terms.
 */
import {Button} from '@astryxdesign/core/Button';
import {FileText} from 'lucide-react';
import {useAuth} from '../auth/AuthProvider';
import {EmptyPanel} from '../components/EmptyPanel';
import {ItemGridView, type ItemGridConfig} from '../components/library/ItemGridView';
import {placeholderIcon} from '../components/library/placeholderIcon';
import {PageHeader} from '../components/PageHeader';

export default function OtherPage() {
  const {isOwner} = useAuth();
  const config: ItemGridConfig = {
    scope: {place: 'other_files'},
    label: 'Other files',
    shape: 'frame',
    noun: 'files',
    sortOptions: [
      {value: 'added', label: 'Recently added'},
      // Music and documents carry no date taken, so that sort is left out.
      {value: 'title', label: 'Name'},
      {value: 'size', label: 'Largest first'},
    ],
    defaultSort: 'added',
    empty: (
      <EmptyPanel
        title="Nothing in Other files yet"
        description="Drop music or documents anywhere in this window to add them."
        icon={<FileText size={28} strokeWidth={1.5} />}
        actions={<Button label="Add files" variant="primary" href="/downloads" />}
      />
    ),
    placeholderFor: item => placeholderIcon(item) ?? FileText,
  };
  return (
    <>
      <PageHeader
        title="Other files"
        actions={isOwner ? <Button label="Clean files" variant="secondary" href="/clean?folder=Other" /> : undefined}
        subtitle={
          isOwner
            ? 'Music, documents and anything else. Each file is seen by the person who added it and the owners.'
            : 'Music, documents and anything else. Only you and the owners see these.'
        }
      />
      <ItemGridView config={config} />
    </>
  );
}

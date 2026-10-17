import {Selector} from '@astryxdesign/core/Selector';
import {VStack} from '@astryxdesign/core/Layout';
import {FileSearch} from 'lucide-react';
import {useSearchParams} from 'react-router';
import {useLibraries} from '../app/LibrariesProvider';
import {PageHeader} from '../components/PageHeader';
import {EmptyPanel} from '../components/EmptyPanel';
import {ItemGridView} from '../components/library/ItemGridView';

export default function SearchPage() {
  const {libraries} = useLibraries();
  const [params, setParams] = useSearchParams();
  const library = params.get('library') ?? '';
  return <>
    <PageHeader title="Search" subtitle="Find titles, shows and filenames across the places you can access." />
    <VStack gap={4}>
      <Selector label="Search in" value={library} options={[{value: '', label: 'All libraries and Inbox'}, ...(libraries ?? []).map(l => ({value: l.id, label: l.name}))]}
        onChange={value => setParams(current => { const next = new URLSearchParams(current); if (value) next.set('library', value); else next.delete('library'); return next; })} width={280} />
      <ItemGridView key={library} config={{scope: {library_id: library || undefined, include_inbox: !library}, label: 'Search', noun: 'files', shape: 'frame',
        sortOptions: [{value: 'title', label: 'Title'}, {value: 'added', label: 'Recently added'}], defaultSort: 'title',
        placeholderFor: () => FileSearch, empty: <EmptyPanel title="No files here yet" description="New files appear here after they are indexed." />}} />
    </VStack>
  </>;
}

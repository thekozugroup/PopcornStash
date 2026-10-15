import {Selector} from '@astryxdesign/core/Selector';
import {Button} from '@astryxdesign/core/Button';
import {useLibraries} from '../../app/LibrariesProvider';
import {libraryIcon} from '../../lib/libraryIcons';

export function DestinationSelect({value, onChange, disabled = false}: {
  value: string; onChange: (value: string) => void; disabled?: boolean;
}) {
  const {libraries, status, reload} = useLibraries();
  const missing = value !== '' && libraries !== null && !libraries.some(library => library.id === value);
  return <div>
    <Selector label="Where it goes" value={value} onChange={onChange} width="100%"
      presentation="adaptive" isDisabled={disabled} isLoading={status === 'loading'}
      options={[{value: '', label: 'Sorted automatically'}, ...(libraries ?? []).map(library => {
        const Icon = libraryIcon(library.icon);
        return {value: library.id, label: library.name, icon: <Icon size={16} strokeWidth={1.5} />};
      })]}
      status={missing ? {type: 'error', message: 'This library is no longer available. Choose another.'} :
        status === 'error' ? {type: 'warning', message: 'Libraries could not be loaded. Automatic sorting is available.'} : undefined}
      statusVariant="detached" />
    {status === 'error' ? <Button label="Try again" variant="ghost" onClick={() => void reload()} /> : null}
  </div>;
}

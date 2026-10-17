import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {CheckboxInput} from '@astryxdesign/core/CheckboxInput';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter, VStack} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useState} from 'react';
import {libraries} from '../../api/client';
import type {Library} from '../../api/types';
import {errorMessage} from '../../lib/errors';

export function DeleteLibraryDialog({library, onClose, onDeleted}: {
  library: Library; onClose: () => void; onDeleted: (folderRemoved: boolean) => void;
}) {
  const [removeFolder, setRemoveFolder] = useState(false);
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const remove = async () => {
    setBusy(true); setError(null);
    try { await libraries.remove(library.id, removeFolder ? library.folder ?? undefined : undefined); onDeleted(removeFolder); }
    catch (e) { setError(errorMessage(e, 'The library was not deleted. Try again.')); }
    finally { setBusy(false); }
  };
  return <Dialog isOpen purpose="form" width={480} onOpenChange={open => { if (!open && !busy) onClose(); }}>
    <Layout header={<DialogHeader title={`Delete ${library.name}?`} onOpenChange={open => { if (!open && !busy) onClose(); }} />}
      content={<LayoutContent><VStack gap={4}>
        <Text as="p">The library leaves the app. Its folder and files stay unless you choose to remove them below.</Text>
        <CheckboxInput label="Also remove the folder and all its files" value={removeFolder} isDisabled={busy || !library.folder}
          description={`Moves ${library.folder ?? 'the folder'}/ to Trash, including files the library does not show. Owners can restore it from Settings.`}
          onChange={value => { setRemoveFolder(value); setConfirmation(''); }} />
        {removeFolder ? <TextInput label={`Type ${library.folder ?? ''} to confirm`} value={confirmation} onChange={setConfirmation} isDisabled={busy} autoComplete="off" /> : null}
        {error ? <Banner status="error" title="Library not deleted" description={error} collapsible={false} /> : null}
      </VStack></LayoutContent>}
      footer={<LayoutFooter><HStack gap={2} hAlign="end" wrap="wrap">
        <Button label="Cancel" variant="secondary" isDisabled={busy} onClick={onClose} />
        <Button label={removeFolder ? 'Delete library and folder' : 'Delete library'} variant="destructive"
          isLoading={busy} isDisabled={removeFolder && confirmation !== library.folder} onClick={() => void remove()} />
      </HStack></LayoutFooter>} />
  </Dialog>;
}

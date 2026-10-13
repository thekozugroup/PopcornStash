/**
 * Rename a passkey so it is easy to tell apart ("Work laptop", "iPhone").
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useState, type FormEvent} from 'react';
import {me} from '../../../api/client';
import type {Passkey} from '../../../api/types';
import {errorMessage} from '../../../lib/errors';

const styles = stylex.create({
  body: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    paddingBlock: 20,
    paddingInline: 24,
  },
  footer: {
    display: 'flex',
    flexWrap: 'wrap',
    justifyContent: 'flex-end',
    gap: 8,
  },
});

export interface RenamePasskeyDialogProps {
  passkey: Passkey | null;
  onClose: () => void;
  onRenamed: (passkey: Passkey) => void;
}

function RenameForm({
  passkey,
  onClose,
  onRenamed,
}: {
  passkey: Passkey;
  onClose: () => void;
  onRenamed: (passkey: Passkey) => void;
}) {
  const [name, setName] = useState(passkey.name);
  const [error, setError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const trimmed = name.trim();
    if (!trimmed) {
      setError('Enter a name, for example Work laptop.');
      return;
    }
    if (trimmed.length > 64) {
      setError('Keep the name under 64 characters.');
      return;
    }
    setIsBusy(true);
    try {
      onRenamed(await me.renamePasskey(passkey.id, trimmed));
      onClose();
    } catch (cause) {
      setError(errorMessage(cause, 'The passkey was not renamed. Try again.'));
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <form onSubmit={onSubmit} noValidate {...stylex.props(styles.body)}>
      <TextInput
        label="Name"
        value={name}
        onChange={value => {
          setName(value);
          if (error) setError(null);
        }}
        autoComplete="off"
        isRequired
        hasAutoFocus
        isDisabled={isBusy}
        status={error ? {type: 'error', message: error} : undefined}
        statusVariant="detached"
        width="100%"
      />
      <div {...stylex.props(styles.footer)}>
        <Button label="Cancel" variant="secondary" onClick={onClose} isDisabled={isBusy} />
        <Button type="submit" label="Save name" variant="primary" isLoading={isBusy} />
      </div>
    </form>
  );
}

export function RenamePasskeyDialog({passkey, onClose, onRenamed}: RenamePasskeyDialogProps) {
  return (
    <Dialog isOpen={passkey !== null} onOpenChange={open => (open ? undefined : onClose())} purpose="form" width={440}>
      <DialogHeader
        title="Rename passkey"
        subtitle="Only you see this name."
        onOpenChange={open => (open ? undefined : onClose())}
      />
      {passkey ? <RenameForm key={passkey.id} passkey={passkey} onClose={onClose} onRenamed={onRenamed} /> : null}
    </Dialog>
  );
}

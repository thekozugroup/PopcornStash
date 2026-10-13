/**
 * "Create a token": a name and an access level, then the token itself,
 * shown once with a Copy button.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {HStack, Layout, LayoutContent, LayoutFooter, VStack} from '@astryxdesign/core/Layout';
import {RadioList, RadioListItem} from '@astryxdesign/core/RadioList';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useId, useState, type FormEvent} from 'react';
import {tokens} from '../../../api/client';
import type {CreatedToken, TokenScope} from '../../../api/types';
import {CopyValue} from '../../../components/settings/CopyValue';
import {errorMessage} from '../../../lib/errors';

const styles = stylex.create({
  // The form wraps the dialog layout without adding a box of its own.
  form: {
    display: 'contents',
  },
  note: {
    lineHeight: 1.55,
  },
});

type Access = 'downloads' | 'library' | 'full';

const ACCESS_SCOPES: Record<Access, TokenScope[]> = {
  downloads: ['downloads:create', 'downloads:read'],
  library: ['library:read'],
  full: ['full'],
};

export interface CreateTokenDialogProps {
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
  onCreated: (token: CreatedToken) => void;
}

export function CreateTokenDialog({isOpen, onOpenChange, onCreated}: CreateTokenDialogProps) {
  const [name, setName] = useState('');
  const [access, setAccess] = useState<Access>('downloads');
  const [nameError, setNameError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [created, setCreated] = useState<CreatedToken | null>(null);
  const formId = useId();

  const close = () => {
    onOpenChange(false);
    setName('');
    setAccess('downloads');
    setNameError(null);
    setFormError(null);
    setCreated(null);
  };

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const trimmed = name.trim();
    if (!trimmed) {
      setNameError('Give the token a name you will recognise later.');
      return;
    }
    setNameError(null);
    setFormError(null);
    setIsBusy(true);
    try {
      const token = await tokens.create({name: trimmed, scopes: ACCESS_SCOPES[access]});
      setCreated(token);
      onCreated(token);
    } catch (cause) {
      setFormError(errorMessage(cause, 'The token was not created. Try again.'));
    } finally {
      setIsBusy(false);
    }
  };

  const header = (
    <DialogHeader
      title={created ? 'Copy your token' : 'Create a token'}
      subtitle={created ? undefined : 'For scripts and apps that cannot pair with a code.'}
      onOpenChange={open => (open ? undefined : close())}
    />
  );

  return (
    <Dialog isOpen={isOpen} onOpenChange={open => (open ? onOpenChange(true) : close())} purpose="form" width={520}>
      {created ? (
        <Layout
          header={header}
          content={
            <LayoutContent>
              <VStack gap={5}>
                <CopyValue label={created.name} value={created.token} copyLabel="Copy token" />
                <Text as="p" type="supporting" color="secondary" xstyle={styles.note}>
                  This is the only time the token is shown. Paste it into the app now. If you lose it, revoke it and
                  create a new one.
                </Text>
              </VStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="end" wrap="wrap">
                <Button label="Done" variant="primary" onClick={close} />
              </HStack>
            </LayoutFooter>
          }
        />
      ) : (
        <form id={formId} onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
          <Layout
            header={header}
            content={
              <LayoutContent>
                <VStack gap={5}>
                  <TextInput
                    label="Name"
                    description="Where it will be used, for example Backup script."
                    value={name}
                    onChange={value => {
                      setName(value);
                      if (nameError) setNameError(null);
                    }}
                    autoComplete="off"
                    isRequired
                    isDisabled={isBusy}
                    hasAutoFocus
                    status={nameError ? {type: 'error', message: nameError} : undefined}
                    statusVariant="detached"
                    width="100%"
                  />
                  <RadioList
                    label="Access"
                    value={access}
                    onChange={value => setAccess(value as Access)}
                    isDisabled={isBusy}>
                    <RadioListItem
                      value="downloads"
                      label="Add downloads"
                      description="Send links and magnets, and see their progress."
                    />
                    <RadioListItem
                      value="library"
                      label="Browse the library"
                      description="Read-only access to your videos and photos."
                    />
                    <RadioListItem
                      value="full"
                      label="Full access"
                      description="Everything you can do. Keep it as safe as a password."
                    />
                  </RadioList>
                  {formError ? (
                    <Banner status="error" title="No token was created" description={formError} collapsible={false} />
                  ) : null}
                </VStack>
              </LayoutContent>
            }
            footer={
              <LayoutFooter>
                <HStack gap={2} hAlign="end" wrap="wrap">
                  <Button label="Cancel" variant="secondary" onClick={close} isDisabled={isBusy} />
                  <Button type="submit" form={formId} label="Create token" variant="primary" isLoading={isBusy} />
                </HStack>
              </LayoutFooter>
            }
          />
        </form>
      )}
    </Dialog>
  );
}

/**
 * Invite someone: choose their role and how long the link lasts, then copy
 * the single-use link.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Dialog, DialogHeader} from '@astryxdesign/core/Dialog';
import {RadioList, RadioListItem} from '@astryxdesign/core/RadioList';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {useState, type FormEvent} from 'react';
import {invites} from '../../../api/client';
import type {Invite, Role} from '../../../api/types';
import {CopyValue} from '../../../components/settings/CopyValue';
import {errorMessage} from '../../../lib/errors';
import {formatDateTime} from '../../../lib/format';

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
    paddingTop: 4,
  },
  note: {
    lineHeight: 1.55,
  },
});

const EXPIRY_OPTIONS = [
  {value: '24', label: '1 day'},
  {value: '72', label: '3 days'},
  {value: '168', label: '7 days'},
];

export interface InviteDialogProps {
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
  onCreated: (invite: Invite) => void;
}

export function InviteDialog({isOpen, onOpenChange, onCreated}: InviteDialogProps) {
  const [role, setRole] = useState<Role>('member');
  const [hours, setHours] = useState('72');
  const [formError, setFormError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [created, setCreated] = useState<Invite | null>(null);

  const close = () => {
    onOpenChange(false);
    setRole('member');
    setHours('72');
    setFormError(null);
    setCreated(null);
  };

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    setFormError(null);
    setIsBusy(true);
    try {
      const invite = await invites.create({role, expires_in_hours: Number(hours)});
      setCreated(invite);
      onCreated(invite);
    } catch (cause) {
      setFormError(errorMessage(cause, 'The invite link was not created. Try again.'));
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <Dialog isOpen={isOpen} onOpenChange={open => (open ? onOpenChange(true) : close())} purpose="form" width={520}>
      <DialogHeader
        title={created ? 'Send this link' : 'Invite someone'}
        subtitle={created ? undefined : 'They create their own passkey, so there is no password to share.'}
        onOpenChange={open => (open ? undefined : close())}
      />
      {created ? (
        <div {...stylex.props(styles.body)}>
          <CopyValue
            label={created.role === 'owner' ? 'Owner invite link' : 'Member invite link'}
            value={created.url}
            copyLabel="Copy link"
          />
          <Text as="p" type="supporting" color="secondary" xstyle={styles.note}>
            The link works once and expires {formatDateTime(created.expires_at)}. Whoever opens it first can join, so
            send it only to the person you are inviting.
          </Text>
          <div {...stylex.props(styles.footer)}>
            <Button label="Done" variant="primary" onClick={close} />
          </div>
        </div>
      ) : (
        <form onSubmit={onSubmit} noValidate {...stylex.props(styles.body)}>
          <RadioList label="Role" value={role} onChange={value => setRole(value as Role)} isDisabled={isBusy}>
            <RadioListItem value="member" label="Member" description="Watch, browse, upload and add downloads." />
            <RadioListItem
              value="owner"
              label="Owner"
              description="Everything a member can do, plus the VPN, settings and who has access."
            />
          </RadioList>
          <Selector
            label="Link expires after"
            options={EXPIRY_OPTIONS}
            value={hours}
            onChange={value => setHours(value ?? '72')}
            isDisabled={isBusy}
            width="100%"
          />
          {formError ? (
            <Banner status="error" title="No invite was created" description={formError} collapsible={false} />
          ) : null}
          <div {...stylex.props(styles.footer)}>
            <Button label="Cancel" variant="secondary" onClick={close} isDisabled={isBusy} />
            <Button type="submit" label="Create invite link" variant="primary" isLoading={isBusy} />
          </div>
        </form>
      )}
    </Dialog>
  );
}

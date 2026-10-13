/**
 * Settings, People (owner): who can use this PopcornStash, their roles,
 * invite links, and invites that are still open.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {ListItem} from '@astryxdesign/core/List';
import {MoreMenu} from '@astryxdesign/core/MoreMenu';
import {useToast} from '@astryxdesign/core/Toast';
import {useState} from 'react';
import {invites, users} from '../../api/client';
import type {Role, User} from '../../api/types';
import {useAuth} from '../../auth/AuthProvider';
import {OwnerOnly} from '../../components/settings/OwnerOnly';
import {EmptyRows, LoadError, LoadingRows, PanelList, Wrap} from '../../components/settings/SectionStates';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {listOpenInvites, type InviteListEntry} from '../../components/settings/settingsApi';
import {useConfirm} from '../../components/settings/useConfirm';
import {useResource} from '../../components/settings/useResource';
import {errorMessage} from '../../lib/errors';
import {formatDate, formatRelative} from '../../lib/format';
import {InviteDialog} from './people/InviteDialog';
import {SettingsSectionHeader} from './SettingsSectionHeader';

const styles = stylex.create({
  groups: {
    display: 'flex',
    flexDirection: 'column',
  },
});

function roleName(role: Role): string {
  return role === 'owner' ? 'Owner' : 'Member';
}

function sortPeople(list: User[]): User[] {
  return [...list].sort((a, b) =>
    a.role === b.role ? a.display_name.localeCompare(b.display_name) : a.role === 'owner' ? -1 : 1,
  );
}

function PersonRow({
  person,
  isMe,
  isLastOwner,
  onRole,
  onRemove,
}: {
  person: User;
  isMe: boolean;
  isLastOwner: boolean;
  onRole: (person: User, role: Role) => void;
  onRemove: (person: User) => void;
}) {
  const nextRole: Role = person.role === 'owner' ? 'member' : 'owner';
  const joined = `${roleName(person.role)}, joined ${formatDate(person.created_at)}.`;
  return (
    <ListItem
      label={isMe ? `${person.display_name} (you)` : person.display_name}
      description={
        <Wrap>
          {joined}
          {isLastOwner ? ` The only owner, so this role cannot change until someone else is an owner.` : null}
        </Wrap>
      }
      endContent={
        isLastOwner && isMe ? undefined : (
          <MoreMenu
            label={`Options for ${person.display_name}`}
            alignment="end"
            items={[
              ...(isLastOwner
                ? []
                : [
                    {
                      label: nextRole === 'owner' ? 'Make owner' : 'Make member',
                      onClick: () => onRole(person, nextRole),
                    },
                  ]),
              ...(isMe || isLastOwner
                ? []
                : [
                    {type: 'divider' as const},
                    {
                      label: 'Remove from PopcornStash',
                      variant: 'destructive' as const,
                      onClick: () => onRemove(person),
                    },
                  ]),
            ]}
          />
        )
      }
    />
  );
}

function InviteRow({invite, onRevoke}: {invite: InviteListEntry; onRevoke: (invite: InviteListEntry) => void}) {
  const parts = [`Expires ${formatRelative(invite.expires_at).toLowerCase()}`];
  if (invite.created_by_name) parts.push(`created by ${invite.created_by_name}`);
  return (
    <ListItem
      label={invite.role === 'owner' ? 'Owner invite' : 'Member invite'}
      description={<Wrap>{parts.join(', ')}</Wrap>}
      endContent={<Button label="Revoke" variant="secondary" onClick={() => onRevoke(invite)} />}
    />
  );
}

function People() {
  const {user} = useAuth();
  const showToast = useToast();
  const people = useResource(signal => users.list(signal), {key: 'users'});
  const openInvites = useResource(signal => listOpenInvites(signal), {key: 'invites'});
  const {ask, dialog} = useConfirm();
  const [isInviting, setIsInviting] = useState(false);

  const list = sortPeople(people.data ?? []);
  const ownerCount = list.filter(person => person.role === 'owner').length;

  const changeRole = (person: User, role: Role) => {
    const run = async () => {
      const updated = await users.setRole(person.id, role);
      people.mutate(current => current.map(item => (item.id === updated.id ? updated : item)));
      showToast({body: `${person.display_name} is now ${role === 'owner' ? 'an owner' : 'a member'}.`});
    };
    if (role === 'owner') {
      ask({
        title: `Make ${person.display_name} an owner?`,
        description:
          'Owners can change the VPN and settings, empty the trash, delete items, and invite or remove people, including you.',
        actionLabel: 'Make owner',
        actionVariant: 'primary',
        failure: 'The role did not change. Try again.',
        run,
      });
    } else {
      run().catch(cause =>
        showToast({type: 'error', body: errorMessage(cause, 'The role did not change. Try again.')}),
      );
    }
  };

  const remove = (person: User) =>
    ask({
      title: `Remove ${person.display_name}?`,
      description:
        'They are signed out everywhere and their passkeys stop working. Videos and photos they added stay in the library.',
      actionLabel: 'Remove',
      failure: 'They were not removed. Try again.',
      run: async () => {
        await users.remove(person.id);
        people.mutate(current => current.filter(item => item.id !== person.id));
        showToast({body: `${person.display_name} was removed.`});
      },
    });

  const revokeInvite = (invite: InviteListEntry) =>
    ask({
      title: 'Revoke this invite?',
      description: 'The link stops working. Anyone who has it will need a new one.',
      actionLabel: 'Revoke',
      failure: 'The invite was not revoked. Try again.',
      run: async () => {
        await invites.revoke(invite.id);
        openInvites.mutate(current => current.filter(item => item.id !== invite.id));
        showToast({body: 'Invite revoked.'});
      },
    });

  return (
    <>
      <SettingsSectionHeader
        title="People"
        subtitle="Invite family members and choose who can manage the server."
        actions={<Button label="Invite someone" variant="primary" onClick={() => setIsInviting(true)} />}
      />
      <div {...stylex.props(styles.groups)}>
        <SettingsGroup
          title="People"
          description="Owners manage the VPN, settings and who has access. Members watch, browse, upload and add downloads.">
          {people.isLoading ? (
            <LoadingRows rows={3} label="Loading people" />
          ) : people.error !== undefined && !people.data ? (
            <LoadError title="People did not load" error={people.error} onRetry={people.reload} />
          ) : (
            <PanelList density="spacious">
              {list.map(person => (
                <PersonRow
                  key={person.id}
                  person={person}
                  isMe={person.id === user?.id}
                  isLastOwner={person.role === 'owner' && ownerCount <= 1}
                  onRole={changeRole}
                  onRemove={remove}
                />
              ))}
            </PanelList>
          )}
        </SettingsGroup>

        <SettingsGroup
          title="Open invites"
          description="Each link works once. Revoke one if it went to the wrong person.">
          {openInvites.isLoading ? (
            <LoadingRows rows={2} label="Loading invites" />
          ) : openInvites.error !== undefined && !openInvites.data ? (
            <LoadError title="Invites did not load" error={openInvites.error} onRetry={openInvites.reload} />
          ) : (openInvites.data ?? []).length === 0 ? (
            <EmptyRows>No open invites. Use Invite someone to create a link for a family member.</EmptyRows>
          ) : (
            <PanelList density="spacious">
              {(openInvites.data ?? []).map(invite => (
                <InviteRow key={invite.id} invite={invite} onRevoke={revokeInvite} />
              ))}
            </PanelList>
          )}
        </SettingsGroup>
      </div>

      <InviteDialog isOpen={isInviting} onOpenChange={setIsInviting} onCreated={() => openInvites.reload()} />
      {dialog}
    </>
  );
}

export default function PeopleSettingsPage() {
  return (
    <OwnerOnly>
      <People />
    </OwnerOnly>
  );
}

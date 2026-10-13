/**
 * Settings, Security (everyone): my passkeys (add, rename, remove; the last
 * one cannot be removed), the browsers signed in to my account, and for
 * owners the audit log.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {ListItem} from '@astryxdesign/core/List';
import {MoreMenu} from '@astryxdesign/core/MoreMenu';
import {Text} from '@astryxdesign/core/Text';
import {useToast} from '@astryxdesign/core/Toast';
import {useState} from 'react';
import {me} from '../../api/client';
import type {Passkey, Session} from '../../api/types';
import {createPasskey, PasskeyError} from '../../api/webauthn';
import {useAuth} from '../../auth/AuthProvider';
import {guessDeviceName} from '../../auth/deviceName';
import {passkeyAvailability} from '../../auth/passkeyAvailability';
import {PasskeyNotice} from '../../components/PasskeyNotice';
import {sessionName} from '../../components/settings/describe';
import {EmptyRows, LoadError, LoadingRows, PanelList, Wrap} from '../../components/settings/SectionStates';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {revokeOtherSessions} from '../../components/settings/settingsApi';
import {useConfirm} from '../../components/settings/useConfirm';
import {useResource} from '../../components/settings/useResource';
import {errorMessage, isCancelled} from '../../lib/errors';
import {formatDate, formatRelative} from '../../lib/format';
import {AuditLog} from './security/AuditLog';
import {RenamePasskeyDialog} from './security/RenamePasskeyDialog';
import {SettingsSectionHeader} from './SettingsSectionHeader';

const styles = stylex.create({
  groups: {
    display: 'flex',
    flexDirection: 'column',
  },
  note: {
    lineHeight: 1.55,
  },
});

const LAST_PASSKEY = 'This is your only passkey. Add another one first so you can still sign in.';

function PasskeyRow({
  passkey,
  isLast,
  onRename,
  onRemove,
}: {
  passkey: Passkey;
  isLast: boolean;
  onRename: (passkey: Passkey) => void;
  onRemove: (passkey: Passkey) => void;
}) {
  const used = passkey.last_used_at
    ? `last used ${formatRelative(passkey.last_used_at).toLowerCase()}`
    : 'not used to sign in yet';
  return (
    <ListItem
      label={passkey.name}
      description={<Wrap>{`Added ${formatDate(passkey.created_at)}, ${used}`}</Wrap>}
      endContent={
        <MoreMenu
          label={`Options for ${passkey.name}`}
          alignment="end"
          items={[
            {label: 'Rename', onClick: () => onRename(passkey)},
            // The last passkey cannot be removed; the note under the list says why.
            ...(isLast
              ? []
              : [
                  {type: 'divider' as const},
                  {label: 'Remove', variant: 'destructive' as const, onClick: () => onRemove(passkey)},
                ]),
          ]}
        />
      }
    />
  );
}

function SessionRow({session, onRevoke}: {session: Session; onRevoke: (session: Session) => void}) {
  const name = sessionName(session.kind, session.user_agent);
  const parts = [
    session.current ? 'This browser, active now' : `Last active ${formatRelative(session.last_seen_at).toLowerCase()}`,
    session.ip ? `from ${session.ip}` : null,
    `signed in ${formatDate(session.created_at)}`,
  ].filter(Boolean);
  return (
    <ListItem
      label={name}
      description={<Wrap>{parts.join(', ')}</Wrap>}
      endContent={
        session.current ? undefined : <Button label="Sign out" variant="secondary" onClick={() => onRevoke(session)} />
      }
    />
  );
}

export default function SecuritySettingsPage() {
  const {state, isOwner} = useAuth();
  const showToast = useToast();
  const passkeys = useResource(signal => me.passkeys(signal), {key: 'passkeys'});
  const sessions = useResource(signal => me.sessions(signal), {key: 'sessions'});
  const {ask, dialog} = useConfirm();
  const [renaming, setRenaming] = useState<Passkey | null>(null);
  const [isAdding, setIsAdding] = useState(false);

  const availability = passkeyAvailability(state);
  const passkeyList = passkeys.data ?? [];
  const isLastPasskey = passkeyList.length <= 1;
  const sessionList = [...(sessions.data ?? [])].sort((a, b) =>
    a.current === b.current ? b.last_seen_at - a.last_seen_at : a.current ? -1 : 1,
  );
  const otherSessions = sessionList.filter(session => !session.current);

  const addPasskey = async () => {
    if (isAdding) return;
    setIsAdding(true);
    try {
      const flow = await me.passkeyAddStart();
      const credential = await createPasskey(flow.options);
      const added = await me.passkeyAddFinish({flow_id: flow.flow_id, credential, passkey_name: guessDeviceName()});
      passkeys.mutate(list => [...list.filter(item => item.id !== added.id), added]);
      showToast({body: `Passkey added. You can rename it to tell it apart.`});
    } catch (cause) {
      if (isCancelled(cause)) return;
      const message =
        cause instanceof PasskeyError && cause.kind === 'invalid-state'
          ? 'This device already has a passkey for your account. Use another device or security key.'
          : errorMessage(cause, 'The passkey was not added. Try again.');
      showToast({type: 'error', body: message});
    } finally {
      setIsAdding(false);
    }
  };

  const removePasskey = (passkey: Passkey) =>
    ask({
      title: `Remove ${passkey.name}?`,
      description: 'You will not be able to sign in with it any more. Your other passkeys keep working.',
      actionLabel: 'Remove passkey',
      failure: 'The passkey was not removed. Try again.',
      run: async () => {
        await me.deletePasskey(passkey.id);
        passkeys.mutate(list => list.filter(item => item.id !== passkey.id));
        showToast({body: `${passkey.name} was removed.`});
      },
    });

  const revokeSession = (session: Session) =>
    ask({
      title: `Sign out ${sessionName(session.kind, session.user_agent)}?`,
      description: 'That browser is signed out right away. Signing in there again needs a passkey.',
      actionLabel: 'Sign out',
      failure: 'That browser is still signed in. Try again.',
      run: async () => {
        await me.revokeSession(session.id);
        sessions.mutate(list => list.filter(item => item.id !== session.id));
      },
    });

  const revokeOthers = () =>
    ask({
      title: 'Sign out everywhere else?',
      description: `${otherSessions.length === 1 ? 'One other browser is' : `${otherSessions.length} other browsers are`} signed out right away. This browser stays signed in.`,
      actionLabel: 'Sign out others',
      failure: 'Some browsers are still signed in. Try again.',
      run: async () => {
        const result = await revokeOtherSessions(sessionList);
        sessions.mutate(list => list.filter(item => !result.revoked.includes(item.id)));
        if (result.failure !== undefined) throw result.failure;
        showToast({body: 'Signed out everywhere else.'});
      },
    });

  return (
    <>
      <SettingsSectionHeader title="Security" subtitle="Your passkeys and the browsers signed in to your account." />
      <div {...stylex.props(styles.groups)}>
        <SettingsGroup
          title="Passkeys"
          description="Passkeys are how you sign in. Keep at least two, for example on your phone and your computer, so losing one device does not lock you out."
          actions={
            <Button
              label="Add a passkey"
              variant="primary"
              onClick={() => void addPasskey()}
              isLoading={isAdding}
              isDisabled={!availability.ok}
            />
          }>
          <PasskeyNotice availability={availability} />
          {passkeys.isLoading ? (
            <LoadingRows rows={2} label="Loading passkeys" />
          ) : passkeys.error !== undefined && !passkeys.data ? (
            <LoadError title="Passkeys did not load" error={passkeys.error} onRetry={passkeys.reload} />
          ) : (
            <>
              <PanelList density="spacious">
                {passkeyList.map(passkey => (
                  <PasskeyRow
                    key={passkey.id}
                    passkey={passkey}
                    isLast={isLastPasskey}
                    onRename={setRenaming}
                    onRemove={removePasskey}
                  />
                ))}
              </PanelList>
              {isLastPasskey ? (
                <Text as="p" type="supporting" color="secondary" xstyle={styles.note}>
                  {LAST_PASSKEY} If this device is lost, the owner can print a recovery link from the server.
                </Text>
              ) : null}
            </>
          )}
        </SettingsGroup>

        <SettingsGroup
          title="Signed-in browsers"
          description="Sign out a browser you no longer use, or one you do not recognise."
          actions={
            otherSessions.length > 0 ? (
              <Button label="Sign out everywhere else" variant="secondary" onClick={revokeOthers} />
            ) : undefined
          }>
          {sessions.isLoading ? (
            <LoadingRows rows={3} label="Loading browsers" />
          ) : sessions.error !== undefined && !sessions.data ? (
            <LoadError title="Signed-in browsers did not load" error={sessions.error} onRetry={sessions.reload} />
          ) : sessionList.length === 0 ? (
            <EmptyRows>No browsers are signed in.</EmptyRows>
          ) : (
            <PanelList density="spacious">
              {sessionList.map(session => (
                <SessionRow key={session.id} session={session} onRevoke={revokeSession} />
              ))}
            </PanelList>
          )}
        </SettingsGroup>

        {isOwner ? (
          <SettingsGroup
            isStacked
            title="Audit log"
            description="Sign-ins, invites, settings changes and deletions by everyone on this server. Only owners see it.">
            <AuditLog />
          </SettingsGroup>
        ) : null}
      </div>

      <RenamePasskeyDialog
        passkey={renaming}
        onClose={() => setRenaming(null)}
        onRenamed={updated => passkeys.mutate(list => list.map(item => (item.id === updated.id ? updated : item)))}
      />
      {dialog}
    </>
  );
}

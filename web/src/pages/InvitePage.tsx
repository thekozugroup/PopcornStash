/**
 * Accept an invite (`/invite/:code`): check the link, ask for a name, and
 * create this person's passkey (`/api/auth/invite/start` + `/finish`).
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Spinner} from '@astryxdesign/core/Spinner';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useState, type FormEvent} from 'react';
import {useNavigate, useParams} from 'react-router';
import {ApiError, auth, invites} from '../api/client';
import {createPasskey} from '../api/webauthn';
import {AuthLayout} from '../auth/AuthLayout';
import {useAuth} from '../auth/AuthProvider';
import {guessDeviceName} from '../auth/deviceName';
import {passkeyAvailability} from '../auth/passkeyAvailability';
import {PasskeyNotice} from '../components/PasskeyNotice';
import {useResource} from '../components/settings/useResource';
import {errorMessage, isCancelled} from '../lib/errors';
import {formatDateTime} from '../lib/format';

const styles = stylex.create({
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
  },
  center: {
    display: 'grid',
    placeItems: 'center',
    minHeight: '100dvh',
  },
});

const FOOTNOTE = 'Private and self-hosted. Your videos and photos stay on your server.';

function isGone(error: unknown): boolean {
  return error instanceof ApiError && (error.code === 'not_found' || error.status === 410);
}

export default function InvitePage() {
  const {code = ''} = useParams();
  const navigate = useNavigate();
  const {state, status, refresh, signOut} = useAuth();
  const check = useResource(signal => invites.check(code, signal), {key: `invite:${code}`});
  const [name, setName] = useState('');
  const [nameError, setNameError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [isGoneNow, setIsGoneNow] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [isSigningOut, setIsSigningOut] = useState(false);

  if ((status === 'loading' && !state) || check.isLoading) {
    return (
      <div {...stylex.props(styles.center)}>
        <Spinner size="lg" aria-label="Checking the invite" />
      </div>
    );
  }

  if (check.error !== undefined && !check.data && !isGone(check.error)) {
    return (
      <AuthLayout
        title="The invite could not be checked"
        subtitle={errorMessage(check.error, 'PopcornStash did not answer. Check the connection and try again.')}
        footnote={FOOTNOTE}>
        <Button variant="primary" label="Try again" onClick={check.reload} width="100%" />
      </AuthLayout>
    );
  }

  if (isGoneNow || !check.data?.valid) {
    return (
      <AuthLayout
        title="This invite link no longer works"
        subtitle="Invite links work once and expire after a few days. Ask the person who invited you for a new link."
        footnote={FOOTNOTE}>
        {state?.authenticated ? <Button variant="secondary" label="Go to PopcornStash" href="/" width="100%" /> : null}
      </AuthLayout>
    );
  }

  const invite = check.data;
  const availability = passkeyAvailability(state);
  const isSignedIn = Boolean(state?.authenticated);

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const trimmed = name.trim();
    if (!trimmed) {
      setNameError('Enter the name your family will see.');
      return;
    }
    setNameError(null);
    setFormError(null);
    setIsBusy(true);
    try {
      const flow = await auth.inviteStart({code, display_name: trimmed});
      const credential = await createPasskey(flow.options);
      await auth.inviteFinish({flow_id: flow.flow_id, credential, passkey_name: guessDeviceName()});
      await refresh();
      navigate('/', {replace: true});
    } catch (cause) {
      if (isCancelled(cause)) return;
      if (isGone(cause)) setIsGoneNow(true);
      else setFormError(errorMessage(cause));
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <AuthLayout
      title="Join PopcornStash"
      subtitle={`You are invited as ${invite.role === 'owner' ? 'an owner' : 'a member'}. Create a passkey on this device to finish. There is no password.`}
      footnote={
        invite.expires_at
          ? `This invite works once and expires ${formatDateTime(invite.expires_at)}. ${FOOTNOTE}`
          : FOOTNOTE
      }>
      {isSignedIn ? (
        <Banner
          status="info"
          title={`You are signed in as ${state?.user?.display_name ?? 'someone else'}`}
          description="This invite is for someone new. To use it on this device, sign out first."
          collapsible={false}
          endContent={
            <Button
              label="Sign out"
              variant="secondary"
              isLoading={isSigningOut}
              onClick={() => {
                setIsSigningOut(true);
                void signOut().finally(() => setIsSigningOut(false));
              }}
            />
          }
        />
      ) : null}
      <PasskeyNotice availability={availability} />
      <form onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
        <TextInput
          label="Your name"
          description="Shown to the people you share PopcornStash with."
          value={name}
          onChange={value => {
            setName(value);
            if (nameError) setNameError(null);
          }}
          autoComplete="name"
          htmlName="display_name"
          isRequired
          isDisabled={isBusy || isSignedIn}
          status={nameError ? {type: 'error', message: nameError} : undefined}
          statusVariant="detached"
          width="100%"
        />
        {formError ? (
          <Banner status="error" title="The passkey was not created" description={formError} collapsible={false} />
        ) : null}
        <Button
          type="submit"
          variant="primary"
          label="Create passkey and join"
          isLoading={isBusy}
          isDisabled={!availability.ok || isSignedIn}
          width="100%"
        />
      </form>
    </AuthLayout>
  );
}

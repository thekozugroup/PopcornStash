/**
 * Recover owner access (`/recover/:code`): the one-time link printed by
 * `popcornstash admin reset-owner` registers a new owner passkey on this
 * device (`/api/auth/recover/start` + `/finish`).
 */
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {useState} from 'react';
import {useNavigate, useParams} from 'react-router';
import {ApiError, auth} from '../api/client';
import {createPasskey} from '../api/webauthn';
import {AuthLayout} from '../auth/AuthLayout';
import {useAuth} from '../auth/AuthProvider';
import {guessDeviceName} from '../auth/deviceName';
import {passkeyAvailability} from '../auth/passkeyAvailability';
import {PasskeyNotice} from '../components/PasskeyNotice';
import {CopyValue} from '../components/settings/CopyValue';
import {errorMessage, isCancelled} from '../lib/errors';

const RESET_COMMAND = 'docker exec popcornstash popcornstash admin reset-owner';

function isGone(error: unknown): boolean {
  return (
    error instanceof ApiError &&
    (error.code === 'not_found' || error.code === 'bad_request' || error.code === 'forbidden' || error.status === 410)
  );
}

export default function RecoverPage() {
  const {code = ''} = useParams();
  const navigate = useNavigate();
  const {state, refresh} = useAuth();
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isExpired, setIsExpired] = useState(false);

  const availability = passkeyAvailability(state);

  if (isExpired) {
    return (
      <AuthLayout
        title="This recovery link no longer works"
        subtitle="Recovery links work once and expire 30 minutes after they are printed. Print a new one on the server.">
        <CopyValue
          label="Run this on the server, then open the link it prints"
          value={RESET_COMMAND}
          copyLabel="Copy command"
        />
      </AuthLayout>
    );
  }

  const recover = async () => {
    if (isBusy) return;
    setIsBusy(true);
    setError(null);
    try {
      const flow = await auth.recoverStart({code});
      const credential = await createPasskey(flow.options);
      await auth.recoverFinish({flow_id: flow.flow_id, credential, passkey_name: guessDeviceName()});
      await refresh();
      navigate('/settings/security', {replace: true});
    } catch (cause) {
      if (isCancelled(cause)) return;
      if (isGone(cause)) setIsExpired(true);
      else setError(errorMessage(cause));
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <AuthLayout
      title="Recover owner access"
      subtitle="Create a new passkey for the owner account on this device. Afterwards, remove any passkeys you lost in Settings, Security."
      footnote="This link works once and expires 30 minutes after it was printed.">
      <PasskeyNotice availability={availability} />
      {error ? (
        <Banner status="error" title="The passkey was not created" description={error} collapsible={false} />
      ) : null}
      <Button
        variant="primary"
        label="Create new owner passkey"
        onClick={() => void recover()}
        isLoading={isBusy}
        isDisabled={!availability.ok}
        width="100%"
      />
    </AuthLayout>
  );
}

/**
 * Passkey sign-in (`/api/auth/login/start` + `/finish`). Discoverable
 * credentials, so there is no username to type: one button opens the
 * browser's passkey sheet.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Spinner} from '@astryxdesign/core/Spinner';
import {Text} from '@astryxdesign/core/Text';
import {useState} from 'react';
import {Navigate, useNavigate, useSearchParams} from 'react-router';
import {ApiError, auth} from '../api/client';
import {getPasskey} from '../api/webauthn';
import {AuthLayout} from '../auth/AuthLayout';
import {useAuth} from '../auth/AuthProvider';
import {safeNextPath} from '../auth/deviceName';
import {passkeyAvailability} from '../auth/passkeyAvailability';
import {PasskeyNotice} from '../components/PasskeyNotice';
import {errorMessage, isCancelled} from '../lib/errors';

const styles = stylex.create({
  actions: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
  hint: {
    lineHeight: 1.6,
  },
  center: {
    display: 'grid',
    placeItems: 'center',
    minHeight: '100dvh',
  },
});

export default function LoginPage() {
  const {state, status, refresh, idleExpired} = useAuth();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const next = safeNextPath(params.get('next'));
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (status === 'loading' && !state) {
    return (
      <div {...stylex.props(styles.center)}>
        <Spinner size="lg" aria-label="Loading" />
      </div>
    );
  }
  if (state?.setup_required) return <Navigate to="/setup" replace />;
  if (state?.authenticated) return <Navigate to={next} replace />;

  const availability = passkeyAvailability(state);

  const signIn = async () => {
    if (isBusy) return;
    setIsBusy(true);
    setError(null);
    try {
      const flow = await auth.loginStart();
      const credential = await getPasskey(flow.options);
      await auth.loginFinish({flow_id: flow.flow_id, credential});
      await refresh(true);
      navigate(next, {replace: true});
    } catch (cause) {
      if (isCancelled(cause)) return;
      if (cause instanceof ApiError && cause.status === 401) {
        setError(
          cause.message && cause.code !== 'unauthorized'
            ? cause.message
            : 'That passkey is not registered on this server. Use a passkey you created here, or ask the owner for an invite.',
        );
      } else {
        setError(errorMessage(cause));
      }
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <AuthLayout
      title="Sign in to PopcornStash"
      subtitle={idleExpired ? "Locked after 15 minutes without activity. Use your passkey to continue." : "Use the passkey saved on this device, or scan the code your browser shows with your phone."}
      footnote="Private and self-hosted. Your videos and photos stay on your server.">
      <PasskeyNotice availability={availability} />
      {error ? (
        <Banner status="error" title="Sign-in did not finish" description={error} collapsible={false} />
      ) : null}
      <div {...stylex.props(styles.actions)}>
        <Button
          variant="primary"
          label="Sign in with a passkey"
          onClick={() => void signIn()}
          isLoading={isBusy}
          isDisabled={!availability.ok}
          width="100%"
        />
        <Text as="p" type="supporting" color="secondary" xstyle={styles.hint}>
          New here? Ask the owner for an invite link. Lost every passkey? The owner can print a recovery link from the
          server.
        </Text>
      </div>
    </AuthLayout>
  );
}

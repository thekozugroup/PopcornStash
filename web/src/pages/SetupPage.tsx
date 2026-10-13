/**
 * First run: enter the one-time setup code from the container log and create
 * the owner's passkey (`/api/auth/setup/start` + `/finish`).
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Code} from '@astryxdesign/core/Code';
import {Spinner} from '@astryxdesign/core/Spinner';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useState, type FormEvent} from 'react';
import {Navigate, useNavigate} from 'react-router';
import {ApiError, auth} from '../api/client';
import {createPasskey} from '../api/webauthn';
import {AuthLayout} from '../auth/AuthLayout';
import {useAuth} from '../auth/AuthProvider';
import {guessDeviceName} from '../auth/deviceName';
import {passkeyAvailability} from '../auth/passkeyAvailability';
import {PasskeyNotice} from '../components/PasskeyNotice';
import {errorMessage, isCancelled} from '../lib/errors';

const styles = stylex.create({
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
  },
  hint: {
    lineHeight: 1.6,
    overflowWrap: 'anywhere',
  },
  center: {
    display: 'grid',
    placeItems: 'center',
    minHeight: '100dvh',
  },
});

/** Uppercases and groups the code as XXXX-XXXX-XXXX while typing. */
function normalizeSetupCode(raw: string): string {
  const compact = raw.toUpperCase().replace(/[^A-Z0-9]/g, '').slice(0, 12);
  return compact.match(/.{1,4}/g)?.join('-') ?? '';
}

type FieldErrors = {code?: string; name?: string};

export default function SetupPage() {
  const {state, status, refresh} = useAuth();
  const navigate = useNavigate();
  const [code, setCode] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);

  if (status === 'loading' && !state) {
    return (
      <div {...stylex.props(styles.center)}>
        <Spinner size="lg" aria-label="Loading" />
      </div>
    );
  }
  if (state && !state.setup_required) {
    return <Navigate to={state.authenticated ? '/' : '/login'} replace />;
  }

  const availability = passkeyAvailability(state);

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const errors: FieldErrors = {};
    if (code.replace(/-/g, '').length !== 12) errors.code = 'Enter the 12-character code from the container log.';
    if (!displayName.trim()) errors.name = 'Enter the name your family will see.';
    setFieldErrors(errors);
    setFormError(null);
    if (errors.code || errors.name) return;

    setIsBusy(true);
    try {
      const flow = await auth.setupStart({setup_code: code, display_name: displayName.trim()});
      const credential = await createPasskey(flow.options);
      await auth.setupFinish({flow_id: flow.flow_id, credential, passkey_name: guessDeviceName()});
      await refresh();
      navigate('/', {replace: true});
    } catch (error) {
      if (isCancelled(error)) return;
      if (error instanceof ApiError && (error.code === 'bad_request' || error.code === 'forbidden')) {
        setFieldErrors({code: error.message || 'That setup code was not accepted.'});
      } else {
        setFormError(errorMessage(error));
      }
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <AuthLayout
      title="Set up PopcornStash"
      subtitle="Create the owner passkey. You will use it to sign in, protect downloads and invite your family."
      footnote="Private and self-hosted. Your videos and photos stay on your server.">
      <PasskeyNotice availability={availability} />
      <form onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
        <TextInput
          label="Setup code"
          description="Printed in the container log when PopcornStash starts."
          value={code}
          onChange={value => setCode(normalizeSetupCode(value))}
          placeholder="XXXX-XXXX-XXXX"
          autoComplete="one-time-code"
          htmlName="setup_code"
          isRequired
          isDisabled={isBusy}
          status={fieldErrors.code ? {type: 'error', message: fieldErrors.code} : undefined}
          statusVariant="detached"
          width="100%"
        />
        <TextInput
          label="Your name"
          description="Shown to the people you invite."
          value={displayName}
          onChange={setDisplayName}
          autoComplete="name"
          htmlName="display_name"
          isRequired
          isDisabled={isBusy}
          status={fieldErrors.name ? {type: 'error', message: fieldErrors.name} : undefined}
          statusVariant="detached"
          width="100%"
        />
        {formError ? (
          <Banner status="error" title="The passkey was not created" description={formError} collapsible={false} />
        ) : null}
        <Button
          type="submit"
          variant="primary"
          label="Create owner passkey"
          isLoading={isBusy}
          isDisabled={!availability.ok}
          width="100%"
        />
      </form>
      <Text as="p" type="supporting" color="secondary" xstyle={styles.hint}>
        Lost the log? Run <Code>docker exec popcornstash popcornstash admin setup-code</Code> on the server to
        print a new code. The one in the log then stops working.
      </Text>
    </AuthLayout>
  );
}

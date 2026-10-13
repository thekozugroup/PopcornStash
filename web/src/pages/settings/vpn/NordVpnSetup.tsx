/**
 * NordVPN quick setup: paste an access token, pick a country and protocol.
 * The server uses the token once to fetch service credentials and a server,
 * then forgets it. The answer is the new VPN status ("connecting"); the
 * status panel follows live `vpn` events from there.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {Link} from '@astryxdesign/core/Link';
import {RadioList, RadioListItem} from '@astryxdesign/core/RadioList';
import {Selector} from '@astryxdesign/core/Selector';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useToast} from '@astryxdesign/core/Toast';
import {useMemo, useState, type FormEvent} from 'react';
import {ApiError, vpn} from '../../../api/client';
import type {VpnProtocol, VpnStatus} from '../../../api/types';
import {Steps} from '../../../components/settings/Steps';
import {useResource} from '../../../components/settings/useResource';
import {errorMessage} from '../../../lib/errors';

const styles = stylex.create({
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: 24,
    maxWidth: 560,
  },
  howTo: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  submit: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 12,
  },
});

const FASTEST = 'fastest';

export interface NordVpnSetupProps {
  /** Primary when no VPN is set up yet; secondary when replacing one. */
  isPrimary: boolean;
  onConnected: (status: VpnStatus) => void;
}

export function NordVpnSetup({isPrimary, onConnected}: NordVpnSetupProps) {
  const showToast = useToast();
  const countries = useResource(signal => vpn.nordvpnCountries(signal), {key: 'nord-countries'});
  const [token, setToken] = useState('');
  const [country, setCountry] = useState(FASTEST);
  const [protocol, setProtocol] = useState<VpnProtocol>('wireguard');
  const [tokenError, setTokenError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);

  const countryOptions = useMemo(
    () => [
      {value: FASTEST, label: 'Fastest available', description: 'NordVPN picks the best server for you'},
      ...[...(countries.data ?? [])]
        .sort((a, b) => a.name.localeCompare(b.name))
        .map(item => ({value: String(item.id), label: item.name})),
    ],
    [countries.data],
  );

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const trimmed = token.trim();
    setFormError(null);
    if (!trimmed) {
      setTokenError('Paste the access token from your Nord Account.');
      return;
    }
    setTokenError(null);
    setIsBusy(true);
    try {
      const status = await vpn.setupNordvpn({
        access_token: trimmed,
        country_id: country === FASTEST ? undefined : Number(country),
        protocol,
      });
      setToken('');
      onConnected(status);
      showToast({
        body: status.torrents_off
          ? 'NordVPN is set up. It connects once torrents can run on this server.'
          : 'NordVPN is set up. Connecting now.',
      });
    } catch (cause) {
      if (cause instanceof ApiError && cause.code === 'bad_request') setTokenError(cause.message);
      else setFormError(errorMessage(cause, 'NordVPN could not be set up. Try again.'));
    } finally {
      setIsBusy(false);
    }
  };

  const countriesFailed = countries.error !== undefined && !countries.data;

  return (
    <form onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
      <div {...stylex.props(styles.howTo)}>
        <TextInput
          label="Access token"
          type="password"
          value={token}
          onChange={value => {
            setToken(value);
            if (tokenError) setTokenError(null);
          }}
          description="Used once to fetch your VPN details. PopcornStash does not keep it."
          autoComplete="off"
          htmlName="nordvpn_access_token"
          isRequired
          isDisabled={isBusy}
          status={tokenError ? {type: 'error', message: tokenError} : undefined}
          statusVariant="detached"
          width="100%"
        />
        <Text as="p" type="supporting" color="secondary">
          To get a token:
        </Text>
        <Steps label="How to get a token">
          <>
            Sign in to{' '}
            <Link href="https://my.nordaccount.com/" isExternalLink type="inherit">
              Nord Account
            </Link>{' '}
            and open NordVPN.
          </>
          <>Choose Set up NordVPN manually.</>
          <>Select Generate new token, then copy it here.</>
        </Steps>
      </div>

      <Selector
        label="Country"
        options={countryOptions}
        value={country}
        onChange={value => setCountry(value ?? FASTEST)}
        hasSearch
        searchPlaceholder="Search countries"
        isLoading={countries.isLoading}
        isDisabled={isBusy}
        description={
          countriesFailed ? 'The country list did not load, so NordVPN will pick the fastest server.' : undefined
        }
        presentation="adaptive"
        width="100%"
      />

      <RadioList
        label="Protocol"
        value={protocol}
        onChange={value => setProtocol(value as VpnProtocol)}
        isDisabled={isBusy}>
        <RadioListItem
          value="wireguard"
          label="WireGuard (recommended)"
          description="Faster and lighter. NordVPN calls it NordLynx."
        />
        <RadioListItem value="openvpn" label="OpenVPN" description="Use this if your network blocks WireGuard." />
      </RadioList>

      {formError ? (
        <Banner status="error" title="NordVPN is not set up" description={formError} collapsible={false} />
      ) : null}

      <div {...stylex.props(styles.submit)}>
        <Button
          type="submit"
          label="Connect NordVPN"
          variant={isPrimary ? 'primary' : 'secondary'}
          isLoading={isBusy}
        />
      </div>
    </form>
  );
}

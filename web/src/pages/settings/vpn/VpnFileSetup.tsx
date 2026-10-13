/**
 * The other ways to set up the VPN: a WireGuard `.conf`, or an OpenVPN
 * `.ovpn` with its service username and password. Both answer with the new
 * VPN status.
 */
import * as stylex from '@stylexjs/stylex';
import {Banner} from '@astryxdesign/core/Banner';
import {Button} from '@astryxdesign/core/Button';
import {FileInput} from '@astryxdesign/core/FileInput';
import {SegmentedControl, SegmentedControlItem} from '@astryxdesign/core/SegmentedControl';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useToast} from '@astryxdesign/core/Toast';
import {useState, type FormEvent} from 'react';
import {vpn} from '../../../api/client';
import type {VpnStatus} from '../../../api/types';
import {errorMessage} from '../../../lib/errors';

const styles = stylex.create({
  wrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    maxWidth: 560,
  },
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
  },
  row: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 16,
  },
  half: {
    flex: '1 1 220px',
    minWidth: 0,
  },
});

type Kind = 'wireguard' | 'openvpn';

const MAX_FILE_BYTES = 256 * 1024;

function firstFile(value: File | File[] | null): File | null {
  if (!value) return null;
  return Array.isArray(value) ? (value[0] ?? null) : value;
}

export function VpnFileSetup({onConnected}: {onConnected: (status: VpnStatus) => void}) {
  const showToast = useToast();
  const [kind, setKind] = useState<Kind>('wireguard');
  const [file, setFile] = useState<File | null>(null);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [errors, setErrors] = useState<{file?: string; username?: string; password?: string}>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);

  const extension = kind === 'wireguard' ? '.conf' : '.ovpn';

  const switchKind = (next: string) => {
    setKind(next as Kind);
    setFile(null);
    setErrors({});
    setFormError(null);
  };

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isBusy) return;
    const next: typeof errors = {};
    if (!file) next.file = `Choose the ${extension} file from your VPN provider.`;
    else if (!file.name.toLowerCase().endsWith(extension)) next.file = `That is not a ${extension} file.`;
    if (kind === 'openvpn') {
      if (!username.trim()) next.username = 'Enter the service username from your provider.';
      if (!password) next.password = 'Enter the service password from your provider.';
    }
    setErrors(next);
    setFormError(null);
    if (Object.keys(next).length > 0 || !file) return;

    setIsBusy(true);
    try {
      const status =
        kind === 'wireguard'
          ? await vpn.setupWireguardFile(file)
          : await vpn.setupOpenvpn(file, username.trim(), password);
      setFile(null);
      setPassword('');
      onConnected(status);
      showToast({body: `${kind === 'wireguard' ? 'WireGuard' : 'OpenVPN'} file saved. Connecting now.`});
    } catch (cause) {
      setFormError(errorMessage(cause, 'The file was not accepted. Try again.'));
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <div {...stylex.props(styles.wrap)}>
      <SegmentedControl label="Configuration type" value={kind} onChange={switchKind} isDisabled={isBusy}>
        <SegmentedControlItem value="wireguard" label="WireGuard file" />
        <SegmentedControlItem value="openvpn" label="OpenVPN file" />
      </SegmentedControl>
      <form onSubmit={onSubmit} noValidate {...stylex.props(styles.form)}>
        <FileInput
          key={kind}
          label={kind === 'wireguard' ? 'WireGuard file' : 'OpenVPN file'}
          description={
            kind === 'wireguard'
              ? 'A .conf file from your VPN provider. PopcornStash checks it and keeps it on the server only.'
              : 'An .ovpn file from your VPN provider, plus the service username and password from its manual setup page. These are not your account sign-in.'
          }
          value={file}
          onChange={value => {
            setFile(firstFile(value));
            setErrors(current => ({...current, file: undefined}));
          }}
          accept={extension}
          maxSize={MAX_FILE_BYTES}
          isRequired
          isDisabled={isBusy}
          status={errors.file ? {type: 'error', message: errors.file} : undefined}
          statusVariant="detached"
          width="100%"
        />
        {kind === 'openvpn' ? (
          <div {...stylex.props(styles.row)} role="group" aria-label="Service credentials">
            <div {...stylex.props(styles.half)}>
              <TextInput
                label="Service username"
                value={username}
                onChange={setUsername}
                autoComplete="off"
                isRequired
                isDisabled={isBusy}
                status={errors.username ? {type: 'error', message: errors.username} : undefined}
                statusVariant="detached"
                width="100%"
              />
            </div>
            <div {...stylex.props(styles.half)}>
              <TextInput
                label="Service password"
                type="password"
                value={password}
                onChange={setPassword}
                autoComplete="off"
                isRequired
                isDisabled={isBusy}
                status={errors.password ? {type: 'error', message: errors.password} : undefined}
                statusVariant="detached"
                width="100%"
              />
            </div>
          </div>
        ) : null}
        {formError ? (
          <Banner status="error" title="The file was not used" description={formError} collapsible={false} />
        ) : null}
        <div>
          <Button type="submit" label="Use this file" variant="secondary" isLoading={isBusy} />
        </div>
      </form>
    </div>
  );
}

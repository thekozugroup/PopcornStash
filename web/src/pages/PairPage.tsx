/**
 * Pair a device (`/pair?code=XXXX-XXXX`, signed in). The Chrome extension or
 * Android app shows a code and opens this page; the signed-in person checks
 * the device and the access it will get, then approves or denies it.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {Text} from '@astryxdesign/core/Text';
import {TextInput} from '@astryxdesign/core/TextInput';
import {useId, useState, type FormEvent} from 'react';
import {useSearchParams} from 'react-router';
import {pairing} from '../api/client';
import type {PendingPairing} from '../api/types';
import {EmptyPanel} from '../components/EmptyPanel';
import {PageHeader} from '../components/PageHeader';
import {accessSentence, clientKindName, pairingAccess, upperFirst} from '../components/settings/describe';
import {LoadError, LoadingRows} from '../components/settings/SectionStates';
import {useResource} from '../components/settings/useResource';
import {formatRelative} from '../lib/format';

const styles = stylex.create({
  column: {
    display: 'flex',
    flexDirection: 'column',
    gap: 24,
    maxWidth: 640,
  },
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 24,
    paddingBlock: {default: 28, '@media (max-width: 520px)': 20},
    paddingInline: {default: 28, '@media (max-width: 520px)': 20},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 16,
    backgroundColor: colorVars['--color-background-surface'],
  },
  device: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  codeBox: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    paddingBlock: 16,
    paddingInline: 18,
    borderRadius: 10,
    backgroundColor: colorVars['--color-background-muted'],
  },
  code: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: 24,
    fontWeight: 600,
    letterSpacing: '0.08em',
    color: colorVars['--color-text-primary'],
  },
  access: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  bullets: {
    margin: 0,
    paddingInlineStart: 20,
    listStyleType: 'disc',
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    color: colorVars['--color-text-primary'],
    lineHeight: 1.55,
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
  form: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-end',
    gap: 12,
    maxWidth: 520,
  },
  field: {
    flex: '1 1 240px',
    minWidth: 0,
  },
});

/** Uppercases and groups as XXXX-XXXX while typing. */
function normalizeCode(raw: string): string {
  const compact = raw
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, '')
    .slice(0, 8);
  return compact.match(/.{1,4}/g)?.join('-') ?? '';
}

function CodeForm({
  initial,
  onSubmit,
  isPrimary,
}: {
  initial?: string;
  onSubmit: (code: string) => void;
  isPrimary: boolean;
}) {
  const [code, setCode] = useState(initial ?? '');
  const [error, setError] = useState<string | null>(null);
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (code.replace(/-/g, '').length !== 8) {
      setError('Enter the 8-character code shown on the device.');
      return;
    }
    setError(null);
    onSubmit(code);
  };
  return (
    <form onSubmit={submit} noValidate {...stylex.props(styles.form)}>
      <div {...stylex.props(styles.field)}>
        <TextInput
          label="Code on the device"
          value={code}
          onChange={value => {
            setCode(normalizeCode(value));
            if (error) setError(null);
          }}
          placeholder="XXXX-XXXX"
          autoComplete="one-time-code"
          status={error ? {type: 'error', message: error} : undefined}
          statusVariant="detached"
          width="100%"
        />
      </div>
      <Button type="submit" label="Find device" variant={isPrimary ? 'primary' : 'secondary'} />
    </form>
  );
}

type Outcome = {kind: 'approved' | 'denied'; name: string};

function Request({request, onOutcome}: {request: PendingPairing; onOutcome: (outcome: Outcome) => void}) {
  const access = pairingAccess(request.client_kind);
  const [error, setError] = useState<unknown>(undefined);
  const decide = async (approve: boolean) => {
    setError(undefined);
    try {
      if (approve) await pairing.approve(request.user_code);
      else await pairing.deny(request.user_code);
      onOutcome({kind: approve ? 'approved' : 'denied', name: request.client_name});
    } catch (cause) {
      setError(cause);
    }
  };
  return (
    <section aria-label="Device asking to pair" {...stylex.props(styles.panel)}>
      <div {...stylex.props(styles.device)}>
        <Text as="p" type="large" weight="semibold">
          {request.client_name}
        </Text>
        <Text as="p" color="secondary">
          {clientKindName(request.client_kind)}, asked {formatRelative(request.created_at).toLowerCase()}
          {request.ip ? ` from ${request.ip}` : ''}
        </Text>
      </div>
      <div {...stylex.props(styles.codeBox)}>
        <Text type="supporting" color="secondary">
          Check that the device shows this code
        </Text>
        <span {...stylex.props(styles.code)}>{request.user_code}</span>
      </div>
      <div {...stylex.props(styles.access)}>
        {access.can.length === 1 ? (
          // One permission reads as a sentence, not a one-item list.
          <Text as="p">{accessSentence(access.can, 'If you approve, it can')}</Text>
        ) : (
          <>
            <Text type="label">If you approve, it can</Text>
            <ul {...stylex.props(styles.bullets)}>
              {access.can.map(item => (
                <li key={item}>{upperFirst(item)}</li>
              ))}
            </ul>
          </>
        )}
        {access.cannot ? (
          <Text as="p" type="supporting" color="secondary">
            {access.cannot}
          </Text>
        ) : null}
      </div>
      {error !== undefined ? <LoadError title="That did not work" error={error} /> : null}
      <div {...stylex.props(styles.actions)}>
        <Button label="Approve" variant="primary" clickAction={() => decide(true)} />
        <Button label="Deny" variant="secondary" clickAction={() => decide(false)} />
      </div>
    </section>
  );
}

export default function PairPage() {
  const [params, setParams] = useSearchParams();
  const code = normalizeCode(params.get('code') ?? '');
  const hasCode = code.replace(/-/g, '').length === 8;
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const expiredId = useId();
  const pending = useResource(signal => pairing.pending(signal), {enabled: hasCode && !outcome, key: `pair:${code}`});

  const lookUp = (next: string) => {
    setOutcome(null);
    setParams({code: next}, {replace: true});
  };

  const request = pending.data?.find(item => normalizeCode(item.user_code) === code) ?? null;

  let body;
  if (outcome?.kind === 'approved') {
    body = (
      <EmptyPanel
        title={`${outcome.name} is connected`}
        description="You can close this tab. Revoke it any time in Settings, Devices."
        actions={<Button label="Open Devices" variant="secondary" href="/settings/devices" />}
      />
    );
  } else if (outcome?.kind === 'denied') {
    body = (
      <EmptyPanel
        title="Request denied"
        description="The device was not connected. If that was a mistake, start pairing again on the device."
        actions={<Button label="Open Devices" variant="secondary" href="/settings/devices" />}
      />
    );
  } else if (!hasCode) {
    body = (
      <div {...stylex.props(styles.panel)}>
        <Text as="p" color="secondary">
          Open the Chrome extension or the Android app and start pairing. Type the code it shows here.
        </Text>
        <CodeForm onSubmit={lookUp} isPrimary />
      </div>
    );
  } else if (pending.isLoading) {
    body = <LoadingRows rows={2} label="Looking up the device" />;
  } else if (pending.error !== undefined && !pending.data) {
    body = <LoadError title="The device could not be looked up" error={pending.error} onRetry={pending.reload} />;
  } else if (!request) {
    // One panel with one action: say what happened, then take a new code.
    body = (
      <section aria-labelledby={expiredId} {...stylex.props(styles.panel)}>
        <div {...stylex.props(styles.device)}>
          <Heading level={2} id={expiredId}>
            This code has expired or was already used
          </Heading>
          <Text as="p" color="secondary">
            No device is waiting with that code. Codes last 10 minutes. Start pairing again on the device, then type
            the new code here.
          </Text>
        </div>
        <CodeForm key={code} onSubmit={lookUp} isPrimary />
      </section>
    );
  } else {
    body = <Request request={request} onOutcome={setOutcome} />;
  }

  return (
    <>
      <PageHeader title="Pair a device" subtitle="Connect the Chrome extension or the Android app to your account." />
      <div {...stylex.props(styles.column)}>{body}</div>
    </>
  );
}

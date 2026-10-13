/**
 * Settings, Devices (everyone): pairing requests waiting for approval, the
 * devices and tokens connected to this account, a way to create a token by
 * hand, and how to install the Chrome extension.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, typographyVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Link} from '@astryxdesign/core/Link';
import {ListItem} from '@astryxdesign/core/List';
import {Text} from '@astryxdesign/core/Text';
import {useToast} from '@astryxdesign/core/Toast';
import {useState} from 'react';
import {pairing, tokens} from '../../api/client';
import type {PendingPairing, Token} from '../../api/types';
import {useAuth} from '../../auth/AuthProvider';
import {clientKindName, pairingAccess, scopesInWords} from '../../components/settings/describe';
import {EmptyRows, LoadError, LoadingRows, RowsPanel, PanelList} from '../../components/settings/SectionStates';
import {SettingsGroup} from '../../components/settings/SettingsGroup';
import {Steps} from '../../components/settings/Steps';
import {useConfirm} from '../../components/settings/useConfirm';
import {usePolling} from '../../components/settings/usePolling';
import {useResource} from '../../components/settings/useResource';
import {formatDate, formatRelative} from '../../lib/format';
import {errorMessage} from '../../lib/errors';
import {CreateTokenDialog} from './devices/CreateTokenDialog';
import {SettingsSectionHeader} from './SettingsSectionHeader';

const EXTENSION_DOCS = 'https://github.com/thekozugroup/PopcornStash/blob/main/docs/extension.md';
const PENDING_POLL_MS = 5000;

const styles = stylex.create({
  groups: {
    display: 'flex',
    flexDirection: 'column',
  },
  request: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    columnGap: 24,
    rowGap: 16,
    paddingBlock: 20,
    paddingInline: {default: 20, '@media (max-width: 520px)': 16},
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  requestText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    flex: '1 1 280px',
    minWidth: 0,
  },
  code: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: {default: 'flex-end', '@media (max-width: 640px)': 'flex-start'},
    gap: 2,
  },
  codeValue: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: 20,
    fontWeight: 600,
    letterSpacing: '0.06em',
    color: colorVars['--color-text-primary'],
    fontVariantNumeric: 'tabular-nums',
  },
  requestActions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
    flexBasis: '100%',
  },
  meta: {
    lineHeight: 1.55,
  },
  url: {
    fontFamily: typographyVars['--font-family-code'],
    fontSize: '0.8125rem',
    color: colorVars['--color-text-primary'],
    overflowWrap: 'anywhere',
  },
});

function PendingRequest({
  request,
  isOnly,
  onDone,
}: {
  request: PendingPairing;
  /** One request: Approve is the page's primary action. Several: all stay secondary. */
  isOnly: boolean;
  onDone: () => void;
}) {
  const showToast = useToast();
  const access = pairingAccess(request.client_kind);
  const decide = async (approve: boolean) => {
    try {
      if (approve) await pairing.approve(request.user_code);
      else await pairing.deny(request.user_code);
      showToast({
        body: approve ? `${request.client_name} is connected.` : `${request.client_name} was not connected.`,
      });
    } catch (cause) {
      showToast({type: 'error', body: errorMessage(cause, 'That did not work. Try again.')});
    } finally {
      onDone();
    }
  };
  return (
    <li {...stylex.props(styles.request)}>
      <div {...stylex.props(styles.requestText)}>
        <Text weight="medium">{request.client_name}</Text>
        <Text as="p" type="supporting" color="secondary" xstyle={styles.meta}>
          {clientKindName(request.client_kind)}, asked {formatRelative(request.created_at).toLowerCase()}
          {request.ip ? ` from ${request.ip}` : ''}
        </Text>
        <Text as="p" type="supporting" color="secondary" xstyle={styles.meta}>
          {access.can.join('. ')}. {access.cannot ?? ''}
        </Text>
      </div>
      <div {...stylex.props(styles.code)}>
        <Text type="supporting" color="secondary">
          Code on the device
        </Text>
        <span {...stylex.props(styles.codeValue)}>{request.user_code}</span>
      </div>
      <div {...stylex.props(styles.requestActions)}>
        <Button label="Approve" variant={isOnly ? 'primary' : 'secondary'} clickAction={() => decide(true)} />
        <Button label="Deny" variant="secondary" clickAction={() => decide(false)} />
      </div>
    </li>
  );
}

function TokenRow({token, onRevoke}: {token: Token; onRevoke: (token: Token) => void}) {
  const lastUsed = token.last_used_at
    ? `Last used ${formatRelative(token.last_used_at).toLowerCase()}`
    : 'Not used yet';
  return (
    <ListItem
      label={token.name}
      description={
        <>
          {clientKindName(token.client_kind)}. {scopesInWords(token.scopes)}.
          <br />
          {lastUsed}. Added {formatDate(token.created_at)}. Starts with {token.prefix}.
        </>
      }
      endContent={<Button label="Revoke" variant="secondary" onClick={() => onRevoke(token)} />}
    />
  );
}

export default function DevicesSettingsPage() {
  const {state} = useAuth();
  const showToast = useToast();
  const pending = useResource(signal => pairing.pending(signal), {key: 'pairing-pending'});
  const tokenList = useResource(signal => tokens.list(signal), {key: 'tokens'});
  const {ask, dialog} = useConfirm();
  const [isCreating, setIsCreating] = useState(false);

  usePolling(pending.reload, PENDING_POLL_MS);

  const serverAddress = state?.public_url ?? window.location.origin;

  const revoke = (token: Token) =>
    ask({
      title: `Revoke ${token.name}?`,
      description: 'It stops working right away and will need to pair again. Downloads it already added keep going.',
      actionLabel: 'Revoke',
      failure: 'The token was not revoked. Try again.',
      run: async () => {
        await tokens.revoke(token.id);
        tokenList.mutate(list => list.filter(item => item.id !== token.id));
        showToast({body: `${token.name} was revoked.`});
      },
    });

  const pendingRequests = pending.data ?? [];

  return (
    <>
      <SettingsSectionHeader
        title="Devices"
        subtitle="The Chrome extension, phones and other apps connected to your account."
      />
      <div {...stylex.props(styles.groups)}>
        {pendingRequests.length > 0 ? (
          <SettingsGroup
            title="Waiting for approval"
            description="Check that the code matches the one on the device before you approve it. Codes expire after 10 minutes.">
            <RowsPanel>
              <ul aria-label="Pairing requests">
                {pendingRequests.map(request => (
                  <PendingRequest
                    key={request.pairing_id}
                    request={request}
                    isOnly={pendingRequests.length === 1}
                    onDone={() => {
                      pending.reload();
                      tokenList.reload();
                    }}
                  />
                ))}
              </ul>
            </RowsPanel>
          </SettingsGroup>
        ) : null}

        <SettingsGroup
          title="Connected devices"
          description="Each device has its own token. Revoke one if a device is lost or you stop using it."
          actions={<Button label="Create a token" variant="secondary" onClick={() => setIsCreating(true)} />}>
          {tokenList.isLoading ? (
            <LoadingRows rows={3} label="Loading devices" />
          ) : tokenList.error !== undefined && !tokenList.data ? (
            <LoadError title="Devices did not load" error={tokenList.error} onRetry={tokenList.reload} />
          ) : (tokenList.data ?? []).length === 0 ? (
            <EmptyRows>
              No devices yet. Pair the Chrome extension or the Android app, or create a token for a script.
            </EmptyRows>
          ) : (
            <PanelList density="spacious">
              {(tokenList.data ?? []).map(token => (
                <TokenRow key={token.id} token={token} onRevoke={revoke} />
              ))}
            </PanelList>
          )}
          {pending.error !== undefined && !pending.data ? (
            <LoadError title="Pairing requests did not load" error={pending.error} onRetry={pending.reload} />
          ) : null}
        </SettingsGroup>

        <SettingsGroup
          title="Chrome extension"
          description="Send links, magnets and files from Chrome straight to Downloads.">
          <Steps label="How to connect the Chrome extension">
            <>
              Install the extension.{' '}
              <Link href={EXTENSION_DOCS} isExternalLink type="inherit">
                How to install it
              </Link>
            </>
            <>
              Open it and enter this address: <span {...stylex.props(styles.url)}>{serverAddress}</span>
            </>
            <>It shows a code. Approve it on the page it opens, or here, once the codes match.</>
          </Steps>
        </SettingsGroup>
      </div>

      <CreateTokenDialog
        isOpen={isCreating}
        onOpenChange={setIsCreating}
        onCreated={({token: _secret, ...created}) =>
          tokenList.mutate(list => [created, ...list.filter(item => item.id !== created.id)])
        }
      />
      {dialog}
    </>
  );
}

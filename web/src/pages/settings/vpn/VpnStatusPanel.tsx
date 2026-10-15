/**
 * The VPN status in words with a status dot, what the VPN protects, the
 * connection facts, and Reconnect / Remove. Updates live from `vpn` events.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {MetadataList, MetadataListItem} from '@astryxdesign/core/MetadataList';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {StatusDot} from '@astryxdesign/core/StatusDot';
import {Text} from '@astryxdesign/core/Text';
import {Timestamp} from '@astryxdesign/core/Timestamp';
import {useToast} from '@astryxdesign/core/Toast';
import type {Ref} from 'react';
import {vpn} from '../../../api/client';
import type {VpnStatus} from '../../../api/types';
import {SeparatedParts} from '../../../components/SeparatedParts';
import {LoadError} from '../../../components/settings/SectionStates';
import {useConfirm} from '../../../components/settings/useConfirm';
import {errorMessage} from '../../../lib/errors';
import {providerBrand} from '../../../lib/vpn';
import {protocolName, vpnView} from './vpnView';

const styles = stylex.create({
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 20,
    scrollMarginTop: 88,
    outlineStyle: 'none',
    paddingBlock: {default: 28, '@media (max-width: 520px)': 20},
    paddingInline: {default: 28, '@media (max-width: 520px)': 20},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 'var(--radius-container)',
    backgroundColor: colorVars['--color-background-surface'],
    minWidth: 0,
  },
  head: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    columnGap: 24,
    rowGap: 16,
  },
  status: {
    display: 'grid',
    gridTemplateColumns: 'auto minmax(0, 1fr)',
    columnGap: 12,
    rowGap: 6,
    alignItems: 'baseline',
    flex: '1 1 380px',
    minWidth: 0,
  },
  dot: {
    display: 'inline-flex',
    transform: 'translateY(-2px)',
  },
  title: {
    overflowWrap: 'anywhere',
    textWrap: 'balance',
  },
  detail: {
    gridColumnStart: '2',
    lineHeight: 1.55,
    maxWidth: '68ch',
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
  },
  explain: {
    gridColumnStart: '2',
    lineHeight: 1.55,
    maxWidth: '68ch',
  },
  facts: {
    paddingTop: 20,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  // Facts read as columns: MetadataList spaces items 16px apart, and this
  // trailing room doubles that between columns without widening wrapped rows.
  fact: {
    paddingInlineEnd: 16,
  },
  skeleton: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
});

const DOT = {connected: 'success', warning: 'warning', error: 'error', neutral: 'neutral'} as const;

export interface VpnStatusPanelProps {
  status: VpnStatus | null;
  error: unknown;
  isLoading: boolean;
  onReload: () => void;
  onChange: (status: VpnStatus) => void;
  /** Focus target after a setup, so the new status is seen and announced. */
  panelRef?: Ref<HTMLElement>;
}

export function VpnStatusPanel({status, error, isLoading, onReload, onChange, panelRef}: VpnStatusPanelProps) {
  const showToast = useToast();
  const {ask, dialog} = useConfirm();

  if (isLoading) {
    return (
      <section ref={panelRef} tabIndex={-1} aria-label="VPN status" aria-busy="true" {...stylex.props(styles.panel)}>
        <div {...stylex.props(styles.skeleton)}>
          <Skeleton width="46%" height={22} />
          <Skeleton width="70%" height={14} index={1} />
        </div>
      </section>
    );
  }
  if (!status) {
    return <LoadError title="The VPN status did not load" error={error} onRetry={onReload} />;
  }

  const view = vpnView(status, false);
  // NordVPN's brand (the country is in the status line, the server below), or an
  // uploaded file's label ("WireGuard · home-router"), which already names the protocol.
  // Listed only when the status line above does not already name it (DESIGN.md, Say It
  // Once): "Protected · NordVPN · Netherlands" needs no "Provider: NordVPN" under it.
  const providerLabel =
    status.provider === 'nordvpn' ? providerBrand(status) : (status.label ?? providerBrand(status));
  const provider =
    providerLabel && ![view.title, view.detail].some(line => line?.includes(providerLabel)) ? providerLabel : null;
  const protocol = status.provider === 'nordvpn' ? protocolName(status.protocol) : null;
  // Without a VPN in use there is no tunnel to reconnect; a saved VPN can still be removed.
  const canReconnect = status.configured && !view.isDegraded && !view.isNoVpn;

  const reconnect = async () => {
    try {
      onChange(await vpn.reconnect());
    } catch (cause) {
      showToast({type: 'error', body: errorMessage(cause, 'The VPN did not restart. Try again.')});
    }
  };

  const remove = () =>
    ask({
      title: 'Remove the VPN?',
      description:
        status.provider === 'nordvpn'
          ? 'Torrents stop until you set up a VPN again. Web links and uploads keep working. PopcornStash did not keep your NordVPN token, so you will need a new one to connect again.'
          : 'Torrents stop until you set up a VPN again. Web links and uploads keep working. The configuration file is deleted from the server.',
      actionLabel: 'Remove VPN',
      failure: 'The VPN was not removed. Try again.',
      run: async () => {
        onChange(await vpn.remove());
        showToast({body: 'VPN removed. Torrents are stopped until you set one up again.'});
      },
    });

  return (
    <section ref={panelRef} tabIndex={-1} aria-label="VPN status" {...stylex.props(styles.panel)}>
      <div {...stylex.props(styles.head)}>
        <div {...stylex.props(styles.status)} role="status">
          <span aria-hidden="true" {...stylex.props(styles.dot)}>
            <StatusDot variant={DOT[view.tone]} label={view.tone} isPulsing={status.state === 'connecting'} />
          </span>
          <Text as="p" type="large" weight="semibold" xstyle={styles.title}>
            {view.titleParts ? <SeparatedParts parts={view.titleParts} /> : view.title}
          </Text>
          {view.detail ? (
            <Text as="p" color="secondary" xstyle={styles.detail}>
              {view.detail}
            </Text>
          ) : null}
          {view.isDegraded || view.isNoVpn ? null : (
            <Text as="p" type="supporting" color="secondary" xstyle={styles.explain}>
              Only torrent traffic goes through the VPN, so streaming and this page stay on your own network. A kill
              switch stops torrents the moment the tunnel drops, and their DNS lookups go through the tunnel too.
            </Text>
          )}
        </div>
        {status.configured ? (
          <div {...stylex.props(styles.actions)}>
            {canReconnect ? (
              <Button
                label="Reconnect"
                variant="secondary"
                clickAction={reconnect}
                isDisabled={status.state === 'connecting'}
              />
            ) : null}
            <Button label="Remove" variant="secondary" onClick={remove} />
          </div>
        ) : null}
      </div>

      {(status.configured || view.isNoVpn) && !view.isDegraded ? (
        <div {...stylex.props(styles.facts)}>
          <MetadataList orientation="horizontal">
            {status.public_ip ? (
              <MetadataListItem label="Public address" xstyle={styles.fact}>{status.public_ip}</MetadataListItem>
            ) : null}
            {/* Without a VPN in use, a saved VPN's facts would describe a tunnel nothing goes through. */}
            {provider && !view.isNoVpn ? (
              <MetadataListItem label="Provider" xstyle={styles.fact}>{provider}</MetadataListItem>
            ) : null}
            {status.server && !view.isNoVpn ? (
              <MetadataListItem label="Server" xstyle={styles.fact}>{status.server}</MetadataListItem>
            ) : null}
            {protocol && !view.isNoVpn ? (
              <MetadataListItem label="Protocol" xstyle={styles.fact}>{protocol}</MetadataListItem>
            ) : null}
            {status.state === 'connected' && status.since && !view.isNoVpn ? (
              <MetadataListItem label="Connected" xstyle={styles.fact}>
                <Timestamp value={status.since} format="relative" type="inherit" color="inherit" />
              </MetadataListItem>
            ) : null}
            <MetadataListItem label="Kill switch" xstyle={styles.fact}>{status.kill_switch ? 'On' : 'Off'}</MetadataListItem>
          </MetadataList>
        </div>
      ) : null}

      {dialog}
    </section>
  );
}

/**
 * One line about the VPN when it affects torrents: blocked, connecting, not
 * set up, unavailable, or allowed to run without a VPN (a permanent warning).
 * The owner gets a link to Settings, VPN; family members get an explanation
 * of what still works. Renders nothing while torrents are protected.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Text} from '@astryxdesign/core/Text';
import type {VpnStatus} from '../../api/types';
import {describeVpn, isNoVpnMode, torrentsAffected, torrentsOff, torrentsOffReason} from '../../lib/vpn';
import {StatusTag} from '../StatusTag';

const styles = stylex.create({
  line: {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    columnGap: 12,
    rowGap: 6,
    margin: 0,
    minWidth: 0,
  },
  detail: {
    flex: '1 1 260px',
    minWidth: 0,
    lineHeight: 1.5,
  },
  // A ghost button keeps the 44px target; its label lines up with the panel's text.
  settings: {
    flexShrink: 0,
    marginInline: -12,
  },
});

/** Off on purpose: the tag already says "Torrents off", so only a real reason is added. */
function offDetail(status: VpnStatus): string {
  return [torrentsOffReason(status), 'Web links and uploads still work.'].filter(Boolean).join(' ');
}

function memberDetail(status: VpnStatus | null, isUnavailable: boolean): string {
  if (isUnavailable || !status) return 'Web links and uploads still work.';
  if (torrentsOff(status)) return offDetail(status);
  if (isNoVpnMode(status)) {
    return 'The server is set up this way.';
  }
  switch (status.state) {
    case 'connecting':
    case 'disconnected':
      return 'Torrents wait until the VPN connects. Web links and uploads still work.';
    case 'not_configured':
      return 'Torrents need a VPN, and the owner has not set one up yet. Web links and uploads still work.';
    default:
      return 'Torrents are stopped until the owner fixes the VPN. Web links and uploads still work.';
  }
}

function ownerDetail(status: VpnStatus | null, isUnavailable: boolean): string {
  if (isUnavailable || !status) return 'Web links and uploads still work.';
  if (torrentsOff(status)) return offDetail(status);
  if (isNoVpnMode(status)) {
    return 'STASH_ALLOW_NO_VPN is on. Your router must protect the traffic.';
  }
  switch (status.state) {
    case 'connecting':
    case 'disconnected':
      return 'Torrents wait until the VPN connects. Web links and uploads still work.';
    case 'not_configured':
      return 'Set up a VPN to start torrents. Web links and uploads still work.';
    case 'error':
      return status.error ?? 'The VPN could not connect. Torrents stay stopped until it does.';
    default:
      return describeVpn(status).detail;
  }
}

export interface VpnLineProps {
  status: VpnStatus | null;
  isUnavailable: boolean;
  isOwner: boolean;
}

export function VpnLine({status, isUnavailable, isOwner}: VpnLineProps) {
  if (!torrentsAffected(status, isUnavailable)) return null;
  const summary = describeVpn(status, {isUnavailable});
  const noVpn = isNoVpnMode(status);
  // While the VPN connects there is nothing to do but wait.
  const needsSettings = isOwner && (noVpn || status?.state !== 'connecting');
  return (
    <div {...stylex.props(styles.line)}>
      <StatusTag tone={summary.tone} live>
        {summary.label}
      </StatusTag>
      <Text as="p" type="supporting" color="secondary" xstyle={styles.detail}>
        {isOwner ? ownerDetail(status, isUnavailable) : memberDetail(status, isUnavailable)}
      </Text>
      {needsSettings ? (
        <Button
          label={status?.state === 'not_configured' && !noVpn ? 'Set up the VPN' : 'Open VPN settings'}
          variant="ghost"
          href="/settings/vpn"
          xstyle={styles.settings}
        />
      ) : null}
    </div>
  );
}

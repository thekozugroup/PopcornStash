/**
 * VPN status in plain words, the same phrasing as the web app's status tag
 * (web/src/lib/vpn.ts). Torrents only run through the tunnel, so every state
 * says what happens to torrents.
 */

import type {VpnProvider, VpnStatus} from './types';

export type StatusTone = 'connected' | 'warning' | 'error' | 'neutral';

export interface VpnSummary {
  tone: StatusTone;
  /** Short line for the tag: "Protected · NordVPN · Netherlands". */
  label: string;
  /** One sentence shown under the tag when torrents are not running. */
  detail: string;
}

const PROVIDER_NAMES: Record<VpnProvider, string> = {
  nordvpn: 'NordVPN',
  wireguard: 'WireGuard',
  openvpn: 'OpenVPN',
};

function providerName(status: VpnStatus): string | null {
  if (status.label) return status.label;
  return status.provider ? (PROVIDER_NAMES[status.provider] ?? null) : null;
}

export function describeVpn(status: VpnStatus | null, unavailable = false): VpnSummary {
  if (!status) {
    return unavailable
      ? {tone: 'neutral', label: 'VPN status unavailable', detail: 'PopcornStash could not read the VPN status.'}
      : {tone: 'neutral', label: 'Checking VPN', detail: 'Checking the VPN connection.'};
  }

  if (status.allow_no_vpn && status.state === 'not_configured') {
    return {
      tone: 'warning',
      label: 'No VPN: torrents use this network',
      detail: 'This server allows torrents without a VPN. Your router must protect the traffic.',
    };
  }

  switch (status.state) {
    case 'connected':
      if (status.protected === false) {
        return {
          tone: 'error',
          label: 'Torrents blocked: VPN not protecting',
          detail: 'The tunnel is up, but the public address is not a VPN address. Torrents stay stopped.',
        };
      }
      return {
        tone: 'connected',
        label: ['Protected', providerName(status), status.country].filter(Boolean).join(' · '),
        detail: 'Torrents run only through the VPN.',
      };
    case 'connecting':
      return {
        tone: 'warning',
        label: 'Torrents paused: VPN connecting',
        detail: 'Torrents wait until the VPN is connected. Web links still download.',
      };
    case 'disconnected':
      return {
        tone: 'warning',
        label: 'Torrents paused: VPN disconnected',
        detail: 'Torrents wait until the VPN reconnects. Web links still download.',
      };
    case 'error':
      return {
        tone: 'error',
        label: 'Torrents blocked: VPN error',
        detail: status.error ?? 'The VPN could not connect. Torrents stay stopped until it does.',
      };
    case 'unreachable':
      return {
        tone: 'error',
        label: 'Torrents blocked: VPN service offline',
        detail: 'The torrent service is not answering. Torrents stay stopped until it is back.',
      };
    case 'not_configured':
    default:
      return {
        tone: 'error',
        label: 'Torrents blocked: no VPN',
        detail: 'The owner can set up a VPN in PopcornStash Settings. Web links still download.',
      };
  }
}

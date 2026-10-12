/**
 * VPN status in plain words, shared by the sidebar tag, Downloads and
 * Settings. Torrents only run through the tunnel, so every state says what
 * happens to torrents.
 */
import type {VpnProvider, VpnStatus} from '../api/types';
import type {StatusTone} from '../components/StatusTag';

const PROVIDER_NAMES: Record<VpnProvider, string> = {
  nordvpn: 'NordVPN',
  wireguard: 'WireGuard',
  openvpn: 'OpenVPN',
};

export interface VpnSummary {
  tone: StatusTone;
  /** Short line for tags: "Protected · NordVPN · Netherlands". */
  label: string;
  /** One sentence for pages and tooltips. */
  detail: string;
}

export function providerName(status: VpnStatus): string | null {
  if (status.label) return status.label;
  return status.provider ? PROVIDER_NAMES[status.provider] : null;
}

export function describeVpn(status: VpnStatus | null, options: {isUnavailable?: boolean} = {}): VpnSummary {
  if (!status) {
    return options.isUnavailable
      ? {tone: 'neutral', label: 'VPN status unavailable', detail: 'PopcornStash could not read the VPN status.'}
      : {tone: 'neutral', label: 'Checking VPN', detail: 'Checking the VPN connection.'};
  }

  const provider = providerName(status);

  if (status.allow_no_vpn && status.state === 'not_configured') {
    return {
      tone: 'warning',
      label: 'No VPN: torrents use this network',
      detail:
        'Torrents are allowed without a VPN because this server is set up that way. Your router must protect the traffic.',
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
        label: ['Protected', provider, status.country].filter(Boolean).join(' · '),
        detail: status.public_ip
          ? `Torrents run only through the VPN. Public address ${status.public_ip}.`
          : 'Torrents run only through the VPN.',
      };
    case 'connecting':
      return {
        tone: 'warning',
        label: 'Torrents paused: VPN connecting',
        detail: 'Torrents wait until the VPN is connected. Nothing uses your home connection.',
      };
    case 'disconnected':
      return {
        tone: 'warning',
        label: 'Torrents paused: VPN disconnected',
        detail: 'Torrents wait until the VPN reconnects. Nothing uses your home connection.',
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
        detail: 'The VPN container is not answering. Torrents stay stopped until it is back.',
      };
    case 'not_configured':
    default:
      return {
        tone: 'error',
        label: 'Torrents blocked: no VPN',
        detail: 'Set up a VPN in Settings to start torrents. Web links and uploads still work.',
      };
  }
}

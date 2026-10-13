/**
 * The VPN status in words for the VPN settings page. Builds on the shared
 * `describeVpn` (sidebar tag) and adds the page-level sentences, including
 * the container setup case where torrents cannot run at all.
 */
import type {VpnStatus} from '../../../api/types';
import type {StatusTone} from '../../../components/StatusTag';
import {describeVpn, providerName} from '../../../lib/vpn';

export interface VpnView {
  tone: StatusTone;
  /** The status line next to the dot. */
  title: string;
  /** One or two sentences under it, or null. */
  detail: string | null;
  /** Torrents cannot run on this server (degraded container or no torrent engine). */
  isDegraded: boolean;
}

/** Torrents are off on this server by design; `status.error` says why. */
export function isTorrentsTurnedOff(status: VpnStatus): boolean {
  return status.torrents_off === true;
}

export function vpnView(status: VpnStatus | null, isUnavailable: boolean): VpnView {
  if (!status) {
    return isUnavailable
      ? {
          tone: 'neutral',
          title: 'VPN status unavailable',
          detail: 'PopcornStash could not read the VPN status.',
          isDegraded: false,
        }
      : {tone: 'neutral', title: 'Checking the VPN', detail: null, isDegraded: false};
  }

  if (isTorrentsTurnedOff(status)) {
    // The server's reason: no NET_ADMIN or /dev/net/tun, the kill switch failed, or
    // the web server runs without the supervisor.
    return {
      tone: 'error',
      title: 'Torrents are off on this server',
      detail: `${status.error ?? 'Torrents are turned off on this server.'} Web links and uploads still work.`,
      isDegraded: true,
    };
  }

  const summary = describeVpn(status);
  const provider = providerName(status);

  switch (status.state) {
    case 'connected':
      if (status.protected === false)
        return {tone: summary.tone, title: summary.label, detail: summary.detail, isDegraded: false};
      // "Protected · NordVPN · Netherlands" (DESIGN.md). The public address,
      // the proof of protection, is listed with the connection facts.
      return {
        tone: 'connected',
        title: ['Protected', provider, status.country].filter(Boolean).join(' · '),
        detail: null,
        isDegraded: false,
      };
    case 'not_configured':
      if (status.allow_no_vpn)
        return {tone: summary.tone, title: summary.label, detail: summary.detail, isDegraded: false};
      return {
        tone: 'error',
        title: 'Torrents blocked: no VPN set up',
        detail: 'Connect a VPN below to start torrents. Web links and uploads already work.',
        isDegraded: false,
      };
    case 'connecting':
      return {
        tone: 'warning',
        title: 'Torrents paused: VPN connecting',
        detail: provider
          ? `Connecting to ${provider}${status.country ? ` in ${status.country}` : ''}. Torrents wait until the tunnel is up, and nothing uses your home connection.`
          : summary.detail,
        isDegraded: false,
      };
    case 'unreachable':
      return {
        tone: 'error',
        title: 'Torrents blocked: the torrent engine is not answering',
        detail: status.error ?? summary.detail,
        isDegraded: false,
      };
    case 'error':
      return {
        tone: 'error',
        title: 'Torrents blocked: the VPN could not connect',
        detail: status.error ?? summary.detail,
        isDegraded: false,
      };
    case 'disconnected':
    default:
      return {tone: summary.tone, title: summary.label, detail: summary.detail, isDegraded: false};
  }
}

export function protocolName(protocol: VpnStatus['protocol']): string | null {
  if (protocol === 'wireguard') return 'WireGuard';
  if (protocol === 'openvpn') return 'OpenVPN';
  return null;
}

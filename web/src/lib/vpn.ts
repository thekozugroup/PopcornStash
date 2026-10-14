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

/**
 * A provider id as its name ("nordvpn" → "NordVPN", "wireguard" → "WireGuard",
 * "openvpn" → "OpenVPN"). An id this version does not know is shown as stored.
 */
export function providerName(provider: string | null | undefined): string | null {
  if (!provider) return null;
  return Object.hasOwn(PROVIDER_NAMES, provider) ? PROVIDER_NAMES[provider as VpnProvider] : provider;
}

/** The provider as a brand: "NordVPN", "WireGuard", "OpenVPN". */
export function providerBrand(status: VpnStatus): string | null {
  return providerName(status.provider);
}

/**
 * What the VPN is called in status lines. NordVPN reads as the brand and the
 * country ("NordVPN · Netherlands"; the server name is listed with the
 * connection facts). An uploaded file keeps the server's label, which already
 * names the file type ("WireGuard · home-router"). The country is added only
 * when the label does not already carry it ("NordVPN · Netherlands · nl812").
 */
export function vpnName(status: VpnStatus): string | null {
  const base = status.provider === 'nordvpn' ? 'NordVPN' : (status.label ?? providerBrand(status));
  if (!base) return status.country;
  if (!status.country || base.split(' · ').includes(status.country)) return base;
  return `${base} · ${status.country}`;
}

/**
 * The parts of "Protected · NordVPN · Netherlands" (DESIGN.md), never a part
 * twice. Status lines that may wrap render them as parts so a line never
 * starts or ends with a dangling separator.
 */
export function protectedParts(status: VpnStatus): string[] {
  const name = vpnName(status);
  return ['Protected', ...(name ? name.split(' · ') : [])].filter(part => part.trim() !== '');
}

/** "Protected · NordVPN · Netherlands" as one line of text. */
export function protectedLabel(status: VpnStatus): string {
  return protectedParts(status).join(' · ');
}

/**
 * The documented fallback for `error` when torrents are off and the
 * supervisor gave no reason (architecture section 11). It only repeats the
 * state, so pages that already say "Torrents off" leave it out.
 */
const TORRENTS_OFF_FALLBACK = 'Torrents are turned off on this server.';

/** Why torrents are off, when the server says more than "turned off". */
export function torrentsOffReason(status: VpnStatus): string | null {
  const reason = status.error?.trim();
  return reason && reason !== TORRENTS_OFF_FALLBACK ? reason : null;
}

/**
 * The server runs torrents without a VPN (STASH_ALLOW_NO_VPN): torrents use this
 * network whatever the tunnel state says, and a saved VPN is not used. Every VPN
 * surface shows the permanent warning then, never "paused" or "connecting".
 */
export function isNoVpnMode(status: VpnStatus | null | undefined): boolean {
  return status?.allow_no_vpn === true && status.torrents_off !== true;
}

export function describeVpn(status: VpnStatus | null, options: {isUnavailable?: boolean} = {}): VpnSummary {
  if (!status) {
    return options.isUnavailable
      ? {tone: 'neutral', label: 'VPN status unavailable', detail: 'Web links and uploads still work.'}
      : {tone: 'neutral', label: 'Checking VPN', detail: ''};
  }

  if (isNoVpnMode(status)) {
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
        label: protectedLabel(status),
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
      // Off on purpose (for example no NET_ADMIN): "off", the word every page uses for
      // this state, with the server's reason as the detail.
      if (status.torrents_off) {
        return {tone: 'error', label: 'Torrents off', detail: status.error ?? TORRENTS_OFF_FALLBACK};
      }
      // The engine is restarting or stopped answering. Nothing torrent-related uses the
      // home connection meanwhile.
      return {
        tone: 'error',
        label: 'Torrents unavailable',
        detail: status.error ?? 'The torrent engine is not answering. Torrents stay stopped until it is back.',
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

/** True when torrents are off on this server by design, so nothing waits for the VPN. */
export function torrentsOff(status: VpnStatus | null): boolean {
  return status?.torrents_off === true;
}

/**
 * True when the VPN line belongs on screen: torrents cannot run right now (or
 * the state cannot be read), or they run without a VPN.
 */
export function torrentsAffected(status: VpnStatus | null, isUnavailable: boolean): boolean {
  if (isUnavailable) return true;
  if (!status) return false;
  if (isNoVpnMode(status)) return true;
  return !(status.state === 'connected' && status.protected !== false);
}

/**
 * Whether the Downloads page shows its VPN line. While the server does not answer
 * (`isServerQuiet`: the downloads could not be loaded) and nothing is known about the
 * VPN, the page already says so once; a line would repeat it, or claim that links and
 * uploads still work when nothing does. A status read before stays.
 */
export function showsVpnLine(status: VpnStatus | null, isUnavailable: boolean, isServerQuiet: boolean): boolean {
  if (!status && isServerQuiet) return false;
  return torrentsAffected(status, isUnavailable);
}

/**
 * True when a new torrent waits for the VPN instead of starting. Never while
 * torrents are off (nothing waits) or allowed without a VPN (they start at once).
 */
export function torrentsWait(status: VpnStatus | null, isUnavailable: boolean): boolean {
  if (torrentsOff(status) || isNoVpnMode(status)) return false;
  return torrentsAffected(status, isUnavailable);
}

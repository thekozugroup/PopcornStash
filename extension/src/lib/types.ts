/** API shapes from docs/architecture.md section 8 that the extension reads. */

export type DownloadKind = 'torrent' | 'http' | 'upload';
export type DownloadState =
  | 'queued'
  | 'downloading'
  | 'paused'
  | 'seeding'
  | 'completed'
  | 'sorting'
  | 'sorted'
  | 'failed'
  | 'cancelled';

export interface Download {
  id: string;
  kind: DownloadKind;
  name: string;
  source: string;
  state: DownloadState;
  /** 0..1 */
  progress: number;
  total_bytes: number | null;
  done_bytes: number;
  down_bps: number;
  up_bps: number;
  peers: number | null;
  eta_s: number | null;
  error: string | null;
  created_at: number;
  completed_at: number | null;
  item_ids: string[];
}

export type VpnState = 'connected' | 'connecting' | 'disconnected' | 'error' | 'not_configured' | 'unreachable';
export type VpnProvider = 'nordvpn' | 'wireguard' | 'openvpn';

export interface VpnStatus {
  configured: boolean;
  reachable: boolean;
  state: VpnState;
  provider: VpnProvider | null;
  protocol: 'wireguard' | 'openvpn' | null;
  label: string | null;
  server: string | null;
  country: string | null;
  public_ip: string | null;
  protected: boolean | null;
  since: number | null;
  error: string | null;
  kill_switch: boolean;
  allow_no_vpn: boolean;
}

export interface PairingStarted {
  pairing_id: string;
  user_code: string;
  poll_secret: string;
  expires_at: number;
  verify_url: string;
  /** Seconds between polls the server suggests (optional). */
  interval?: number;
}

export type PairingPoll =
  | {status: 'pending'}
  | {status: 'approved'; token: string; scopes: string[]; server_name: string}
  | {status: 'denied'}
  | {status: 'expired'};

export interface ApiErrorBody {
  error: {code: string; message: string};
}

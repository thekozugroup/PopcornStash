/**
 * The VPN state as a status tag ("Protected · NordVPN · Netherlands",
 * "Torrents paused: VPN connecting", "Torrents blocked: no VPN").
 */
import {useVpnStatus} from '../hooks/useVpnStatus';
import {describeVpn} from '../lib/vpn';
import {StatusTag} from './StatusTag';

export function VpnStatusTag({wrap = true}: {wrap?: boolean}) {
  const {status, isUnavailable} = useVpnStatus();
  const summary = describeVpn(status, {isUnavailable});
  return (
    <StatusTag tone={summary.tone} wrap={wrap} live title={summary.detail}>
      {summary.label}
    </StatusTag>
  );
}

/**
 * The VPN state as a status tag ("Protected · NordVPN · Netherlands",
 * "Torrents paused: VPN connecting", "Torrents blocked: no VPN").
 *
 * One line: in the narrow sidebar a long VPN name ends in an ellipsis rather
 * than wrapping a separator onto its own line. The whole label stays in the
 * text (screen readers read all of it) and in the tooltip with the detail.
 */
import {useVpnStatus} from '../hooks/useVpnStatus';
import {describeVpn} from '../lib/vpn';
import {StatusTag} from './StatusTag';

export function VpnStatusTag() {
  const {status, isUnavailable} = useVpnStatus();
  const summary = describeVpn(status, {isUnavailable});
  return (
    <StatusTag tone={summary.tone} live title={`${summary.label}. ${summary.detail}`}>
      {summary.label}
    </StatusTag>
  );
}

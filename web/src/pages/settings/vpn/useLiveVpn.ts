/**
 * VPN status for the VPN page: loads `GET /api/vpn/status`, follows the live
 * `vpn` events, and takes the status a setup call returns right away so the
 * page shows "connecting" before the first event arrives.
 */
import {useCallback, useState} from 'react';
import {vpn} from '../../../api/client';
import {useStashEvent} from '../../../api/events';
import type {VpnStatus} from '../../../api/types';
import {useResource} from '../../../components/settings/useResource';

export interface LiveVpn {
  status: VpnStatus | null;
  error: unknown;
  isLoading: boolean;
  reload: () => void;
  /** Use a status the server just returned (setup, reconnect, remove). */
  accept: (status: VpnStatus) => void;
}

export function useLiveVpn(): LiveVpn {
  const resource = useResource(signal => vpn.status(signal), {key: 'vpn-status'});
  const [live, setLive] = useState<VpnStatus | null>(null);

  useStashEvent('vpn', next => setLive(next));

  const {reload: reloadResource} = resource;
  const reload = useCallback(() => {
    setLive(null);
    reloadResource();
  }, [reloadResource]);

  const status = live ?? resource.data ?? null;
  return {
    status,
    error: status ? undefined : resource.error,
    isLoading: !status && resource.isLoading,
    reload,
    accept: setLive,
  };
}

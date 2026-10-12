/**
 * Live VPN status: loads `GET /api/vpn/status`, then follows `vpn` events.
 */
import {useEffect, useState} from 'react';
import {vpn} from '../api/client';
import {useStashEvent} from '../api/events';
import type {VpnStatus} from '../api/types';

export interface VpnStatusResult {
  status: VpnStatus | null;
  isLoading: boolean;
  isUnavailable: boolean;
}

export function useVpnStatus(enabled = true): VpnStatusResult {
  const [status, setStatus] = useState<VpnStatus | null>(null);
  const [isLoading, setIsLoading] = useState(enabled);
  const [isUnavailable, setIsUnavailable] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    vpn
      .status(controller.signal)
      .then(next => {
        setStatus(next);
        setIsUnavailable(false);
      })
      .catch(() => {
        if (!controller.signal.aborted) setIsUnavailable(true);
      })
      .finally(() => {
        if (!controller.signal.aborted) setIsLoading(false);
      });
    return () => controller.abort();
  }, [enabled]);

  useStashEvent(
    'vpn',
    next => {
      setStatus(next);
      setIsUnavailable(false);
      setIsLoading(false);
    },
    enabled,
  );

  return {status, isLoading, isUnavailable};
}

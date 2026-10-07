import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createPollingController, settlePair } from '../utils/polling';
import type { GeoIPInfo, HttpConnectivityResult } from '../utils/diagnostics';

export interface NetworkStatus {
  status: string;
  message?: string;
  ipv4?: string;
  ipv6?: string;
  probes?: HttpConnectivityResult[];
}

export interface IPInfo {
  ipv4?: string;
  ipv4_type?: string;
  ipv4_geoip?: GeoIPInfo | null;
  ipv6?: string;
  ipv6_type?: string;
  ipv6_geoip?: GeoIPInfo | null;
  local_ipv4?: string;
  local_ipv6?: string;
  dual_stack_enabled?: boolean;
  ipv6_priority?: boolean;
  ipv6_interface?: string | null;
  ipv6_source?: 'interface' | 'route_fallback' | null;
  local_addresses?: { address: string; interface: string | null; family: 'ipv4' | 'ipv6'; source: string }[];
  public_ipv4_probe?: {
    status: 'success' | 'error' | 'skipped'; url: string | null; error: string | null;
    proxy_policy: string; checked_at: number | null; address_family: string | null; cache_hit: boolean;
  };
}

interface NetworkData {
  networkStatus: NetworkStatus | null;
  ipInfo: IPInfo | null;
  loading: boolean;
  ipError: string | null;
  statusError: string | null;
}

const listeners = new Set<(data: NetworkData) => void>();
let data: NetworkData = { networkStatus: null, ipInfo: null, loading: true, ipError: null, statusError: null };
const update = (patch: Partial<NetworkData>) => {
  data = { ...data, ...patch };
  for (const listener of listeners) listener(data);
};
const errorText = (error: unknown) => typeof error === 'string' ? error : String(error ?? '查询失败');
const poller = createPollingController({
  fetch: (includeGeoip: boolean) => settlePair(
    invoke<NetworkStatus>('get_network_status'),
    invoke<IPInfo>('get_ip_info', { includeGeoip }),
  ),
  onValue: ({ first: status, second: ip }) => update({
    ...(status.status === 'fulfilled' ? { networkStatus: status.value, statusError: null } : { statusError: errorText(status.reason) }),
    ...(ip.status === 'fulfilled' ? { ipInfo: ip.value, ipError: null } : { ipError: errorText(ip.reason) }),
  }),
  onError: error => update({ ipError: errorText(error), statusError: errorText(error) }),
  onLoading: loading => update({ loading }),
});

/** Manual refresh joins the same physical request and respects the owner's current settings. */
export const refreshNetworkData = () => poller.refresh();

/** App is the sole polling owner; page subscribers never change cadence or GeoIP settings. */
export function useNetworkData(intervalSecs = 5, includeGeoip = true, options?: { owner?: boolean }) {
  const isOwner = options?.owner ?? false;
  const [snapshot, setSnapshot] = useState(data);

  useEffect(() => {
    setSnapshot(data);
    listeners.add(setSnapshot);
    return () => { listeners.delete(setSnapshot); };
  }, []);

  useEffect(() => {
    if (!isOwner) return;
    // Clear old location results when settings change; an earlier generation
    // can no longer put them back after GeoIP is disabled or re-enabled.
    if (data.ipInfo) update({ ipInfo: { ...data.ipInfo, ipv4_geoip: null, ipv6_geoip: null } });
    poller.configure(includeGeoip, Math.max(1, intervalSecs) * 1000);
    return () => poller.stop();
  }, [isOwner, intervalSecs, includeGeoip]);

  return { ...snapshot, refresh: refreshNetworkData };
}

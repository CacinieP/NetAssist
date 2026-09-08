import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface NetworkStatus {
  status: string;
  message?: string;
  ipv4?: string;
  ipv6?: string;
}

export interface IPInfo {
  ipv4?: string;
  ipv4_type?: string;
  ipv4_geoip?: any;
  ipv6?: string;
  ipv6_type?: string;
  ipv6_geoip?: any;
  local_ipv4?: string;
  local_ipv6?: string;
  dual_stack_enabled?: boolean;
  ipv6_priority?: boolean;
}

let globalNetworkListeners: Set<(status: NetworkStatus, ipInfo: IPInfo) => void> = new Set();
let globalNetworkInterval: ReturnType<typeof setInterval> | null = null;
let globalNetworkData: { status: NetworkStatus | null; ipInfo: IPInfo | null } = {
  status: null,
  ipInfo: null,
};
// Config of the running poll, used to detect owner re-config (e.g. the user
// changed refresh_interval_secs / show_geoip in Settings).
let globalNetworkConfig: { intervalSecs: number; includeGeoip: boolean } | null = null;

function startGlobalNetworkPolling(intervalSecs: number, includeGeoip: boolean) {
  if (globalNetworkInterval) {
    if (
      globalNetworkConfig &&
      globalNetworkConfig.intervalSecs === intervalSecs &&
      globalNetworkConfig.includeGeoip === includeGeoip
    ) {
      // An owner already configured an identical poll; never restart it from
      // a child component (previously the last-mounted useNetworkData — e.g.
      // the dashboard's NetworkStatus card with (5s, no-geoip) — would
      // hijack the interval and drop GeoIP for the whole app).
      return;
    }
    // Owner changed its configuration: restart with the new settings.
    clearInterval(globalNetworkInterval);
    globalNetworkInterval = null;
  }

  globalNetworkConfig = { intervalSecs, includeGeoip };

  const poll = async () => {
    try {
      const [status, ip] = await Promise.all([
        invoke<NetworkStatus>('get_network_status'),
        invoke<IPInfo>('get_ip_info', { includeGeoip }),
      ]);
      globalNetworkData = { status, ipInfo: ip };
      for (const listener of globalNetworkListeners) {
        listener(status, ip);
      }
    } catch {
      // Will retry next interval
    }
  };

  // Initial poll
  poll();
  globalNetworkInterval = setInterval(poll, intervalSecs * 1000);
}

function stopGlobalNetworkPolling() {
  if (globalNetworkInterval) {
    clearInterval(globalNetworkInterval);
    globalNetworkInterval = null;
  }
  globalNetworkConfig = null;
}

interface UseNetworkDataOptions {
  /** Only the owning component (App) may start/configure the shared poll. */
  owner?: boolean;
}

/**
 * Shared hook for network status and IP info.
 *
 * Exactly one owner (the app shell) configures the global poll with the
 * user's settings (`refresh_interval_secs`, `show_geoip`); every other
 * consumer only subscribes to the latest values, so entering a page can no
 * longer change the global cadence or disable GeoIP app-wide.
 */
export function useNetworkData(
  intervalSecs: number = 5,
  includeGeoip: boolean = true,
  options?: UseNetworkDataOptions
) {
  const isOwner = options?.owner ?? false;
  const [networkStatus, setNetworkStatus] = useState<NetworkStatus | null>(globalNetworkData.status);
  const [ipInfo, setIpInfo] = useState<IPInfo | null>(globalNetworkData.ipInfo);

  useEffect(() => {
    if (globalNetworkData.status) setNetworkStatus(globalNetworkData.status);
    if (globalNetworkData.ipInfo) setIpInfo(globalNetworkData.ipInfo);

    const listener = (status: NetworkStatus, ip: IPInfo) => {
      setNetworkStatus(status);
      setIpInfo(ip);
    };

    globalNetworkListeners.add(listener);

    if (isOwner) {
      startGlobalNetworkPolling(intervalSecs, includeGeoip);
    }

    return () => {
      globalNetworkListeners.delete(listener);
      if (isOwner && globalNetworkListeners.size === 0) {
        stopGlobalNetworkPolling();
      }
    };
  }, [intervalSecs, includeGeoip, isOwner]);

  return { networkStatus, ipInfo, setNetworkStatus, setIpInfo };
}

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
  ipv4_type: string;
  ipv4_geoip: any;
  ipv6?: string;
  ipv6_type: string;
  ipv6_geoip: any;
  has_ipv4: boolean;
  has_ipv6: boolean;
  dual_stack: boolean;
}

let globalNetworkListeners: Set<(status: NetworkStatus, ipInfo: IPInfo) => void> = new Set();
let globalNetworkInterval: ReturnType<typeof setInterval> | null = null;
let globalNetworkData: { status: NetworkStatus | null; ipInfo: IPInfo | null } = {
  status: null,
  ipInfo: null,
};

function startGlobalNetworkPolling(intervalSecs: number, includeGeoip: boolean) {
  if (globalNetworkInterval) {
    clearInterval(globalNetworkInterval);
  }

  const poll = async () => {
    try {
      const [status, ip] = await Promise.all([
        invoke<NetworkStatus>('get_network_status'),
        invoke<IPInfo>('get_ip_info', { include_geoip: includeGeoip }),
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
}

/**
 * Shared hook for network status and IP info.
 * Polls once per settings.refresh_interval_secs globally.
 */
export function useNetworkData(
  intervalSecs: number = 5,
  includeGeoip: boolean = true
) {
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
    startGlobalNetworkPolling(intervalSecs, includeGeoip);

    return () => {
      globalNetworkListeners.delete(listener);
      if (globalNetworkListeners.size === 0) {
        stopGlobalNetworkPolling();
      }
    };
  }, [intervalSecs, includeGeoip]);

  return { networkStatus, ipInfo, setNetworkStatus, setIpInfo };
}

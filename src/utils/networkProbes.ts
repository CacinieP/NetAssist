import { createPollingController } from './polling.ts';
import type { GeoIPInfo, HttpConnectivityResult, DNSStats } from './diagnostics.ts';

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

export interface ProbeConfig {
  ready: boolean;
  automatic: boolean;
  intervalMs: number;
  includeGeoip: boolean;
  dnsServer: string;
}

export interface NetworkData {
  networkStatus: NetworkStatus | null;
  ipInfo: IPInfo | null;
  httpProbe: HttpConnectivityResult | null;
  dnsStats: DNSStats | null;
  dnsServer: string | null;
  loading: boolean;
  localLoading: boolean;
  ready: boolean;
  automatic: boolean;
  hasRun: boolean;
  geoipQueried: boolean;
  lastCheckedAt: number | null;
  stale: boolean;
  resultId: number;
  ipError: string | null;
  statusError: string | null;
  httpError: string | null;
  dnsError: string | null;
  localError: string | null;
}

export function probePreferences(state: {
  hydrated: boolean; loading: boolean;
  settings: { auto_probe_enabled?: boolean; refresh_interval_secs: number; show_geoip: boolean; primary_dns: string };
}): ProbeConfig {
  return {
    ready: state.hydrated && !state.loading,
    automatic: state.settings.auto_probe_enabled === true,
    intervalMs: Math.max(1, Math.min(state.settings.refresh_interval_secs || 5, 3600)) * 1000,
    includeGeoip: state.settings.show_geoip,
    dnsServer: state.settings.primary_dns || '8.8.8.8',
  };
}

export function probeSummary(data: Pick<NetworkData, 'loading' | 'lastCheckedAt' | 'stale'>) {
  const previous = data.lastCheckedAt ? `上次检测：${new Date(data.lastCheckedAt).toLocaleString()}` : '未检测';
  return `${data.loading ? '检测中；' : ''}${previous}${data.stale ? ' · 设置已变更或检测已停止，请重新检测' : ''}`;
}

/** The only owner of routine outbound probes. Subscribers and local sampling never trigger them. */
export function createNetworkProbeCoordinator(deps: {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  schedule?: (callback: () => void, ms: number) => () => void;
  now?: () => number;
}) {
  const now = deps.now ?? Date.now;
  const listeners = new Set<(data: NetworkData) => void>();
  let data: NetworkData = {
    networkStatus: null, ipInfo: null, httpProbe: null, dnsStats: null, dnsServer: null,
    loading: false, localLoading: false, ready: false, automatic: false,
    hasRun: false, geoipQueried: false, lastCheckedAt: null, stale: false, resultId: 0,
    ipError: null, statusError: null, httpError: null, dnsError: null, localError: null,
  };
  let config: ProbeConfig | null = null;
  let generation = 0;
  let nextResultId = 0;
  let resultConfigGeneration = -1;
  let localInterval: number | null = null;
  let localIP: IPInfo | null = null;
  let activeIP: IPInfo | null = null;
  const update = (patch: Partial<NetworkData>) => {
    data = { ...data, ...patch };
    for (const listener of listeners) listener(data);
  };
  const mergedIP = (): IPInfo | null => {
    if (!localIP) return activeIP;
    // Passive results contain no public IP. Copy only fields they actually measure.
    return { ...activeIP, ...localIP, ipv4: activeIP?.ipv4, ipv4_type: activeIP?.ipv4_type,
      ipv4_geoip: activeIP?.ipv4_geoip, public_ipv4_probe: activeIP?.public_ipv4_probe,
      ipv6_geoip: activeIP?.ipv6 === localIP.ipv6 ? activeIP?.ipv6_geoip : null };
  };
  const errorText = (error: unknown) => String(error ?? '检测失败');
  const local = createPollingController({
    fetch: () => deps.invoke<IPInfo>('get_local_ip_info_only'),
    onValue: value => { localIP = value; update({ ipInfo: mergedIP(), localError: null }); },
    onError: error => update({ localError: errorText(error) }),
    onLoading: localLoading => update({ localLoading }),
    schedule: deps.schedule,
  });
  const active = createPollingController({
    fetch: async (request: ProbeConfig) => {
      // allSettled holds the physical lock until every outbound request completes.
      const [status, ip, http, dns] = await Promise.allSettled([
        deps.invoke<NetworkStatus>('get_network_status'),
        deps.invoke<IPInfo>('get_ip_info', { includeGeoip: request.includeGeoip }),
        deps.invoke<HttpConnectivityResult>('test_http_connectivity', { url: null }),
        deps.invoke<DNSStats>('test_dns', { server: request.dnsServer }),
      ]);
      return { status, ip, http, dns, request };
    },
    onValue: ({ status, ip, http, dns, request }) => {
      activeIP = ip.status === 'fulfilled' ? ip.value : null;
      resultConfigGeneration = generation;
      update({
        networkStatus: status.status === 'fulfilled' ? status.value : null,
        ipInfo: mergedIP(),
        httpProbe: http.status === 'fulfilled' ? http.value : null,
        dnsStats: dns.status === 'fulfilled' ? dns.value : null,
        statusError: status.status === 'rejected' ? errorText(status.reason) : null,
        ipError: ip.status === 'rejected' ? errorText(ip.reason) : null,
        httpError: http.status === 'rejected' ? errorText(http.reason) : null,
        dnsError: dns.status === 'rejected' ? errorText(dns.reason) : null,
        dnsServer: request.dnsServer, hasRun: true, geoipQueried: request.includeGeoip,
        lastCheckedAt: now(), stale: false, resultId: ++nextResultId,
      });
    },
    onError: error => {
      activeIP = null;
      resultConfigGeneration = generation;
      update({ networkStatus: null, ipInfo: mergedIP(), httpProbe: null, dnsStats: null,
        statusError: errorText(error), ipError: errorText(error), httpError: errorText(error), dnsError: errorText(error),
        hasRun: true, lastCheckedAt: now(), stale: false, resultId: ++nextResultId });
    },
    onLoading: loading => update({ loading }),
    schedule: deps.schedule,
  });
  return {
    getSnapshot: () => data,
    subscribe(listener: (data: NetworkData) => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    configure(next: ProbeConfig) {
      if (JSON.stringify(config) === JSON.stringify(next)) return;
      if (localInterval !== next.intervalMs) {
        localInterval = next.intervalMs;
        local.configure(undefined, next.intervalMs);
      }
      generation += 1;
      if (config?.includeGeoip !== next.includeGeoip && activeIP) {
        activeIP = { ...activeIP, ipv4_geoip: null, ipv6_geoip: null };
        update({ ipInfo: mergedIP(), geoipQueried: false });
      }
      config = { ...next };
      update({ ready: next.ready, automatic: next.ready && next.automatic, loading: false, stale: data.hasRun });
      if (next.ready) active.configure(next, next.intervalMs, { immediate: next.automatic, automatic: next.automatic });
      else active.stop();
    },
    refresh: () => {
      if (!config?.ready) return Promise.resolve();
      update({ loading: true });
      return active.refresh();
    },
    isResultCurrent: (resultId: number) => !!config?.ready && data.hasRun &&
      !data.stale && generation === resultConfigGeneration && data.resultId === resultId,
    stop() {
      generation += 1;
      active.stop(); local.stop(); config = null; localInterval = null;
      update({ ready: false, automatic: false, loading: false, localLoading: false, stale: data.hasRun });
    },
  };
}

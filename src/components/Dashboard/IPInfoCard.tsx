import { RefreshCw } from "lucide-react";
import { useSettingsStore } from "../../store/settingsStore";
import { useNetworkData } from "../../hooks/useNetworkData";
import { probeDescription, probeLimitations } from "../../utils/diagnostics";
import GeoIPLocation from "./GeoIPLocation";

export default function IPInfoCard() {
  const { settings } = useSettingsStore();
  const { ipInfo, refresh, loading, ipError } = useNetworkData();
  const geoOptions = { enabled: settings.show_geoip, loading, error: ipError };
  const publicProbe = ipInfo?.public_ipv4_probe;

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">IP 信息</h3>
        <button onClick={() => { void refresh(); }} disabled={loading}
          className="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded transition-colors disabled:opacity-50" title="刷新共享网络与 IP 信息">
          <RefreshCw className={`w-4 h-4 text-gray-600 dark:text-gray-400 ${loading ? 'animate-spin' : ''}`} />
        </button>
      </div>
      {ipError && <p className="text-xs text-red-600">本次刷新失败：{ipError}；已有数据为上次结果。</p>}
      <div>
        <p className="text-sm"><span className="text-blue-600 font-medium">NetAssist 探测公网 IPv4：</span>
          <span className="font-mono text-gray-800 dark:text-gray-200">{ipInfo?.ipv4 || (loading ? "获取中…" : "未获取到")}</span></p>
        <GeoIPLocation geoip={ipInfo?.ipv4_geoip} {...geoOptions} />
        {publicProbe && <p className="mt-1 text-xs text-gray-500 dark:text-gray-400 break-all">
          {probeDescription(publicProbe)}{publicProbe.cache_hit ? " · 缓存结果" : ""}
          {publicProbe.error ? ` · ${publicProbe.error}` : ""}
        </p>}
      </div>
      <div className="text-sm"><span className="text-green-600 font-medium">本地 IPv4：</span>
        <span className="font-mono text-gray-800 dark:text-gray-200">{ipInfo?.local_ipv4 || "未检测到"}</span></div>
      <div className="pt-3 border-t border-gray-100 dark:border-gray-700">
        <p className="text-sm"><span className="text-purple-600 font-medium">本地接口 IPv6：</span>
          <span className="font-mono text-gray-800 dark:text-gray-200 break-all">{ipInfo?.ipv6 || "未检测到"}</span></p>
        {ipInfo?.ipv6 && <>
          <p className="text-xs text-gray-500 dark:text-gray-400">接口：{ipInfo.ipv6_interface || "未确定"} · 来源：{ipInfo.ipv6_source === 'route_fallback' ? "路由回退" : "接口枚举"} · 类型：{ipInfo.ipv6_type}</p>
          <GeoIPLocation geoip={ipInfo.ipv6_geoip} {...geoOptions} />
        </>}
        <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">此地址是本地接口候选，未验证目标连接的实际 IPv6 出口。{ipInfo?.dual_stack_enabled ? "本地已配置 IPv4 和 IPv6；不代表双栈互联网连通。" : ""}</p>
        {(ipInfo?.local_addresses?.length ?? 0) > 1 && <details className="mt-2 text-xs text-gray-500 dark:text-gray-400">
          <summary className="cursor-pointer">全部本地地址与接口</summary>
          {ipInfo?.local_addresses?.map(address => <p className="font-mono break-all" key={`${address.interface}-${address.address}`}>
            {address.family.toUpperCase()} · {address.interface || "未确定接口"} · {address.address}
          </p>)}
        </details>}
      </div>
      <p className="text-xs text-gray-500 dark:text-gray-400">{probeLimitations}</p>
    </div>
  );
}

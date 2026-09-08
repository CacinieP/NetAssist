import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw } from "lucide-react";
import { useSettingsStore } from "../../store/settingsStore";
import { useNetworkData } from "../../hooks/useNetworkData";

export default function IPInfoCard() {
  const { settings } = useSettingsStore();

  // Subscribe to the app-level network poll (owned by App.tsx). This card
  // used to run its own invoke loop on a second timer, doubling the
  // public-IP/GeoIP requests. Now it just renders the shared data.
  const { ipInfo, setIpInfo } = useNetworkData();
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState(false);

  const fetchIPInfo = useCallback(async () => {
    setRefreshing(true);
    try {
      const info = await invoke<any>("get_ip_info", {
        includeGeoip: settings.show_geoip,
      });
      setIpInfo(info);
      setRefreshError(false);
    } catch (error) {
      console.error("Failed to fetch IP info:", error);
      setRefreshError(true);
    } finally {
      setRefreshing(false);
    }
  }, [settings.show_geoip, setIpInfo]);

  // Format location for display
  const formatLocation = (geoip?: { country: string; city: string; region: string }, type?: string) => {
    if (geoip && geoip.country && geoip.country !== "本地网络" && geoip.country !== "未知") {
      const parts = [geoip.country, geoip.city].filter(Boolean);
      return parts.join(" ");
    }
    // Rust IPType is serialized lowercase: "public" | "private" | "linklocal" | ...
    if (type === "public") return "公网";
    if (type === "private") return "内网";
    return type || "未知";
  };

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">IP 信息</h3>
        <button
          onClick={fetchIPInfo}
          disabled={refreshing}
          className="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded transition-colors disabled:opacity-50"
          title="刷新IP信息"
        >
          <RefreshCw className={`w-4 h-4 text-gray-600 dark:text-gray-400 ${refreshing ? 'animate-spin' : ''}`} />
        </button>
      </div>

      {/* IPv4 - Public */}
      <div className="mb-4">
        <div className="flex items-center gap-2 mb-2">
          <span className="text-blue-600 font-medium text-sm">公网 IPv4:</span>
          <span className="font-mono text-sm text-gray-800 dark:text-gray-200">
            {refreshError ? "加载失败" : ipInfo?.ipv4 || "加载中..."}
          </span>
        </div>
        {settings.show_geoip && ipInfo?.ipv4_geoip && (
          <div className="flex items-center gap-2 pl-6">
            <span className="text-gray-500 dark:text-gray-400 text-sm">位置:</span>
            <span className="text-gray-600 dark:text-gray-300 text-sm">
              {formatLocation(ipInfo.ipv4_geoip, ipInfo.ipv4_type)}
            </span>
          </div>
        )}
      </div>

      {/* IPv4 - Local */}
      <div className="mb-4">
        <div className="flex items-center gap-2 mb-2">
          <span className="text-green-600 font-medium text-sm">本地 IPv4:</span>
          <span className="font-mono text-sm text-gray-800 dark:text-gray-200">
            {ipInfo?.local_ipv4 || "未检测到"}
          </span>
        </div>
        <div className="flex items-center gap-2 pl-6">
          <span className="text-gray-500 dark:text-gray-400 text-sm">类型:</span>
          <span className="text-gray-600 dark:text-gray-300 text-sm">局域网 (内网)</span>
        </div>
      </div>

      {/* IPv6 */}
      {ipInfo?.ipv6 ? (
        <div className="mt-4 pt-4 border-t border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2 mb-2">
            <span className="text-purple-600 font-medium text-sm">IPv6:</span>
            <span className="font-mono text-sm text-gray-800 dark:text-gray-200">{ipInfo.ipv6}</span>
          </div>
          <div className="flex items-center gap-2 pl-6">
            <span className="text-gray-500 dark:text-gray-400 text-sm">类型:</span>
            <span className="text-gray-600 dark:text-gray-300 text-sm">{ipInfo.ipv6_type || "未知"}</span>
          </div>
          {settings.show_geoip && ipInfo.ipv6_geoip && (
            <div className="flex items-center gap-2 pl-6">
              <span className="text-gray-500 dark:text-gray-400 text-sm">位置:</span>
              <span className="text-gray-600 dark:text-gray-300 text-sm">
                {formatLocation(ipInfo.ipv6_geoip, ipInfo.ipv6_type)}
              </span>
            </div>
          )}
        </div>
      ) : (
        <div className="mt-4 pt-4 border-t border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2 text-gray-500 dark:text-gray-400 text-sm">
            <span>IPv6: 未检测到</span>
          </div>
        </div>
      )}
    </div>
  );
}

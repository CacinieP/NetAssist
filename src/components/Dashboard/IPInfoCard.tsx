import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw } from "lucide-react";
import { useSettingsStore } from "../../store/settingsStore";

interface IPInfo {
  ipv4?: string;
  ipv4_type?: string;
  ipv4_geoip?: {
    country: string;
    city: string;
    region: string;
  };
  ipv6?: string;
  ipv6_type?: string;
  ipv6_geoip?: {
    country: string;
    city: string;
    region: string;
  };
  local_ipv4?: string;
  local_ipv6?: string;
}

export default function IPInfoCard() {
  const { settings } = useSettingsStore();

  const [ipInfo, setIpInfo] = useState<IPInfo>({});
  const [refreshing, setRefreshing] = useState(false);
  const errorCountRef = useRef(0);

  const fetchIPInfo = useCallback(async () => {
    setRefreshing(true);
    try {
      const info = await invoke<any>("get_ip_info", {
        include_geoip: settings.show_geoip,
      });
      setIpInfo({
        ipv4: info.ipv4 || undefined,
        ipv4_type: info.ipv4_type,
        ipv4_geoip: info.ipv4_geoip,
        ipv6: info.ipv6 || undefined,
        ipv6_type: info.ipv6_type,
        ipv6_geoip: info.ipv6_geoip,
        local_ipv4: info.local_ipv4 || undefined,
        local_ipv6: info.local_ipv6 || undefined,
      });
      errorCountRef.current = 0;
    } catch (error) {
      console.error("Failed to fetch IP info:", error);
      // Only clear data after 3 consecutive failures (avoid flash on transient errors)
      errorCountRef.current += 1;
      if (errorCountRef.current >= 3) {
        setIpInfo({});
      }
    } finally {
      setRefreshing(false);
    }
  }, [settings.show_geoip]);

  useEffect(() => {
    // Initial fetch
    fetchIPInfo();

    // Avoid over-polling public IP / geoip
    const intervalMs = Math.max(5000, (settings.refresh_interval_secs || 10) * 1000);
    const interval = setInterval(fetchIPInfo, intervalMs);

    return () => clearInterval(interval);
  }, [settings.refresh_interval_secs, settings.show_geoip, fetchIPInfo]);

  // Format location for display
  const formatLocation = (geoip?: { country: string; city: string; region: string }, type?: string) => {
    if (geoip && geoip.country && geoip.country !== "本地网络" && geoip.country !== "未知") {
      const parts = [geoip.country, geoip.city].filter(Boolean);
      return parts.join(" ");
    }
    return type === "Public" ? "公网" : type === "Private" ? "内网" : type || "未知";
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
            {ipInfo.ipv4 || "加载中..."}
          </span>
        </div>
        {settings.show_geoip && ipInfo.ipv4_geoip && (
          <div className="flex items-center gap-2 pl-6">
            <span className="text-gray-500 dark:text-gray-400 text-sm">位置:</span>
            <span className="text-gray-600 dark:text-gray-300 text-sm">
              {formatLocation(ipInfo.ipv4_geoip)}
            </span>
          </div>
        )}
      </div>

      {/* IPv4 - Local */}
      <div className="mb-4">
        <div className="flex items-center gap-2 mb-2">
          <span className="text-green-600 font-medium text-sm">本地 IPv4:</span>
          <span className="font-mono text-sm text-gray-800 dark:text-gray-200">
            {ipInfo.local_ipv4 || "未检测到"}
          </span>
        </div>
        <div className="flex items-center gap-2 pl-6">
          <span className="text-gray-500 dark:text-gray-400 text-sm">类型:</span>
          <span className="text-gray-600 dark:text-gray-300 text-sm">局域网 (内网)</span>
        </div>
      </div>

      {/* IPv6 */}
      {ipInfo.ipv6 && (
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
                {formatLocation(ipInfo.ipv6_geoip)}
              </span>
            </div>
          )}
        </div>
      )}

      {/* No IPv6 message */}
      {!ipInfo.ipv6 && (
        <div className="mt-4 pt-4 border-t border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2 text-gray-500 dark:text-gray-400 text-sm">
            <span>IPv6: 未检测到</span>
          </div>
        </div>
      )}
    </div>
  );
}

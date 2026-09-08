import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNetworkData } from "../../hooks/useNetworkData";
import { useSettingsStore } from "../../store/settingsStore";

interface ConnectionInfo {
  pid: number;
  process_name: string;
  protocol: string;
  local_address: string;
  local_port: number;
  remote_address: string;
  remote_port: number;
  state: string;
}

export default function ConnectionManager() {
  const [connections, setConnections] = useState<ConnectionInfo[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { settings } = useSettingsStore();

  // IP info comes from the app-level shared poll (owned by App.tsx). This
  // page previously called get_ip_info (with GeoIP) on its own 3s timer,
  // i.e. a public-IP + GeoIP HTTP request every 3 seconds.
  const { ipInfo } = useNetworkData();

  // Guard against out-of-order responses: only apply the result of the most
  // recent request.
  const requestSeq = useRef(0);

  const fetchConnections = useCallback(async () => {
    const seq = ++requestSeq.current;
    try {
      setLoading(true);
      setError(null);
      const conns = await invoke<ConnectionInfo[]>("get_active_connections");
      if (seq !== requestSeq.current) return; // stale response
      setTotalCount(conns.length);
      setConnections(conns.slice(0, 100)); // Limit display to 100 for performance
    } catch (err) {
      if (seq !== requestSeq.current) return;
      // Tauri rejects invoke() with a string, NOT an Error instance — the old
      // `instanceof Error` check always fell through to the generic message.
      const errorMsg = typeof err === "string" ? err : String(err ?? "获取连接数据失败");
      console.error("Failed to fetch connection data:", err);
      setError(errorMsg);
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchConnections();

    // Auto-refresh honoring the user's refresh interval (min 3s for the
    // connection snapshot).
    const intervalMs = Math.max(3000, (settings.refresh_interval_secs || 5) * 1000);
    const interval = setInterval(fetchConnections, intervalMs);

    return () => clearInterval(interval);
  }, [fetchConnections, settings.refresh_interval_secs]);

  return (
    <div className="p-6 space-y-6">
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800 dark:text-gray-100">连接管理</h2>
        <p className="text-gray-500 dark:text-gray-400">管理活跃的网络连接</p>
      </div>

      {/* Error Banner */}
      {error && (
        <div className="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 text-yellow-700 dark:text-yellow-300 px-4 py-3 rounded-lg flex items-center justify-between">
          <span>{error}</span>
          <button
            onClick={() => setError(null)}
            className="text-yellow-600 dark:text-yellow-400 hover:text-yellow-800 dark:hover:text-yellow-200 text-sm"
          >
            关闭
          </button>
        </div>
      )}

      {/* IP Address Information */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-3">IP地址信息</h3>

        <div className="space-y-3">
          <div className="space-y-1">
            <div className="flex items-start gap-3">
              <span className="text-blue-600 font-mono text-sm w-12">IPv4:</span>
              <span className="font-mono text-sm text-gray-800 dark:text-gray-200">{ipInfo?.ipv4 || "获取中..."}</span>
            </div>
            <div className="flex items-start gap-3 pl-16">
              <span className="text-gray-500 dark:text-gray-400 text-sm">📍</span>
              <span className="text-gray-600 dark:text-gray-400 text-sm">{ipInfo?.ipv4_geoip?.country || "未知地区"}</span>
            </div>
          </div>

          <div className="space-y-1">
            <div className="flex items-start gap-3">
              <span className="text-purple-600 font-mono text-sm w-12">IPv6:</span>
              <span className="font-mono text-sm text-gray-800 dark:text-gray-200">{ipInfo?.ipv6 || "未连接"}</span>
            </div>
            <div className="flex items-start gap-3 pl-16">
              <span className="text-gray-500 dark:text-gray-400 text-sm">📍</span>
              <span className="text-gray-600 dark:text-gray-400 text-sm">{ipInfo?.ipv6_geoip?.country || "未知地区"}</span>
            </div>
          </div>
        </div>
      </div>

      {/* Active Connections */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <div className="flex justify-between items-center mb-4">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">活跃连接</h3>
            <span className="text-green-600 text-sm">✅ 正常</span>
            <span className="text-gray-500 dark:text-gray-400 text-sm">({totalCount}个)</span>
            {totalCount > 100 && (
              <span className="text-xs text-orange-600">显示前100个</span>
            )}
          </div>
          <div className="flex gap-2">
            <button
              onClick={fetchConnections}
              className="px-3 py-1.5 text-sm bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
              disabled={loading}
            >
              {loading ? "刷新中..." : "刷新"}
            </button>
          </div>
        </div>

        <div className="space-y-2 h-[400px] overflow-y-auto">
          {connections.length === 0 ? (
            <div className="text-center text-gray-500 dark:text-gray-400 py-10">暂无活跃连接或权限不足</div>
          ) : (
            connections.map((conn, index) => (
              <div key={`${conn.pid}-${conn.remote_address}-${index}`} className="flex items-start gap-3 p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
                <div className="flex-1">
                  <div className="font-medium text-gray-800 dark:text-gray-100 text-sm">
                    {conn.process_name} ({conn.pid}) → <span className="font-mono">{conn.remote_address}:{conn.remote_port}</span>
                  </div>
                  <div className="text-xs text-gray-500 dark:text-gray-400 flex items-center gap-2 mt-1">
                    <span>{conn.protocol}</span>
                    <span>|</span>
                    <span>{conn.state}</span>
                    <span>|</span>
                    <span className="font-mono">{conn.local_address}:{conn.local_port}</span>
                  </div>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

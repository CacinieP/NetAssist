import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface GeoIPInfo {
  country: string;
  region: string;
  city: string;
}

interface IPInfo {
  ipv4: string | null;
  ipv6: string | null;
  ipv4_geoip: GeoIPInfo | null;
  ipv6_geoip: GeoIPInfo | null;
}

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
  const [ipInfo, setIpInfo] = useState<IPInfo | null>(null);
  const [connections, setConnections] = useState<ConnectionInfo[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchData = async () => {
    try {
      setLoading(true);
      setError(null);

      const ip = await invoke<IPInfo>("get_ip_info");
      setIpInfo(ip);

      const conns = await invoke<ConnectionInfo[]>("get_active_connections");
      setTotalCount(conns.length);
      setConnections(conns.slice(0, 100)); // Limit display to 100 for performance

    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : "获取连接数据失败";
      console.error("Failed to fetch connection data:", err);
      setError(errorMsg);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();

    // Auto-refresh every 3 seconds
    const interval = setInterval(fetchData, 3000);

    return () => clearInterval(interval);
  }, []);

  const formatLocation = (geoip: GeoIPInfo | null | undefined) => {
    if (geoip && geoip.country) {
      return `${geoip.country} ${geoip.region} ${geoip.city}`;
    }
    return "未知地区";
  };

  return (
    <div className="p-6 space-y-6">
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800">连接管理</h2>
        <p className="text-gray-500">管理活跃的网络连接</p>
      </div>

      {/* Error Banner */}
      {error && (
        <div className="bg-yellow-50 border border-yellow-200 text-yellow-700 px-4 py-3 rounded-lg flex items-center justify-between">
          <span>{error}</span>
          <button
            onClick={() => setError(null)}
            className="text-yellow-600 hover:text-yellow-800 text-sm"
          >
            关闭
          </button>
        </div>
      )}

      {/* IP Address Information */}
      <div className="bg-white rounded-lg border border-gray-200 p-4">
        <h3 className="text-sm font-medium text-gray-700 mb-3">IP地址信息</h3>

        <div className="space-y-3">
          <div className="space-y-1">
            <div className="flex items-start gap-3">
              <span className="text-blue-600 font-mono text-sm w-12">IPv4:</span>
              <span className="font-mono text-sm text-gray-800">{ipInfo?.ipv4 || "获取中..."}</span>
            </div>
            <div className="flex items-start gap-3 pl-15">
              <span className="text-gray-500 text-sm">📍</span>
              <span className="text-gray-600 text-sm">{formatLocation(ipInfo?.ipv4_geoip)}</span>
            </div>
          </div>

          <div className="space-y-1">
            <div className="flex items-start gap-3">
              <span className="text-purple-600 font-mono text-sm w-12">IPv6:</span>
              <span className="font-mono text-sm text-gray-800">{ipInfo?.ipv6 || "未连接"}</span>
            </div>
            <div className="flex items-start gap-3 pl-15">
              <span className="text-gray-500 text-sm">📍</span>
              <span className="text-gray-600 text-sm">{formatLocation(ipInfo?.ipv6_geoip)}</span>
            </div>
          </div>
        </div>

        <div className="mt-4 flex gap-2">
          <button
            onClick={fetchData}
            className="px-3 py-1.5 text-sm bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
          >
            刷新数据
          </button>
        </div>
      </div>

      {/* Active Connections */}
      <div className="bg-white rounded-lg border border-gray-200 p-4">
        <div className="flex justify-between items-center mb-4">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-medium text-gray-700">活跃连接</h3>
            <span className="text-green-600 text-sm">✅ 正常</span>
            <span className="text-gray-500 text-sm">({totalCount}个)</span>
            {totalCount > 100 && (
              <span className="text-xs text-orange-600">显示前100个</span>
            )}
          </div>
          <div className="flex gap-2">
            <button
              onClick={fetchData}
              className="px-3 py-1.5 text-sm bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
              disabled={loading}
            >
              {loading ? "刷新中..." : "刷新"}
            </button>
          </div>
        </div>

        <div className="space-y-2 h-[400px] overflow-y-auto">
          {connections.length === 0 ? (
            <div className="text-center text-gray-500 py-10">暂无活跃连接或权限不足</div>
          ) : (
            connections.map((conn, index) => (
              <div key={`${conn.pid}-${conn.remote_address}-${index}`} className="flex items-start gap-3 p-3 bg-gray-50 rounded-lg">
                <div className="flex-1">
                  <div className="font-medium text-gray-800 text-sm">
                    {conn.process_name} ({conn.pid}) → <span className="font-mono">{conn.remote_address}:{conn.remote_port}</span>
                  </div>
                  <div className="text-xs text-gray-500 flex items-center gap-2 mt-1">
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

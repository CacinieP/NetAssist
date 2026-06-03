import { useState, useEffect } from "react";
import { useRealtimeTraffic } from "../../hooks/useTrafficData";
import { useNetworkData } from "../../hooks/useNetworkData";

export default function NetworkStatus() {
  const { stats: trafficStats } = useRealtimeTraffic(1000);
  const { networkStatus } = useNetworkData(5, false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (networkStatus) {
      setLoading(false);
    }
  }, [networkStatus]);

  const formatSpeed = (bps: number) => {
    if (bps < 1024) return `${bps.toFixed(1)} B/s`;
    if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
  };

  const isConnected = networkStatus?.status === "normal";

  if (loading) {
    return (
      <div className="bg-white rounded-lg border border-gray-200 p-4">
        <div className="flex items-center gap-2">
          <span className="text-gray-400">加载中...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-4">
      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-lg">
            {isConnected ? "✅" : "❌"}
          </span>
          <span className="text-gray-700">网络状态:</span>
          <span className={isConnected ? "text-green-600 font-medium" : "text-red-600 font-medium"}>
            {networkStatus?.message || "未知"}
          </span>
        </div>
        {isConnected && (
          <div className="flex items-center gap-4 text-sm">
            <span className="text-blue-600">
              ↓ {formatSpeed(trafficStats?.download_bps || 0)}
            </span>
            <span className="text-green-600">
              ↑ {formatSpeed(trafficStats?.upload_bps || 0)}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

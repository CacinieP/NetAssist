import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface NetworkStatus {
  status: string;
  message: string;
  timestamp: number;
}

interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp: number;
}

export default function NetworkStatus() {
  const [networkStatus, setNetworkStatus] = useState<NetworkStatus>({
    status: "unknown",
    message: "加载中...",
    timestamp: 0,
  });
  const [trafficStats, setTrafficStats] = useState<TrafficStats>({
    download_bps: 0,
    upload_bps: 0,
    timestamp: 0,
  });
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const fetchData = async () => {
      try {
        // Get network status
        const status = await invoke<NetworkStatus>("get_network_status");
        setNetworkStatus(status);

        // Get traffic stats
        const traffic = await invoke<TrafficStats>("get_realtime_traffic");
        setTrafficStats(traffic);
      } catch (error) {
        console.error("Failed to fetch network status:", error);
        setNetworkStatus({
          status: "error",
          message: "获取失败",
          timestamp: Date.now(),
        });
      } finally {
        setLoading(false);
      }
    };

    fetchData();
    const interval = setInterval(fetchData, 1000); // Update every second

    return () => clearInterval(interval);
  }, []);

  const formatSpeed = (bps: number) => {
    if (bps < 1024) return `${bps.toFixed(1)} B/s`;
    if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
  };

  const isConnected = networkStatus.status === "normal";

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
            {networkStatus.message}
          </span>
        </div>
        {isConnected && (
          <div className="flex items-center gap-4 text-sm">
            <span className="text-blue-600">
              ↓ {formatSpeed(trafficStats.download_bps)}
            </span>
            <span className="text-green-600">
              ↑ {formatSpeed(trafficStats.upload_bps)}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

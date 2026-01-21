import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AppTraffic {
  name: string;
  pid: number;
  download_bytes: number;
  upload_bytes: number;
  current_download_bps: number;
  current_upload_bps: number;
}

interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp: number;
}

export default function TrafficMonitor() {
  const [apps, setApps] = useState<AppTraffic[]>([]);
  const [stats, setStats] = useState<TrafficStats | null>(null);
  const [searchTerm, setSearchTerm] = useState("");

  // Sanitize search input to prevent XSS and limit length
  const handleSearchChange = (value: string) => {
    // Limit length to prevent DOS (100 chars max)
    const sanitized = value.slice(0, 100);
    setSearchTerm(sanitized);
  };

  const fetchData = async () => {
    try {
      // Fetch overall stats
      const currentStats = await invoke<TrafficStats>("get_realtime_traffic");
      setStats(currentStats);

      // Fetch app ranking
      const ranking = await invoke<AppTraffic[]>("get_app_traffic_ranking");
      setApps(ranking);
    } catch (error) {
      console.error("Failed to fetch traffic data:", error);
    }
  };

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 3000); // Refresh every 3 seconds
    return () => clearInterval(interval);
  }, []);

  // Filter apps with sanitized search term
  const filteredApps = apps.filter((app) =>
    app.name.toLowerCase().includes(searchTerm.toLowerCase())
  ).slice(0, 50); // Limit to top 50 to avoid rendering too many



  const formatSpeed = (bps: number) => {
    if (bps < 1024) return `${Math.round(bps)} B/s`;
    if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
  };

  return (
    <div className="p-6 space-y-6">
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800">流量监控 (Traffic Monitor)</h2>
        <p className="text-gray-500">实时流量统计与应用排行</p>
      </div>

      {/* Daily Statistics (Real-time snapshot for now) */}
      <div className="bg-white rounded-lg border border-gray-200 p-4">
        <h3 className="text-sm font-medium text-gray-700 mb-3">实时流量快照</h3>
        <div className="flex gap-6">
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 bg-blue-500 rounded-full"></div>
            <span className="text-gray-600">上行:</span>
            <span className="font-mono text-gray-800">
              {stats ? formatSpeed(stats.upload_bps) : "-"}
            </span>
            <span className="text-green-600">✅</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-3 h-3 bg-green-500 rounded-full"></div>
            <span className="text-gray-600">下行:</span>
            <span className="font-mono text-gray-800">
              {stats ? formatSpeed(stats.download_bps) : "-"}
            </span>
            <span className="text-green-600">✅</span>
          </div>
        </div>
      </div>

      {/* App Traffic Ranking */}
      <div className="bg-white rounded-lg border border-gray-200 p-4">
        <div className="flex justify-between items-center mb-4">
          <h3 className="text-sm font-medium text-gray-700">应用进程列表 ({apps.length})</h3>
          <div className="flex gap-2">
            <input
              type="text"
              placeholder="搜索应用..."
              value={searchTerm}
              onChange={(e) => handleSearchChange(e.target.value)}
              maxLength={100}
              className="px-3 py-1.5 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
            <button
              onClick={fetchData}
              className="px-3 py-1.5 text-sm bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
            >
              刷新
            </button>
          </div>
        </div>

        <div className="space-y-2 h-[500px] overflow-y-auto">
          {filteredApps.length === 0 ? (
            <div className="text-center text-gray-500 py-10">未找到进程</div>
          ) : (
            filteredApps.map((app, index) => (
              <div key={`${app.pid}-${index}`} className="flex items-center justify-between p-3 bg-gray-50 rounded-lg">
                <div className="flex items-center gap-3">
                  <span className="text-gray-500 font-medium w-6">{index + 1}</span>
                  <div>
                    <div className="font-medium text-gray-800">{app.name}</div>
                    <div className="text-xs text-gray-400">PID: {app.pid}</div>
                  </div>
                </div>
                <div className="flex items-center gap-4">
                  <div className="text-right">
                    <div className="font-mono text-xs text-gray-500">
                      ↓ {formatSpeed(app.current_download_bps)}
                    </div>
                  </div>
                  <button className="px-3 py-1 text-xs border border-gray-300 rounded hover:bg-gray-100 transition-colors opacity-50 cursor-not-allowed" disabled>
                    暂不可限速
                  </button>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

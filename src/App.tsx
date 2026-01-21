import { useState, useEffect } from "react";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "./store/settingsStore";
import StatusBar from "./components/StatusBar/StatusBar";
import Navigation from "./components/Navigation/Navigation";
import Dashboard from "./components/Dashboard/Dashboard";
import TrafficMonitorEnhanced from "./components/TrafficMonitor/TrafficMonitorEnhanced";
import ConnectionManager from "./components/ConnectionManager/ConnectionManager";
import EmergencyKit from "./components/EmergencyKit/EmergencyKit";
import Settings from "./components/Settings/Settings";

// Interfaces matching Rust models
interface NetworkStatus {
  status: string;
  message: string;
  timestamp: number;
}

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

interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp: number;
}

function App() {
  const { settings, loadSettings } = useSettingsStore();

  const [networkStatus, setNetworkStatus] = useState<NetworkStatus | null>(null);
  const [ipInfo, setIpInfo] = useState<IPInfo | null>(null);
  const [traffic, setTraffic] = useState<TrafficStats | null>(null);

  // Error states with user feedback
  const [error, setError] = useState<string | null>(null);
  const [retryCount, setRetryCount] = useState(0);

  // Load persisted settings
  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // Initial data fetch and periodic refresh with retry
  useEffect(() => {
    const fetchData = async () => {
      try {
        setError(null);

        // Fetch both in parallel for better performance
        const [status, ip] = await Promise.all([
          invoke<NetworkStatus>("get_network_status"),
          invoke<IPInfo>("get_ip_info", {
            include_geoip: settings.show_geoip,
          }),
        ]);

        setNetworkStatus(status);
        setIpInfo(ip);

        // Reset retry count on success
        setRetryCount(0);
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : "获取网络状态失败";
        console.error("Failed to fetch initial data:", err);
        setError(errorMsg);

        // Exponential backoff retry (max 3 retries)
        if (retryCount < 3) {
          const delay = Math.pow(2, retryCount) * 1000;
          setTimeout(() => {
            setRetryCount(prev => prev + 1);
          }, delay);
        }
      }
    };

    // Initial fetch
    fetchData();

    // Avoid over-polling public IP / geoip with bounds checking
    const intervalMs = Math.max(
      5000,
      Math.min(
        (settings.refresh_interval_secs || 10) * 1000,
        60000 // Max 60 seconds
      )
    );
    const interval = setInterval(fetchData, intervalMs);

    return () => clearInterval(interval);
  }, [settings.refresh_interval_secs, settings.show_geoip, retryCount]);

  // Poll for real-time traffic
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const stats = await invoke<TrafficStats>("get_realtime_traffic");
        setTraffic(stats);
      } catch (error) {
        console.error("Failed to fetch traffic stats:", error);
        // Don't show error for traffic as it's realtime and can recover
      }
    }, 1000);

    return () => clearInterval(interval);
  }, []);

  // Format location string
  const getLocationString = () => {
    if (!settings.show_geoip) return "已关闭";

    const geoip = ipInfo?.ipv4_geoip;
    if (geoip?.country && geoip?.region && geoip?.city) {
      return `${geoip.country} ${geoip.region} ${geoip.city}`;
    }
    return "正在获取位置...";
  };

  const statusBarProps = {
    networkStatus: (networkStatus?.status === "normal" ? "normal" : "abnormal") as "normal" | "abnormal",
    ipv4: ipInfo?.ipv4 || "获取中...",
    ipv6: ipInfo?.ipv6 || "未连接",
    location: getLocationString(),
    downloadSpeed: traffic?.download_bps || 0,
    uploadSpeed: traffic?.upload_bps || 0,
  };

  return (
    <BrowserRouter>
      <div className="h-screen flex flex-col bg-gray-50">
        {/* Error Banner */}
        {error && (
          <div className="bg-red-50 border-b border-red-200 px-4 py-2 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span className="text-red-600">⚠️</span>
              <span className="text-red-700 text-sm">{error}</span>
            </div>
            <button
              onClick={() => setError(null)}
              className="text-red-600 hover:text-red-800 text-sm"
            >
              关闭
            </button>
          </div>
        )}
        <StatusBar {...statusBarProps} />
        <div className="flex-1 flex overflow-hidden">
          <Navigation />
          <main className="flex-1 overflow-auto scrollbar-thin">
            <Routes>
              <Route path="/" element={<Dashboard />} />
              <Route path="/traffic" element={<TrafficMonitorEnhanced />} />
              <Route path="/connections" element={<ConnectionManager />} />
              <Route path="/emergency" element={<EmergencyKit />} />
              <Route path="/settings" element={<Settings />} />
            </Routes>
          </main>
        </div>
      </div>
    </BrowserRouter>
  );
}

export default App;

import { useState, useEffect, useRef, useCallback, lazy, Suspense } from "react";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "./store/settingsStore";
import { useRealtimeTraffic } from "./hooks/useTrafficData";
import { useNetworkData } from "./hooks/useNetworkData";
import { notify } from "./utils/notify";
import i18n from "./i18n";
import StatusBar from "./components/StatusBar/StatusBar";
import Navigation from "./components/Navigation/Navigation";

// Route-level code-splitting: each page loads on demand so the initial bundle
// only carries the shell (StatusBar/Navigation) plus the active route.
const Dashboard = lazy(() => import("./components/Dashboard/Dashboard"));
const TrafficMonitorEnhanced = lazy(
  () => import("./components/TrafficMonitor/TrafficMonitorEnhanced")
);
const ConnectionManager = lazy(
  () => import("./components/ConnectionManager/ConnectionManager")
);
const EmergencyKit = lazy(() => import("./components/EmergencyKit/EmergencyKit"));
const Settings = lazy(() => import("./components/Settings/Settings"));

function App() {
  const { settings, loadSettings } = useSettingsStore();
  const { t } = useTranslation();

  // Error state with user feedback
  const [error, setError] = useState<string | null>(null);
  const retryCountRef = useRef(0);
  const errorRetryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Use shared traffic hook — single global 1s poll
  const { stats: traffic } = useRealtimeTraffic(1000);

  // Use shared network data hook — single global poll with settings interval
  const intervalSecs = Math.max(
    5,
    Math.min(settings.refresh_interval_secs || 10, 60)
  );
  const { networkStatus, ipInfo } = useNetworkData(intervalSecs, settings.show_geoip);

  // Load persisted settings
  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // Apply dark mode by toggling the `dark` class on <html>.
  // The Settings checkbox writes settings.dark_mode via the store; this effect
  // is the single source of truth that reflects it to the DOM (and thus to all
  // Tailwind `dark:` variants and the CSS base theme).
  useEffect(() => {
    document.documentElement.classList.toggle("dark", settings.dark_mode);
  }, [settings.dark_mode]);

  // Sync the active i18n language to the persisted settings.language value.
  useEffect(() => {
    if (settings.language && settings.language !== i18n.language) {
      void i18n.changeLanguage(settings.language);
    }
  }, [settings.language]);

  // Network-abnormal notification: fire a native notification on the
  // normal→abnormal transition (only once per transition), gated by the
  // notify_network_abnormal setting.
  const prevStatusRef = useRef<string | null>(null);
  useEffect(() => {
    const current = networkStatus?.status ?? null;
    const prev = prevStatusRef.current;
    if (
      prev === "normal" &&
      current === "abnormal" &&
      settings.notify_network_abnormal
    ) {
      void notify(t("notify.network_abnormal_title"), t("notify.network_abnormal_body"));
    }
    prevStatusRef.current = current;
  }, [networkStatus, settings.notify_network_abnormal]);

  // Derive error state from network data (replaces old retry logic)
  useEffect(() => {
    if (networkStatus === null && ipInfo === null) {
      // Still loading or both failed — no action needed, hook handles polling
    }
    // Clear error when we get successful data
    if (networkStatus || ipInfo) {
      setError(null);
      retryCountRef.current = 0;
    }
  }, [networkStatus, ipInfo]);

  // Cleanup retry timer on unmount
  useEffect(() => {
    return () => {
      if (errorRetryTimerRef.current) {
        clearTimeout(errorRetryTimerRef.current);
      }
    };
  }, []);

  // Format location string
  const getLocationString = useCallback(() => {
    if (!settings.show_geoip) return "已关闭";

    const geoip = ipInfo?.ipv4_geoip;
    if (geoip?.country && geoip?.region && geoip?.city) {
      return `${geoip.country} ${geoip.region} ${geoip.city}`;
    }
    return "正在获取位置...";
  }, [settings.show_geoip, ipInfo?.ipv4_geoip]);

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
      <div className="h-screen flex flex-col bg-gray-50 dark:bg-gray-900">
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
            <Suspense
              fallback={
                <div className="flex h-full items-center justify-center text-gray-400">
                  加载中…
                </div>
              }
            >
              <Routes>
                <Route path="/" element={<Dashboard />} />
                <Route path="/traffic" element={<TrafficMonitorEnhanced />} />
                <Route path="/connections" element={<ConnectionManager />} />
                <Route path="/emergency" element={<EmergencyKit />} />
                <Route path="/settings" element={<Settings />} />
              </Routes>
            </Suspense>
          </main>
        </div>
      </div>
    </BrowserRouter>
  );
}

export default App;

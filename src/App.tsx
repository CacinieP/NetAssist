import { useState, useEffect, useRef, lazy, Suspense } from "react";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "./store/settingsStore";
import { useRealtimeTraffic, useRecordTrafficPoint } from "./hooks/useTrafficData";
import { useNetworkData, isNetworkResultCurrent } from "./hooks/useNetworkData";
import { useTrafficAlertMonitor } from "./hooks/useTrafficAlertMonitor";
import { notify } from "./utils/notify";
import { geoIPDisplay, probeDescription, probeLimitations } from "./utils/diagnostics";
import { probeSummary } from "./utils/networkProbes";
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
  const { settings, hydrated: settingsHydrated, loading: settingsLoading, loadSettings, error: settingsError } = useSettingsStore();
  const { t } = useTranslation();

  // Error state with user feedback
  const [error, setError] = useState<string | null>(null);

  // Use shared traffic hook — single global 1s poll
  const { stats: traffic } = useRealtimeTraffic(1000);
  useRecordTrafficPoint(5000);

  // Local data always updates; outbound probes wait for a click or saved opt-in.
  const probes = useNetworkData({ owner: true });
  const { networkStatus, ipInfo, loading, ipError, statusError } = probes;

  // Traffic-threshold watchdog: app-level (not page-level) so the detection
  // and its notifications keep running on Dashboard/Settings/Emergency too —
  // it used to live in the Traffic page's useEffect and stopped the moment
  // the user navigated away.
  useTrafficAlertMonitor();

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

  // Network-abnormal notification: fire a native notification on the
  // normal→abnormal transition (only once per transition), gated by the
  // notify_network_abnormal setting.
  const prevStatusRef = useRef<string | null>(null);
  useEffect(() => {
    if (!probes.automatic || probes.stale || !probes.ready) {
      prevStatusRef.current = null;
      return;
    }
    if (loading || !probes.hasRun) return;
    const current = statusError ? null : networkStatus?.status ?? null;
    const prev = prevStatusRef.current;
    if (
      prev === "normal" &&
      current === "abnormal" &&
      settingsHydrated && !settingsLoading && settings.notify_network_abnormal
    ) {
      void notify(t("notify.network_abnormal_title"), t("notify.network_abnormal_body"), () => {
        const currentSettings = useSettingsStore.getState();
        return currentSettings.hydrated && !currentSettings.loading && currentSettings.settings.auto_probe_enabled && currentSettings.settings.notify_network_abnormal && isNetworkResultCurrent(probes.resultId);
      });
    }
    prevStatusRef.current = current;
  }, [networkStatus, statusError, loading, probes.automatic, probes.stale, probes.ready, probes.hasRun, probes.resultId, settingsHydrated, settingsLoading, settings.notify_network_abnormal, t]);

  // Surface settings-load failures (loadSettings sets store error).
  useEffect(() => {
    if (settingsError) {
      setError(settingsError);
    }
  }, [settingsError]);

  const location = geoIPDisplay(ipInfo?.ipv4_geoip, { enabled: settings.show_geoip, loading, error: ipError, hasQueried: probes.geoipQueried });
  const statusBarProps = {
    networkStatus: (!probes.hasRun ? (loading ? "loading" : "idle") : statusError ? "abnormal" : networkStatus?.status === "normal" ? "normal" : "abnormal") as "normal" | "abnormal" | "loading" | "idle",
    networkMessage: probes.hasRun ? `上次：${statusError ? "检测失败" : networkStatus?.message || "未获取结果"}` : undefined,
    probeSummary: probeSummary(probes),
    automatic: probes.automatic,
    onProbe: () => { void probes.refresh(); },
    probeDisabled: !probes.ready || loading,
    ipv4: ipInfo?.ipv4 || (loading ? "获取中…" : probes.hasRun ? "未获取到" : "未检测"),
    ipv6: ipInfo?.ipv6 || "未检测到",
    ipv6Detail: `本地接口：${ipInfo?.ipv6_interface || "未确定"}；此地址不代表实际出口。`,
    location: location.text,
    locationDetail: location.detail,
    probeDetail: [...(networkStatus?.probes?.map(probeDescription) ?? []), probeLimitations].join("\n"),
    downloadSpeed: traffic?.download_bps ?? null,
    uploadSpeed: traffic?.upload_bps ?? null,
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

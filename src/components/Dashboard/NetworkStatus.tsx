import { useRealtimeTraffic } from "../../hooks/useTrafficData";
import { useNetworkData } from "../../hooks/useNetworkData";
import { probeSummary } from "../../utils/networkProbes";
import { probeDescription, probeLimitations } from "../../utils/diagnostics";

export default function NetworkStatus() {
  const { stats: trafficStats } = useRealtimeTraffic(1000);
  // Subscribe to the app-level network poll (owned by App.tsx). This card
  // must NOT restart the global interval or drop GeoIP for the whole app.
  const probes = useNetworkData();
  const { networkStatus, statusError } = probes;

  const formatSpeed = (bps: number) => {
    if (bps < 1024) return `${bps.toFixed(1)} B/s`;
    if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
  };

  const isConnected = networkStatus?.status === "normal" && !statusError;

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-lg">
            {!probes.hasRun || probes.stale ? "—" : isConnected ? "✅" : "❌"}
          </span>
          <span className="text-gray-700 dark:text-gray-300">目标连通性（上次结果）:</span>
          <span className={!probes.hasRun || probes.stale ? "text-gray-500" : isConnected ? "text-green-600 font-medium" : "text-red-600 font-medium"}>
            {statusError ? `检测失败：${statusError}` : networkStatus?.message || (probes.loading ? "检测中…" : "未检测")}
          </span>
        </div>
        <p className="text-xs text-gray-500 dark:text-gray-400">{probeSummary(probes)}</p>
        {networkStatus?.probes?.map(probe => <p key={probe.url} className="text-xs text-gray-500 dark:text-gray-400 break-all">
          {probe.success ? "可达" : "失败"} · {probeDescription(probe)}{probe.error ? ` · ${probe.error}` : ""}
        </p>)}
        <p className="text-xs text-gray-500 dark:text-gray-400">{probeLimitations}</p>
        {isConnected && (
          <div className="flex items-center gap-4 text-sm">
            <span className="text-blue-600">
              ↓ {trafficStats ? formatSpeed(trafficStats.download_bps) : "—"}
            </span>
            <span className="text-green-600">
              ↑ {trafficStats ? formatSpeed(trafficStats.upload_bps) : "—"}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

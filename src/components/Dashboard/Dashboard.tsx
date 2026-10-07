import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRealtimeTraffic } from "../../hooks/useTrafficData";
import { useSettingsStore } from "../../store/settingsStore";
import { formatSpeed, formatBytes } from "../../utils/formatUtils";
import NetworkStatus from "./NetworkStatus";
import IPInfoCard from "./IPInfoCard";
import MetricCard from "./MetricCard";
import TrafficChart from "./TrafficChart";
import { createPollingController } from "../../utils/polling";
import { httpMetric, dnsMetric, probeDescription, probeLimitations } from "../../utils/diagnostics";
import { useNetworkData } from "../../hooks/useNetworkData";
import { probeSummary } from "../../utils/networkProbes";

interface CumulativeTraffic {
  total_download_bytes: number;
  total_upload_bytes: number;
  start_timestamp: number;
  end_timestamp: number;
  period: string;
}

type Period = "day" | "week" | "month";

interface ConnectionInfo {
  pid: number;
}

export default function Dashboard() {
  const [bandwidth, setBandwidth] = useState("加载中...");
  const [connections, setConnections] = useState("加载中...");
  const [cumulative, setCumulative] = useState<CumulativeTraffic | null>(null);
  const [period, setPeriod] = useState<Period>("day");
  const { settings } = useSettingsStore();

  const probes = useNetworkData();
  const { httpProbe } = probes;
  const idleMetric = { value: probes.loading ? "检测中…" : "未检测", unit: "", error: null };
  const http = probes.httpError ? { value: "检测失败", unit: "", error: probes.httpError } : httpProbe ? httpMetric(httpProbe) : idleMetric;
  const dns = probes.dnsError ? { value: "检测失败", unit: "", error: probes.dnsError } : probes.dnsStats ? dnsMetric(probes.dnsStats) : idleMetric;

  // Individual error states for local metrics
  const [errors, setErrors] = useState({
    bandwidth: null as string | null,
    connections: null as string | null,
    cumulative: null as string | null,
  });

  // Use shared traffic hook — no more independent polling
  const { stats: traffic, error: trafficError, recordingError } = useRealtimeTraffic(1000);

  const [cumulativePoller] = useState(() => createPollingController({
    fetch: (requestedPeriod: Period) => invoke<CumulativeTraffic>("get_cumulative_traffic", { period: requestedPeriod }),
    onValue: data => {
      setCumulative(data);
      setErrors(prev => ({ ...prev, cumulative: null }));
    },
    onError: () => setErrors(prev => ({ ...prev, cumulative: "获取失败" })),
  }));

  const [connectionsPoller] = useState(() => createPollingController({
    fetch: () => invoke<ConnectionInfo[]>("get_active_connections"),
    onValue: value => {
      setConnections(`${value.length}`);
      setErrors(prev => ({ ...prev, connections: null }));
    },
    onError: error => {
      setConnections("获取失败");
      setErrors(prev => ({ ...prev, connections: String(error) }));
    },
  }));

  useEffect(() => {
    if (traffic) {
      setBandwidth(formatSpeed(traffic.download_bps + traffic.upload_bps));
      setErrors(prev => ({ ...prev, bandwidth: null }));
    } else {
      setBandwidth(trafficError ? "获取失败" : "加载中...");
      setErrors(prev => ({ ...prev, bandwidth: trafficError }));
    }
  }, [traffic, trafficError]);

  useEffect(() => {
    connectionsPoller.configure(undefined, Math.max(2000, (settings.refresh_interval_secs || 5) * 1000));
    return () => connectionsPoller.stop();
  }, [connectionsPoller, settings.refresh_interval_secs]);

  useEffect(() => {
    setCumulative(null);
    setErrors(prev => ({ ...prev, cumulative: null }));
    cumulativePoller.configure(period, Math.max(5000, (settings.refresh_interval_secs || 5) * 1000));
    return () => cumulativePoller.stop();
  }, [cumulativePoller, period, settings.refresh_interval_secs]);

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800 dark:text-gray-100">仪表盘</h2>
        <p className="text-gray-500 dark:text-gray-400">本地监控持续运行；主动探测默认手动发起。</p>
        <div className="flex flex-wrap items-center gap-3 mt-3">
          <button onClick={() => { void probes.refresh(); }} disabled={!probes.ready || probes.loading}
            className="px-3 py-2 bg-blue-600 text-white rounded-lg disabled:opacity-50">
            {probes.loading ? "检测中…" : probes.hasRun ? "重新检测" : "开始检测"}
          </button>
          <span className="text-sm text-gray-500">{probes.automatic ? "自动检测已开启" : "手动检测模式"} · {probeSummary(probes)}</span>
        </div>
        <p className="text-xs text-gray-500 mt-2">检测会发送 HTTP、DNS 和公网 IP 请求；启用地区查询时还会请求 GeoIP 服务。可在设置中开启自动检测。</p>
      </div>

      {/* Network Status */}
      <NetworkStatus />

      {/* IP Information */}
      <IPInfoCard />

      {/* Cumulative Traffic Card */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">累计流量（路由接口统计）</h3>
          <div className="flex gap-1">
            {[
              { key: "day" as Period, label: "今日" },
              { key: "week" as Period, label: "本周" },
              { key: "month" as Period, label: "本月" },
            ].map(p => (
              <button
                key={p.key}
                onClick={() => setPeriod(p.key)}
                className={`px-2 py-1 text-xs rounded transition-colors ${
                  period === p.key
                    ? "bg-blue-50 dark:bg-blue-900/40 text-blue-600 dark:text-blue-300"
                    : "bg-gray-50 dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-600"
                }`}
              >
                {p.label}
              </button>
            ))}
          </div>
        </div>
        {errors.cumulative ? (
          <div className="text-center text-red-500 py-4">{errors.cumulative}</div>
        ) : cumulative && cumulative.period === period ? (
          (cumulative.total_download_bytes === 0 && cumulative.total_upload_bytes === 0) ? (
            <div className="text-center text-gray-400 dark:text-gray-500 py-4 text-sm">
              当前采集区间尚无流量；应用每 5 秒采集一次
            </div>
          ) : (
            <div className="grid grid-cols-3 gap-4">
              <div className="text-center">
                <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">总流量</div>
                <div className="text-lg font-semibold text-gray-800 dark:text-gray-100">
                  {formatBytes(cumulative.total_download_bytes + cumulative.total_upload_bytes)}
                </div>
              </div>
              <div className="text-center">
                <div className="text-xs text-green-600 mb-1">下载</div>
                <div className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  {formatBytes(cumulative.total_download_bytes)}
                </div>
              </div>
              <div className="text-center">
                <div className="text-xs text-blue-600 mb-1">上传</div>
                <div className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  {formatBytes(cumulative.total_upload_bytes)}
                </div>
              </div>
            </div>
          )
        ) : (
          <div className="text-center text-gray-400 dark:text-gray-500 py-4">加载中...</div>
        )}
      </div>

      <p className="text-xs text-gray-500 dark:text-gray-400">累计流量仅包含应用成功采集的区间，不包含未采集或离线期间的流量。{recordingError ? `最近一次历史记录失败：${recordingError}` : ""}</p>

      {/* Metric Cards Grid */}
      <div className="grid grid-cols-2 gap-4">
        <MetricCard
          title="实时流量（路由接口）"
          value={bandwidth}
          status={bandwidth === "加载中..." ? "pending" : errors.bandwidth ? "abnormal" : "normal"}
          unit=""
        />
        <MetricCard
          title="HTTP 探测延迟"
          statusLabel={probes.loading ? "检测中" : !probes.hasRun ? "未检测" : probes.stale ? "上次结果（待重测）" : http.error ? "上次失败" : "上次成功"}
          value={http.value}
          status={!probes.hasRun || probes.stale ? "pending" : http.error ? "abnormal" : "normal"}
          unit={http.unit}
          detail={[probeSummary(probes), httpProbe && probeDescription(httpProbe), http.error].filter(Boolean).join(" · ")}
        />
        <MetricCard
          title="DNS响应"
          statusLabel={probes.loading ? "检测中" : !probes.hasRun ? "未检测" : probes.stale ? "上次结果（待重测）" : dns.error ? "上次失败" : "上次成功"}
          value={dns.value}
          status={!probes.hasRun || probes.stale ? "pending" : dns.error ? "abnormal" : "normal"}
          unit={dns.unit}
          detail={`${probeSummary(probes)} · 服务器：${probes.dnsServer || settings.primary_dns || "8.8.8.8"}${dns.error ? ` · ${dns.error}` : ""}`}
        />
        <MetricCard
          title="活跃连接"
          value={connections}
          status={connections === "加载中..." ? "pending" : errors.connections ? "abnormal" : "normal"}
          unit=""
        />
      </div>

      <p className="text-xs text-gray-500 dark:text-gray-400">HTTP 延迟包含向指定目标发起请求的耗时；不能代表所有网站、游戏或应用的延迟。{probeLimitations}</p>

      {/* Real-time Traffic Chart */}
      <TrafficChart />
    </div>
  );
}

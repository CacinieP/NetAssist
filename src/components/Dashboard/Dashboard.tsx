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
import type { HttpConnectivityResult, DNSStats } from "../../utils/diagnostics";

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
  const [latency, setLatency] = useState("加载中...");
  const [dns, setDns] = useState("加载中...");
  const [connections, setConnections] = useState("加载中...");
  const [cumulative, setCumulative] = useState<CumulativeTraffic | null>(null);
  const [period, setPeriod] = useState<Period>("day");
  const { settings } = useSettingsStore();
  const [httpProbe, setHttpProbe] = useState<HttpConnectivityResult | null>(null);
  const [latencyUnit, setLatencyUnit] = useState("");
  const [dnsUnit, setDnsUnit] = useState("");

  // Individual error states for each metric
  const [errors, setErrors] = useState({
    bandwidth: null as string | null,
    latency: null as string | null,
    dns: null as string | null,
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

  const [metricsPoller] = useState(() => createPollingController({
    fetch: (server: string) => Promise.allSettled([
      invoke<HttpConnectivityResult>("test_http_connectivity", { url: null }),
      invoke<DNSStats>("test_dns", { server }),
      invoke<ConnectionInfo[]>("get_active_connections"),
    ]),
    onValue: ([httpResult, dnsResult, connectionResult]) => {
      const http = httpResult.status === "fulfilled" ? httpMetric(httpResult.value) : { value: "检测失败", unit: "", error: String(httpResult.reason) };
      const dnsValue = dnsResult.status === "fulfilled" ? dnsMetric(dnsResult.value) : { value: "检测失败", unit: "", error: String(dnsResult.reason) };
      setHttpProbe(httpResult.status === "fulfilled" ? httpResult.value : null);
      setLatency(http.value);
      setLatencyUnit(http.unit);
      setDns(dnsValue.value);
      setDnsUnit(dnsValue.unit);
      setConnections(connectionResult.status === "fulfilled" ? `${connectionResult.value.length}` : "获取失败");
      setErrors(prev => ({ ...prev, latency: http.error, dns: dnsValue.error,
        connections: connectionResult.status === "fulfilled" ? null : String(connectionResult.reason) }));
    },
    onError: error => setErrors(prev => ({ ...prev, latency: String(error), dns: String(error), connections: String(error) })),
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
    setLatency("加载中...");
    setDns("加载中...");
    setConnections("加载中...");
    setHttpProbe(null);
    setErrors(prev => ({ ...prev, latency: null, dns: null, connections: null }));
    metricsPoller.configure(settings.primary_dns || "8.8.8.8", Math.max(2000, (settings.refresh_interval_secs || 5) * 1000));
    return () => metricsPoller.stop();
  }, [metricsPoller, settings.refresh_interval_secs, settings.primary_dns]);

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
        <p className="text-gray-500 dark:text-gray-400">网络状态概览</p>
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
          value={latency}
          status={latency === "加载中..." ? "pending" : errors.latency ? "abnormal" : "normal"}
          unit={latency === "加载中..." ? "" : latencyUnit}
          detail={[httpProbe && probeDescription(httpProbe), errors.latency].filter(Boolean).join(" · ")}
        />
        <MetricCard
          title="DNS响应"
          value={dns}
          status={dns === "加载中..." ? "pending" : errors.dns ? "abnormal" : "normal"}
          unit={dns === "加载中..." ? "" : dnsUnit}
          detail={`服务器：${settings.primary_dns || "8.8.8.8"}${errors.dns ? ` · ${errors.dns}` : ""}`}
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

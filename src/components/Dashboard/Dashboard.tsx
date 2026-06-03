import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRealtimeTraffic, useRecordTrafficPoint } from "../../hooks/useTrafficData";
import { formatSpeed, formatBytes } from "../../utils/formatUtils";
import NetworkStatus from "./NetworkStatus";
import IPInfoCard from "./IPInfoCard";
import MetricCard from "./MetricCard";
import TrafficChart from "./TrafficChart";

interface CumulativeTraffic {
  total_download_bytes: number;
  total_upload_bytes: number;
  start_timestamp: number;
  end_timestamp: number;
  period: string;
}

type Period = "day" | "week" | "month";

interface HttpConnectivityResult {
  url: string;
  success: boolean;
  latency_ms: number;
  status_code: number | null;
  error: string | null;
}

interface DNSStats {
  avg_latency_ms: number;
  success_rate: number;
}

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

  // Individual error states for each metric
  const [errors, setErrors] = useState({
    bandwidth: null as string | null,
    latency: null as string | null,
    dns: null as string | null,
    connections: null as string | null,
    cumulative: null as string | null,
  });

  // Use shared traffic hook — no more independent polling
  const { stats: traffic } = useRealtimeTraffic(1000);

  // Record traffic points every 60 seconds using shared hook
  useRecordTrafficPoint(60000);

  // Fetch cumulative traffic — useCallback with period ref to avoid stale closures
  const periodRef = useRef(period);
  periodRef.current = period;

  const fetchCumulative = useCallback(async () => {
    try {
      const data = await invoke<CumulativeTraffic>("get_cumulative_traffic", { period: periodRef.current });
      setCumulative(data);
      setErrors(prev => ({ ...prev, cumulative: null }));
    } catch (error) {
      console.error("Failed to fetch cumulative traffic:", error);
      setErrors(prev => ({ ...prev, cumulative: "获取失败" }));
    }
  }, []);

  const fetchMetrics = useCallback(async () => {
    await Promise.allSettled([
      invoke<HttpConnectivityResult>("test_http_connectivity", { url: null })
        .then(http => {
          setLatency(http.success ? `${Math.round(http.latency_ms)}` : "超时");
          setErrors(prev => ({ ...prev, latency: null }));
        })
        .catch(err => {
          console.error("HTTP connectivity fetch failed:", err);
          setErrors(prev => ({ ...prev, latency: "检测失败" }));
          setLatency("错误");
        }),

      invoke<DNSStats>("test_dns", { server: "8.8.8.8" })
        .then(dnsRes => {
          setDns(`${Math.round(dnsRes.avg_latency_ms)}`);
          setErrors(prev => ({ ...prev, dns: null }));
        })
        .catch(err => {
          console.error("DNS fetch failed:", err);
          setErrors(prev => ({ ...prev, dns: "获取失败" }));
          setDns("错误");
        }),

      invoke<ConnectionInfo[]>("get_active_connections")
        .then(conns => {
          setConnections(`${conns.length}`);
          setErrors(prev => ({ ...prev, connections: null }));
        })
        .catch(err => {
          console.error("Connections fetch failed:", err);
          setErrors(prev => ({ ...prev, connections: "获取失败" }));
          setConnections("错误");
        }),
    ]);
  }, []);

  // Fetch metrics when bandwidth changes (from shared traffic hook)
  useEffect(() => {
    if (traffic) {
      const totalBps = traffic.download_bps + traffic.upload_bps;
      setBandwidth(formatSpeed(totalBps));
      setErrors(prev => ({ ...prev, bandwidth: null }));
    }
  }, [traffic]);

  useEffect(() => {
    // Initial fetch
    fetchMetrics();
    fetchCumulative();

    // Poll for metric updates (every 2 seconds) — traffic is handled by shared hook
    const interval = setInterval(fetchMetrics, 2000);

    // Poll cumulative traffic every 5 seconds
    const cumulativeInterval = setInterval(fetchCumulative, 5000);

    return () => {
      clearInterval(interval);
      clearInterval(cumulativeInterval);
    };
  }, [fetchMetrics, fetchCumulative]);

  // Re-fetch cumulative when period changes
  useEffect(() => {
    fetchCumulative();
  }, [period, fetchCumulative]);

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800">仪表盘</h2>
        <p className="text-gray-500">网络状态概览</p>
      </div>

      {/* Network Status */}
      <NetworkStatus />

      {/* IP Information */}
      <IPInfoCard />

      {/* Cumulative Traffic Card */}
      <div className="bg-white rounded-lg border border-gray-200 p-4">
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-medium text-gray-700">累计流量统计</h3>
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
                    ? "bg-blue-50 text-blue-600"
                    : "bg-gray-50 text-gray-600 hover:bg-gray-100"
                }`}
              >
                {p.label}
              </button>
            ))}
          </div>
        </div>
        {errors.cumulative ? (
          <div className="text-center text-red-500 py-4">{errors.cumulative}</div>
        ) : cumulative ? (
          <div className="grid grid-cols-3 gap-4">
            <div className="text-center">
              <div className="text-xs text-gray-500 mb-1">总流量</div>
              <div className="text-lg font-semibold text-gray-800">
                {formatBytes(cumulative.total_download_bytes + cumulative.total_upload_bytes)}
              </div>
            </div>
            <div className="text-center">
              <div className="text-xs text-green-600 mb-1">下载</div>
              <div className="text-sm font-medium text-gray-700">
                {formatBytes(cumulative.total_download_bytes)}
              </div>
            </div>
            <div className="text-center">
              <div className="text-xs text-blue-600 mb-1">上传</div>
              <div className="text-sm font-medium text-gray-700">
                {formatBytes(cumulative.total_upload_bytes)}
              </div>
            </div>
          </div>
        ) : (
          <div className="text-center text-gray-400 py-4">加载中...</div>
        )}
      </div>

      {/* Metric Cards Grid */}
      <div className="grid grid-cols-2 gap-4">
        <MetricCard
          title="总带宽"
          value={bandwidth}
          status={errors.bandwidth ? "abnormal" : "normal"}
          unit=""
        />
        <MetricCard
          title="网络延迟"
          value={latency}
          status={errors.latency || latency === "超时" ? "abnormal" : "normal"}
          unit={latency === "超时" || errors.latency ? "" : "ms"}
        />
        <MetricCard
          title="DNS响应"
          value={dns}
          status={errors.dns ? "abnormal" : "normal"}
          unit={errors.dns ? "" : "ms"}
        />
        <MetricCard
          title="活跃连接"
          value={connections}
          status={errors.connections ? "abnormal" : "normal"}
          unit=""
        />
      </div>

      {/* Real-time Traffic Chart */}
      <TrafficChart />
    </div>
  );
}

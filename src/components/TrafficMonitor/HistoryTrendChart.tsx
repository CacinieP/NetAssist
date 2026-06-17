import { useState, useEffect, useRef } from "react";
import * as echarts from "echarts";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "../../store/settingsStore";

interface TrafficHistoryPoint {
  timestamp: number;
  download_bps: number;
  upload_bps: number;
}

interface TrafficHistory {
  data: TrafficHistoryPoint[];
  start_timestamp: number;
  end_timestamp: number;
}

interface HistoryTrendChartProps {
  hours: number;
  onHoursChange: (hours: number) => void;
}

const HISTORY_RANGES = [
  { label: "1小时", hours: 1 },
  { label: "6小时", hours: 6 },
  { label: "24小时", hours: 24 },
  { label: "7天", hours: 24 * 7 },
  { label: "30天", hours: 24 * 30 },
];

export default function HistoryTrendChart({ hours, onHoursChange }: HistoryTrendChartProps) {
  const chartRef = useRef<HTMLDivElement>(null);
  const chartInstance = useRef<echarts.ECharts | null>(null);
  const [history, setHistory] = useState<TrafficHistory | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { settings } = useSettingsStore();
  const isDark = settings.dark_mode;
  const axisColor = isDark ? "#9ca3af" : "#6b7280";
  const splitColor = isDark ? "#374151" : "#e5e7eb";
  const titleColor = isDark ? "#e5e7eb" : "#374151";

  // Fetch history data
  const fetchHistory = async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await invoke<TrafficHistory>("get_traffic_history", { hours });
      setHistory(data);
    } catch (err) {
      console.error("Failed to fetch traffic history:", err);
      setError("加载历史数据失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchHistory();
  }, [hours]);

  // Initialize chart instance + resize listener (re-init on theme change)
  useEffect(() => {
    if (!chartRef.current) return;

    chartInstance.current = echarts.init(chartRef.current, isDark ? "dark" : undefined);

    const handleResize = () => {
      chartInstance.current?.resize();
    };
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      chartInstance.current?.dispose();
      chartInstance.current = null;
    };
  }, [isDark]);

  // Update chart data when history/hours change
  useEffect(() => {
    if (!chartInstance.current) return;

    if (!history || history.data.length === 0) {
      chartInstance.current.clear();
      return;
    }

    // Format data for chart
    const downloadData = history.data.map(p => [p.timestamp, p.download_bps / 1024]); // Convert to KB/s
    const uploadData = history.data.map(p => [p.timestamp, p.upload_bps / 1024]);

    const option: echarts.EChartsOption = {
      backgroundColor: "transparent",
      title: {
        text: "流量历史趋势",
        left: "left",
        textStyle: {
          fontSize: 14,
          fontWeight: "normal",
          color: titleColor,
        },
      },
      tooltip: {
        trigger: "axis",
        formatter: (params: any) => {
          if (!params[0]) return "";
          const time = new Date(params[0].value[0]).toLocaleString();
          const fmtVal = (v: number) => v >= 1024 ? `${(v / 1024).toFixed(2)} MB/s` : `${v.toFixed(2)} KB/s`;
          return `${time}<br/>下载: ${fmtVal(params[0].value[1])}<br/>上传: ${fmtVal(params[1].value[1])}`;
        },
      },
      legend: {
        data: ["下载", "上传"],
        bottom: 0,
        textStyle: { color: axisColor },
      },
      grid: {
        left: "3%",
        right: "4%",
        bottom: "15%",
        top: "15%",
        containLabel: true,
      },
      xAxis: {
        type: "time",
        axisLabel: {
          color: axisColor,
          formatter: (value: number) => {
            const date = new Date(value);
            if (hours <= 1) return date.toLocaleTimeString();
            if (hours <= 24) return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
            return date.toLocaleDateString([], { month: '2-digit', day: '2-digit' });
          },
        },
        axisLine: { lineStyle: { color: splitColor } },
        splitLine: { lineStyle: { color: splitColor } },
      },
      yAxis: {
        type: "value",
        axisLabel: {
          color: axisColor,
          formatter: (value: number) => {
            if (value >= 1024) return `${(value / 1024).toFixed(0)} MB/s`;
            return `${value.toFixed(0)} KB/s`;
          },
        },
        splitLine: { lineStyle: { color: splitColor } },
      },
      dataZoom: [
        {
          type: "inside",
          start: 0,
          end: 100,
        },
        {
          type: "slider",
          start: 0,
          end: 100,
          height: 20,
          bottom: 40,
        },
      ],
      series: [
        {
          name: "下载",
          type: "line",
          smooth: true,
          showSymbol: false,
          areaStyle: {
            color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
              { offset: 0, color: "rgba(34, 197, 94, 0.3)" },
              { offset: 1, color: "rgba(34, 197, 94, 0.05)" },
            ]),
          },
          itemStyle: { color: "#22c55e" },
          data: downloadData,
        },
        {
          name: "上传",
          type: "line",
          smooth: true,
          showSymbol: false,
          areaStyle: {
            color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
              { offset: 0, color: "rgba(59, 130, 246, 0.3)" },
              { offset: 1, color: "rgba(59, 130, 246, 0.05)" },
            ]),
          },
          itemStyle: { color: "#3b82f6" },
          data: uploadData,
        },
      ],
      animationDuration: 500,
    };

    chartInstance.current.setOption(option, true);
  }, [history, hours, axisColor, splitColor, titleColor]);

  const isEmpty = !loading && history && history.data.length === 0;

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">历史趋势图</h3>
        <div className="flex gap-1">
          {HISTORY_RANGES.map(range => (
            <button
              key={range.hours}
              onClick={() => onHoursChange(range.hours)}
              className={`px-2 py-1 text-xs rounded transition-colors ${
                hours === range.hours
                  ? "bg-blue-600 text-white"
                  : "bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-600"
              }`}
            >
              {range.label}
            </button>
          ))}
        </div>
      </div>

      {error && (
        <div className="mb-2 p-2 bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-300 text-xs rounded flex items-center gap-2">
          <span>⚠️</span>
          {error}
          <button onClick={fetchHistory} className="ml-auto underline">重试</button>
        </div>
      )}

      {loading ? (
        <div className="flex items-center justify-center" style={{ height: "300px" }}>
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600"></div>
        </div>
      ) : isEmpty ? (
        <div className="flex flex-col items-center justify-center text-center" style={{ height: "300px" }}>
          <div className="text-4xl mb-3 opacity-60">📊</div>
          <p className="text-gray-500 dark:text-gray-400 text-sm">暂无历史数据</p>
          <p className="text-gray-400 dark:text-gray-500 text-xs mt-1">
            流量数据每分钟记录一次，请保持应用运行后稍后查看
          </p>
        </div>
      ) : (
        <div ref={chartRef} style={{ width: "100%", height: "300px" }} />
      )}

      {!loading && history && !isEmpty && (
        <div className="mt-2 text-xs text-gray-500 dark:text-gray-400 text-center">
          {history.data.length} 个数据点
          {history.start_timestamp && history.end_timestamp && (
            <span className="ml-2">
              ({new Date(history.start_timestamp).toLocaleString()} - {new Date(history.end_timestamp).toLocaleString()})
            </span>
          )}
        </div>
      )}
    </div>
  );
}

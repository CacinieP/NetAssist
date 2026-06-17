import { useEffect, useRef, useState } from "react";
import * as echarts from "echarts";
import { useRealtimeTraffic } from "../../hooks/useTrafficData";
import { useSettingsStore } from "../../store/settingsStore";

export default function TrafficChart() {
  const chartRef = useRef<HTMLDivElement>(null);
  const chartInstance = useRef<echarts.ECharts | null>(null);
  const dataRef = useRef<{ time: number; download: number; upload: number }[]>([]);
  const [dataError, setDataError] = useState(false);
  const { settings } = useSettingsStore();
  const isDark = settings.dark_mode;

  // Use shared traffic hook
  const { stats } = useRealtimeTraffic(1000);

  // Theme-aware palette for ECharts (text/axis colors only; series colors stay).
  const axisColor = isDark ? "#9ca3af" : "#6b7280";
  const splitColor = isDark ? "#374151" : "#e5e7eb";
  const titleColor = isDark ? "#e5e7eb" : "#374151";

  // Initialize chart (re-init on theme change so text/axis colors update)
  useEffect(() => {
    if (!chartRef.current) return;

    chartInstance.current = echarts.init(chartRef.current, isDark ? "dark" : undefined);

    const option: echarts.EChartsOption = {
      backgroundColor: "transparent",
      title: {
        text: "实时流量（Real-time）",
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
          const time = new Date(params[0].value[0]).toLocaleTimeString();
          const dl = params[0].value[1];
          const ul = params[1].value[1];
          const fmtVal = (v: number) => v >= 1024 ? `${(v / 1024).toFixed(2)} MB/s` : `${v.toFixed(2)} KB/s`;
          return `${time}<br/>下行: ${fmtVal(dl)}<br/>上行: ${fmtVal(ul)}`;
        },
      },
      legend: {
        data: ["下行", "上行"],
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
          formatter: (value: number) => new Date(value).toLocaleTimeString(),
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
      series: [
        {
          name: "下行",
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
          data: [],
        },
        {
          name: "上行",
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
          data: [],
        },
      ],
      animationDuration: 500,
    };

    chartInstance.current.setOption(option);

    const handleResize = () => {
      chartInstance.current?.resize();
    };
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      chartInstance.current?.dispose();
      chartInstance.current = null;
    };
  }, [isDark, axisColor, splitColor, titleColor]);

  // Update chart data when traffic stats change
  useEffect(() => {
    if (!stats || !chartInstance.current) return;

    const now = Date.now();
    const downloadKB = stats.download_bps / 1024;
    const uploadKB = stats.upload_bps / 1024;

    dataRef.current.push({ time: now, download: downloadKB, upload: uploadKB });
    if (dataRef.current.length > 60) {
      dataRef.current.shift();
    }

    setDataError(false);

    chartInstance.current.setOption({
      series: [
        { data: dataRef.current.map(d => [d.time, d.download]) },
        { data: dataRef.current.map(d => [d.time, d.upload]) },
      ],
    });
  }, [stats]);

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 relative">
      {dataError && (
        <div className="absolute top-2 right-2 z-10 flex items-center gap-1 text-red-500 text-sm bg-white dark:bg-gray-700 px-2 py-1 rounded">
          <span className="w-2 h-2 bg-red-500 rounded-full animate-pulse"></span>
          数据更新失败
        </div>
      )}
      <div ref={chartRef} style={{ width: "100%", height: "300px" }} />
    </div>
  );
}

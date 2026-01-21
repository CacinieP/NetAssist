import { useEffect, useRef, useState } from "react";
import * as echarts from "echarts";
import { invoke } from "@tauri-apps/api/core";

interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp: number;
}

export default function TrafficChart() {
  const chartRef = useRef<HTMLDivElement>(null);
  const chartInstance = useRef<echarts.ECharts | null>(null);
  const dataRef = useRef<{ time: number; download: number; upload: number }[]>([]);
  const [dataError, setDataError] = useState(false);

  useEffect(() => {
    if (!chartRef.current) return;

    // Initialize chart
    chartInstance.current = echarts.init(chartRef.current);

    // Initial Empty Option
    const option: echarts.EChartsOption = {
      title: {
        text: "实时流量（Real-time）",
        left: "left",
        textStyle: {
          fontSize: 14,
          fontWeight: "normal",
          color: "#374151",
        },
      },
      tooltip: {
        trigger: "axis",
        formatter: (params: any) => {
          if (!params[0]) return "";
          const time = new Date(params[0].value[0]).toLocaleTimeString();
          return `${time}<br/>下行: ${params[0].value[1].toFixed(2)} KB/s<br/>上行: ${params[1].value[1].toFixed(2)} KB/s`;
        },
      },
      legend: {
        data: ["下行", "上行"],
        bottom: 0,
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
          formatter: (value: number) => new Date(value).toLocaleTimeString(),
        },
      },
      yAxis: {
        type: "value",
        axisLabel: {
          formatter: "{value} KB/s",
        },
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

    // Fetch and update loop
    const fetchData = async () => {
      try {
        const stats = await invoke<TrafficStats>("get_realtime_traffic");
        const now = Date.now();
        const downloadKB = stats.download_bps / 1024;
        const uploadKB = stats.upload_bps / 1024;

        // Maintain array of last 60 seconds (approx)
        dataRef.current.push({ time: now, download: downloadKB, upload: uploadKB });
        if (dataRef.current.length > 60) {
          dataRef.current.shift();
        }

        // Update chart
        chartInstance.current?.setOption({
          series: [
            {
              data: dataRef.current.map(d => [d.time, d.download])
            },
            {
              data: dataRef.current.map(d => [d.time, d.upload])
            }
          ]
        });

      } catch (error) {
        console.error("Chart data fetch failed", error);
        setDataError(true);
        // Don't update chart data on error, but keep existing display
      }
    };

    const interval = setInterval(fetchData, 1000);

    // Handle resize
    const handleResize = () => {
      chartInstance.current?.resize();
    };
    window.addEventListener("resize", handleResize);

    return () => {
      clearInterval(interval);
      window.removeEventListener("resize", handleResize);
      chartInstance.current?.dispose();
    };
  }, []);

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-4 relative">
      {dataError && (
        <div className="absolute top-2 right-2 z-10 flex items-center gap-1 text-red-500 text-sm bg-white px-2 py-1 rounded">
          <span className="w-2 h-2 bg-red-500 rounded-full animate-pulse"></span>
          数据更新失败
        </div>
      )}
      <div ref={chartRef} style={{ width: "100%", height: "300px" }} />
    </div>
  );
}

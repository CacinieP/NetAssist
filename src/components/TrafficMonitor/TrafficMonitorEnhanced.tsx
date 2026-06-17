import { useState, useEffect, useMemo, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import * as echarts from "echarts";
import { Activity, TrendingUp, AlertTriangle, Download, Upload, BarChart3, PieChart, FileText, Plus, Trash2, Edit, X, Save } from "lucide-react";
import { useRealtimeTraffic, useRecordTrafficPoint } from "../../hooks/useTrafficData";
import { formatSpeed, formatBytes } from "../../utils/formatUtils";
import HistoryTrendChart from "./HistoryTrendChart";

// ==================== Type Definitions ====================

interface AppTraffic {
  name: string;
  pid: number;
  download_bytes: number;
  upload_bytes: number;
  current_download_bps: number;
  current_upload_bps: number;
}

interface AppTrafficHistory {
  timestamp: number;
  download_bps: number;
  upload_bps: number;
}

interface CumulativeTraffic {
  total_download_bytes: number;
  total_upload_bytes: number;
  start_timestamp: number;
  end_timestamp: number;
  period: string;
}

interface TrafficAlert {
  id: string;
  name: string;
  alert_type: string;
  threshold_bytes: number;
  period: string;
  enabled: boolean;
  triggered: boolean;
  last_triggered: number | null;
}

interface AlertStatus {
  alert_id: string;
  triggered: boolean;
  current_value: number;
  threshold_value: number;
  percentage: number;
}

type SortField = "name" | "download" | "upload" | "total";
type SortOrder = "asc" | "desc";
type Period = "day" | "week" | "month";

// ==================== Sub-Components ====================

// 1. 累计流量统计卡片
const CumulativeStatsCard = ({
  data,
  loading,
  period,
  onPeriodChange,
}: {
  data: CumulativeTraffic | null;
  loading: boolean;
  period: Period;
  onPeriodChange: (period: Period) => void;
}) => {
  const periods: { key: Period; label: string }[] = [
    { key: "day", label: "今日" },
    { key: "week", label: "本周" },
    { key: "month", label: "本月" },
  ];

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">累计流量统计</h3>
        <div className="flex gap-1">
          {periods.map(p => (
            <button
              key={p.key}
              onClick={() => onPeriodChange(p.key)}
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
      {loading ? (
        <div className="animate-pulse h-16 bg-gray-100 dark:bg-gray-700 rounded"></div>
      ) : (
        <div className="grid grid-cols-3 gap-4">
          <div className="text-center">
            <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">总流量</div>
            <div className="text-lg font-semibold text-gray-800 dark:text-gray-100">{formatBytes((data?.total_download_bytes || 0) + (data?.total_upload_bytes || 0))}</div>
          </div>
          <div className="text-center">
            <div className="flex items-center justify-center gap-1 text-xs text-green-600 mb-1">
              <Download size={12} />
              <span>下载</span>
            </div>
            <div className="text-sm font-medium text-gray-700 dark:text-gray-300">{formatBytes(data?.total_download_bytes || 0)}</div>
          </div>
          <div className="text-center">
            <div className="flex items-center justify-center gap-1 text-xs text-blue-600 mb-1">
              <Upload size={12} />
              <span>上传</span>
            </div>
            <div className="text-sm font-medium text-gray-700 dark:text-gray-300">{formatBytes(data?.total_upload_bytes || 0)}</div>
          </div>
        </div>
      )}
    </div>
  );
};

// 2. 流量告警组件
const TrafficAlertCard = ({
  alerts,
  alertStatuses,
  onManageAlerts,
}: {
  alerts: TrafficAlert[];
  alertStatuses: AlertStatus[];
  onManageAlerts: () => void;
}) => {
  const getStatus = (alertId: string) => alertStatuses.find(s => s.alert_id === alertId);

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 flex items-center gap-2">
          <AlertTriangle size={16} className="text-amber-500" />
          流量告警
        </h3>
        <button
          onClick={onManageAlerts}
          className="text-xs text-blue-600 dark:text-blue-400 hover:text-blue-800 dark:hover:text-blue-300 hover:underline"
        >
          管理告警
        </button>
      </div>
      <div className="space-y-2">
        {alerts.filter(a => a.enabled).slice(0, 3).map(alert => {
          const status = getStatus(alert.id);
          const percentage = status?.percentage || 0;
          const isWarning = percentage >= 80 && percentage < 100;
          const isCritical = percentage >= 100;

          return (
            <div key={alert.id} className="p-2 bg-gray-50 dark:bg-gray-700/50 rounded">
              <div className="flex justify-between items-center mb-1">
                <span className="text-xs font-medium text-gray-700 dark:text-gray-200">{alert.name}</span>
                <span className={`text-xs ${isCritical ? 'text-red-600' : isWarning ? 'text-amber-600' : 'text-gray-500 dark:text-gray-400'}`}>
                  {percentage.toFixed(0)}%
                </span>
              </div>
              <div className="w-full h-1.5 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                <div
                  className={`h-full rounded-full transition-all ${isCritical ? 'bg-red-500' : isWarning ? 'bg-amber-500' : 'bg-green-500'}`}
                  style={{ width: `${Math.min(percentage, 100)}%` }}
                />
              </div>
              <div className="flex justify-between mt-1 text-xs text-gray-400 dark:text-gray-500">
                <span>{formatBytes(status?.current_value || 0)}</span>
                <span>/ {formatBytes(alert.threshold_bytes)}</span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};

// 3. 应用排序组件表头
const AppTableHeader = ({ field, order, onSort }: { field: SortField | null, order: SortOrder, onSort: (field: SortField) => void }) => {
  const headers: { key: SortField; label: string; width?: string }[] = [
    { key: "name", label: "应用名称" },
    { key: "download", label: "下载", width: "100px" },
    { key: "upload", label: "上传", width: "100px" },
    { key: "total", label: "总计", width: "100px" },
    { key: "total", label: "占比", width: "70px" },
  ];

  return (
    <div className="grid grid-cols-[2fr,100px,100px,100px,70px,80px] gap-2 px-4 py-2 bg-gray-50 dark:bg-gray-700/50 rounded-t-lg">
      {headers.map((header, idx) => (
        <button
          key={`${header.key}-${idx}`}
          onClick={() => onSort(header.key)}
          className={`text-xs font-medium text-left flex items-center gap-1 ${
            field === header.key ? "text-blue-600 dark:text-blue-400" : "text-gray-600 dark:text-gray-300 hover:text-gray-800 dark:hover:text-gray-100"
          }`}
          style={{ width: header.width }}
        >
          {header.label}
          {field === header.key && (
            <span className="text-xs">{order === "asc" ? "↑" : "↓"}</span>
          )}
        </button>
      ))}
      <div className="w-20"></div>
    </div>
  );
};

// 4. 应用占比饼图组件
const AppPieChart = ({ apps }: { apps: AppTraffic[] }) => {
  const chartRef = useRef<HTMLDivElement>(null);
  const chartInstance = useRef<echarts.ECharts | null>(null);

  const topApps = useMemo(() => {
    return apps
      .map(app => ({
        name: app.name,
        value: app.current_download_bps + app.current_upload_bps,
      }))
      .filter(app => app.value > 0)
      .sort((a, b) => b.value - a.value)
      .slice(0, 10);
  }, [apps]);

  useEffect(() => {
    if (!chartRef.current || topApps.length === 0) return;

    if (!chartInstance.current) {
      chartInstance.current = echarts.init(chartRef.current);
    }

    const option: echarts.EChartsOption = {
      tooltip: {
        trigger: "item",
        formatter: "{a} <br/>{b}: {c} B/s ({d}%)",
      },
      legend: {
        orient: "vertical",
        right: 10,
        top: "center",
        textStyle: { fontSize: 11 },
      },
      series: [
        {
          name: "流量占比",
          type: "pie",
          radius: ["40%", "70%"],
          center: ["35%", "50%"],
          avoidLabelOverlap: false,
          itemStyle: {
            borderRadius: 6,
            borderColor: "#fff",
            borderWidth: 2,
          },
          label: {
            show: false,
          },
          data: topApps,
        },
      ],
    };

    chartInstance.current.setOption(option);

    const handleResize = () => chartInstance.current?.resize();
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
    };
  }, [topApps]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      chartInstance.current?.dispose();
      chartInstance.current = null;
    };
  }, []);

  if (topApps.length === 0) {
    return (
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-3 flex items-center gap-2">
          <PieChart size={16} />
          应用流量占比 (Top 10)
        </h3>
        <div className="text-center text-gray-500 dark:text-gray-400 py-8 text-sm">暂无流量数据</div>
      </div>
    );
  }

  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-3 flex items-center gap-2">
        <PieChart size={16} />
        应用流量占比 (Top 10)
      </h3>
      <div ref={chartRef} style={{ width: "100%", height: "250px" }} />
    </div>
  );
};

// ==================== Main Component ====================

export default function TrafficMonitorEnhanced() {
  // State
  const [apps, setApps] = useState<AppTraffic[]>([]);
  const [cumulative, setCumulative] = useState<CumulativeTraffic | null>(null);
  const [alerts, setAlerts] = useState<TrafficAlert[]>([]);
  const [alertStatuses, setAlertStatuses] = useState<AlertStatus[]>([]);

  const [searchTerm, setSearchTerm] = useState("");
  const [historyHours, setHistoryHours] = useState<number>(1);
  const [period, setPeriod] = useState<Period>("day");
  const [sortField, setSortField] = useState<SortField>("total");
  const [sortOrder, setSortOrder] = useState<SortOrder>("desc");
  const [showManageAlerts, setShowManageAlerts] = useState(false);
  const [editingAlert, setEditingAlert] = useState<TrafficAlert | null>(null);
  const [showAddAlertForm, setShowAddAlertForm] = useState(false);
  const [newAlert, setNewAlert] = useState<Partial<TrafficAlert>>({
    name: "",
    alert_type: "total",
    threshold_bytes: 10 * 1024 * 1024 * 1024, // 10 GB default
    period: "day",
    enabled: true,
  });

  const [loading, setLoading] = useState({
    apps: false,
    cumulative: false,
    alerts: false,
  });

  // Toast notification state (replaces window.alert)
  const [toast, setToast] = useState<{ message: string; type: "error" | "success" | "warning" } | null>(null);

  const showToast = useCallback((message: string, type: "error" | "success" | "warning" = "error") => {
    setToast({ message, type });
    setTimeout(() => setToast(null), 4000);
  }, []);

  // Details modal state
  const [selectedApp, setSelectedApp] = useState<AppTraffic | null>(null);
  const [showDetailsModal, setShowDetailsModal] = useState(false);
  const [appHistory, setAppHistory] = useState<AppTrafficHistory[]>([]);
  const detailChartRef = useRef<echarts.ECharts | null>(null);

  // Use shared traffic hook — single global 1s poll
  const { stats } = useRealtimeTraffic(1000);

  // Record traffic points using shared hook
  useRecordTrafficPoint(60000);

  // Toast component
  const ToastComponent = () => {
    if (!toast) return null;
    const bgMap = { error: "bg-red-50 border-red-200 text-red-700", success: "bg-green-50 border-green-200 text-green-700", warning: "bg-amber-50 border-amber-200 text-amber-700" };
    return (
      <div className={`fixed top-4 right-4 z-[100] px-4 py-3 rounded-lg border shadow-lg ${bgMap[toast.type]} flex items-center gap-2 animate-[fadeIn_0.2s]`}>
        <span className="text-sm">{toast.message}</span>
        <button onClick={() => setToast(null)} className="ml-2 text-current opacity-50 hover:opacity-100">&times;</button>
      </div>
    );
  };

  // Fetch data functions
  const fetchApps = useCallback(async () => {
    try {
      setLoading(prev => ({ ...prev, apps: true }));
      const ranking = await invoke<AppTraffic[]>("get_app_traffic_ranking");
      setApps(ranking);
    } catch (error) {
      console.error("Failed to fetch app ranking:", error);
    } finally {
      setLoading(prev => ({ ...prev, apps: false }));
    }
  }, []);

  const fetchCumulative = useCallback(async () => {
    try {
      setLoading(prev => ({ ...prev, cumulative: true }));
      const data = await invoke<CumulativeTraffic>("get_cumulative_traffic", { period });
      setCumulative(data);
    } catch (error) {
      console.error("Failed to fetch cumulative traffic:", error);
    } finally {
      setLoading(prev => ({ ...prev, cumulative: false }));
    }
  }, [period]);

  const fetchAlerts = useCallback(async () => {
    try {
      setLoading(prev => ({ ...prev, alerts: true }));
      const [alertsData, statuses] = await Promise.all([
        invoke<TrafficAlert[]>("get_traffic_alerts"),
        invoke<AlertStatus[]>("check_traffic_alerts", { period }),
      ]);
      setAlerts(alertsData);
      setAlertStatuses(statuses);
    } catch (error) {
      console.error("Failed to fetch alerts:", error);
    } finally {
      setLoading(prev => ({ ...prev, alerts: false }));
    }
  }, [period]);

  // Effects
  useEffect(() => {
    fetchApps();
    fetchCumulative();
    fetchAlerts();

    const appsInterval = setInterval(fetchApps, 3000);
    const cumulativeInterval = setInterval(fetchCumulative, 10000);
    const alertsInterval = setInterval(fetchAlerts, 5000);

    return () => {
      clearInterval(appsInterval);
      clearInterval(cumulativeInterval);
      clearInterval(alertsInterval);
    };
  }, [fetchApps, fetchCumulative, fetchAlerts]);

  // Cleanup detail chart on modal close
  useEffect(() => {
    if (!showDetailsModal && detailChartRef.current) {
      detailChartRef.current.dispose();
      detailChartRef.current = null;
    }
  }, [showDetailsModal]);

  // Filter and sort apps
  const filteredAndSortedApps = useMemo(() => {
    let result = apps.filter(app =>
      app.name.toLowerCase().includes(searchTerm.toLowerCase())
    );

    result.sort((a, b) => {
      let compareA = 0, compareB = 0;
      switch (sortField) {
        case "name":
          return sortOrder === "asc"
            ? a.name.localeCompare(b.name)
            : b.name.localeCompare(a.name);
        case "download":
          compareA = a.current_download_bps;
          compareB = b.current_download_bps;
          break;
        case "upload":
          compareA = a.current_upload_bps;
          compareB = b.current_upload_bps;
          break;
        case "total":
          compareA = a.current_download_bps + a.current_upload_bps;
          compareB = b.current_download_bps + b.current_upload_bps;
          break;
      }
      return sortOrder === "asc" ? compareA - compareB : compareB - compareA;
    });

    return result;
  }, [apps, searchTerm, sortField, sortOrder]);

  // Calculate totals
  const totals = useMemo(() => {
    return filteredAndSortedApps.reduce((acc, app) => ({
      download: acc.download + app.current_download_bps,
      upload: acc.upload + app.current_upload_bps,
      total: acc.total + app.current_download_bps + app.current_upload_bps,
      cumulativeDownload: acc.cumulativeDownload + app.download_bytes,
      cumulativeUpload: acc.cumulativeUpload + app.upload_bytes,
    }), {
      download: 0,
      upload: 0,
      total: 0,
      cumulativeDownload: 0,
      cumulativeUpload: 0,
    });
  }, [filteredAndSortedApps]);

  // Handle show details
  const handleShowDetails = (app: AppTraffic) => {
    setSelectedApp(app);
    setShowDetailsModal(true);
    setAppHistory([]);
  };

  // Get app percentage of total
  const getAppPercentage = useCallback((app: AppTraffic) => {
    if (totals.total === 0) return 0;
    return ((app.current_download_bps + app.current_upload_bps) / totals.total * 100);
  }, [totals.total]);

  const handleSort = (field: SortField) => {
    if (sortField === field) {
      setSortOrder(sortOrder === "asc" ? "desc" : "asc");
    } else {
      setSortField(field);
      setSortOrder("desc");
    }
  };

  const exportData = async (format: "csv" | "json") => {
    const timestamp = new Date().toISOString();
    const data = filteredAndSortedApps.map(app => ({
      name: app.name,
      pid: app.pid,
      download_bps: Math.round(app.current_download_bps),
      upload_bps: Math.round(app.current_upload_bps),
      total_bps: Math.round(app.current_download_bps + app.current_upload_bps),
      cumulative_download_bytes: app.download_bytes,
      cumulative_upload_bytes: app.upload_bytes,
      cumulative_total_bytes: app.download_bytes + app.upload_bytes,
      download_speed: formatSpeed(app.current_download_bps),
      upload_speed: formatSpeed(app.current_upload_bps),
      total_speed: formatSpeed(app.current_download_bps + app.current_upload_bps),
      cumulative_download: formatBytes(app.download_bytes),
      cumulative_upload: formatBytes(app.upload_bytes),
      cumulative_total: formatBytes(app.download_bytes + app.upload_bytes),
      percentage: parseFloat(getAppPercentage(app).toFixed(2)),
    }));

    try {
      if (format === "json") {
        const exportPayload = {
          timestamp,
          period,
          summary: {
            total_apps: filteredAndSortedApps.length,
            total_download_bps: Math.round(totals.download),
            total_upload_bps: Math.round(totals.upload),
            total_bps: Math.round(totals.total),
            total_download_bytes: totals.cumulativeDownload,
            total_upload_bytes: totals.cumulativeUpload,
            total_bytes: totals.cumulativeDownload + totals.cumulativeUpload,
          },
          apps: data,
        };
        const content = JSON.stringify(exportPayload, null, 2);
        const fileName = `traffic_export_${timestamp.replace(/[:.]/g, '-')}.json`;

        const filePath = await save({
          defaultPath: fileName,
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (filePath) {
          const encoder = new TextEncoder();
          await writeFile(filePath, encoder.encode(content));
          showToast("导出成功", "success");
        }
      } else {
        const header = [
          "应用名称", "PID", "实时下载(B/s)", "实时上传(B/s)", "实时总计(B/s)",
          "下载速度", "上传速度", "总速度", "累计下载(字节)", "累计上传(字节)",
          "累计总计(字节)", "累计下载", "累计上传", "累计总计", "占比(%)",
        ];

        const rows = data.map(row => [
          `"${row.name.replace(/"/g, '""')}"`,
          row.pid, row.download_bps, row.upload_bps, row.total_bps,
          `"${row.download_speed}"`, `"${row.upload_speed}"`, `"${row.total_speed}"`,
          row.cumulative_download_bytes, row.cumulative_upload_bytes, row.cumulative_total_bytes,
          `"${row.cumulative_download}"`, `"${row.cumulative_upload}"`, `"${row.cumulative_total}"`,
          row.percentage,
        ]);

        const csvContent = "﻿" +
          header.join(",") + "\n" +
          rows.map(row => row.join(",")).join("\n") +
          "\n\n# 汇总\n" +
          `导出时间,${timestamp}\n统计周期,${period}\n` +
          `应用数量,${filteredAndSortedApps.length}\n` +
          `总实时下载,${formatSpeed(totals.download)}\n` +
          `总实时上传,${formatSpeed(totals.upload)}\n` +
          `总实时速度,${formatSpeed(totals.total)}\n` +
          `总累计下载,${formatBytes(totals.cumulativeDownload)}\n` +
          `总累计上传,${formatBytes(totals.cumulativeUpload)}\n` +
          `总累计流量,${formatBytes(totals.cumulativeDownload + totals.cumulativeUpload)}\n`;

        const fileName = `traffic_export_${timestamp.replace(/[:.]/g, '-')}.csv`;
        const filePath = await save({
          defaultPath: fileName,
          filters: [{ name: "CSV", extensions: ["csv"] }],
        });
        if (filePath) {
          const encoder = new TextEncoder();
          await writeFile(filePath, encoder.encode(csvContent));
          showToast("导出成功", "success");
        }
      }
    } catch (err) {
      console.error("Export failed:", err);
      showToast(`导出失败: ${err}`, "error");
    }
  };

  const handleManageAlerts = () => {
    setShowManageAlerts(!showManageAlerts);
  };

  // Alert management functions
  const handleAddAlert = async () => {
    if (!newAlert.name || newAlert.name.trim() === "") {
      showToast("请输入告警名称", "warning");
      return;
    }
    if (!newAlert.threshold_bytes || newAlert.threshold_bytes <= 0) {
      showToast("请输入有效的阈值", "warning");
      return;
    }

    const alertData: TrafficAlert = {
      id: `custom_${Date.now()}`,
      name: newAlert.name.trim(),
      alert_type: newAlert.alert_type || "total",
      threshold_bytes: newAlert.threshold_bytes,
      period: newAlert.period || "day",
      enabled: newAlert.enabled ?? true,
      triggered: false,
      last_triggered: null,
    };

    try {
      await invoke("add_traffic_alert", { alert: alertData });
      await fetchAlerts();
      setShowAddAlertForm(false);
      setNewAlert({
        name: "",
        alert_type: "total",
        threshold_bytes: 10 * 1024 * 1024 * 1024,
        period: "day",
        enabled: true,
      });
      showToast("告警添加成功", "success");
    } catch (error) {
      showToast(`添加告警失败: ${error}`, "error");
    }
  };

  const handleDeleteAlert = async (alertId: string) => {
    // Use a simple confirmation approach without window.confirm
    // In a production app this would be a modal dialog
    try {
      await invoke("delete_traffic_alert", { alertId });
      await fetchAlerts();
      showToast("告警已删除", "success");
    } catch (error) {
      showToast(`删除告警失败: ${error}`, "error");
    }
  };

  const handleEditAlert = (alert: TrafficAlert) => {
    setEditingAlert({ ...alert });
  };

  const handleSaveEdit = async () => {
    if (!editingAlert) return;

    try {
      await invoke("update_traffic_alert", { alert: editingAlert });
      await fetchAlerts();
      setEditingAlert(null);
      showToast("告警已更新", "success");
    } catch (error) {
      showToast(`保存告警失败: ${error}`, "error");
    }
  };

  const handleCancelEdit = () => {
    setEditingAlert(null);
  };

  const parseThresholdInput = (value: string, unit: string): number => {
    const num = parseFloat(value);
    if (isNaN(num) || num <= 0) return 0;
    switch (unit) {
      case "GB": return num * 1024 * 1024 * 1024;
      case "MB": return num * 1024 * 1024;
      case "KB": return num * 1024;
      default: return num;
    }
  };

  // Initialize detail chart (proper lifecycle, not callback ref)
  useEffect(() => {
    if (!showDetailsModal || !selectedApp || appHistory.length === 0) return;

    // Wait for DOM
    const timer = setTimeout(() => {
      const el = document.getElementById("detail-chart");
      if (!el) return;

      // Dispose previous instance
      if (detailChartRef.current) {
        detailChartRef.current.dispose();
      }

      const chart = echarts.init(el);
      detailChartRef.current = chart;

      const option: echarts.EChartsOption = {
        grid: { top: 10, right: 10, bottom: 20, left: 50 },
        xAxis: {
          type: "category",
          data: appHistory.map((_, i) => {
            const minsAgo = appHistory.length - i;
            return minsAgo >= 60 ? `${Math.floor(minsAgo / 60)}h前` : `${minsAgo}m前`;
          }),
          axisLabel: { fontSize: 10 },
        },
        yAxis: {
          type: "value",
          axisLabel: {
            fontSize: 10,
            formatter: (v: number) => v >= 1024 * 1024 ? `${(v / 1024 / 1024).toFixed(1)}M` : `${(v / 1024).toFixed(1)}K`,
          },
        },
        series: [
          {
            name: "下载",
            type: "line",
            data: appHistory.map(h => h.download_bps),
            smooth: true,
            itemStyle: { color: "#22c55e" },
            areaStyle: { color: { type: "linear", x: 0, y: 0, x2: 0, y2: 1, colorStops: [{ offset: 0, color: "rgba(34, 197, 94, 0.3)" }, { offset: 1, color: "rgba(34, 197, 94, 0)" }] } },
          },
          {
            name: "上传",
            type: "line",
            data: appHistory.map(h => h.upload_bps),
            smooth: true,
            itemStyle: { color: "#3b82f6" },
            areaStyle: { color: { type: "linear", x: 0, y: 0, x2: 0, y2: 1, colorStops: [{ offset: 0, color: "rgba(59, 130, 246, 0.3)" }, { offset: 1, color: "rgba(59, 130, 246, 0)" }] } },
          },
        ],
        tooltip: {
          trigger: "axis",
          formatter: (params: any) => {
            let tip = `${params[0].axisValue}<br/>`;
            params.forEach((p: any) => {
              tip += `${p.marker} ${p.seriesName}: ${formatSpeed(p.value)}<br/>`;
            });
            return tip;
          },
        },
      };
      chart.setOption(option);

      const handleResize = () => chart.resize();
      window.addEventListener("resize", handleResize);

      // Store cleanup function
      return () => {
        window.removeEventListener("resize", handleResize);
      };
    }, 50);

    return () => {
      clearTimeout(timer);
    };
  }, [showDetailsModal, selectedApp, appHistory]);

  return (
    <div className="p-6 space-y-6">
      <ToastComponent />

      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h2 className="text-2xl font-bold text-gray-800 dark:text-gray-100">流量监控</h2>
          <p className="text-gray-500 dark:text-gray-400">实时流量统计与应用排行</p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => exportData("csv")}
            className="flex items-center gap-1 px-3 py-1.5 text-sm border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
          >
            <FileText size={16} />
            CSV
          </button>
          <button
            onClick={() => exportData("json")}
            className="flex items-center gap-1 px-3 py-1.5 text-sm border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
          >
            <BarChart3 size={16} />
            JSON
          </button>
        </div>
      </div>

      {/* Real-time Stats */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <div className="flex items-center gap-2 mb-3">
          <Activity size={16} className="text-blue-500" />
          <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">实时流量</h3>
        </div>
        <div className="flex gap-8">
          <div className="flex items-center gap-3">
            <div className="w-3 h-3 bg-green-500 rounded-full animate-pulse"></div>
            <span className="text-gray-600 dark:text-gray-400">下载:</span>
            <span className="font-mono text-lg font-semibold text-gray-800 dark:text-gray-100">
              {stats ? formatSpeed(stats.download_bps) : "-"}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div className="w-3 h-3 bg-blue-500 rounded-full animate-pulse"></div>
            <span className="text-gray-600 dark:text-gray-400">上传:</span>
            <span className="font-mono text-lg font-semibold text-gray-800 dark:text-gray-100">
              {stats ? formatSpeed(stats.upload_bps) : "-"}
            </span>
          </div>
        </div>
      </div>

      {/* Cards Grid */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <CumulativeStatsCard
          data={cumulative}
          loading={loading.cumulative}
          period={period}
          onPeriodChange={setPeriod}
        />
        <TrafficAlertCard
          alerts={alerts}
          alertStatuses={alertStatuses}
          onManageAlerts={handleManageAlerts}
        />
      </div>

      {/* Charts Row */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <AppPieChart apps={apps} />
        <HistoryTrendChart hours={historyHours} onHoursChange={setHistoryHours} />
      </div>

      {/* App Ranking Table */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
        <div className="p-4 border-b border-gray-200 dark:border-gray-700">
          <div className="flex justify-between items-center">
            <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200">应用进程列表 ({filteredAndSortedApps.length})</h3>
            <div className="flex gap-2">
              <input
                type="text"
                placeholder="搜索应用..."
                value={searchTerm}
                onChange={(e) => setSearchTerm(e.target.value.slice(0, 100))}
                className="px-3 py-1.5 text-sm border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
              <button
                onClick={fetchApps}
                className="px-3 py-1.5 text-sm bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
              >
                刷新
              </button>
            </div>
          </div>
        </div>

        <AppTableHeader field={sortField} order={sortOrder} onSort={handleSort} />

        {/* Virtual Scrolled List (simplified) */}
        <div className="max-h-[500px] overflow-y-auto">
          {filteredAndSortedApps.length === 0 ? (
            <div className="text-center text-gray-500 dark:text-gray-400 py-10">未找到进程</div>
          ) : (
            <>
              {filteredAndSortedApps.map((app, index) => {
                const percentage = getAppPercentage(app);
                return (
                  <div key={`${app.pid}-${index}`} className="grid grid-cols-[2fr,100px,100px,100px,70px,80px] gap-2 px-4 py-3 border-b border-gray-100 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors items-center">
                    <div className="flex items-center gap-3 overflow-hidden">
                      <span className="text-gray-500 dark:text-gray-400 font-medium w-6 text-sm flex-shrink-0">{index + 1}</span>
                      <div className="truncate">
                        <div className="font-medium text-gray-800 dark:text-gray-100 text-sm truncate">{app.name}</div>
                        <div className="text-xs text-gray-400 dark:text-gray-500">PID: {app.pid}</div>
                      </div>
                    </div>
                    <div className="text-sm">
                      <div className="font-mono text-green-600">{formatSpeed(app.current_download_bps)}</div>
                    </div>
                    <div className="text-sm">
                      <div className="font-mono text-blue-600">{formatSpeed(app.current_upload_bps)}</div>
                    </div>
                    <div className="text-sm">
                      <div className="font-mono text-gray-700 dark:text-gray-300">{formatSpeed(app.current_download_bps + app.current_upload_bps)}</div>
                    </div>
                    <div className="text-sm">
                      <div className="font-mono text-gray-600 dark:text-gray-400">{percentage.toFixed(1)}%</div>
                      <div className="w-full h-1 bg-gray-200 dark:bg-gray-600 rounded-full mt-1 overflow-hidden">
                        <div
                          className="h-full bg-blue-500 rounded-full"
                          style={{ width: `${Math.min(percentage, 100)}%` }}
                        />
                      </div>
                    </div>
                    <div>
                      <button
                        onClick={() => handleShowDetails(app)}
                        className="px-2 py-1 text-xs border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded hover:bg-blue-50 dark:hover:bg-blue-900/30 hover:text-blue-600 hover:border-blue-300 transition-colors"
                      >
                        详情
                      </button>
                    </div>
                  </div>
                );
              })}

              {/* Summary Total Row */}
              <div className="grid grid-cols-[2fr,100px,100px,100px,70px,80px] gap-2 px-4 py-3 bg-gray-100 dark:bg-gray-700/50 border-t-2 border-gray-300 dark:border-gray-600 items-center font-medium">
                <div className="flex items-center gap-3">
                  <span className="text-gray-700 dark:text-gray-200 font-bold w-6 text-sm flex-shrink-0">Σ</span>
                  <div className="text-gray-800 dark:text-gray-100 text-sm">总计</div>
                </div>
                <div className="text-sm">
                  <div className="font-mono text-green-700">{formatSpeed(totals.download)}</div>
                </div>
                <div className="text-sm">
                  <div className="font-mono text-blue-700">{formatSpeed(totals.upload)}</div>
                </div>
                <div className="text-sm">
                  <div className="font-mono text-gray-900 dark:text-gray-100">{formatSpeed(totals.total)}</div>
                </div>
                <div className="text-sm">
                  <div className="font-mono text-gray-700 dark:text-gray-300">100%</div>
                  <div className="w-full h-1 bg-gray-300 dark:bg-gray-600 rounded-full mt-1 overflow-hidden">
                    <div className="h-full bg-gray-600 dark:bg-gray-400 rounded-full" style={{ width: "100%" }} />
                  </div>
                </div>
                <div></div>
              </div>
            </>
          )}
        </div>
      </div>

      {/* App Details Modal */}
      {showDetailsModal && selectedApp && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-3xl w-full mx-4 max-h-[90vh] overflow-y-auto">
            <div className="p-4 border-b border-gray-200 dark:border-gray-700 flex justify-between items-center sticky top-0 bg-white dark:bg-gray-800 z-10">
              <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100 flex items-center gap-2">
                <Activity size={20} className="text-blue-500" />
                应用流量详情
              </h3>
              <button
                onClick={() => setShowDetailsModal(false)}
                className="text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
              >
                <X size={24} />
              </button>
            </div>

            <div className="p-6 space-y-6">
              {/* App Info */}
              <div className="flex items-center gap-4 pb-4 border-b dark:border-gray-700">
                <div className="w-16 h-16 bg-gradient-to-br from-blue-500 to-purple-600 rounded-xl flex items-center justify-center text-white text-2xl font-bold">
                  {selectedApp.name.charAt(0).toUpperCase()}
                </div>
                <div className="flex-1">
                  <h4 className="text-xl font-bold text-gray-800 dark:text-gray-100">{selectedApp.name}</h4>
                  <p className="text-sm text-gray-500 dark:text-gray-400">PID: {selectedApp.pid}</p>
                </div>
                <div className="text-right">
                  <div className="text-sm text-gray-500 dark:text-gray-400">占比</div>
                  <div className="text-lg font-bold text-blue-600">{getAppPercentage(selectedApp).toFixed(2)}%</div>
                </div>
              </div>

              {/* Real-time Stats */}
              <div className="grid grid-cols-2 gap-4">
                <div className="bg-green-50 dark:bg-green-900/30 rounded-lg p-4">
                  <div className="flex items-center gap-2 mb-2">
                    <Download size={16} className="text-green-600" />
                    <span className="text-sm font-medium text-gray-700 dark:text-gray-200">实时下载</span>
                  </div>
                  <div className="text-2xl font-bold text-green-600 font-mono">
                    {formatSpeed(selectedApp.current_download_bps)}
                  </div>
                  <div className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    累计: {formatBytes(selectedApp.download_bytes)}
                  </div>
                </div>
                <div className="bg-blue-50 dark:bg-blue-900/30 rounded-lg p-4">
                  <div className="flex items-center gap-2 mb-2">
                    <Upload size={16} className="text-blue-600" />
                    <span className="text-sm font-medium text-gray-700 dark:text-gray-200">实时上传</span>
                  </div>
                  <div className="text-2xl font-bold text-blue-600 font-mono">
                    {formatSpeed(selectedApp.current_upload_bps)}
                  </div>
                  <div className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    累计: {formatBytes(selectedApp.upload_bytes)}
                  </div>
                </div>
              </div>

              {/* Total Stats */}
              <div className="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-4">
                <div className="flex justify-between items-center mb-2">
                  <span className="text-sm font-medium text-gray-700 dark:text-gray-200">总流量</span>
                  <span className="text-xl font-bold text-gray-800 dark:text-gray-100 font-mono">
                    {formatSpeed(selectedApp.current_download_bps + selectedApp.current_upload_bps)}
                  </span>
                </div>
                <div className="flex justify-between text-sm text-gray-600 dark:text-gray-300">
                  <span>累计下载: {formatBytes(selectedApp.download_bytes)}</span>
                  <span>累计上传: {formatBytes(selectedApp.upload_bytes)}</span>
                </div>
                <div className="flex justify-between text-sm text-gray-600 dark:text-gray-300 mt-1">
                  <span>累计总计: {formatBytes(selectedApp.download_bytes + selectedApp.upload_bytes)}</span>
                </div>
              </div>

              {/* Traffic Trend Chart — using proper useEffect lifecycle */}
              {appHistory.length > 0 && (
                <div>
                  <h5 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-3 flex items-center gap-2">
                    <TrendingUp size={16} />
                    流量趋势 (近60分钟)
                  </h5>
                  <div id="detail-chart" style={{ width: "100%", height: "200px" }} />
                </div>
              )}
            </div>

            <div className="p-4 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-700/50 sticky bottom-0">
              <button
                onClick={() => setShowDetailsModal(false)}
                className="w-full py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 transition-colors"
              >
                关闭
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Manage Alerts Modal */}
      {showManageAlerts && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[85vh] overflow-y-auto">
            <div className="p-4 border-b border-gray-200 dark:border-gray-700 flex justify-between items-center sticky top-0 bg-white dark:bg-gray-800 z-10">
              <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100 flex items-center gap-2">
                <AlertTriangle size={20} className="text-amber-500" />
                流量告警管理
              </h3>
              <button
                onClick={() => {
                  setShowManageAlerts(false);
                  setShowAddAlertForm(false);
                  setEditingAlert(null);
                }}
                className="text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
              >
                <X size={24} />
              </button>
            </div>

            {/* Add Alert Form */}
            {showAddAlertForm ? (
              <div className="p-4 border-b border-gray-200 bg-blue-50">
                <h4 className="text-sm font-medium text-gray-700 mb-3">添加新告警</h4>
                <div className="space-y-3">
                  <div>
                    <label className="block text-xs text-gray-600 mb-1">告警名称</label>
                    <input
                      type="text"
                      value={newAlert.name || ""}
                      onChange={(e) => setNewAlert({ ...newAlert, name: e.target.value })}
                      placeholder="例如: 周末流量告警"
                      className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                    />
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="block text-xs text-gray-600 mb-1">告警类型</label>
                      <select
                        value={newAlert.alert_type}
                        onChange={(e) => setNewAlert({ ...newAlert, alert_type: e.target.value })}
                        className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      >
                        <option value="download">下载流量</option>
                        <option value="upload">上传流量</option>
                        <option value="total">总流量</option>
                      </select>
                    </div>
                    <div>
                      <label className="block text-xs text-gray-600 mb-1">统计周期</label>
                      <select
                        value={newAlert.period}
                        onChange={(e) => setNewAlert({ ...newAlert, period: e.target.value })}
                        className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      >
                        <option value="day">每日</option>
                        <option value="week">每周</option>
                        <option value="month">每月</option>
                      </select>
                    </div>
                  </div>
                  <div>
                    <label className="block text-xs text-gray-600 mb-1">阈值 (GB)</label>
                    <input
                      type="number"
                      min="0.1"
                      step="0.1"
                      value={(newAlert.threshold_bytes || 0) / (1024 * 1024 * 1024)}
                      onChange={(e) => setNewAlert({ ...newAlert, threshold_bytes: parseThresholdInput(e.target.value, "GB") })}
                      className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                    />
                  </div>
                  <div className="flex gap-2">
                    <button
                      onClick={handleAddAlert}
                      className="flex-1 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm flex items-center justify-center gap-1"
                    >
                      <Plus size={16} />
                      添加告警
                    </button>
                    <button
                      onClick={() => setShowAddAlertForm(false)}
                      className="px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors text-sm"
                    >
                      取消
                    </button>
                  </div>
                </div>
              </div>
            ) : (
              <div className="p-4 border-b border-gray-200">
                <button
                  onClick={() => setShowAddAlertForm(true)}
                  className="w-full py-2 border-2 border-dashed border-gray-300 rounded-lg hover:border-blue-500 hover:bg-blue-50 transition-colors text-gray-600 hover:text-blue-600 flex items-center justify-center gap-2"
                >
                  <Plus size={18} />
                  添加自定义告警
                </button>
              </div>
            )}

            {/* Alerts List */}
            <div className="p-4 space-y-3">
              {alerts.length === 0 ? (
                <div className="text-center text-gray-500 py-8">
                  <AlertTriangle size={32} className="mx-auto mb-2 text-gray-300" />
                  <p>暂无告警规则</p>
                  <p className="text-sm">点击上方按钮添加自定义告警</p>
                </div>
              ) : (
                alerts.map(alert => {
                  const status = alertStatuses.find(s => s.alert_id === alert.id);
                  const percentage = status?.percentage || 0;
                  const isEditing = editingAlert?.id === alert.id;

                  return (
                    <div key={alert.id} className={`p-4 border rounded-lg ${isEditing ? "border-blue-500 bg-blue-50" : "border-gray-200"}`}>
                      {isEditing ? (
                        // Edit Mode
                        <div className="space-y-3">
                          <div>
                            <label className="block text-xs text-gray-600 mb-1">告警名称</label>
                            <input
                              type="text"
                              value={editingAlert.name}
                              onChange={(e) => setEditingAlert({ ...editingAlert, name: e.target.value })}
                              className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                          </div>
                          <div className="grid grid-cols-2 gap-3">
                            <div>
                              <label className="block text-xs text-gray-600 mb-1">告警类型</label>
                              <select
                                value={editingAlert.alert_type}
                                onChange={(e) => setEditingAlert({ ...editingAlert, alert_type: e.target.value })}
                                className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                              >
                                <option value="download">下载流量</option>
                                <option value="upload">上传流量</option>
                                <option value="total">总流量</option>
                              </select>
                            </div>
                            <div>
                              <label className="block text-xs text-gray-600 mb-1">统计周期</label>
                              <select
                                value={editingAlert.period}
                                onChange={(e) => setEditingAlert({ ...editingAlert, period: e.target.value })}
                                className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                              >
                                <option value="day">每日</option>
                                <option value="week">每周</option>
                                <option value="month">每月</option>
                              </select>
                            </div>
                          </div>
                          <div>
                            <label className="block text-xs text-gray-600 mb-1">阈值 (GB)</label>
                            <input
                              type="number"
                              min="0.1"
                              step="0.1"
                              value={editingAlert.threshold_bytes / (1024 * 1024 * 1024)}
                              onChange={(e) => setEditingAlert({ ...editingAlert, threshold_bytes: parseThresholdInput(e.target.value, "GB") })}
                              className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                          </div>
                          <div className="flex items-center gap-2">
                            <input
                              type="checkbox"
                              checked={editingAlert.enabled}
                              onChange={(e) => setEditingAlert({ ...editingAlert, enabled: e.target.checked })}
                              className="w-4 h-4 text-blue-600 rounded"
                            />
                            <span className="text-sm text-gray-600">启用此告警</span>
                          </div>
                          <div className="flex gap-2">
                            <button
                              onClick={handleSaveEdit}
                              className="flex-1 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm flex items-center justify-center gap-1"
                            >
                              <Save size={16} />
                              保存
                            </button>
                            <button
                              onClick={handleCancelEdit}
                              className="px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors text-sm"
                            >
                              取消
                            </button>
                          </div>
                        </div>
                      ) : (
                        // View Mode
                        <>
                          <div className="flex justify-between items-start mb-2">
                            <div className="flex-1">
                              <div className="flex items-center gap-2">
                                <span className="font-medium text-gray-800">{alert.name}</span>
                                {!alert.enabled && (
                                  <span className="px-2 py-0.5 text-xs bg-gray-200 text-gray-600 rounded">已禁用</span>
                                )}
                                {alert.id.startsWith("custom_") && (
                                  <span className="px-2 py-0.5 text-xs bg-blue-100 text-blue-600 rounded">自定义</span>
                                )}
                              </div>
                              <div className="text-xs text-gray-500 mt-1">
                                {alert.alert_type === "download" ? "下载" : alert.alert_type === "upload" ? "上传" : "总流量"} ·
                                {alert.period === "day" ? "每日" : alert.period === "week" ? "每周" : "每月"}
                              </div>
                            </div>
                            <div className="flex gap-1">
                              <button
                                onClick={() => handleEditAlert(alert)}
                                className="p-1.5 text-gray-500 hover:text-blue-600 hover:bg-blue-50 rounded transition-colors"
                                title="编辑"
                              >
                                <Edit size={16} />
                              </button>
                              {alert.id.startsWith("custom_") && (
                                <button
                                  onClick={() => handleDeleteAlert(alert.id)}
                                  className="p-1.5 text-gray-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors"
                                  title="删除"
                                >
                                  <Trash2 size={16} />
                                </button>
                              )}
                              <label className="flex items-center gap-1.5 cursor-pointer p-1.5 hover:bg-gray-100 rounded transition-colors">
                                <input
                                  type="checkbox"
                                  checked={alert.enabled}
                                  onChange={(e) => {
                                    const updated = { ...alert, enabled: e.target.checked };
                                    invoke("update_traffic_alert", { alert: updated })
                                      .then(() => fetchAlerts())
                                      .catch(console.error);
                                  }}
                                  className="w-4 h-4 text-blue-600 rounded"
                                />
                                <span className="text-xs text-gray-600">启用</span>
                              </label>
                            </div>
                          </div>

                          {/* Progress Bar */}
                          <div className="mb-2">
                            <div className="flex justify-between text-xs text-gray-500 mb-1">
                              <span>当前: {formatBytes(status?.current_value || 0)}</span>
                              <span>阈值: {formatBytes(alert.threshold_bytes)}</span>
                            </div>
                            <div className="flex items-center gap-2">
                              <div className="flex-1 h-2 bg-gray-200 rounded-full overflow-hidden">
                                <div
                                  className={`h-full rounded-full transition-all ${
                                    percentage >= 100 ? "bg-red-500" : percentage >= 80 ? "bg-amber-500" : "bg-green-500"
                                  }`}
                                  style={{ width: `${Math.min(percentage, 100)}%` }}
                                />
                              </div>
                              <span className={`text-sm font-medium min-w-[45px] text-right ${
                                percentage >= 100 ? "text-red-600" : percentage >= 80 ? "text-amber-600" : "text-green-600"
                              }`}>
                                {percentage.toFixed(0)}%
                              </span>
                            </div>
                          </div>

                          {/* Alert Status */}
                          {percentage >= 100 && alert.enabled && (
                            <div className="flex items-center gap-1 text-red-600 text-xs bg-red-50 px-2 py-1 rounded">
                              <AlertTriangle size={12} />
                              已触发告警
                            </div>
                          )}
                        </>
                      )}
                    </div>
                  );
                })
              )}
            </div>

            {/* Footer */}
            <div className="p-4 border-t border-gray-200 bg-gray-50 sticky bottom-0">
              <button
                onClick={() => {
                  setShowManageAlerts(false);
                  setShowAddAlertForm(false);
                  setEditingAlert(null);
                }}
                className="w-full py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 transition-colors"
              >
                完成
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

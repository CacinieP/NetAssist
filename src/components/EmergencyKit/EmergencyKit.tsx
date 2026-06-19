import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { Activity, AlertTriangle, CheckCircle, XCircle, X, Wrench, Clock, RefreshCw, Settings, Network, FileDown, ChevronDown } from "lucide-react";

type DiagnosticStatus = "pass" | "fail" | "warning";

interface DiagnosticItem {
  status: DiagnosticStatus;
  message: string;
  details: any;
  duration_ms: number;
}

interface RepairAction {
  action_type: string;
  name: string;
  description: string;
  priority: number;
  estimated_time_seconds: number;
}

interface DiagnosticResult {
  overall_status: DiagnosticStatus;
  network_connectivity: DiagnosticItem;
  ip_configuration: DiagnosticItem;
  dns_resolution: DiagnosticItem;
  network_quality: DiagnosticItem;
  recommendations: RepairAction[];
  timestamp: number;
}

interface FixHistory {
  timestamp: number;
  action_type: string;
  name: string;
  success: boolean;
  error?: string;
}

type FixType = "reset_network_stack" | "flush_dns_cache" | "release_renew_ip" | "switch_dns" | "toggle_ipv6" | "reset_adapter" | "restart_network_service";

const FIX_ICONS: Record<FixType, JSX.Element> = {
  reset_network_stack: <Settings size={20} />,
  flush_dns_cache: <RefreshCw size={20} />,
  release_renew_ip: <Network size={20} />,
  switch_dns: <Network size={20} />,
  toggle_ipv6: <Activity size={20} />,
  reset_adapter: <Wrench size={20} />,
  restart_network_service: <RefreshCw size={20} />,
};

const FIX_WARNINGS: Record<FixType, { title: string; message: string }> = {
  reset_network_stack: {
    title: "刷新 DNS 解析服务",
    message: "将重启 mDNSResponder 解析服务并清空 DNS 缓存，可能短暂影响域名解析。"
  },
  flush_dns_cache: {
    title: "清空 DNS 缓存",
    message: "此操作将清空本地 DNS 解析缓存。"
  },
  release_renew_ip: {
    title: "重新获取 IP 地址",
    message: "此操作将释放当前 IP 并向 DHCP 服务器重新请求，可能导致短暂的网络中断。"
  },
  switch_dns: {
    title: "切换 DNS 服务器",
    message: "此操作将把主网络服务切换到备用 DNS 服务器（8.8.8.8 / 1.1.1.1）。"
  },
  toggle_ipv6: {
    title: "切换 IPv6",
    message: "将在主网络服务上启用或关闭 IPv6（Automatic ↔ Off），可能短暂影响网络连接。"
  },
  reset_adapter: {
    title: "重置网络适配器",
    message: "将禁用并重新启用主网络适配器（需要管理员授权，会弹出系统密码框），期间网络会短暂中断。"
  },
  restart_network_service: {
    title: "刷新网络解析服务",
    message: "将重启 DNS 解析服务以重置网络通信，可能短暂影响域名解析。"
  }
};

export default function EmergencyKit() {
  const [result, setResult] = useState<DiagnosticResult | null>(null);
  const [isDiagnosing, setIsDiagnosing] = useState(false);
  const [diagnosingStep, setDiagnosingStep] = useState<string>("");
  const [fixing, setFixing] = useState<string | null>(null);
  const [showConfirm, setShowConfirm] = useState<{ type: FixType; name: string } | null>(null);
  const [fixError, setFixError] = useState<string | null>(null);
  const [fixHistory, setFixHistory] = useState<FixHistory[]>([]);
  const [autoFixing, setAutoFixing] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [exportToast, setExportToast] = useState<{ msg: string; ok: boolean } | null>(null);
  // Manual tools are shown in a modal (opened on demand) instead of always
  // laid out as cards on the page, to reduce visual clutter.
  const [showManualTools, setShowManualTools] = useState(false);
  // Collapsible diagnostic detail: which item keys are expanded. Empty by
  // default so details are hidden until the user clicks a row.
  const [expandedDetails, setExpandedDetails] = useState<Set<string>>(new Set());

  const showToast = (msg: string, ok: boolean) => {
    setExportToast({ msg, ok });
    setTimeout(() => setExportToast(null), 3000);
  };

  // Export the latest diagnostic report to a JSON file chosen by the user.
  const exportReport = async () => {
    if (!result) {
      showToast("请先运行诊断", false);
      return;
    }
    try {
      const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
      const fileName = `netassist_diagnostic_${timestamp}.json`;
      const content = JSON.stringify(result, null, 2);
      const filePath = await save({
        defaultPath: fileName,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (filePath) {
        const encoder = new TextEncoder();
        await writeFile(filePath, encoder.encode(content));
        showToast("诊断报告导出成功", true);
      }
    } catch (err) {
      console.error("Export report failed:", err);
      showToast(`导出失败: ${err}`, false);
    }
  };

  // Load fix history from localStorage on mount
  useEffect(() => {
    try {
      const saved = localStorage.getItem("netassist_fix_history");
      if (saved) {
        setFixHistory(JSON.parse(saved));
      }
    } catch (e) {
      console.error("Failed to load fix history:", e);
    }
  }, []);

  const saveToHistory = (actionType: string, name: string, success: boolean, error?: string) => {
    const entry: FixHistory = {
      timestamp: Date.now(),
      action_type: actionType,
      name,
      success,
      error,
    };
    const newHistory = [entry, ...fixHistory].slice(0, 10); // Keep last 10
    setFixHistory(newHistory);
    try {
      localStorage.setItem("netassist_fix_history", JSON.stringify(newHistory));
    } catch (e) {
      console.error("Failed to save fix history:", e);
    }
  };

  const startDiagnosis = async () => {
    try {
      setIsDiagnosing(true);
      setResult(null);
      setDiagnosingStep("正在检查网络连接...");

      const res = await invoke<DiagnosticResult>("run_diagnostics");
      setResult(res);
      setDiagnosingStep("");
    } catch (error) {
      console.error("Diagnostic failed:", error);
      setFixError("诊断失败: " + (error instanceof Error ? error.message : String(error)));
    } finally {
      setIsDiagnosing(false);
      setDiagnosingStep("");
    }
  };

  const applyFix = async (fixType: string) => {
    try {
      setFixing(fixType);
      setFixError(null);

      await invoke("apply_quick_fix", { fixType });

      saveToHistory(fixType, FIX_WARNINGS[fixType as FixType]?.title || fixType, true);

      // Re-run diagnostics after fix
      setDiagnosingStep("正在重新诊断...");
      await new Promise(resolve => setTimeout(resolve, 1000));
      const res = await invoke<DiagnosticResult>("run_diagnostics");
      setResult(res);
      setDiagnosingStep("");
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : "修复操作失败";
      console.error("Fix failed:", err);
      const fixName = FIX_WARNINGS[fixType as FixType]?.title || fixType;
      setFixError(`${fixName}失败: ${errorMsg}`);
      saveToHistory(fixType, fixName, false, errorMsg);
    } finally {
      setFixing(null);
      setDiagnosingStep("");
    }
  };

  const applyAutoFix = async () => {
    if (!result || result.recommendations.length === 0) return;

    setAutoFixing(true);
    const actions = [...result.recommendations];

    for (const action of actions) {
      setDiagnosingStep(`正在执行: ${action.name}...`);
      await applyFix(action.action_type);
      await new Promise(resolve => setTimeout(resolve, 2000)); // Wait between fixes
    }

    setAutoFixing(false);
  };

  const handleFixClick = (fixType: string, fixName: string) => {
    const typedFixType = fixType as FixType;
    if (FIX_WARNINGS[typedFixType]) {
      setShowConfirm({ type: typedFixType, name: fixName });
    } else {
      applyFix(fixType);
    }
  };

  const confirmFix = () => {
    if (showConfirm) {
      setShowConfirm(null);
      applyFix(showConfirm.type);
    }
  };

  const cancelFix = () => {
    setShowConfirm(null);
  };

  const renderStatusIcon = (status: DiagnosticStatus) => {
    switch (status) {
      case "pass": return <CheckCircle className="w-5 h-5 text-green-600" />;
      case "fail": return <XCircle className="w-5 h-5 text-red-600" />;
      case "warning": return <AlertTriangle className="w-5 h-5 text-yellow-600" />;
      default: return <Activity className="w-5 h-5 text-gray-400" />;
    }
  };

  // Toggle a diagnostic detail item's expanded state (click to expand/collapse).
  const toggleDetail = (key: string) => {
    setExpandedDetails(prev => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  const getStatusBadge = (status: DiagnosticStatus) => {
    const styles = {
      pass: "bg-green-100 text-green-800 border-green-200",
      fail: "bg-red-100 text-red-800 border-red-200",
      warning: "bg-yellow-100 text-yellow-800 border-yellow-200",
    };
    const labels = { pass: "正常", fail: "异常", warning: "警告" };
    return (
      <span className={`px-2 py-1 text-xs font-medium rounded-full border ${styles[status]}`}>
        {labels[status]}
      </span>
    );
  };

  const formatDuration = (ms: number) => {
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  };

  const getOverallStatusColor = (status: DiagnosticStatus) => {
    switch (status) {
      case "pass": return "text-green-600 bg-green-50 border-green-200";
      case "fail": return "text-red-600 bg-red-50 border-red-200";
      case "warning": return "text-yellow-600 bg-yellow-50 border-yellow-200";
    }
  };

  return (
    <div className="p-6 space-y-6">
      {/* Export Toast */}
      {exportToast && (
        <div className={`fixed top-4 right-4 z-[100] px-4 py-3 rounded-lg border shadow-lg text-sm flex items-center gap-2 ${
          exportToast.ok
            ? "bg-green-50 dark:bg-green-900/30 border-green-200 dark:border-green-800 text-green-700 dark:text-green-300"
            : "bg-red-50 dark:bg-red-900/30 border-red-200 dark:border-red-800 text-red-700 dark:text-red-300"
        }`}>
          <span>{exportToast.msg}</span>
          <button onClick={() => setExportToast(null)} className="ml-2 opacity-50 hover:opacity-100">&times;</button>
        </div>
      )}

      {/* Header */}
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800 dark:text-gray-100 flex items-center gap-2">
          <Activity className="w-7 h-7 text-blue-600" />
          断网急救中心
        </h2>
        <p className="text-gray-500 dark:text-gray-400 mt-1">智能网络故障诊断与快速修复工具</p>
      </div>

      {/* Error Message */}
      {fixError && (
        <div className="bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 text-red-600 dark:text-red-300 px-4 py-3 rounded-lg flex items-center justify-between">
          <span className="flex items-center gap-2">
            <XCircle className="w-5 h-5" />
            {fixError}
          </span>
          <button
            onClick={() => setFixError(null)}
            className="text-red-600 dark:text-red-400 hover:text-red-800 dark:hover:text-red-200 text-sm underline"
          >
            关闭
          </button>
        </div>
      )}

      {/* Current Status Card */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-6 shadow-sm">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className={`p-3 rounded-full ${result ? getOverallStatusColor(result.overall_status) : "bg-gray-100 dark:bg-gray-700"}`}>
              {result ? renderStatusIcon(result.overall_status) : <Activity className="w-6 h-6 text-gray-400 dark:text-gray-500 animate-pulse" />}
            </div>
            <div>
              <h3 className="text-lg font-medium text-gray-800 dark:text-gray-100">网络健康状态</h3>
              <div className="flex items-center gap-2 mt-1">
                {result ? (
                  <>
                    {getStatusBadge(result.overall_status)}
                    {result.overall_status === "pass" && (
                      <span className="text-sm text-gray-500 dark:text-gray-400">所有检查项正常</span>
                    )}
                    {result.overall_status === "fail" && (
                      <span className="text-sm text-gray-500 dark:text-gray-400">发现 {result.recommendations.length} 个问题</span>
                    )}
                    {result.overall_status === "warning" && (
                      <span className="text-sm text-gray-500 dark:text-gray-400">部分功能异常</span>
                    )}
                  </>
                ) : (
                  <span className="text-gray-500 dark:text-gray-400">点击下方按钮开始诊断</span>
                )}
              </div>
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={exportReport}
              disabled={!result}
              title={result ? "导出诊断报告" : "请先运行诊断"}
              className="px-4 py-2 border border-gray-300 dark:border-gray-600 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors text-gray-700 flex items-center gap-2 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <FileDown className="w-4 h-4" />
              导出报告
            </button>
            <button
              onClick={() => setShowHistory(!showHistory)}
              className="px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors text-gray-700 dark:text-gray-200 flex items-center gap-2"
            >
              <Clock className="w-4 h-4" />
              历史记录
            </button>
            <button
              onClick={startDiagnosis}
              disabled={isDiagnosing || autoFixing}
              className={`px-6 py-3 rounded-lg text-white font-medium transition-colors flex items-center gap-2 ${
                (isDiagnosing || autoFixing) ? "bg-gray-400 dark:bg-gray-600 cursor-not-allowed" : "bg-blue-600 hover:bg-blue-700"
              }`}
            >
              {isDiagnosing ? (
                <>
                  <RefreshCw className="w-5 h-5 animate-spin" />
                  诊断中...
                </>
              ) : (
                <>
                  <Activity className="w-5 h-5" />
                  开始诊断
                </>
              )}
            </button>
          </div>
        </div>

        {/* Diagnosis Progress */}
        {diagnosingStep && (
          <div className="mt-4 p-3 bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-800 rounded-lg">
            <div className="flex items-center gap-2 text-blue-700 dark:text-blue-300">
              <RefreshCw className="w-4 h-4 animate-spin" />
              <span className="text-sm">{diagnosingStep}</span>
            </div>
          </div>
        )}
      </div>

      {/* Fix History */}
      {showHistory && fixHistory.length > 0 && (
        <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 shadow-sm">
          <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-3 flex items-center gap-2">
            <Clock className="w-4 h-4" />
            修复历史记录
          </h3>
          <div className="space-y-2">
            {fixHistory.map((entry, idx) => (
              <div key={idx} className="flex items-center justify-between p-2 bg-gray-50 dark:bg-gray-700/50 rounded">
                <div className="flex items-center gap-2">
                  {entry.success ? (
                    <CheckCircle className="w-4 h-4 text-green-600" />
                  ) : (
                    <XCircle className="w-4 h-4 text-red-600" />
                  )}
                  <span className="text-sm text-gray-700 dark:text-gray-200">{entry.name}</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-xs text-gray-500 dark:text-gray-400">
                    {new Date(entry.timestamp).toLocaleTimeString()}
                  </span>
                  {entry.error && (
                    <span className="text-xs text-red-600 dark:text-red-400">{entry.error}</span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Diagnostic Results — collapsible rows */}
      {result && (
        <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 shadow-sm">
          <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-3">诊断详情</h3>

          <div className="space-y-2">
            {([
              { key: "connectivity", title: "网络连接", item: result.network_connectivity, extra: null as null | JSX.Element },
              { key: "ip", title: "IP 配置", item: result.ip_configuration, extra: result.ip_configuration.details.ipv4 ? (
                <div className="ml-7 mt-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded text-sm">
                  <span className="text-gray-500 dark:text-gray-400">IPv4: </span>
                  <span className="font-mono text-gray-800 dark:text-gray-200">{result.ip_configuration.details.ipv4}</span>
                </div>
              ) : null },
              { key: "dns", title: "DNS 解析", item: result.dns_resolution, extra: null },
              { key: "quality", title: "网络质量", item: result.network_quality, extra: null },
            ]).map(({ key, title, item, extra }) => {
              const open = expandedDetails.has(key);
              return (
                <div key={key} className="border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden">
                  <button
                    onClick={() => toggleDetail(key)}
                    className="w-full flex items-center justify-between p-3 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
                  >
                    <div className="flex items-center gap-2">
                      {renderStatusIcon(item.status)}
                      <span className="font-medium text-gray-800 dark:text-gray-100">{title}</span>
                    </div>
                    <div className="flex items-center gap-2">
                      {getStatusBadge(item.status)}
                      <span className="text-xs text-gray-500 dark:text-gray-400">{formatDuration(item.duration_ms)}</span>
                      <ChevronDown className={`w-4 h-4 text-gray-400 transition-transform ${open ? "rotate-180" : ""}`} />
                    </div>
                  </button>
                  {open && (
                    <div className="px-3 pb-3">
                      <p className="text-sm text-gray-600 dark:text-gray-300 ml-7">{item.message}</p>
                      {extra}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Quick Fixes - Auto Fix */}
      {result && result.recommendations.length > 0 && (
        <div className="bg-gradient-to-r from-orange-50 to-red-50 dark:from-orange-900/30 dark:to-red-900/30 rounded-lg border border-orange-200 dark:border-orange-800 p-4">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-gray-800 dark:text-gray-100 flex items-center gap-2">
              <AlertTriangle className="w-4 h-4 text-orange-600" />
              检测到 {result.recommendations.length} 个问题
            </h3>
            {!autoFixing && (
              <button
                onClick={applyAutoFix}
                disabled={isDiagnosing || fixing !== null}
                className="px-3 py-1.5 text-xs bg-orange-600 text-white rounded-lg hover:bg-orange-700 transition-colors disabled:opacity-50 flex items-center gap-1"
              >
                <Wrench className="w-3 h-3" />
                一键修复全部
              </button>
            )}
          </div>

          {autoFixing && (
            <div className="mb-3 p-2 bg-white dark:bg-gray-800 rounded border border-orange-200 dark:border-orange-800">
              <div className="flex items-center gap-2 text-orange-700 dark:text-orange-300 text-sm">
                <RefreshCw className="w-4 h-4 animate-spin" />
                {diagnosingStep}
              </div>
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
            {result.recommendations.map((action, idx) => (
              <button
                key={idx}
                onClick={() => handleFixClick(action.action_type, action.name)}
                disabled={fixing !== null || autoFixing}
                className="p-3 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg hover:border-orange-300 dark:hover:border-orange-700 hover:bg-orange-50 dark:hover:bg-gray-700 transition-all text-left disabled:opacity-50"
              >
                <div className="flex items-center gap-2">
                  {FIX_ICONS[action.action_type as FixType] || <Wrench className="w-4 h-4" />}
                  <span className="font-medium text-gray-800 dark:text-gray-100">{action.name}</span>
                </div>
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Manual Tools — trigger button (tools shown in a modal on click) */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 shadow-sm flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 flex items-center gap-2">
            <Wrench className="w-4 h-4" />
            手动修复工具
          </h3>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">DNS 缓存、IP 续租、IPv6、适配器重置等</p>
        </div>
        <button
          onClick={() => setShowManualTools(true)}
          disabled={fixing !== null || autoFixing}
          className="px-4 py-2 border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors disabled:opacity-50 flex items-center gap-2"
        >
          <Wrench className="w-4 h-4" />
          打开工具
        </button>
      </div>

      {/* Manual Tools Modal */}
      {showManualTools && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[85vh] overflow-y-auto">
            <div className="p-4 border-b border-gray-200 dark:border-gray-700 flex justify-between items-center sticky top-0 bg-white dark:bg-gray-800 z-10">
              <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100 flex items-center gap-2">
                <Wrench size={20} className="text-blue-500" />
                手动修复工具
              </h3>
              <button
                onClick={() => setShowManualTools(false)}
                className="text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
              >
                <X size={24} />
              </button>
            </div>
            <div className="p-4 grid grid-cols-1 md:grid-cols-2 gap-3">
              {Object.entries(FIX_WARNINGS).map(([type, info]) => (
                <button
                  key={type}
                  onClick={() => {
                    setShowManualTools(false);
                    handleFixClick(type as FixType, info.title);
                  }}
                  disabled={fixing !== null || autoFixing}
                  className="p-3 border border-gray-200 dark:border-gray-700 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 hover:border-gray-300 dark:hover:border-gray-600 transition-all text-left disabled:opacity-50"
                >
                  <div className="flex items-center gap-2 mb-1">
                    {FIX_ICONS[type as FixType]}
                    <span className="font-medium text-gray-800 dark:text-gray-100">{info.title}</span>
                  </div>
                  <p className="text-sm text-gray-600 dark:text-gray-300 line-clamp-2">{info.message}</p>
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Confirmation Modal */}
      {showConfirm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-md w-full mx-4 p-6">
            <div className="flex items-center gap-3 mb-4">
              <AlertTriangle className="w-6 h-6 text-amber-500" />
              <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100">
                {FIX_WARNINGS[showConfirm.type].title}
              </h3>
            </div>
            <p className="text-gray-600 dark:text-gray-300 mb-6">
              {FIX_WARNINGS[showConfirm.type].message}
            </p>
            <div className="flex gap-3 justify-end">
              <button
                onClick={cancelFix}
                disabled={fixing !== null}
                className="px-4 py-2 border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors disabled:opacity-50"
              >
                取消
              </button>
              <button
                onClick={confirmFix}
                disabled={fixing !== null}
                className="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors disabled:opacity-50 flex items-center gap-2"
              >
                {fixing === showConfirm.type && <RefreshCw className="w-4 h-4 animate-spin" />}
                {fixing === showConfirm.type ? "执行中..." : "确认执行"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

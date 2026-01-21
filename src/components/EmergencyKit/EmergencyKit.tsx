import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Activity, AlertTriangle, CheckCircle, XCircle, Wrench, Clock, RefreshCw, Settings, Network } from "lucide-react";

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
    title: "重置网络协议栈",
    message: "此操作将重置 TCP/IP 协议栈到默认状态，可能导致短暂的网络中断。是否继续？"
  },
  flush_dns_cache: {
    title: "清空 DNS 缓存",
    message: "此操作将清空本地 DNS 解析缓存。是否继续？"
  },
  release_renew_ip: {
    title: "重新获取 IP 地址",
    message: "此操作将释放当前 IP 并向 DHCP 服务器重新请求，可能导致短暂的网络中断。是否继续？"
  },
  switch_dns: {
    title: "切换 DNS 服务器",
    message: "此操作将切换到备用 DNS 服务器（8.8.8.8 / 1.1.1.1）。是否继续？"
  },
  toggle_ipv6: {
    title: "切换 IPv6",
    message: "此操作将重新初始化网络配置以切换 IPv6 状态。是否继续？"
  },
  reset_adapter: {
    title: "重置网络适配器",
    message: "此操作将重置网络适配器，可能导致短暂的网络中断。是否继续？"
  },
  restart_network_service: {
    title: "重启网络服务",
    message: "此操作将重启系统网络服务，将导致网络中断。是否继续？"
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
      {/* Header */}
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800 flex items-center gap-2">
          <Activity className="w-7 h-7 text-blue-600" />
          断网急救中心
        </h2>
        <p className="text-gray-500 mt-1">智能网络故障诊断与快速修复工具</p>
      </div>

      {/* Error Message */}
      {fixError && (
        <div className="bg-red-50 border border-red-200 text-red-600 px-4 py-3 rounded-lg flex items-center justify-between">
          <span className="flex items-center gap-2">
            <XCircle className="w-5 h-5" />
            {fixError}
          </span>
          <button
            onClick={() => setFixError(null)}
            className="text-red-600 hover:text-red-800 text-sm underline"
          >
            关闭
          </button>
        </div>
      )}

      {/* Current Status Card */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className={`p-3 rounded-full ${result ? getOverallStatusColor(result.overall_status) : "bg-gray-100"}`}>
              {result ? renderStatusIcon(result.overall_status) : <Activity className="w-6 h-6 text-gray-400 animate-pulse" />}
            </div>
            <div>
              <h3 className="text-lg font-medium text-gray-800">网络健康状态</h3>
              <div className="flex items-center gap-2 mt-1">
                {result ? (
                  <>
                    {getStatusBadge(result.overall_status)}
                    {result.overall_status === "pass" && (
                      <span className="text-sm text-gray-500">所有检查项正常</span>
                    )}
                    {result.overall_status === "fail" && (
                      <span className="text-sm text-gray-500">发现 {result.recommendations.length} 个问题</span>
                    )}
                    {result.overall_status === "warning" && (
                      <span className="text-sm text-gray-500">部分功能异常</span>
                    )}
                  </>
                ) : (
                  <span className="text-gray-500">点击下方按钮开始诊断</span>
                )}
              </div>
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={() => setShowHistory(!showHistory)}
              className="px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors text-gray-700 flex items-center gap-2"
            >
              <Clock className="w-4 h-4" />
              历史记录
            </button>
            <button
              onClick={startDiagnosis}
              disabled={isDiagnosing || autoFixing}
              className={`px-6 py-3 rounded-lg text-white font-medium transition-colors flex items-center gap-2 ${
                (isDiagnosing || autoFixing) ? "bg-gray-400 cursor-not-allowed" : "bg-blue-600 hover:bg-blue-700"
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
          <div className="mt-4 p-3 bg-blue-50 border border-blue-200 rounded-lg">
            <div className="flex items-center gap-2 text-blue-700">
              <RefreshCw className="w-4 h-4 animate-spin" />
              <span className="text-sm">{diagnosingStep}</span>
            </div>
          </div>
        )}
      </div>

      {/* Fix History */}
      {showHistory && fixHistory.length > 0 && (
        <div className="bg-white rounded-lg border border-gray-200 p-4 shadow-sm">
          <h3 className="text-sm font-medium text-gray-700 mb-3 flex items-center gap-2">
            <Clock className="w-4 h-4" />
            修复历史记录
          </h3>
          <div className="space-y-2">
            {fixHistory.map((entry, idx) => (
              <div key={idx} className="flex items-center justify-between p-2 bg-gray-50 rounded">
                <div className="flex items-center gap-2">
                  {entry.success ? (
                    <CheckCircle className="w-4 h-4 text-green-600" />
                  ) : (
                    <XCircle className="w-4 h-4 text-red-600" />
                  )}
                  <span className="text-sm text-gray-700">{entry.name}</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-xs text-gray-500">
                    {new Date(entry.timestamp).toLocaleTimeString()}
                  </span>
                  {entry.error && (
                    <span className="text-xs text-red-600">{entry.error}</span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Diagnostic Results */}
      {result && (
        <div className="bg-white rounded-lg border border-gray-200 p-4 shadow-sm">
          <h3 className="text-sm font-medium text-gray-700 mb-3">诊断详情</h3>

          <div className="space-y-3">
            {/* Connectivity */}
            <div className="p-3 border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {renderStatusIcon(result.network_connectivity.status)}
                  <span className="font-medium text-gray-800">网络连接</span>
                </div>
                <div className="flex items-center gap-2">
                  {getStatusBadge(result.network_connectivity.status)}
                  <span className="text-xs text-gray-500">{formatDuration(result.network_connectivity.duration_ms)}</span>
                </div>
              </div>
              <p className="text-sm text-gray-600 mt-1 ml-7">{result.network_connectivity.message}</p>
            </div>

            {/* IP Configuration */}
            <div className="p-3 border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {renderStatusIcon(result.ip_configuration.status)}
                  <span className="font-medium text-gray-800">IP 配置</span>
                </div>
                <div className="flex items-center gap-2">
                  {getStatusBadge(result.ip_configuration.status)}
                  <span className="text-xs text-gray-500">{formatDuration(result.ip_configuration.duration_ms)}</span>
                </div>
              </div>
              <p className="text-sm text-gray-600 mt-1 ml-7">{result.ip_configuration.message}</p>
              {result.ip_configuration.details.ipv4 && (
                <div className="ml-7 mt-2 p-2 bg-gray-50 rounded text-sm">
                  <span className="text-gray-500">IPv4: </span>
                  <span className="font-mono text-gray-800">{result.ip_configuration.details.ipv4}</span>
                </div>
              )}
            </div>

            {/* DNS Resolution */}
            <div className="p-3 border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {renderStatusIcon(result.dns_resolution.status)}
                  <span className="font-medium text-gray-800">DNS 解析</span>
                </div>
                <div className="flex items-center gap-2">
                  {getStatusBadge(result.dns_resolution.status)}
                  <span className="text-xs text-gray-500">{formatDuration(result.dns_resolution.duration_ms)}</span>
                </div>
              </div>
              <p className="text-sm text-gray-600 mt-1 ml-7">{result.dns_resolution.message}</p>
            </div>

            {/* Network Quality */}
            <div className="p-3 border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {renderStatusIcon(result.network_quality.status)}
                  <span className="font-medium text-gray-800">网络质量</span>
                </div>
                <div className="flex items-center gap-2">
                  {getStatusBadge(result.network_quality.status)}
                  <span className="text-xs text-gray-500">{formatDuration(result.network_quality.duration_ms)}</span>
                </div>
              </div>
              <p className="text-sm text-gray-600 mt-1 ml-7">{result.network_quality.message}</p>
            </div>
          </div>
        </div>
      )}

      {/* Quick Fixes - Auto Fix */}
      {result && result.recommendations.length > 0 && (
        <div className="bg-gradient-to-r from-orange-50 to-red-50 rounded-lg border border-orange-200 p-4">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-gray-800 flex items-center gap-2">
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
            <div className="mb-3 p-2 bg-white rounded border border-orange-200">
              <div className="flex items-center gap-2 text-orange-700 text-sm">
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
                className="p-3 bg-white border border-gray-200 rounded-lg hover:border-orange-300 hover:bg-orange-50 transition-all text-left disabled:opacity-50"
              >
                <div className="flex items-center gap-2 mb-1">
                  {FIX_ICONS[action.action_type as FixType] || <Wrench className="w-4 h-4" />}
                  <span className="font-medium text-gray-800">{action.name}</span>
                </div>
                <p className="text-sm text-gray-600">{action.description}</p>
                <div className="mt-2 text-xs text-gray-500 flex items-center gap-1">
                  <Clock className="w-3 h-3" />
                  预计 {action.estimated_time_seconds} 秒
                </div>
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Manual Tools */}
      <div className="bg-white rounded-lg border border-gray-200 p-4 shadow-sm">
        <h3 className="text-sm font-medium text-gray-700 mb-3">手动修复工具</h3>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
          {Object.entries(FIX_WARNINGS).map(([type, info]) => (
            <button
              key={type}
              onClick={() => handleFixClick(type as FixType, info.title)}
              disabled={fixing !== null || autoFixing}
              className="p-3 border border-gray-200 rounded-lg hover:bg-gray-50 hover:border-gray-300 transition-all text-left disabled:opacity-50"
            >
              <div className="flex items-center gap-2 mb-1">
                {FIX_ICONS[type as FixType]}
                <span className="font-medium text-gray-800">{info.title}</span>
              </div>
              <p className="text-sm text-gray-600 line-clamp-2">{info.message}</p>
            </button>
          ))}
        </div>
      </div>

      {/* Confirmation Modal */}
      {showConfirm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg shadow-xl max-w-md w-full mx-4 p-6">
            <div className="flex items-center gap-3 mb-4">
              <AlertTriangle className="w-6 h-6 text-amber-500" />
              <h3 className="text-lg font-semibold text-gray-800">
                {FIX_WARNINGS[showConfirm.type].title}
              </h3>
            </div>
            <p className="text-gray-600 mb-6">
              {FIX_WARNINGS[showConfirm.type].message}
            </p>
            <div className="flex gap-3 justify-end">
              <button
                onClick={cancelFix}
                disabled={fixing !== null}
                className="px-4 py-2 border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors disabled:opacity-50"
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

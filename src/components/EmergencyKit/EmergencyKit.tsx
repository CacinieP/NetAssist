import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { documentDir, join } from "@tauri-apps/api/path";
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { Activity, AlertTriangle, CheckCircle, XCircle, X, Wrench, Clock, RefreshCw, Settings, Network, FileDown, ChevronDown, Square, Undo2, Info } from "lucide-react";
import { invokeErrorMessage } from "../../utils/errors";
import {
  CHECK_KEYS,
  MANUAL_FIX_TYPES,
  historyFixType,
  isStale,
  problemCount,
  type CheckKey,
  type DiagnosticStatus,
  type RepairAction,
  type RepairCapability,
  type RepairRisk,
} from "../../utils/emergency";
import { useEmergencyStore } from "../../store/emergencyStore";
import { useSettingsStore } from "../../store/settingsStore";

interface FixHistory {
  timestamp: number;
  action_type: string;
  name: string;
  success: boolean;
  error?: string;
}

const HISTORY_KEY = "netassist_fix_history";

const FIX_ICONS: Record<string, JSX.Element> = {
  reset_network_stack: <Settings size={20} />,
  flush_dns_cache: <RefreshCw size={20} />,
  release_renew_ip: <Network size={20} />,
  switch_dns: <Network size={20} />,
  restore_auto_dns: <Undo2 size={20} />,
  toggle_ipv6: <Activity size={20} />,
  reset_adapter: <Wrench size={20} />,
};

const CHECK_LABEL_KEYS: Record<CheckKey, string> = {
  network_connectivity: "emergency.connectivity",
  ip_configuration: "emergency.ip_config",
  dns_resolution: "emergency.dns_resolution",
  network_quality: "emergency.network_quality",
};

const RISK_STYLES: Record<RepairRisk, string> = {
  low: "bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300",
  medium: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300",
  high: "bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300",
};

type Toast = { msg: string; kind: "ok" | "error" | "info" };

export default function EmergencyKit() {
  const { t, i18n } = useTranslation();
  const result = useEmergencyStore(s => s.result);
  const isDiagnosing = useEmergencyStore(s => s.isDiagnosing);
  const progress = useEmergencyStore(s => s.progress);
  const capabilities = useEmergencyStore(s => s.capabilities);
  const runDiagnosis = useEmergencyStore(s => s.runDiagnosis);
  const cancelDiagnosis = useEmergencyStore(s => s.cancelDiagnosis);
  const loadCapabilities = useEmergencyStore(s => s.loadCapabilities);
  const { primary_dns, secondary_dns } = useSettingsStore(s => s.settings);

  const [diagnosingStep, setDiagnosingStep] = useState<string>("");
  const [fixing, setFixing] = useState<string | null>(null);
  const [showConfirm, setShowConfirm] = useState<string | null>(null);
  const [fixError, setFixError] = useState<string | null>(null);
  const [fixHistory, setFixHistory] = useState<FixHistory[]>([]);
  const [autoFixing, setAutoFixing] = useState(false);
  const [autoFixStep, setAutoFixStep] = useState("");
  // Batch fix plan awaiting confirmation; each recommendation can be unticked.
  const [autoFixPlan, setAutoFixPlan] = useState<{ action: RepairAction; selected: boolean }[] | null>(null);
  // Checked between batch steps; an in-flight repair cannot be aborted.
  const autoFixCancelled = useRef(false);
  const [showHistory, setShowHistory] = useState(false);
  const [toast, setToast] = useState<Toast | null>(null);
  // Manual tools are shown in a modal (opened on demand) instead of always
  // laid out as cards on the page, to reduce visual clutter.
  const [showManualTools, setShowManualTools] = useState(false);
  // Collapsible diagnostic detail: which item keys are expanded. Empty by
  // default so details are hidden until the user clicks a row.
  const [expandedDetails, setExpandedDetails] = useState<Set<string>>(new Set());
  // Re-render periodically so the "may be outdated" note appears on its own.
  const [now, setNow] = useState(() => Date.now());

  const fixTitle = (fixType: string) => t(`emergency.fix.${historyFixType(fixType)}.title`, { defaultValue: fixType });
  const fixMessage = (fixType: string) => t(`emergency.fix.${historyFixType(fixType)}.msg`, { defaultValue: "" });

  const showToast = (msg: string, kind: Toast["kind"]) => {
    setToast({ msg, kind });
    setTimeout(() => setToast(null), 4000);
  };

  useEffect(() => {
    void loadCapabilities();
  }, [loadCapabilities]);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);

  // Export the latest diagnostic report to a JSON file chosen by the user.
  const exportReport = async () => {
    if (!result) {
      showToast(t("emergency.run_diagnosis_first"), "error");
      return;
    }
    try {
      const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
      const fileName = `netassist_diagnostic_${timestamp}.json`;
      let defaultPath = fileName;
      try {
        defaultPath = await join(await documentDir(), fileName);
      } catch {
        // Fall back to the dialog's own default folder.
      }
      const filePath = await save({
        defaultPath,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath) {
        showToast(t("emergency.export_cancelled"), "info");
        return;
      }
      await writeFile(filePath, new TextEncoder().encode(JSON.stringify(result, null, 2)));
      showToast(t("emergency.export_success", { path: filePath }), "ok");
    } catch (err) {
      console.error("Export report failed:", err);
      const error = invokeErrorMessage(err, t("emergency.unknown_error"));
      showToast(`${t("emergency.export_failed", { error })} ${t("emergency.export_scope_hint")}`, "error");
    }
  };

  // Load fix history from localStorage on mount
  useEffect(() => {
    try {
      const saved = localStorage.getItem(HISTORY_KEY);
      if (saved) {
        setFixHistory(JSON.parse(saved));
      }
    } catch (e) {
      console.error("Failed to load fix history:", e);
    }
  }, []);

  // Leaving the page unmounts this component; stop any remaining batch steps.
  useEffect(() => () => { autoFixCancelled.current = true; }, []);

  const saveToHistory = (actionType: string, success: boolean, error?: string) => {
    const entry: FixHistory = {
      timestamp: Date.now(),
      action_type: actionType,
      name: fixTitle(actionType),
      success,
      error,
    };
    // Functional update so repeated saves inside one loop (auto-fix) each
    // build on the LATEST state instead of a stale render-frame snapshot.
    setFixHistory(prev => {
      const newHistory = [entry, ...prev].slice(0, 10); // Keep last 10
      try {
        localStorage.setItem(HISTORY_KEY, JSON.stringify(newHistory));
      } catch (e) {
        console.error("Failed to save fix history:", e);
      }
      return newHistory;
    });
  };

  const clearHistory = () => {
    setFixHistory([]);
    try {
      localStorage.removeItem(HISTORY_KEY);
    } catch (e) {
      console.error("Failed to clear fix history:", e);
    }
  };

  const startDiagnosis = async () => {
    setFixError(null);
    try {
      await runDiagnosis();
    } catch (error) {
      console.error("Diagnostic failed:", error);
      setFixError(t("emergency.diagnosis_failed", { error: invokeErrorMessage(error, t("emergency.unknown_error")) }));
    }
  };

  // Re-check after repairs. A failure here says nothing about the repair
  // itself, so it is reported separately instead of as a failed fix.
  const rediagnose = async (fixType: string) => {
    setDiagnosingStep(t("emergency.rediagnosing"));
    try {
      await new Promise(resolve => setTimeout(resolve, 1000));
      await runDiagnosis();
    } catch (err) {
      console.error("Re-diagnosis failed:", err);
      showToast(
        t("emergency.fix_done_recheck_failed", {
          name: fixTitle(fixType),
          error: invokeErrorMessage(err, t("emergency.unknown_error")),
        }),
        "error",
      );
    } finally {
      setDiagnosingStep("");
    }
  };

  // Returns whether the fix succeeded. Batch runs pass rediagnose=false and
  // re-diagnose once at the end instead of after every step.
  const applyFix = async (fixType: string, { rediagnose: rerun = true } = {}): Promise<boolean> => {
    setFixing(fixType);
    setFixError(null);
    try {
      await invoke("apply_quick_fix", { fixType });
      saveToHistory(fixType, true);
    } catch (err) {
      const errorMsg = invokeErrorMessage(err, t("emergency.fix_error_fallback"));
      console.error("Fix failed:", err);
      setFixError(t("emergency.fix_failed", { name: fixTitle(fixType), error: errorMsg }));
      saveToHistory(fixType, false, errorMsg);
      return false;
    } finally {
      setFixing(null);
    }
    if (rerun) await rediagnose(fixType);
    return true;
  };

  const openAutoFixPlan = () => {
    if (!result || result.recommendations.length === 0) return;
    setAutoFixPlan(result.recommendations.map(action => ({ action, selected: true })));
  };

  const toggleAutoFixItem = (idx: number) => {
    setAutoFixPlan(plan => plan && plan.map((item, i) => (i === idx ? { ...item, selected: !item.selected } : item)));
  };

  const runAutoFix = async () => {
    const actions = (autoFixPlan ?? []).filter(item => item.selected).map(item => item.action);
    setAutoFixPlan(null);
    if (actions.length === 0) return;

    autoFixCancelled.current = false;
    setAutoFixing(true);
    try {
      for (const [idx, action] of actions.entries()) {
        if (autoFixCancelled.current) return;
        setAutoFixStep(t("emergency.auto_fix_step", { current: idx + 1, total: actions.length, name: fixTitle(action.action_type) }));
        const ok = await applyFix(action.action_type, { rediagnose: false });
        const remaining = actions.length - idx - 1;
        if (!ok) {
          if (remaining > 0) {
            setFixError(prev => `${prev ?? t("emergency.fix_error_fallback")}${t("emergency.auto_fix_remaining", { count: remaining })}`);
          }
          return;
        }
        if (autoFixCancelled.current) return;
        if (remaining > 0) await new Promise(resolve => setTimeout(resolve, 2000));
      }
      setAutoFixStep("");
      await rediagnose(actions[actions.length - 1].action_type);
    } finally {
      setAutoFixing(false);
      setAutoFixStep("");
    }
  };

  const stopAutoFix = () => {
    autoFixCancelled.current = true;
    setAutoFixStep(t("emergency.stopping"));
  };

  const capabilityOf = (fixType: string): RepairCapability | undefined =>
    capabilities?.find(c => c.fix_type === fixType);

  const manualFixTypes: string[] = capabilities ? capabilities.map(c => c.fix_type) : [...MANUAL_FIX_TYPES];

  const confirmFix = () => {
    if (showConfirm) {
      const fixType = showConfirm;
      setShowConfirm(null);
      applyFix(fixType);
    }
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
    return (
      <span className={`px-2 py-1 text-xs font-medium rounded-full border ${styles[status]}`}>
        {t(`emergency.status_${status}`)}
      </span>
    );
  };

  const getRiskBadge = (risk: RepairRisk | undefined) => risk && (
    <span className={`px-1.5 py-0.5 text-[11px] font-normal rounded ${RISK_STYLES[risk]}`}>
      {t(`emergency.risk_${risk}`)}
    </span>
  );

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

  const doneChecks = CHECK_KEYS.filter(key => progress[key]).length;
  const progressText = isDiagnosing
    ? t("emergency.diagnosis_progress", { done: doneChecks, total: CHECK_KEYS.length })
    : diagnosingStep;
  const busy = isDiagnosing || autoFixing || fixing !== null;
  const dnsTargets = [primary_dns, secondary_dns].filter(Boolean).join(" / ");
  const hints = result?.hints ?? [];

  return (
    <div className="p-6 space-y-6">
      {/* Toast */}
      {toast && (
        <div className={`fixed top-4 right-4 z-[100] max-w-md px-4 py-3 rounded-lg border shadow-lg text-sm flex items-center gap-2 ${
          toast.kind === "ok"
            ? "bg-green-50 dark:bg-green-900/30 border-green-200 dark:border-green-800 text-green-700 dark:text-green-300"
            : toast.kind === "error"
              ? "bg-red-50 dark:bg-red-900/30 border-red-200 dark:border-red-800 text-red-700 dark:text-red-300"
              : "bg-gray-50 dark:bg-gray-800 border-gray-200 dark:border-gray-700 text-gray-700 dark:text-gray-200"
        }`}>
          <span className="break-all">{toast.msg}</span>
          <button onClick={() => setToast(null)} className="ml-2 opacity-50 hover:opacity-100">&times;</button>
        </div>
      )}

      {/* Header */}
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800 dark:text-gray-100 flex items-center gap-2">
          <Activity className="w-7 h-7 text-blue-600" />
          {t("emergency.title")}
        </h2>
        <p className="text-gray-500 dark:text-gray-400 mt-1">{t("emergency.subtitle")}</p>
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
            {t("emergency.close_btn")}
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
              <h3 className="text-lg font-medium text-gray-800 dark:text-gray-100">{t("emergency.network_health")}</h3>
              <div className="flex items-center gap-2 mt-1">
                {result ? (
                  <>
                    {getStatusBadge(result.overall_status)}
                    <span className="text-sm text-gray-500 dark:text-gray-400">
                      {result.overall_status === "pass" && t("emergency.all_normal")}
                      {result.overall_status === "fail" && t("emergency.issues_found", { count: problemCount(result) })}
                      {result.overall_status === "warning" && t("emergency.warnings_found", { count: problemCount(result) })}
                    </span>
                  </>
                ) : (
                  <span className="text-gray-500 dark:text-gray-400">{t("emergency.click_to_start")}</span>
                )}
              </div>
              {result && (
                <div className="text-xs text-gray-400 dark:text-gray-500 mt-1">
                  {t("emergency.diagnosed_at", { time: new Date(result.timestamp).toLocaleString(i18n.language) })}
                  {isStale(result.timestamp, now) && (
                    <span className="ml-2 text-amber-600 dark:text-amber-400">{t("emergency.stale")}</span>
                  )}
                </div>
              )}
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={exportReport}
              disabled={!result}
              title={result ? t("emergency.export_report") : t("emergency.run_diagnosis_first")}
              className="px-4 py-2 border border-gray-300 dark:border-gray-600 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors text-gray-700 flex items-center gap-2 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <FileDown className="w-4 h-4" />
              {t("emergency.export_report")}
            </button>
            <button
              onClick={() => setShowHistory(!showHistory)}
              className="px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors text-gray-700 dark:text-gray-200 flex items-center gap-2"
            >
              <Clock className="w-4 h-4" />
              {t("emergency.history")}
            </button>
            {isDiagnosing ? (
              <button
                onClick={cancelDiagnosis}
                className="px-6 py-3 rounded-lg border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-gray-700 font-medium transition-colors flex items-center gap-2"
              >
                <Square className="w-5 h-5" />
                {t("emergency.cancel_diagnosis")}
              </button>
            ) : (
              <button
                onClick={startDiagnosis}
                disabled={autoFixing || fixing !== null}
                className={`px-6 py-3 rounded-lg text-white font-medium transition-colors flex items-center gap-2 ${
                  (autoFixing || fixing !== null) ? "bg-gray-400 dark:bg-gray-600 cursor-not-allowed" : "bg-blue-600 hover:bg-blue-700"
                }`}
              >
                <Activity className="w-5 h-5" />
                {t("emergency.start_diagnosis")}
              </button>
            )}
          </div>
        </div>

        {/* Diagnosis Progress */}
        {progressText && (
          <div className="mt-4 p-3 bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-800 rounded-lg">
            <div className="flex items-center gap-2 text-blue-700 dark:text-blue-300">
              <RefreshCw className="w-4 h-4 animate-spin" />
              <span className="text-sm">{progressText}</span>
            </div>
            {isDiagnosing && (
              <div className="mt-2 grid grid-cols-2 md:grid-cols-4 gap-2">
                {CHECK_KEYS.map(key => {
                  const status = progress[key];
                  return (
                    <div key={key} className="flex items-center gap-1.5 text-xs text-gray-700 dark:text-gray-300">
                      {status ? renderStatusIcon(status) : <RefreshCw className="w-4 h-4 text-blue-500 animate-spin" />}
                      <span>{t(CHECK_LABEL_KEYS[key])}</span>
                      {!status && <span className="text-gray-400">{t("emergency.check_running")}</span>}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Fix History */}
      {showHistory && (
        <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 shadow-sm">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 flex items-center gap-2">
              <Clock className="w-4 h-4" />
              {t("emergency.fix_history")}
            </h3>
            {fixHistory.length > 0 && (
              <button
                onClick={clearHistory}
                className="text-xs text-gray-500 dark:text-gray-400 hover:text-red-600 dark:hover:text-red-400 underline"
              >
                {t("emergency.clear_history")}
              </button>
            )}
          </div>
          {fixHistory.length === 0 ? (
            <p className="text-sm text-gray-500 dark:text-gray-400">{t("emergency.history_empty")}</p>
          ) : (
            <div className="space-y-2">
              {fixHistory.map((entry, idx) => (
                <div key={idx} className="flex items-center justify-between p-2 bg-gray-50 dark:bg-gray-700/50 rounded">
                  <div className="flex items-center gap-2">
                    {entry.success ? (
                      <CheckCircle className="w-4 h-4 text-green-600" />
                    ) : (
                      <XCircle className="w-4 h-4 text-red-600" />
                    )}
                    <span className="text-sm text-gray-700 dark:text-gray-200">
                      {t(`emergency.fix.${historyFixType(entry.action_type)}.title`, { defaultValue: entry.name })}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-gray-500 dark:text-gray-400">
                      {new Date(entry.timestamp).toLocaleString(i18n.language)}
                    </span>
                    {entry.error && (
                      <span className="text-xs text-red-600 dark:text-red-400">{entry.error}</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
          <p className="mt-3 text-xs text-gray-400 dark:text-gray-500">{t("emergency.history_storage_note")}</p>
        </div>
      )}

      {/* Diagnostic Results — collapsible rows */}
      {result && (
        <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 shadow-sm">
          <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-3">{t("emergency.diagnostic_detail")}</h3>

          <div className="space-y-2">
            {CHECK_KEYS.map(key => {
              const item = result[key];
              const open = expandedDetails.has(key);
              const extra = key === "ip_configuration" && (
                <div className="ml-7 mt-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded text-sm">
                  {item.details.local_ipv4 && (
                    <div>
                      <span className="text-gray-500 dark:text-gray-400">{t("emergency.local_ipv4")}: </span>
                      <span className="font-mono text-gray-800 dark:text-gray-200">{item.details.local_ipv4}</span>
                    </div>
                  )}
                  {item.details.local_ipv6 && (
                    <div>
                      <span className="text-gray-500 dark:text-gray-400">{t("emergency.local_ipv6")}: </span>
                      <span className="font-mono text-gray-800 dark:text-gray-200">{item.details.local_ipv6}</span>
                    </div>
                  )}
                </div>
              );
              return (
                <div key={key} className="border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden">
                  <button
                    onClick={() => toggleDetail(key)}
                    className="w-full flex items-center justify-between p-3 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
                  >
                    <div className="flex items-center gap-2">
                      {renderStatusIcon(item.status)}
                      <span className="font-medium text-gray-800 dark:text-gray-100">{t(CHECK_LABEL_KEYS[key])}</span>
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

      {/* Hints that are not automated fixes */}
      {hints.length > 0 && (
        <div className="bg-blue-50 dark:bg-blue-900/20 rounded-lg border border-blue-200 dark:border-blue-800 p-4">
          <h3 className="text-sm font-medium text-gray-800 dark:text-gray-100 flex items-center gap-2 mb-2">
            <Info className="w-4 h-4 text-blue-600" />
            {t("emergency.hints_title")}
          </h3>
          <ul className="list-disc ml-6 space-y-1 text-sm text-gray-700 dark:text-gray-300">
            {hints.map((hint, idx) => <li key={idx}>{hint}</li>)}
          </ul>
        </div>
      )}

      {/* Quick Fixes - Auto Fix */}
      {result && result.recommendations.length > 0 && (
        <div className="bg-gradient-to-r from-orange-50 to-red-50 dark:from-orange-900/30 dark:to-red-900/30 rounded-lg border border-orange-200 dark:border-orange-800 p-4">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-gray-800 dark:text-gray-100 flex items-center gap-2">
              <Wrench className="w-4 h-4 text-orange-600" />
              {t("emergency.available_fixes", { count: result.recommendations.length })}
            </h3>
            {!autoFixing && (
              <button
                onClick={openAutoFixPlan}
                disabled={busy}
                className="px-3 py-1.5 text-xs bg-orange-600 text-white rounded-lg hover:bg-orange-700 transition-colors disabled:opacity-50 flex items-center gap-1"
              >
                <Wrench className="w-3 h-3" />
                {t("emergency.auto_fix_all")}
              </button>
            )}
          </div>

          {autoFixing && (
            <div className="mb-3 p-2 bg-white dark:bg-gray-800 rounded border border-orange-200 dark:border-orange-800 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-orange-700 dark:text-orange-300 text-sm">
                <RefreshCw className="w-4 h-4 animate-spin" />
                {autoFixStep || progressText}
              </div>
              <button
                onClick={stopAutoFix}
                className="px-2 py-1 text-xs border border-orange-300 dark:border-orange-700 text-orange-700 dark:text-orange-300 rounded hover:bg-orange-50 dark:hover:bg-gray-700 flex items-center gap-1"
              >
                <Square className="w-3 h-3" />
                {t("emergency.stop")}
              </button>
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
            {result.recommendations.map((action, idx) => (
              <button
                key={idx}
                onClick={() => setShowConfirm(action.action_type)}
                disabled={busy}
                title={fixMessage(action.action_type) || action.description}
                className="p-3 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg hover:border-orange-300 dark:hover:border-orange-700 hover:bg-orange-50 dark:hover:bg-gray-700 transition-all text-left disabled:opacity-50"
              >
                <div className="flex items-center gap-2">
                  {FIX_ICONS[historyFixType(action.action_type)] || <Wrench className="w-4 h-4" />}
                  <span className="font-medium text-gray-800 dark:text-gray-100">{fixTitle(action.action_type)}</span>
                  {getRiskBadge(action.risk)}
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
            {t("emergency.manual_tools")}
          </h3>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">{t("emergency.manual_tools_desc")}</p>
        </div>
        <button
          onClick={() => setShowManualTools(true)}
          disabled={busy}
          className="px-4 py-2 border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors disabled:opacity-50 flex items-center gap-2"
        >
          <Wrench className="w-4 h-4" />
          {t("emergency.open_tools")}
        </button>
      </div>

      {/* Manual Tools Modal */}
      {showManualTools && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[85vh] overflow-y-auto">
            <div className="p-4 border-b border-gray-200 dark:border-gray-700 flex justify-between items-center sticky top-0 bg-white dark:bg-gray-800 z-10">
              <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100 flex items-center gap-2">
                <Wrench size={20} className="text-blue-500" />
                {t("emergency.manual_tools")}
              </h3>
              <button
                onClick={() => setShowManualTools(false)}
                className="text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
              >
                <X size={24} />
              </button>
            </div>
            <div className="p-4 grid grid-cols-1 md:grid-cols-2 gap-3">
              {manualFixTypes.map(type => {
                const capability = capabilityOf(type);
                const unsupported = capability ? !capability.supported : false;
                return (
                  <button
                    key={type}
                    onClick={() => {
                      setShowManualTools(false);
                      setShowConfirm(type);
                    }}
                    disabled={busy || unsupported}
                    title={unsupported ? capability?.reason ?? t("emergency.unsupported") : undefined}
                    className="p-3 border border-gray-200 dark:border-gray-700 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 hover:border-gray-300 dark:hover:border-gray-600 transition-all text-left disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <div className="flex items-center gap-2 mb-1">
                      {FIX_ICONS[type]}
                      <span className="font-medium text-gray-800 dark:text-gray-100">{fixTitle(type)}</span>
                    </div>
                    <p className="text-sm text-gray-600 dark:text-gray-300 line-clamp-2">
                      {unsupported ? capability?.reason ?? t("emergency.unsupported") : fixMessage(type)}
                    </p>
                  </button>
                );
              })}
            </div>
          </div>
        </div>
      )}

      {/* Batch Fix Confirmation Modal */}
      {autoFixPlan && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-lg w-full mx-4 p-6 max-h-[85vh] overflow-y-auto">
            <div className="flex items-center gap-3 mb-2">
              <AlertTriangle className="w-6 h-6 text-amber-500" />
              <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-100">{t("emergency.auto_fix_plan_title")}</h3>
            </div>
            <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">{t("emergency.auto_fix_plan_desc")}</p>
            <div className="space-y-2 mb-6">
              {autoFixPlan.map(({ action, selected }, idx) => (
                <label key={idx} className="flex items-start gap-3 p-3 border border-gray-200 dark:border-gray-700 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700/50">
                  <input type="checkbox" checked={selected} onChange={() => toggleAutoFixItem(idx)} className="mt-1" />
                  <div>
                    <div className="font-medium text-gray-800 dark:text-gray-100 flex items-center gap-2 flex-wrap">
                      {fixTitle(action.action_type)}
                      {getRiskBadge(action.risk)}
                      <span className="text-xs font-normal text-gray-500 dark:text-gray-400">
                        {t("emergency.est_seconds", { count: action.estimated_time_seconds })}
                      </span>
                    </div>
                    <p className="text-sm text-gray-600 dark:text-gray-300">{fixMessage(action.action_type) || action.description}</p>
                    {action.action_type === "switch_dns" && dnsTargets && (
                      <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">{t("emergency.switch_dns_targets", { servers: dnsTargets })}</p>
                    )}
                  </div>
                </label>
              ))}
            </div>
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setAutoFixPlan(null)}
                className="px-4 py-2 border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
              >
                {t("emergency.cancel")}
              </button>
              <button
                onClick={runAutoFix}
                disabled={!autoFixPlan.some(item => item.selected)}
                className="px-4 py-2 bg-orange-600 text-white rounded-lg hover:bg-orange-700 transition-colors disabled:opacity-50"
              >
                {t("emergency.run_selected", { count: autoFixPlan.filter(item => item.selected).length })}
              </button>
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
                {fixTitle(showConfirm)}
              </h3>
            </div>
            <p className="text-gray-600 dark:text-gray-300 mb-2">{fixMessage(showConfirm)}</p>
            {showConfirm === "switch_dns" && dnsTargets && (
              <p className="text-sm text-gray-500 dark:text-gray-400 mb-2">{t("emergency.switch_dns_targets", { servers: dnsTargets })}</p>
            )}
            <div className="flex gap-3 justify-end mt-6">
              <button
                onClick={() => setShowConfirm(null)}
                disabled={fixing !== null}
                className="px-4 py-2 border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors disabled:opacity-50"
              >
                {t("emergency.cancel")}
              </button>
              <button
                onClick={confirmFix}
                disabled={fixing !== null}
                className="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors disabled:opacity-50 flex items-center gap-2"
              >
                {fixing === showConfirm && <RefreshCw className="w-4 h-4 animate-spin" />}
                {fixing === showConfirm ? t("emergency.executing") : t("emergency.confirm_execute")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

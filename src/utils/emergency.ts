// Types and pure helpers for the emergency kit, shared by the store and the
// page so they can be unit-tested without Tauri.

export type DiagnosticStatus = "pass" | "fail" | "warning";
export type RepairRisk = "low" | "medium" | "high";

export interface DiagnosticItem {
  status: DiagnosticStatus;
  message: string;
  details: any;
  duration_ms: number;
}

export interface RepairAction {
  action_type: string;
  name: string;
  description: string;
  priority: number;
  estimated_time_seconds: number;
  risk: RepairRisk;
}

export interface DiagnosticResult {
  overall_status: DiagnosticStatus;
  network_connectivity: DiagnosticItem;
  ip_configuration: DiagnosticItem;
  dns_resolution: DiagnosticItem;
  network_quality: DiagnosticItem;
  recommendations: RepairAction[];
  /** Keys of the checks that did not pass. */
  problem_checks: string[];
  /** Guidance that is not an automated fix (e.g. check the cable). */
  hints: string[];
  timestamp: number;
}

export interface RepairCapability {
  fix_type: string;
  supported: boolean;
  reason: string | null;
}

export interface DiagnosticsProgressEvent {
  run_id: number | null;
  check: string;
  status: DiagnosticStatus;
  message: string;
}

export const DIAGNOSTICS_PROGRESS_EVENT = "diagnostics-progress";

export const CHECK_KEYS = [
  "network_connectivity",
  "ip_configuration",
  "dns_resolution",
  "network_quality",
] as const;
export type CheckKey = (typeof CHECK_KEYS)[number];
export type CheckProgress = Partial<Record<CheckKey, DiagnosticStatus>>;

/** Manual tools in display order; used until the backend capability list arrives. */
export const MANUAL_FIX_TYPES = [
  "flush_dns_cache",
  "switch_dns",
  "restore_auto_dns",
  "release_renew_ip",
  "toggle_ipv6",
  "reset_adapter",
  "reset_network_stack",
] as const;

/** Results older than this are flagged as possibly outdated. */
export const STALE_AFTER_MS = 5 * 60 * 1000;

export const isStale = (timestamp: number, now: number): boolean => now - timestamp > STALE_AFTER_MS;

/** Number of checks that did not pass (not the number of repair actions). */
export const problemCount = (result: DiagnosticResult): number =>
  Array.isArray(result.problem_checks)
    ? result.problem_checks.length
    : CHECK_KEYS.filter(key => result[key].status !== "pass").length;

/** Fold a progress event into the per-check progress of run `runId`. */
export const applyProgress = (
  progress: CheckProgress,
  event: DiagnosticsProgressEvent,
  runId: number,
): CheckProgress => {
  if (event.run_id !== runId || !(CHECK_KEYS as readonly string[]).includes(event.check)) {
    return progress;
  }
  return { ...progress, [event.check]: event.status };
};

/**
 * Fix type whose label a history entry should show. `restart_network_service`
 * ran the same action as `reset_network_stack` and was merged into it.
 */
export const historyFixType = (actionType: string): string =>
  actionType === "restart_network_service" ? "reset_network_stack" : actionType;

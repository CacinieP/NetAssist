import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "../store/settingsStore";
import { notify } from "../utils/notify";

/** Shape returned by the backend `check_traffic_alerts` command. */
export interface AlertStatus {
  alert_id: string;
  triggered: boolean;
  current_value: number;
  threshold_value: number;
  percentage: number;
}

/** App-shell cadence for the traffic-limit watchdog (ms). */
const DEFAULT_INTERVAL_MS = 5000;

/**
 * Module-scope dedupe set.
 *
 * Deliberately NOT component state: this watchdog is mounted by the app shell
 * and must survive page mounts/unmounts, so switching to Dashboard/Settings
 * and back never re-fires the same "traffic threshold reached" notification.
 * A component-level ref in the Traffic page reset on every mount (and the
 * detection itself stopped as soon as the page unmounted).
 */
let notifiedAlertIds: Set<string> = new Set();

/**
 * `false` until the first successful check.
 *
 * The first check only records the baseline: an alert that was already over
 * its threshold when the app started is not a *new* transition, so it must not
 * produce a notification on every launch.
 */
let baselineRecorded = false;

/** At most one check in flight — a slow backend call must not stack up. */
let checkInFlight = false;

/**
 * Traffic-threshold watchdog for the whole app.
 *
 * Polls `check_traffic_alerts` on an interval that follows the *application*
 * lifecycle (not the Traffic page's), and fires one native notification per
 * not-triggered → triggered transition, gated by `notify_traffic_limit`.
 *
 * The Traffic page keeps its own fetch for rendering live percentages, so
 * its UI stays fresh even when `notify_traffic_limit` is off (and therefore
 * this watchdog is stopped); this hook only owns the notification side.
 */
export function useTrafficAlertMonitor(intervalMs: number = DEFAULT_INTERVAL_MS) {
  const enabled = useSettingsStore((state) => state.settings.notify_traffic_limit);
  // Also read the flag inside the interval, so a Settings change takes effect
  // on the next tick even before React re-runs this effect.
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  useEffect(() => {
    // Notifications off → no watchdog, no backend load.
    if (!enabled) return;

    let cancelled = false;

    const check = async () => {
      if (checkInFlight) return;
      checkInFlight = true;
      try {
        // No arguments: the backend evaluates every alert against its own
        // period (`_period` is an ignored, optional legacy argument).
        const statuses = await invoke<AlertStatus[]>("check_traffic_alerts", {});
        if (cancelled) return;

        const triggered = new Set(
          statuses.filter((s) => s.triggered).map((s) => s.alert_id)
        );

        if (!baselineRecorded) {
          notifiedAlertIds = triggered;
          baselineRecorded = true;
          return;
        }

        const newlyTriggered = [...triggered].filter(
          (id) => !notifiedAlertIds.has(id)
        );
        notifiedAlertIds = triggered;

        if (newlyTriggered.length > 0) {
          void notify(
            "流量告警",
            `有 ${newlyTriggered.length} 项流量阈值已触发，请查看流量监控`
          );
        }
      } catch (error) {
        // Backend unavailable — retry on the next tick.
        console.error("Traffic alert check failed:", error);
      } finally {
        checkInFlight = false;
      }
    };

    void check();
    const timer = setInterval(check, intervalMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [enabled, intervalMs]);
}

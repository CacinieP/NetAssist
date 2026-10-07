import { createPollingController } from './polling.ts';
import { formatBytes } from './formatUtils.ts';

/** Shared command contract for both the status page and notification monitor. */
export interface AlertStatus {
  alert_id: string;
  period_start_timestamp: number;
  triggered: boolean;
  current_value: number;
  threshold_value: number;
  percentage: number;
}

export function trafficAlertPreferences(state: {
  hydrated: boolean; loading: boolean; settings: { notify_traffic_limit: boolean };
}) {
  return { ready: state.hydrated && !state.loading, enabled: state.settings.notify_traffic_limit };
}

export function alertStatusPresentation(status: AlertStatus | undefined) {
  return {
    available: Boolean(status),
    percentage: status?.percentage ?? 0,
    percentText: status ? `${status.percentage.toFixed(0)}%` : '未获取',
    currentText: status ? formatBytes(status.current_value) : '—',
  };
}

/** Notification state survives page changes and App's Strict Mode effect replay. */
export function createTrafficAlertMonitor(options: {
  check: () => Promise<AlertStatus[]>;
  notify: (newlyTriggered: AlertStatus[], stillCurrent: () => boolean) => void | Promise<void>;
  onError: (error: unknown) => void;
  schedule?: (callback: () => void, ms: number) => () => void;
}) {
  let active = false;
  let intervalMs = 5000;
  let notificationGeneration = 0;
  let previousTriggers: Set<string> | null = null;
  const key = (status: AlertStatus) => `${status.alert_id}:${status.period_start_timestamp}`;
  const poller = createPollingController({
    fetch: options.check,
    onValue: statuses => {
      const triggered = new Map(statuses.filter(status => status.triggered).map(status => [key(status), status]));
      const previous = previousTriggers;
      previousTriggers = new Set(triggered.keys());
      // The first successful observation establishes a baseline, including
      // after notifications are re-enabled. Never replay disabled-time events.
      if (previous === null) return;
      const newlyTriggered = [...triggered].filter(([id]) => !previous.has(id)).map(([, status]) => status);
      if (newlyTriggered.length === 0) return;
      const generation = notificationGeneration;
      const stillCurrent = () => active && generation === notificationGeneration;
      void Promise.resolve(options.notify(newlyTriggered, stillCurrent)).catch(error => {
        if (stillCurrent()) options.onError(error);
      });
    },
    // Failed observations do not fabricate zero usage or reset deduplication.
    onError: options.onError,
    schedule: options.schedule,
  });

  return {
    configure(preferences: { ready: boolean; enabled: boolean }, nextIntervalMs = 5000) {
      if (!preferences.ready || !preferences.enabled) {
        previousTriggers = null;
        active = false;
        notificationGeneration += 1;
        poller.stop();
        return;
      }
      if (active && intervalMs === nextIntervalMs) return;
      active = true;
      intervalMs = nextIntervalMs;
      notificationGeneration += 1;
      poller.configure(undefined, intervalMs);
    },
    refresh: poller.refresh,
    stop() {
      active = false;
      notificationGeneration += 1;
      poller.stop();
    },
  };
}

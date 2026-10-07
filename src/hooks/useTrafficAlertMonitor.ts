import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useSettingsStore } from '../store/settingsStore';
import { notify } from '../utils/notify';
import { createTrafficAlertMonitor, trafficAlertPreferences } from '../utils/trafficAlerts';
import type { AlertStatus } from '../utils/trafficAlerts';

const monitor = createTrafficAlertMonitor({
  check: () => invoke<AlertStatus[]>('check_traffic_alerts', {}),
  notify: (statuses, stillCurrent) => notify('流量告警', `有 ${statuses.length} 项流量阈值新近触发，请查看流量监控`, stillCurrent),
  onError: error => console.warn('Traffic alert check failed:', error),
});

/** App owns notifications; the status page can query even when notifications are off. */
export function useTrafficAlertMonitor(intervalMs = 5000) {
  useEffect(() => {
    const sync = () => monitor.configure(trafficAlertPreferences(useSettingsStore.getState()), intervalMs);
    sync();
    // Subscribe directly so a saved setting disables pending notifications
    // immediately, before React's next passive-effect cleanup can run.
    const unsubscribe = useSettingsStore.subscribe(sync);
    return () => { unsubscribe(); monitor.stop(); };
  }, [intervalMs]);
}

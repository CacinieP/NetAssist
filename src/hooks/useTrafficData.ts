import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createTrafficMonitor } from '../utils/trafficPolling';
import type { TrafficStats } from '../utils/trafficPolling';
export type { TrafficStats } from '../utils/trafficPolling';

const monitor = createTrafficMonitor({
  fetchStats: () => invoke<TrafficStats>('get_realtime_traffic'),
  recordPoint: () => invoke('record_traffic_point', {}),
});

/** All pages share one physical realtime request; failed samples clear stale rates. */
export function useRealtimeTraffic(intervalMs = 1000) {
  const [snapshot, setSnapshot] = useState(monitor.getSnapshot);
  useEffect(() => monitor.subscribe(setSnapshot, intervalMs), [intervalMs]);
  return snapshot;
}

/** App is the sole owner. Recording continues while users visit any route. */
export function useRecordTrafficPoint(intervalMs = 5000) {
  useEffect(() => monitor.startRecording(intervalMs), [intervalMs]);
}

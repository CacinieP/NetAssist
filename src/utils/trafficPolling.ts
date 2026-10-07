import { createPollingController } from './polling.ts';

/** Backend rates are bytes/second; timestamp is the OS sample time. */
export interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp?: number;
}

export interface TrafficSnapshot {
  stats: TrafficStats | null;
  error: string | null;
  recordingError: string | null;
}

/** One realtime lane and one app-owned recording lane, regardless of page subscribers. */
export function createTrafficMonitor(options: {
  fetchStats: () => Promise<TrafficStats>;
  recordPoint: () => Promise<unknown>;
  schedule?: (callback: () => void, ms: number) => () => void;
}) {
  const listeners = new Set<(snapshot: TrafficSnapshot) => void>();
  let snapshot: TrafficSnapshot = { stats: null, error: null, recordingError: null };
  let recordingOwners = 0;
  const update = (patch: Partial<TrafficSnapshot>) => {
    snapshot = { ...snapshot, ...patch };
    for (const listener of listeners) listener(snapshot);
  };
  const live = createPollingController({
    fetch: options.fetchStats,
    onValue: stats => update({ stats, error: null }),
    onError: error => update({ stats: null, error: String(error) }),
    schedule: options.schedule,
  });
  const recorder = createPollingController({
    // Recording samples its own OS counters in the backend. A cached UI rate
    // is never passed off as a new physical sample or used to estimate bytes.
    fetch: () => options.recordPoint(),
    onValue: () => update({ recordingError: null }),
    onError: error => update({ recordingError: String(error) }),
    schedule: options.schedule,
  });
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: (snapshot: TrafficSnapshot) => void, intervalMs = 1000) {
      const wasEmpty = listeners.size === 0;
      listeners.add(listener);
      listener(snapshot);
      if (wasEmpty) live.configure(undefined, intervalMs);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) live.stop();
      };
    },
    startRecording(intervalMs = 5000) {
      recordingOwners += 1;
      if (recordingOwners === 1) recorder.configure(undefined, intervalMs);
      return () => {
        recordingOwners -= 1;
        if (recordingOwners === 0) recorder.stop();
      };
    },
  };
}

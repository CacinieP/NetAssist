import { useState, useEffect, useRef } from 'react';

// TrafficStats type shared across components
export interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp?: string;
}

// Global singleton to ensure only one polling interval
let globalTrafficData: TrafficStats | null = null;
let globalTrafficListeners: Set<(data: TrafficStats) => void> = new Set();
let globalTrafficInterval: ReturnType<typeof setInterval> | null = null;

function startGlobalTrafficPolling() {
  if (globalTrafficInterval) return; // Already polling

  globalTrafficInterval = setInterval(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const data = await invoke<TrafficStats>('get_realtime_traffic');
      globalTrafficData = data;
      for (const listener of globalTrafficListeners) {
        listener(data);
      }
    } catch {
      // Silently ignore — will retry next interval
    }
  }, 1000);
}

function stopGlobalTrafficPolling() {
  if (globalTrafficInterval) {
    clearInterval(globalTrafficInterval);
    globalTrafficInterval = null;
  }
}

/**
 * Shared hook for real-time traffic data.
 * Polls get_realtime_traffic once per second globally,
 * no matter how many components use this hook.
 */
export function useRealtimeTraffic() {
  const [stats, setStats] = useState<TrafficStats | null>(globalTrafficData);

  useEffect(() => {
    // Set initial data if available
    if (globalTrafficData) {
      setStats(globalTrafficData);
    }

    globalTrafficListeners.add(setStats);
    startGlobalTrafficPolling();

    return () => {
      globalTrafficListeners.delete(setStats);
      if (globalTrafficListeners.size === 0) {
        stopGlobalTrafficPolling();
      }
    };
  }, []);

  return { stats };
}

/**
 * Records a traffic data point to the backend.
 * Uses a ref to always record the latest value.
 */
export function useRecordTrafficPoint(intervalMs: number = 60000) {
  const { stats } = useRealtimeTraffic();
  const statsRef = useRef(stats);
  statsRef.current = stats;

  useEffect(() => {
    const id = setInterval(async () => {
      const current = statsRef.current;
      if (current) {
        try {
          const { invoke } = await import('@tauri-apps/api/core');
          await invoke('record_traffic_point', {
            download_bps: current.download_bps,
            upload_bps: current.upload_bps,
          });
        } catch {
          // Silently ignore
        }
      }
    }, intervalMs);

    return () => clearInterval(id);
  }, [intervalMs]);
}

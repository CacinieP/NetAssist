import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';

// TrafficStats type shared across components
export interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp?: number;
}

// Global singleton to ensure only one polling interval per frequency bucket
let globalTrafficData: TrafficStats | null = null;
let globalTrafficListeners: Set<(data: TrafficStats) => void> = new Set();
let globalTrafficInterval: ReturnType<typeof setInterval> | null = null;

function startGlobalTrafficPolling(intervalMs: number = 1000) {
  if (globalTrafficInterval) {
    clearInterval(globalTrafficInterval);
  }

  const poll = async () => {
    try {
      const data = await invoke<TrafficStats>('get_realtime_traffic');
      globalTrafficData = data;
      for (const listener of globalTrafficListeners) {
        listener(data);
      }
    } catch {
      // Silently ignore — will retry next interval
    }
  };

  // Initial poll
  poll();
  globalTrafficInterval = setInterval(poll, intervalMs);
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
export function useRealtimeTraffic(intervalMs: number = 1000) {
  const [stats, setStats] = useState<TrafficStats | null>(globalTrafficData);

  useEffect(() => {
    // Set initial data if available
    if (globalTrafficData) {
      setStats(globalTrafficData);
    }

    globalTrafficListeners.add(setStats);
    startGlobalTrafficPolling(intervalMs);

    return () => {
      globalTrafficListeners.delete(setStats);
      if (globalTrafficListeners.size === 0) {
        stopGlobalTrafficPolling();
      }
    };
  }, [intervalMs]);

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

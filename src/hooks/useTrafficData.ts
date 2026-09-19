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
// Single-flight guard: a `get_realtime_traffic` call that has not returned
// yet must not be joined by the next tick (overlapping reads make the
// backend compute the byte-delta over millisecond gaps → rate spikes).
let trafficPollInFlight = false;

function startGlobalTrafficPolling(intervalMs: number = 1000) {
  // Multiple consumers (dashboard cards etc.) mount at the same time. Only
  // the FIRST starts the timer; later mounts must NOT restart the interval
  // or fire an immediate poll — otherwise the backend rate (byte delta since
  // last read) is computed over millisecond gaps and produces spikes.
  if (globalTrafficInterval) {
    return;
  }

  const poll = async () => {
    if (trafficPollInFlight) return;
    trafficPollInFlight = true;
    try {
      const data = await invoke<TrafficStats>('get_realtime_traffic');
      globalTrafficData = data;
      for (const listener of globalTrafficListeners) {
        listener(data);
      }
    } catch {
      // Silently ignore — will retry next interval
    } finally {
      trafficPollInFlight = false;
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
 *
 * Default interval is 5s so the history trend chart starts showing points
 * soon after the page is opened (the backend keeps a 24h rolling window).
 */
export function useRecordTrafficPoint(intervalMs: number = 5000) {
  const { stats } = useRealtimeTraffic();
  const statsRef = useRef(stats);
  statsRef.current = stats;

  useEffect(() => {
    // Single-flight: never stack up `record_traffic_point` writes.
    let inFlight = false;

    const id = setInterval(async () => {
      const current = statsRef.current;
      if (!current || inFlight) return;
      inFlight = true;
      try {
        await invoke('record_traffic_point', {
          downloadBps: current.download_bps,
          uploadBps: current.upload_bps,
        });
      } catch {
        // Silently ignore
      } finally {
        inFlight = false;
      }
    }, intervalMs);

    return () => clearInterval(id);
  }, [intervalMs]);
}

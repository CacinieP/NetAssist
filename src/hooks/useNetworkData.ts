import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useSettingsStore } from '../store/settingsStore';
import { createNetworkProbeCoordinator, probePreferences } from '../utils/networkProbes';
export type { NetworkStatus, IPInfo } from '../utils/networkProbes';

const coordinator = createNetworkProbeCoordinator({ invoke });
export const refreshNetworkData = () => coordinator.refresh();
export const isNetworkResultCurrent = (resultId: number) => coordinator.isResultCurrent(resultId);

/** App owns the coordinator. Mounting a page only subscribes to its existing snapshot. */
export function useNetworkData(options?: { owner?: boolean }) {
  const isOwner = options?.owner ?? false;
  const [snapshot, setSnapshot] = useState(coordinator.getSnapshot);
  useEffect(() => {
    setSnapshot(coordinator.getSnapshot());
    return coordinator.subscribe(setSnapshot);
  }, []);
  useEffect(() => {
    if (!isOwner) return;
    const sync = () => coordinator.configure(probePreferences(useSettingsStore.getState()));
    sync();
    // Invalidate immediately when persisted settings change, before React cleanup.
    const unsubscribe = useSettingsStore.subscribe(sync);
    return () => { unsubscribe(); coordinator.stop(); };
  }, [isOwner]);
  return { ...snapshot, refresh: refreshNetworkData };
}

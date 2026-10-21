import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  applyProgress,
  DIAGNOSTICS_PROGRESS_EVENT,
  type CheckProgress,
  type DiagnosticResult,
  type DiagnosticsProgressEvent,
  type RepairCapability,
} from "../utils/emergency";

// Lives outside the EmergencyKit page so the last result (and a diagnosis
// still running) survive switching tabs.
interface EmergencyStore {
  result: DiagnosticResult | null;
  isDiagnosing: boolean;
  progress: CheckProgress;
  /** Incremented per run; a run whose id is no longer current was cancelled. */
  runId: number;
  capabilities: RepairCapability[] | null;
  /** Resolves to null when the run was cancelled; rejects on backend errors. */
  runDiagnosis: () => Promise<DiagnosticResult | null>;
  /** The backend keeps running (bounded by its per-check timeout); its result is dropped. */
  cancelDiagnosis: () => void;
  loadCapabilities: () => Promise<void>;
}

export const useEmergencyStore = create<EmergencyStore>((set, get) => ({
  result: null,
  isDiagnosing: false,
  progress: {},
  runId: 0,
  capabilities: null,

  runDiagnosis: async () => {
    const runId = get().runId + 1;
    set({ runId, isDiagnosing: true, progress: {} });
    const unlisten = await listen<DiagnosticsProgressEvent>(DIAGNOSTICS_PROGRESS_EVENT, event => {
      if (get().runId === runId) {
        set(state => ({ progress: applyProgress(state.progress, event.payload, runId) }));
      }
    }).catch(() => null);
    try {
      const result = await invoke<DiagnosticResult>("run_diagnostics", { runId });
      if (get().runId !== runId) return null;
      set({ result });
      return result;
    } finally {
      unlisten?.();
      if (get().runId === runId) set({ isDiagnosing: false });
    }
  },

  cancelDiagnosis: () => {
    set(state => ({ runId: state.runId + 1, isDiagnosing: false, progress: {} }));
  },

  loadCapabilities: async () => {
    if (get().capabilities) return;
    try {
      set({ capabilities: await invoke<RepairCapability[]>("get_repair_capabilities") });
    } catch (e) {
      console.error("Failed to load repair capabilities:", e);
    }
  },
}));

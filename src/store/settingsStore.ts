import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface Settings {
  auto_start: boolean;
  minimize_to_tray: boolean;
  refresh_interval_secs: number;
  show_geoip: boolean;
  primary_dns: string;
  secondary_dns: string;
  notify_network_abnormal: boolean;
  notify_traffic_limit: boolean;
  traffic_limit_gb: number;
  dark_mode: boolean;
  language: string;
}

const defaultSettings: Settings = {
  auto_start: false,
  minimize_to_tray: true,
  refresh_interval_secs: 1,
  show_geoip: true,
  primary_dns: "8.8.8.8",
  secondary_dns: "1.1.1.1",
  notify_network_abnormal: true,
  notify_traffic_limit: true,
  traffic_limit_gb: 100,
  dark_mode: false,
  language: "zh-CN",
};

interface SettingsStore {
  settings: Settings;
  loading: boolean;
  saving: boolean;
  error: string | null;
  loadSettings: () => Promise<void>;
  setSettings: (partial: Partial<Settings>) => void;
  saveSettings: () => Promise<boolean>;
  resetSettings: () => Promise<void>;
}

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  settings: defaultSettings,
  loading: false,
  saving: false,
  error: null,

  loadSettings: async () => {
    try {
      set({ loading: true, error: null });
      const settings = await invoke<Settings>("get_settings");
      set({ settings });
    } catch (e: any) {
      set({ error: e?.toString?.() ?? "加载设置失败" });
    } finally {
      set({ loading: false });
    }
  },

  setSettings: (partial) => {
    set({ settings: { ...get().settings, ...partial } });
  },

  saveSettings: async () => {
    // Store previous settings for rollback
    const previousSettings = get().settings;
    try {
      set({ saving: true, error: null });
      const ok = await invoke<boolean>("update_settings", { settings: get().settings });
      if (!ok) {
        throw new Error("保存设置失败: 服务器返回 false");
      }
      return ok;
    } catch (e: any) {
      // Rollback to previous settings on error
      set({ settings: previousSettings, error: e?.toString?.() ?? "保存设置失败" });
      return false;
    } finally {
      set({ saving: false });
    }
  },

  resetSettings: async () => {
    try {
      set({ loading: true, error: null });
      const settings = await invoke<Settings>("reset_settings");
      set({ settings });
    } catch (e: any) {
      set({ error: e?.toString?.() ?? "恢复默认设置失败" });
    } finally {
      set({ loading: false });
    }
  },
}));

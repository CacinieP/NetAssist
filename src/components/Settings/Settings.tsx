import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "../../store/settingsStore";
import type { Settings as SettingsType } from "../../store/settingsStore";

export default function Settings() {
  const { settings, setSettings, saveSettings, saving, error: storeError } = useSettingsStore();
  const { t } = useTranslation();

  // Local state for form data
  const [localSettings, setLocalSettings] = useState<SettingsType>(settings);
  const [validationErrors, setValidationErrors] = useState<Record<string, string>>({});
  const [saveSuccess, setSaveSuccess] = useState(false);

  // Update local state when store changes
  useEffect(() => {
    setLocalSettings(settings);
  }, [settings]);

  // DNS validation function
  const validateDNS = (ip: string): boolean => {
    if (!ip) return true; // Empty is allowed (will use default)
    const dnsRegex = /^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$/;
    return dnsRegex.test(ip);
  };

  // Handle DNS input change with validation
  const handleDNSChange = (field: 'primary_dns' | 'secondary_dns', value: string) => {
    setLocalSettings(prev => ({ ...prev, [field]: value }));
    setSaveSuccess(false);

    if (value && !validateDNS(value)) {
      setValidationErrors(prev => ({
        ...prev,
        [field]: '请输入有效的 IP 地址格式 (如 8.8.8.8)'
      }));
    } else {
      setValidationErrors(prev => ({ ...prev, [field]: '' }));
    }
  };

  // Handle refresh interval change with validation
  const handleRefreshIntervalChange = (value: string) => {
    const num = parseInt(value, 10);
    if (isNaN(num)) {
      setValidationErrors(prev => ({
        ...prev,
        refresh_interval_secs: '请输入有效的数字'
      }));
      return; // Don't update state with NaN
    }
    setLocalSettings(prev => ({ ...prev, refresh_interval_secs: num }));
    setSaveSuccess(false);

    if (num < 1 || num > 3600) {
      setValidationErrors(prev => ({
        ...prev,
        refresh_interval_secs: '刷新间隔必须在 1-3600 秒之间'
      }));
    } else {
      setValidationErrors(prev => ({ ...prev, refresh_interval_secs: '' }));
    }
  };

  // Handle traffic limit change with validation
  const handleTrafficLimitChange = (value: string) => {
    const num = parseFloat(value);
    if (isNaN(num)) {
      setValidationErrors(prev => ({
        ...prev,
        traffic_limit_gb: '请输入有效的数字'
      }));
      return; // Don't update state with NaN
    }
    setLocalSettings(prev => ({ ...prev, traffic_limit_gb: num }));
    setSaveSuccess(false);

    if (num < 0.1 || num > 10000) {
      setValidationErrors(prev => ({
        ...prev,
        traffic_limit_gb: '流量限制必须在 0.1-10000 GB 之间'
      }));
    } else {
      setValidationErrors(prev => ({ ...prev, traffic_limit_gb: '' }));
    }
  };

  // Handle save with validation
  const handleSave = async () => {
    // Validate all fields
    const errors: Record<string, string> = {};

    if (!validateDNS(localSettings.primary_dns)) {
      errors.primary_dns = '请输入有效的主 DNS 地址';
    }

    if (localSettings.secondary_dns && !validateDNS(localSettings.secondary_dns)) {
      errors.secondary_dns = '请输入有效的备用 DNS 地址';
    }

    if (localSettings.refresh_interval_secs < 1 || localSettings.refresh_interval_secs > 3600) {
      errors.refresh_interval_secs = '刷新间隔必须在 1-3600 秒之间';
    }

    if (localSettings.traffic_limit_gb < 0.1 || localSettings.traffic_limit_gb > 10000) {
      errors.traffic_limit_gb = '流量限制必须在 0.1-10000 GB 之间';
    }

    setValidationErrors(errors);

    if (Object.keys(errors).length === 0) {
      setSettings(localSettings);
      const success = await saveSettings();
      if (success) {
        setSaveSuccess(true);
        setTimeout(() => setSaveSuccess(false), 3000);
      }
    }
  };

  // Handle reset
  const handleReset = async () => {
    if (confirm('确定要恢复默认设置吗？')) {
      await useSettingsStore.getState().resetSettings();
      setSaveSuccess(true);
      setTimeout(() => setSaveSuccess(false), 3000);
    }
  };

  return (
    <div className="p-6 space-y-6">
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-gray-800 dark:text-gray-100">{t("settings.title")}</h2>
        <p className="text-gray-500 dark:text-gray-400">{t("settings.subtitle")}</p>
      </div>

      {/* Success Message */}
      {saveSuccess && (
        <div className="bg-green-50 dark:bg-green-900/30 border border-green-200 dark:border-green-800 text-green-700 dark:text-green-300 px-4 py-3 rounded-lg flex items-center gap-2">
          <span>✓</span>
          <span>{t("settings.save_success")}</span>
        </div>
      )}

      {/* Error Message */}
      {storeError && (
        <div className="bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 text-red-700 dark:text-red-300 px-4 py-3 rounded-lg">
          {storeError}
        </div>
      )}

      {/* General Settings */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-4">{t("settings.general")}</h3>

        <div className="space-y-3">
          <label className="flex items-center justify-between cursor-pointer">
            <span className="text-gray-700 dark:text-gray-200">{t("settings.auto_start")}</span>
            <input
              type="checkbox"
              checked={localSettings.auto_start}
              onChange={async (e) => {
                const enabled = e.target.checked;
                setLocalSettings(prev => ({ ...prev, auto_start: enabled }));
                setSettings({ auto_start: enabled });
                setSaveSuccess(false);
                // Apply launch-at-login immediately via the autostart plugin.
                try {
                  await invoke("set_autostart", { enabled });
                } catch (err) {
                  console.error("Failed to toggle autostart:", err);
                }
              }}
              className="w-4 h-4 text-blue-600 rounded focus:ring-2 focus:ring-blue-500"
            />
          </label>
          <label className="flex items-center justify-between cursor-pointer">
            <span className="text-gray-700 dark:text-gray-200">{t("settings.minimize_to_tray")}</span>
            <input
              type="checkbox"
              checked={localSettings.minimize_to_tray}
              onChange={(e) => {
                setLocalSettings(prev => ({ ...prev, minimize_to_tray: e.target.checked }));
                setSettings({ minimize_to_tray: e.target.checked });
                setSaveSuccess(false);
              }}
              className="w-4 h-4 text-blue-600 rounded focus:ring-2 focus:ring-blue-500"
            />
          </label>
          <label className="flex items-center justify-between cursor-pointer">
            <span className="text-gray-700 dark:text-gray-200">{t("settings.dark_mode")}</span>
            <input
              type="checkbox"
              checked={localSettings.dark_mode}
              onChange={(e) => {
                // Dark mode applies instantly (live preview) by writing to the
                // store, which App.tsx reflects to <html class="dark">. Other
                // fields remain in the local draft until "保存设置" is clicked.
                setLocalSettings(prev => ({ ...prev, dark_mode: e.target.checked }));
                setSettings({ dark_mode: e.target.checked });
                setSaveSuccess(false);
              }}
              className="w-4 h-4 text-blue-600 rounded focus:ring-2 focus:ring-blue-500"
            />
          </label>
          <div>
            <label className="text-gray-700 dark:text-gray-200 block mb-2">{t("settings.language")}</label>
            <select
              value={localSettings.language}
              onChange={(e) => {
                const lang = e.target.value;
                setLocalSettings(prev => ({ ...prev, language: lang }));
                setSettings({ language: lang });
                setSaveSuccess(false);
              }}
              className="px-3 py-1.5 border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 w-40"
            >
              <option value="zh-CN">{t("settings.lang_zh")}</option>
              <option value="en-US">{t("settings.lang_en")}</option>
            </select>
          </div>
        </div>
      </div>

      {/* Monitoring Settings */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-4">{t("settings.monitoring")}</h3>

        <div className="space-y-4">
          <div>
            <label className="text-gray-700 dark:text-gray-200 block mb-2">{t("settings.refresh_interval")}</label>
            <select
              value={localSettings.refresh_interval_secs}
              onChange={(e) => handleRefreshIntervalChange(e.target.value)}
              className={`px-3 py-1.5 border rounded-lg focus:outline-none focus:ring-2 w-32 dark:bg-gray-700 dark:text-gray-200 ${
                validationErrors.refresh_interval_secs
                  ? 'border-red-300 focus:ring-red-500'
                  : 'border-gray-300 dark:border-gray-600 focus:ring-blue-500'
              }`}
            >
              <option value={1}>{t("settings.interval_1s")}</option>
              <option value={2}>{t("settings.interval_2s")}</option>
              <option value={5}>{t("settings.interval_5s")}</option>
              <option value={10}>{t("settings.interval_10s")}</option>
              <option value={30}>{t("settings.interval_30s")}</option>
              <option value={60}>{t("settings.interval_1m")}</option>
              <option value={120}>{t("settings.interval_2m")}</option>
              <option value={300}>{t("settings.interval_5m")}</option>
              <option value={600}>{t("settings.interval_10m")}</option>
              <option value={1800}>{t("settings.interval_30m")}</option>
              <option value={3600}>{t("settings.interval_1h")}</option>
            </select>
            {validationErrors.refresh_interval_secs && (
              <p className="text-red-500 text-sm mt-1">{validationErrors.refresh_interval_secs}</p>
            )}
          </div>
          <label className="flex items-center justify-between cursor-pointer">
            <span className="text-gray-700 dark:text-gray-200">{t("settings.show_geoip")}</span>
            <input
              type="checkbox"
              checked={localSettings.show_geoip}
              onChange={(e) => {
                setLocalSettings(prev => ({ ...prev, show_geoip: e.target.checked }));
                setSaveSuccess(false);
              }}
              className="w-4 h-4 text-blue-600 rounded focus:ring-2 focus:ring-blue-500"
            />
          </label>
          <div>
            <label className="text-gray-700 dark:text-gray-200 block mb-2">{t("settings.traffic_limit")}</label>
            <input
              type="number"
              min={0.1}
              max={10000}
              step={0.1}
              value={localSettings.traffic_limit_gb}
              onChange={(e) => handleTrafficLimitChange(e.target.value)}
              className={`px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 w-32 dark:bg-gray-700 dark:text-gray-200 ${
                validationErrors.traffic_limit_gb
                  ? 'border-red-300 focus:ring-red-500'
                  : 'border-gray-300 dark:border-gray-600 focus:ring-blue-500'
              }`}
            />
            {validationErrors.traffic_limit_gb && (
              <p className="text-red-500 text-sm mt-1">{validationErrors.traffic_limit_gb}</p>
            )}
          </div>
        </div>
      </div>

      {/* DNS Settings */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-4">{t("settings.dns_settings")}</h3>

        <div className="space-y-3">
          <div>
            <label className="block text-sm text-gray-700 dark:text-gray-200 mb-1">{t("settings.primary_dns")}</label>
            <input
              type="text"
              value={localSettings.primary_dns}
              onChange={(e) => handleDNSChange('primary_dns', e.target.value)}
              className={`w-full px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 font-mono dark:bg-gray-700 dark:text-gray-200 ${
                validationErrors.primary_dns
                  ? 'border-red-300 focus:ring-red-500'
                  : 'border-gray-300 dark:border-gray-600 focus:ring-blue-500'
              }`}
              placeholder="例如: 8.8.8.8"
            />
            {validationErrors.primary_dns && (
              <p className="text-red-500 text-sm mt-1">{validationErrors.primary_dns}</p>
            )}
          </div>
          <div>
            <label className="block text-sm text-gray-700 dark:text-gray-200 mb-1">{t("settings.secondary_dns")}</label>
            <input
              type="text"
              value={localSettings.secondary_dns}
              onChange={(e) => handleDNSChange('secondary_dns', e.target.value)}
              className={`w-full px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 font-mono dark:bg-gray-700 dark:text-gray-200 ${
                validationErrors.secondary_dns
                  ? 'border-red-300 focus:ring-red-500'
                  : 'border-gray-300 dark:border-gray-600 focus:ring-blue-500'
              }`}
              placeholder="例如: 1.1.1.1"
            />
            {validationErrors.secondary_dns && (
              <p className="text-red-500 text-sm mt-1">{validationErrors.secondary_dns}</p>
            )}
          </div>
        </div>
      </div>

      {/* Notification Settings */}
      <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-200 mb-4">{t("settings.notifications")}</h3>

        <div className="space-y-3">
          <label className="flex items-center justify-between cursor-pointer">
            <span className="text-gray-700 dark:text-gray-200">{t("settings.notify_network_abnormal")}</span>
            <input
              type="checkbox"
              checked={localSettings.notify_network_abnormal}
              onChange={(e) => {
                setLocalSettings(prev => ({ ...prev, notify_network_abnormal: e.target.checked }));
                setSaveSuccess(false);
              }}
              className="w-4 h-4 text-blue-600 rounded focus:ring-2 focus:ring-blue-500"
            />
          </label>
          <label className="flex items-center justify-between cursor-pointer">
            <span className="text-gray-700 dark:text-gray-200">{t("settings.notify_traffic_limit")}</span>
            <input
              type="checkbox"
              checked={localSettings.notify_traffic_limit}
              onChange={(e) => {
                setLocalSettings(prev => ({ ...prev, notify_traffic_limit: e.target.checked }));
                setSaveSuccess(false);
              }}
              className="w-4 h-4 text-blue-600 rounded focus:ring-2 focus:ring-blue-500"
            />
          </label>
        </div>
      </div>

      {/* Action Buttons */}
      <div className="flex gap-3">
        <button
          onClick={handleSave}
          disabled={saving || Object.values(validationErrors).some(Boolean)}
          className="px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {saving ? t("settings.saving") : t("settings.save")}
        </button>
        <button
          onClick={handleReset}
          className="px-6 py-2 border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 dark:bg-gray-800 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
        >
          {t("settings.reset")}
        </button>
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import { Activity, ArrowDown, ArrowUp } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import { formatSpeed } from "../../utils/formatUtils";

interface StatusBarProps {
  networkStatus: "normal" | "abnormal" | "loading" | "idle";
  networkMessage?: string;
  locationDetail?: string;
  ipv6Detail?: string;
  probeDetail?: string;
  probeSummary?: string;
  automatic?: boolean;
  onProbe?: () => void;
  probeDisabled?: boolean;
  ipv4?: string;
  ipv6?: string;
  location?: string;
  downloadSpeed: number | null;
  uploadSpeed: number | null;
}

export default function StatusBar({
  networkStatus,
  networkMessage,
  locationDetail,
  ipv6Detail,
  probeDetail,
  probeSummary,
  automatic,
  onProbe,
  probeDisabled,
  ipv4 = "-",
  ipv6 = "-",
  location = "-",
  downloadSpeed = 0,
  uploadSpeed = 0,
}: StatusBarProps) {
  const { t } = useTranslation();
  const [appVersion, setAppVersion] = useState<string | null>(null);

  // Read the real app version from the Tauri backend (was hardcoded v0.3.0,
  // which drifted from package.json/tauri.conf.json).
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => setAppVersion(null));
  }, []);

  const statusText = networkMessage || (networkStatus === "idle" ? "未检测" : networkStatus === "loading" ? "检测中…" : networkStatus === "normal" ? "探测目标可达" : t("status.abnormal"));
  const statusIcon = networkStatus === "idle" ? "—" : networkStatus === "loading" ? "…" : networkStatus === "normal" ? "✓" : "✗";

  return (
    <div className="min-h-12 bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between px-4 shrink-0">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 py-2 text-sm">
        {/* Network Status */}
        <div className="flex items-center gap-2" title={[probeSummary, probeDetail].filter(Boolean).join("\n")}>
          <span className="text-lg">{statusIcon}</span>
          <span className="text-gray-600 dark:text-gray-400">目标连通性:</span>
          <span
            className={`font-medium ${
              (networkStatus === "loading" || networkStatus === "idle") ? "text-gray-500" : networkStatus === "normal" ? "text-green-600" : "text-red-600"
            }`}
          >
            {statusText}
          </span>
        </div>

        <div className="flex items-center gap-2 text-xs text-gray-500">
          <span>{automatic ? "自动检测" : "手动检测"}</span>
          <span>{probeSummary}</span>
          <button onClick={onProbe} disabled={probeDisabled} className="text-blue-600 disabled:opacity-50" title="发起一轮 HTTP、DNS、公网 IP 及已启用的 GeoIP 查询">开始检测</button>
        </div>

        {/* IP Address */}
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-gray-400 dark:text-gray-500" />
          <span className="text-gray-600 dark:text-gray-400">探测 IPv4:</span>
          <span className="font-mono text-xs">{ipv4}</span>
        </div>

        {/* IPv6 */}
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-gray-400 dark:text-gray-500" />
          <span className="text-gray-600 dark:text-gray-400">本地 IPv6:</span>
          <span className="font-mono text-xs" title={ipv6Detail}>{ipv6}</span>
        </div>

        {/* Location */}
        <div className="flex items-center gap-2">
          <span className="text-gray-600 dark:text-gray-400">{t("status.location")}:</span>
          <span className="text-gray-700 dark:text-gray-300" title={locationDetail}>{location}</span>
        </div>

        {/* Real-time Speed */}
        <div className="flex items-center gap-4" title="路由接口实时流量；不是所有网卡相加">
          <div className="flex items-center gap-1">
            <ArrowDown className="w-4 h-4 text-blue-500" />
            <span className="font-mono text-sm">{downloadSpeed === null ? "—" : formatSpeed(downloadSpeed)}</span>
          </div>
          <div className="flex items-center gap-1">
            <ArrowUp className="w-4 h-4 text-green-500" />
            <span className="font-mono text-sm">{uploadSpeed === null ? "—" : formatSpeed(uploadSpeed)}</span>
          </div>
        </div>
      </div>

      <div className="text-xs text-gray-400 dark:text-gray-500">NetAssist v{appVersion || "—"}</div>
    </div>
  );
}

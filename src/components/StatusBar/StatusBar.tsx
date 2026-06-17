import { Activity, ArrowDown, ArrowUp } from "lucide-react";
import { formatSpeed } from "../../utils/formatUtils";

interface StatusBarProps {
  networkStatus: "normal" | "abnormal";
  ipv4?: string;
  ipv6?: string;
  location?: string;
  downloadSpeed: number;
  uploadSpeed: number;
}

export default function StatusBar({
  networkStatus,
  ipv4 = "-",
  ipv6 = "-",
  location = "-",
  downloadSpeed = 0,
  uploadSpeed = 0,
}: StatusBarProps) {
  const statusText = networkStatus === "normal" ? "正常" : "异常";
  const statusIcon = networkStatus === "normal" ? "✓" : "✗";

  return (
    <div className="h-12 bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between px-4 shrink-0">
      <div className="flex items-center gap-6 text-sm">
        {/* Network Status */}
        <div className="flex items-center gap-2">
          <span className="text-lg">{statusIcon}</span>
          <span className="text-gray-600 dark:text-gray-400">网络:</span>
          <span
            className={`font-medium ${
              networkStatus === "normal" ? "text-green-600" : "text-red-600"
            }`}
          >
            {statusText}
          </span>
        </div>

        {/* IP Address */}
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-gray-400 dark:text-gray-500" />
          <span className="text-gray-600 dark:text-gray-400">IPv4:</span>
          <span className="font-mono text-xs">{ipv4}</span>
        </div>

        {/* IPv6 */}
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-gray-400 dark:text-gray-500" />
          <span className="text-gray-600 dark:text-gray-400">IPv6:</span>
          <span className="font-mono text-xs">{ipv6}</span>
        </div>

        {/* Location */}
        <div className="flex items-center gap-2">
          <span className="text-gray-600 dark:text-gray-400">位置:</span>
          <span className="text-gray-700 dark:text-gray-300">{location}</span>
        </div>

        {/* Real-time Speed */}
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-1">
            <ArrowDown className="w-4 h-4 text-blue-500" />
            <span className="font-mono text-sm">{formatSpeed(downloadSpeed)}</span>
          </div>
          <div className="flex items-center gap-1">
            <ArrowUp className="w-4 h-4 text-green-500" />
            <span className="font-mono text-sm">{formatSpeed(uploadSpeed)}</span>
          </div>
        </div>
      </div>

      <div className="text-xs text-gray-400 dark:text-gray-500">NetAssist v0.3.0</div>
    </div>
  );
}

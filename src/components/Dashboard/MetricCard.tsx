interface MetricCardProps {
  title: string;
  value: string;
  status: "normal" | "abnormal" | "pending";
  detail?: string;
  statusLabel?: string;
  unit: string;
}

export default function MetricCard({
  title,
  value,
  status,
  unit,
  detail,
  statusLabel,
}: MetricCardProps) {
  const statusColor = status === "pending" ? "text-gray-500" : status === "normal" ? "text-green-600" : "text-red-600";
  const statusText = status === "pending" ? "检测中" : status === "normal" ? "正常" : "异常";
  const bgColor = status === "pending" ? "bg-gray-50" : status === "normal" ? "bg-green-50" : "bg-red-50";

  return (
    <div className={`rounded-lg border p-4 ${bgColor} dark:bg-gray-800 border-gray-200 dark:border-gray-700`}>
      <div className="flex justify-between items-start mb-2">
        <span className="text-sm font-medium text-gray-700 dark:text-gray-200">{title}</span>
        <span className={`text-xs ${statusColor}`}>{statusLabel || statusText}</span>
      </div>
      <div className="flex items-baseline gap-1">
        <span className="text-2xl font-bold text-gray-800 dark:text-gray-100">{value}</span>
        {unit && <span className="text-sm text-gray-500 dark:text-gray-400">{unit}</span>}
      </div>
      {detail && <p className="mt-2 text-xs text-gray-500 dark:text-gray-400 break-all">{detail}</p>}
    </div>
  );
}

interface MetricCardProps {
  title: string;
  value: string;
  status: "normal" | "abnormal";
  unit: string;
}

export default function MetricCard({
  title,
  value,
  status,
  unit,
}: MetricCardProps) {
  const statusColor = status === "normal" ? "text-green-600" : "text-red-600";
  const statusText = status === "normal" ? "Normal" : "Abnormal";
  const bgColor = status === "normal" ? "bg-green-50" : "bg-red-50";

  return (
    <div className={`rounded-lg border p-4 ${bgColor} border-gray-200`}>
      <div className="flex justify-between items-start mb-2">
        <span className="text-sm font-medium text-gray-700">{title}</span>
        <span className={`text-xs ${statusColor}`}>{statusText}</span>
      </div>
      <div className="flex items-baseline gap-1">
        <span className="text-2xl font-bold text-gray-800">{value}</span>
        {unit && <span className="text-sm text-gray-500">{unit}</span>}
      </div>
    </div>
  );
}

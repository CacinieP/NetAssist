export interface GeoIPInfo {
  status: "success" | "local" | "no_data" | "failed";
  ip: string;
  country: string;
  region: string;
  city: string;
  provider: string | null;
  queried_at: number;
  error_kind: string | null;
  error: string | null;
}

export interface HttpConnectivityResult {
  url: string;
  success: boolean;
  latency_ms: number;
  status_code: number | null;
  error: string | null;
  error_kind: string | null;
  proxy_policy: string;
  checked_at: number;
  address_family: string | null;
}

export interface DNSStats {
  avg_latency_ms: number;
  success_rate: number;
}

export function geoIPDisplay(geoip: GeoIPInfo | null | undefined, options: {
  enabled: boolean; loading?: boolean; error?: string | null; hasQueried?: boolean;
}) {
  if (!options.enabled) return { text: "已关闭", detail: "GeoIP 查询已关闭" };
  if (options.loading && !geoip) return { text: "正在获取位置…", detail: "GeoIP 查询中" };
  if (options.error) return { text: "查询失败", detail: options.error };
  if (!geoip && options.hasQueried === false) return { text: "未检测", detail: "开始检测后查询地区；开启地区显示不会自动发起查询" };
  if (!geoip) {
    return { text: "无地区数据", detail: "当前地址没有地区查询结果" };
  }
  const detail = [geoip.ip, geoip.provider && `来源：${geoip.provider}`,
    geoip.queried_at > 0 && `查询时间：${new Date(geoip.queried_at).toLocaleString()}`,
    geoip.error_kind && `错误类型：${geoip.error_kind}`, geoip.error].filter(Boolean).join(" · ");
  switch (geoip.status) {
    case "local": return { text: "本地网络（不查询地区）", detail };
    case "failed": return { text: "查询失败", detail };
    case "no_data": return { text: "无地区数据", detail };
    case "success": {
      const parts = [...new Set([geoip.country, geoip.region, geoip.city]
        .map(part => part?.trim()).filter(part => part && part !== "-" && part !== "未知"))];
      return { text: parts.join(" ") || "无地区数据", detail };
    }
  }
}

export function httpMetric(result: HttpConnectivityResult) {
  if (result.success) return { value: `${Math.round(result.latency_ms)}`, unit: "ms", error: null };
  const labels: Record<string, string> = {
    timeout: "超时", dns: "DNS 解析失败", connect: "连接失败", request: "请求失败", http_status: `HTTP ${result.status_code ?? "错误"}`,
    invalid_url: "地址无效", restricted_address: "目标受限",
  };
  return { value: labels[result.error_kind ?? ""] ?? "探测失败", unit: "", error: result.error || "HTTP 探测失败" };
}

export function dnsMetric(result: DNSStats) {
  if (result.success_rate <= 0) return { value: "解析失败", unit: "", error: "DNS 探测全部失败（成功率 0%）" };
  return { value: `${Math.round(result.avg_latency_ms)}`, unit: "ms",
    error: result.success_rate < 1 ? `部分解析失败（成功率 ${(result.success_rate * 100).toFixed(0)}%）` : null };
}

export function probeDescription(probe: { url: string | null; proxy_policy: string; checked_at: number | null; address_family?: string | null }) {
  return [probe.url && `目标：${probe.url}`, probe.proxy_policy === "no_proxy" ? "HTTP 客户端代理已禁用" : `代理策略：${probe.proxy_policy}`,
    probe.address_family && `实际连接：${probe.address_family.toUpperCase()}`,
    probe.checked_at && `检测时间：${new Date(probe.checked_at).toLocaleString()}`].filter(Boolean).join(" · ");
}

export const probeLimitations = "结果仅代表 NetAssist 到上述目标的探测。系统路由、VPN 或 TUN 仍可能接管流量；浏览器及其他应用可使用不同代理和出口。";

// Traffic monitoring shared types

export interface AppTraffic {
  name: string;
  pid: number;
  download_bytes: number;
  upload_bytes: number;
  current_download_bps: number;
  current_upload_bps: number;
}

export interface AppTrafficHistory {
  timestamp: number;
  download_bps: number;
  upload_bps: number;
}

export interface TrafficStats {
  download_bps: number;
  upload_bps: number;
  timestamp: number;
}

export interface CumulativeTraffic {
  total_download_bytes: number;
  total_upload_bytes: number;
  start_timestamp: number;
  end_timestamp: number;
  period: string;
}

export interface TrafficAlert {
  id: string;
  name: string;
  alert_type: string;
  threshold_bytes: number;
  period: string;
  enabled: boolean;
  triggered: boolean;
  last_triggered: number | null;
}

export interface AlertStatus {
  alert_id: string;
  triggered: boolean;
  current_value: number;
  threshold_value: number;
  percentage: number;
}

export type SortField = "name" | "download" | "upload" | "total";
export type SortOrder = "asc" | "desc";
export type Period = "day" | "week" | "month";

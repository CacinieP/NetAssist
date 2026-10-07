import { formatBytes, formatSpeed } from './formatUtils.ts';

type Availability = { traffic_available?: boolean };
export const formatAppSpeed = (app: Availability, value: number) => app.traffic_available === false ? '未提供' : formatSpeed(value);
export const formatAppBytes = (app: Availability, value: number) => app.traffic_available === false ? '未提供' : formatBytes(value);

export function trafficExportMetadata(hasAppMeasurements: boolean) {
  return {
    app_bytes_semantics: 'Legacy cumulative_* and summary total_*_bytes fields are the most recent nettop sampling interval bytes (approximately 1 second, exact interval in sample_seconds), not long-term cumulative traffic.',
    app_rates_semantics: 'Bytes per second. traffic_available=false means this process/platform has no measurement; legacy numeric zero fields are placeholders, not measured zero traffic.',
    app_measurements_available: hasAppMeasurements,
    interface_counters_semantics: 'Raw OS counters for the route-selected interface; not totals for exported_range. Null means unavailable.',
    recorded_history_semantics: 'Successful observed intervals only. Offline and failed samples are not reconstructed.',
  };
}

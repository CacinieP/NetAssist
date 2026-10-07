import type { GeoIPInfo } from '../../utils/diagnostics';
import { geoIPDisplay } from '../../utils/diagnostics';

export default function GeoIPLocation({ geoip, enabled, loading, error }: {
  geoip?: GeoIPInfo | null; enabled: boolean; loading?: boolean; error?: string | null;
}) {
  const location = geoIPDisplay(geoip, { enabled, loading, error });
  return <span title={location.detail} className="text-sm text-gray-600 dark:text-gray-300">
    {location.text}
    {enabled && (geoip || error) && <span className="block text-xs text-gray-500 dark:text-gray-400 break-all">{location.detail}</span>}
  </span>;
}

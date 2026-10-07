import assert from 'node:assert/strict';
import test from 'node:test';
import { formatAppSpeed, formatAppBytes, trafficExportMetadata } from '../src/utils/trafficPresentation.ts';

test('unsupported process traffic is distinct from measured zero', () => {
  assert.equal(formatAppSpeed({ traffic_available: false }, 0), '未提供');
  assert.equal(formatAppBytes({ traffic_available: false }, 0), '未提供');
  assert.equal(formatAppSpeed({ traffic_available: true }, 0), '0 B/s');
  assert.equal(formatAppBytes({ traffic_available: true }, 0), '0 B');
});

test('legacy export keys have explicit sample-interval, missing-data and counter-baseline semantics', () => {
  const metadata = trafficExportMetadata(false);
  assert.equal(metadata.app_measurements_available, false);
  assert.match(metadata.app_bytes_semantics, /not long-term cumulative/);
  assert.match(metadata.app_bytes_semantics, /sample_seconds/);
  assert.match(metadata.app_rates_semantics, /placeholders, not measured zero/);
  assert.match(metadata.interface_counters_semantics, /not totals for exported_range/);
});

import assert from 'node:assert/strict';
import test from 'node:test';
import { geoIPDisplay, dnsMetric, httpMetric, probeDescription, probeLimitations } from '../src/utils/diagnostics.ts';

const geo = { status: 'success', ip: '203.0.113.10', country: 'United States', region: '', city: '',
  provider: 'ipapi.co', queried_at: 1700000000000, error_kind: null, error: null };
const http = { url: 'https://example.org', success: true, latency_ms: 32.4, status_code: 200,
  error: null, error_kind: null, proxy_policy: 'no_proxy', checked_at: 1700000000000, address_family: 'ipv6' };

test('country-only GeoIP success is displayed with provider, queried IP and time', () => {
  const result = geoIPDisplay(geo, { enabled: true });
  assert.equal(result.text, 'United States');
  assert.match(result.detail, /203\.0\.113\.10/);
  assert.match(result.detail, /ipapi\.co/);
  assert.match(result.detail, /查询时间/);
});

test('GeoIP disabled, loading, local, no-data, and failed are distinct in shared formatter', () => {
  const texts = [geoIPDisplay(geo, { enabled: false }), geoIPDisplay(null, { enabled: true, loading: true }),
    ...['local', 'no_data', 'failed'].map(status => geoIPDisplay({ ...geo, status }, { enabled: true }))].map(value => value.text);
  assert.equal(new Set(texts).size, 5);
  assert.equal(texts[0], '已关闭');
  assert.equal(texts[2], '本地网络（不查询地区）');
  assert.equal(texts[3], '无地区数据');
  assert.equal(texts[4], '查询失败');
  assert.equal(geoIPDisplay(null, { enabled: true, error: 'invoke failed' }).text, '查询失败');
});

test('failed GeoIP carries error metadata and never presents public scope as a location', () => {
  const result = geoIPDisplay({ ...geo, status: 'failed', country: '-', error_kind: 'http_429', error: 'rate limited' }, { enabled: true });
  assert.match(result.detail, /http_429/);
  assert.match(result.detail, /rate limited/);
  assert.equal(result.text, '查询失败');
  assert.equal(geoIPDisplay(null, { enabled: true }).text, '无地区数据');
});

test('DNS all-failure is not a normal zero-ms measurement; partial success remains a warning', () => {
  assert.deepEqual(dnsMetric({ avg_latency_ms: 0, success_rate: 0 }), {
    value: '解析失败', unit: '', error: 'DNS 探测全部失败（成功率 0%）',
  });
  assert.match(dnsMetric({ avg_latency_ms: 13, success_rate: 0.5 }).error, /50%/);
  assert.deepEqual(dnsMetric({ avg_latency_ms: 0.4, success_rate: 1 }), { value: '0', unit: 'ms', error: null });
});

test('HTTP timeout, connect failure, status error and success retain distinct meaning', () => {
  assert.deepEqual(httpMetric(http), { value: '32', unit: 'ms', error: null });
  assert.equal(httpMetric({ ...http, success: false, error_kind: 'timeout', error: 'timed out' }).value, '超时');
  assert.equal(httpMetric({ ...http, success: false, error_kind: 'connect', error: 'refused' }).value, '连接失败');
  assert.equal(httpMetric({ ...http, success: false, error_kind: 'dns', error: 'unresolved host' }).value, 'DNS 解析失败');
  assert.equal(httpMetric({ ...http, success: false, status_code: 503, error_kind: 'http_status', error: '503' }).value, 'HTTP 503');
  assert.equal(httpMetric({ ...http, success: false, error_kind: null, error: 'unknown failure' }).value, '探测失败');
});

test('probe explanation includes target, explicit proxy policy, actual family, time and TUN limit', () => {
  const detail = probeDescription(http);
  assert.match(detail, /https:\/\/example\.org/);
  assert.match(detail, /HTTP 客户端代理已禁用/);
  assert.match(detail, /IPV6/);
  assert.match(detail, /检测时间/);
  assert.match(probeLimitations, /TUN/);
  assert.match(probeLimitations, /浏览器/);
});

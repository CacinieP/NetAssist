import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import {
  applyProgress,
  historyFixType,
  isStale,
  MANUAL_FIX_TYPES,
  problemCount,
  STALE_AFTER_MS,
} from '../src/utils/emergency.ts';

const item = status => ({ status, message: '', details: {}, duration_ms: 0 });
const result = (statuses, extra = {}) => ({
  overall_status: 'fail',
  network_connectivity: item(statuses[0]),
  ip_configuration: item(statuses[1]),
  dns_resolution: item(statuses[2]),
  network_quality: item(statuses[3]),
  recommendations: [{}, {}, {}],
  hints: [],
  timestamp: 0,
  ...extra,
});

test('problem count is the number of non-pass checks, not of repair actions', () => {
  assert.equal(problemCount(result(['pass', 'pass', 'fail', 'pass'], { problem_checks: ['dns_resolution'] })), 1);
  assert.equal(problemCount(result(['fail', 'fail', 'fail', 'warning'], {
    problem_checks: ['network_connectivity', 'ip_configuration', 'dns_resolution', 'network_quality'],
  })), 4);
  // Results exported by older versions have no problem_checks.
  assert.equal(problemCount(result(['fail', 'pass', 'warning', 'pass'])), 2);
});

test('progress only folds in events of the current run and known checks', () => {
  const event = (run_id, check, status = 'pass') => ({ run_id, check, status, message: '' });
  let progress = applyProgress({}, event(2, 'dns_resolution', 'fail'), 2);
  assert.deepEqual(progress, { dns_resolution: 'fail' });
  assert.equal(applyProgress(progress, event(1, 'network_quality'), 2), progress);
  assert.equal(applyProgress(progress, event(2, 'bogus'), 2), progress);
  progress = applyProgress(progress, event(2, 'network_quality', 'warning'), 2);
  assert.deepEqual(progress, { dns_resolution: 'fail', network_quality: 'warning' });
});

test('results become stale after five minutes', () => {
  assert.equal(isStale(1_000, 1_000 + STALE_AFTER_MS), false);
  assert.equal(isStale(1_000, 1_001 + STALE_AFTER_MS), true);
});

test('restart_network_service history entries show as reset_network_stack', () => {
  assert.equal(historyFixType('restart_network_service'), 'reset_network_stack');
  assert.equal(historyFixType('switch_dns'), 'switch_dns');
});

const flatten = (obj, prefix = '') => Object.entries(obj).flatMap(([k, v]) =>
  v && typeof v === 'object' ? flatten(v, `${prefix}${k}.`) : [`${prefix}${k}`]);

test('zh and en emergency strings have the same keys, including every manual tool', () => {
  const zh = JSON.parse(readFileSync(new URL('../src/locales/zh.json', import.meta.url))).emergency;
  const en = JSON.parse(readFileSync(new URL('../src/locales/en.json', import.meta.url))).emergency;
  assert.deepEqual(flatten(zh).sort(), flatten(en).sort());
  for (const fixType of MANUAL_FIX_TYPES) {
    assert.ok(zh.fix[fixType]?.title && zh.fix[fixType]?.msg, fixType);
  }
});

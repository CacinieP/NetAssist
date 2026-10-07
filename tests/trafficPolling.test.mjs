import assert from 'node:assert/strict';
import test from 'node:test';
import { createTrafficMonitor } from '../src/utils/trafficPolling.ts';
import { formatBytes, formatSpeed } from '../src/utils/formatUtils.ts';

const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function monitorWithTimers(fetchStats, recordPoint = async () => {}) {
  const timers = [];
  const monitor = createTrafficMonitor({ fetchStats, recordPoint,
    schedule: (callback, ms) => {
      const timer = { callback, ms, active: true };
      timers.push(timer);
      return () => { timer.active = false; };
    },
  });
  return { monitor, timers };
}
const sample = { download_bps: 1024, upload_bps: 3, timestamp: 1000 };

test('many page consumers share one physical traffic request and timer', async () => {
  const pending = deferred();
  let calls = 0;
  const { monitor, timers } = monitorWithTimers(() => { calls++; return pending.promise; });
  const app = [], dashboard = [], card = [];
  const stops = [monitor.subscribe(value => app.push(value)), monitor.subscribe(value => dashboard.push(value)), monitor.subscribe(value => card.push(value))];
  await flush();
  assert.equal(timers.length, 1);
  for (let i = 0; i < 5; i++) timers[0].callback();
  assert.equal(calls, 1);
  pending.resolve(sample);
  await flush();
  for (const states of [app, dashboard, card]) assert.deepEqual(states.at(-1).stats, sample);
  for (const stop of stops) stop();
  assert.equal(timers[0].active, false);
});

test('failed realtime reads clear old rates for every consumer', async () => {
  let fail = false;
  const { monitor, timers } = monitorWithTimers(async () => {
    if (fail) throw new Error('counter command failed');
    return sample;
  });
  const updates = [];
  const stop = monitor.subscribe(value => updates.push(value));
  await flush();
  assert.deepEqual(monitor.getSnapshot().stats, sample);
  fail = true;
  timers[0].callback();
  await flush();
  assert.equal(updates.at(-1).stats, null);
  assert.match(updates.at(-1).error, /counter command failed/);
  stop();
});

test('app-owned recorder survives page navigation and never receives cached rates', async () => {
  const recordedArgs = [];
  const { monitor, timers } = monitorWithTimers(async () => sample, async (...args) => { recordedArgs.push(args); });
  const stopApp = monitor.subscribe(() => {});
  const stopRecording = monitor.startRecording();
  let stopPage = monitor.subscribe(() => {});
  await flush();
  assert.equal(recordedArgs.length, 1); // first backend read establishes baseline
  stopPage();
  stopPage = monitor.subscribe(() => {}); // navigate from dashboard to settings
  const recorder = timers.find(timer => timer.ms === 5000);
  recorder.callback();
  await flush();
  assert.equal(timers.filter(timer => timer.ms === 5000).length, 1);
  assert.deepEqual(recordedArgs, [[], []]);
  stopPage(); stopRecording(); stopApp();
});

test('slow recorder stays single flight and does not start a queued write after owner unmount', async () => {
  const pending = deferred();
  let records = 0;
  const { monitor, timers } = monitorWithTimers(async () => sample, () => { records++; return pending.promise; });
  const stopRecording = monitor.startRecording();
  await flush();
  for (let i = 0; i < 6; i++) timers[0].callback();
  assert.equal(records, 1);
  stopRecording();
  pending.resolve();
  await flush();
  assert.equal(records, 1);
  assert.equal(timers[0].active, false);
});

test('Strict Mode stop/restart holds the old physical lock and ignores old sample', async () => {
  const old = deferred();
  let calls = 0;
  const { monitor } = monitorWithTimers(() => ++calls === 1 ? old.promise : Promise.resolve({ ...sample, timestamp: 2000 }));
  const stop = monitor.subscribe(() => {});
  await flush();
  stop();
  const updates = [];
  const stopAgain = monitor.subscribe(value => updates.push(value));
  await flush();
  assert.equal(calls, 1);
  old.resolve(sample);
  await flush();
  assert.equal(calls, 2);
  assert.equal(updates.some(value => value.stats?.timestamp === 1000), false);
  assert.equal(monitor.getSnapshot().stats.timestamp, 2000);
  stopAgain();
});

test('duplicate recording subscriptions cannot multiply OS-counter writes', async () => {
  let records = 0;
  const { monitor, timers } = monitorWithTimers(async () => sample, async () => { records++; });
  const stopA = monitor.startRecording(), stopB = monitor.startRecording();
  await flush();
  assert.equal(records, 1);
  assert.equal(timers.length, 1);
  stopA();
  assert.equal(timers[0].active, true);
  stopB();
  assert.equal(timers[0].active, false);
});

test('byte-per-second and byte totals retain byte units without bit multiplication', () => {
  assert.equal(formatSpeed(1024), '1.0 KB/s');
  assert.equal(formatBytes(1024), '1.0 KB');
  assert.equal(formatSpeed(5), '5 B/s');
  assert.equal(formatBytes(5), '5 B');
});

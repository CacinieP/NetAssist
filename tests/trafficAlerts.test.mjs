import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { create } from 'zustand';
import { createTrafficAlertMonitor, trafficAlertPreferences, alertStatusPresentation } from '../src/utils/trafficAlerts.ts';

const enabled = { ready: true, enabled: true };
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const status = (id, triggered, period = 1000) => ({ alert_id: id, period_start_timestamp: period, triggered,
  current_value: triggered ? 200 : 0, threshold_value: 100, percentage: triggered ? 200 : 0 });
function harness(check, notify) {
  const notifications = [], errors = [], timers = [];
  const monitor = createTrafficAlertMonitor({ check,
    notify: notify ?? (statuses => { notifications.push(statuses.map(s => `${s.alert_id}:${s.period_start_timestamp}`)); }),
    onError: error => errors.push(String(error)),
    schedule: (callback, ms) => {
      const timer = { callback, ms, active: true };
      timers.push(timer);
      return () => { timer.active = false; };
    },
  });
  return { monitor, notifications, errors, timers };
}

test('startup is a baseline, repeated statuses stay quiet, clearing permits one new notification', async () => {
  let result = [status('existing', true)];
  const { monitor, notifications } = harness(async () => result);
  monitor.configure(enabled);
  await monitor.refresh();
  await monitor.refresh();
  assert.deepEqual(notifications, []);
  result = [status('existing', false)];
  await monitor.refresh();
  result = [status('existing', true)];
  await monitor.refresh();
  await monitor.refresh();
  assert.deepEqual(notifications, [['existing:1000']]);
  monitor.stop();
});

test('newly created triggered alerts notify once, including same id in a new period', async () => {
  let result = [status('old', true)];
  const { monitor, notifications } = harness(async () => result);
  monitor.configure(enabled);
  await monitor.refresh();
  result = [status('old', true), status('new', true), status('below', false)];
  await monitor.refresh();
  result = [status('old', true, 2000), status('new', true), status('below', false)];
  await monitor.refresh();
  assert.deepEqual(notifications, [['new:1000'], ['old:2000']]);
  monitor.stop();
});

test('page-independent monitor keeps ticking with unchanged preferences', async () => {
  let calls = 0;
  const { monitor, notifications, timers } = harness(async () => [status('limit', ++calls > 1)]);
  monitor.configure(enabled);
  await monitor.refresh();
  // Navigating pages does not own or stop the monitor. Repeated settings-store
  // notifications also must not replace the timer or establish a new baseline.
  monitor.configure(enabled);
  monitor.configure(enabled);
  assert.equal(timers.length, 1);
  timers[0].callback();
  await flush();
  assert.deepEqual(notifications, [['limit:1000']]);
  monitor.stop();
});

test('failed checks preserve the last successful baseline and retry without duplicate alerts', async () => {
  let fail = true, result = [status('existing', true)];
  const { monitor, errors, notifications } = harness(async () => { if (fail) throw new Error('OS read failed'); return result; });
  monitor.configure(enabled);
  await monitor.refresh();
  assert.match(errors[0], /OS read failed/);
  fail = false;
  await monitor.refresh(); // first success is still only a baseline
  fail = true;
  await monitor.refresh();
  fail = false;
  await monitor.refresh();
  assert.deepEqual(notifications, []);
  result = [status('existing', true), status('new', true)];
  await monitor.refresh();
  assert.deepEqual(notifications, [['new:1000']]);
  monitor.stop();
});

test('slow checks cannot overlap; disabling invalidates the old result and re-enable takes a fresh baseline', async () => {
  const pending = deferred();
  let calls = 0;
  const { monitor, notifications, timers } = harness(() => ++calls === 1 ? pending.promise : Promise.resolve([status('new', true)]));
  monitor.configure(enabled);
  await flush();
  for (let i = 0; i < 5; i++) timers[0].callback();
  assert.equal(calls, 1);
  monitor.configure({ ready: true, enabled: false });
  monitor.configure(enabled);
  assert.equal(calls, 1);
  pending.resolve([status('old', true)]);
  await monitor.refresh();
  assert.equal(calls, 2);
  assert.deepEqual(notifications, []);
  monitor.stop();
});

test('re-enable does not replay events that happened while notifications were disabled', async () => {
  let result = [];
  const { monitor, notifications } = harness(async () => result);
  monitor.configure(enabled);
  await monitor.refresh();
  monitor.configure({ ready: true, enabled: false });
  result = [status('while-off', true)];
  monitor.configure(enabled);
  await monitor.refresh();
  assert.deepEqual(notifications, []);
  result = [status('while-off', false)];
  await monitor.refresh();
  result = [status('while-off', true)];
  await monitor.refresh();
  assert.deepEqual(notifications, [['while-off:1000']]);
  monitor.stop();
});

test('Strict Mode cleanup holds the physical lock and rejects the cancelled baseline', async () => {
  const pending = deferred();
  let calls = 0;
  const { monitor, notifications } = harness(() => ++calls === 1 ? pending.promise : Promise.resolve([status('current', true)]));
  monitor.configure(enabled);
  await flush();
  monitor.stop();
  monitor.configure(enabled);
  pending.resolve([status('outdated', true)]);
  await monitor.refresh();
  assert.equal(calls, 2);
  assert.deepEqual(notifications, []);
  monitor.stop();
});

test('Strict Mode replay after a successful baseline retains deduplication', async () => {
  const { monitor, notifications } = harness(async () => [status('already-triggered', true)]);
  monitor.configure(enabled);
  await monitor.refresh();
  monitor.stop();
  monitor.configure(enabled);
  await monitor.refresh();
  assert.deepEqual(notifications, []);
  monitor.stop();
});

test('a pending notification is invalidated immediately when preferences change', async () => {
  const permission = deferred();
  let result = [];
  const deliveries = [];
  const { monitor } = harness(async () => result, async (newlyTriggered, stillCurrent) => {
    await permission.promise;
    if (stillCurrent()) deliveries.push(newlyTriggered);
  });
  monitor.configure(enabled);
  await monitor.refresh();
  result = [status('new', true)];
  await monitor.refresh();
  monitor.configure({ ready: true, enabled: false });
  monitor.configure(enabled); // even a quick off/on cannot revive the old event
  permission.resolve();
  await flush();
  assert.deepEqual(deliveries, []);
  monitor.stop();
});

// Load the actual production functions with injected module dependencies. These
// checks exercise settings/permission awaits rather than a mirror state machine.
function settingsStore(invoke) {
  const source = stripTypeScriptTypes(fs.readFileSync(new URL('../src/store/settingsStore.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;\s*$/gm, '').replace('export const useSettingsStore', 'const useSettingsStore');
  return new Function('create', 'invoke', `${source}; return useSettingsStore;`)(create, invoke);
}
function notificationSender(dependencies) {
  const source = stripTypeScriptTypes(fs.readFileSync(new URL('../src/utils/notify.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;\s*$/gm, '').replace('export async function notify', 'async function notify');
  return new Function('sendNotification', 'isPermissionGranted', 'requestPermission', `${source}; return notify;`)(
    dependencies.sendNotification, dependencies.isPermissionGranted, dependencies.requestPermission,
  );
}

test('persisted disabled settings cannot be bypassed by default true while loading', async () => {
  const pending = deferred();
  const store = settingsStore(() => pending.promise);
  let calls = 0;
  const { monitor } = harness(async () => { calls++; return []; });
  const sync = () => monitor.configure(trafficAlertPreferences(store.getState()));
  sync();
  const unsubscribe = store.subscribe(sync);
  const load = store.getState().loadSettings();
  await flush();
  assert.equal(store.getState().settings.notify_traffic_limit, true);
  assert.equal(store.getState().hydrated, false);
  assert.equal(calls, 0);
  pending.resolve({ ...store.getState().settings, notify_traffic_limit: false });
  await load;
  await flush();
  assert.equal(store.getState().hydrated, true);
  assert.equal(calls, 0);
  unsubscribe(); monitor.stop();
});

test('failed settings loading stays unhydrated and cannot enable notifications', async () => {
  const store = settingsStore(async () => { throw new Error('settings unavailable'); });
  await store.getState().loadSettings();
  assert.equal(store.getState().hydrated, false);
  assert.equal(trafficAlertPreferences(store.getState()).ready, false);
  assert.match(store.getState().error, /settings unavailable/);
});

test('a successful initial settings load enables the monitor with a silent baseline', async () => {
  const store = settingsStore(async () => ({ ...store.getState().settings, notify_traffic_limit: true }));
  const { monitor, notifications } = harness(async () => [status('existing', true)]);
  const unsubscribe = store.subscribe(() => monitor.configure(trafficAlertPreferences(store.getState())));
  await store.getState().loadSettings();
  await monitor.refresh();
  assert.equal(store.getState().hydrated, true);
  assert.deepEqual(notifications, []);
  unsubscribe(); monitor.stop();
});

test('production notification helper rechecks after permission query and permission request', async () => {
  for (const phase of ['query', 'request']) {
    const pending = deferred();
    let allowed = true;
    const sent = [];
    const notify = notificationSender({
      sendNotification: value => sent.push(value),
      isPermissionGranted: () => phase === 'query' ? pending.promise : Promise.resolve(false),
      requestPermission: () => pending.promise,
    });
    const task = notify('title', 'body', () => allowed);
    await flush();
    allowed = false;
    pending.resolve(phase === 'query' ? true : 'granted');
    await task;
    assert.deepEqual(sent, [], phase);
  }
});

test('production notification helper sends once when still permitted, and skips all work when disabled', async () => {
  let permissions = 0;
  const sent = [];
  const notify = notificationSender({
    sendNotification: value => sent.push(value),
    isPermissionGranted: async () => { permissions++; return true; },
    requestPermission: async () => 'granted',
  });
  await notify('new threshold', 'one alert', () => false);
  assert.equal(permissions, 0);
  await notify('new threshold', 'one alert', () => true);
  assert.deepEqual(sent, [{ title: 'new threshold', body: 'one alert' }]);
});

test('unavailable alert status is visibly different from measured zero usage', () => {
  assert.deepEqual(alertStatusPresentation(undefined), { available: false, percentage: 0, percentText: '未获取', currentText: '—' });
  assert.deepEqual(alertStatusPresentation(status('zero', false)), { available: true, percentage: 0, percentText: '0%', currentText: '0 B' });
});

test('repeated backend rows count one actual trigger and ignore untriggered statuses', async () => {
  let result = [];
  const { monitor, notifications } = harness(async () => result);
  monitor.configure(enabled);
  await monitor.refresh();
  result = [status('new', true), status('new', true), status('other', false)];
  await monitor.refresh();
  assert.deepEqual(notifications, [['new:1000']]);
  monitor.stop();
});

test('interval changes preserve baseline and discard previous in-flight results', async () => {
  const pending = deferred();
  let calls = 0;
  const { monitor, notifications, timers } = harness(() => {
    calls++;
    if (calls === 1) return Promise.resolve([status('existing', true)]);
    if (calls === 2) return pending.promise;
    return Promise.resolve([status('existing', true), status('latest', true)]);
  });
  monitor.configure(enabled, 5000);
  await monitor.refresh();
  const slow = monitor.refresh();
  await flush();
  monitor.configure(enabled, 10000);
  assert.equal(calls, 2);
  assert.equal(timers[0].active, false);
  pending.resolve([status('stale', true)]);
  await slow;
  assert.equal(calls, 3);
  assert.deepEqual(notifications, [['latest:1000']]);
  monitor.stop();
});

import assert from 'node:assert/strict';
import test from 'node:test';
import { createPollingController, settlePair } from '../src/utils/polling.ts';

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

function harness(fetch) {
  const values = [], errors = [], loading = [], timers = [];
  const controller = createPollingController({ fetch,
    onValue: value => values.push(value), onError: error => errors.push(error),
    onLoading: value => loading.push(value),
    schedule: (callback, ms) => {
      const timer = { callback, ms, cancelled: false };
      timers.push(timer);
      return () => { timer.cancelled = true; };
    },
  });
  return { controller, values, errors, loading, timers };
}

test('slow requests coalesce timer ticks and manual refresh without overlap', async () => {
  const pending = deferred();
  let calls = 0;
  const { controller, timers, values } = harness(() => { calls++; return pending.promise; });
  controller.configure('current', 1000);
  await flush();
  for (let i = 0; i < 10; i++) timers[0].callback();
  const refreshOne = controller.refresh(), refreshTwo = controller.refresh();
  assert.equal(refreshOne, refreshTwo);
  assert.equal(calls, 1);
  pending.resolve('shared');
  await refreshOne;
  assert.deepEqual(values, ['shared']);
  assert.equal(calls, 1);
  controller.stop();
});

test('period and interval changes wait for old request and only commit latest period', async () => {
  const day = deferred(), month = deferred();
  const calls = [];
  const { controller, values, timers } = harness(period => {
    calls.push(period);
    return period === 'day' ? day.promise : month.promise;
  });
  controller.configure('day', 5000);
  await flush();
  controller.stop();
  controller.configure('week', 10000);
  controller.configure('month', 2000);
  assert.deepEqual(calls, ['day']);
  assert.equal(timers[0].cancelled, true);
  assert.equal(timers[1].cancelled, true);
  assert.equal(timers[2].ms, 2000);
  day.resolve('old day data');
  await flush();
  assert.deepEqual(values, []);
  assert.deepEqual(calls, ['day', 'month']);
  month.resolve('current month data');
  await controller.refresh();
  assert.deepEqual(values, ['current month data']);
  controller.stop();
});

test('failed old generations cannot overwrite new state or publish errors', async () => {
  const old = deferred();
  const { controller, values, errors } = harness(config => config === 'old' ? old.promise : Promise.resolve('new'));
  controller.configure('old', 1000);
  await flush();
  controller.configure('new', 1000);
  old.reject(new Error('outdated error'));
  await controller.refresh();
  assert.deepEqual(errors, []);
  assert.deepEqual(values, ['new']);
  controller.stop();
});

test('unmount invalidates data, errors, and loading writes without resetting the physical lock', async () => {
  const pending = deferred();
  const { controller, values, errors, loading, timers } = harness(() => pending.promise);
  controller.configure(true, 1000);
  await flush();
  const running = controller.refresh();
  controller.stop();
  const loadingAtUnmount = loading.slice();
  pending.reject(new Error('late error'));
  await running;
  assert.deepEqual(values, []);
  assert.deepEqual(errors, []);
  assert.deepEqual(loading, loadingAtUnmount);
  assert.equal(timers[0].cancelled, true);
});

test('partial Promise rejection holds lane until sibling settles, including configuration change', async () => {
  const ip = deferred();
  const calls = [];
  const { controller, values } = harness(includeGeoip => {
    calls.push(includeGeoip);
    return includeGeoip
      ? settlePair(Promise.reject('status error'), ip.promise)
      : settlePair(Promise.resolve('status'), Promise.resolve('geoip disabled'));
  });
  controller.configure(true, 1000);
  await flush();
  controller.configure(false, 1000);
  await flush();
  assert.deepEqual(calls, [true]);
  ip.resolve('stale geoip');
  await controller.refresh();
  assert.deepEqual(calls, [true, false]);
  assert.equal(values.length, 1);
  assert.equal(values[0].second.value, 'geoip disabled');
  controller.stop();
});

test('manual refresh during a GeoIP configuration change returns the final shared generation', async () => {
  const old = deferred();
  const { controller, values } = harness(config => config ? old.promise : Promise.resolve('disabled'));
  controller.configure(true, 1000);
  await flush();
  const manual = controller.refresh();
  controller.configure(false, 1000);
  old.resolve('stale enabled');
  await manual;
  assert.deepEqual(values, ['disabled']);
  controller.stop();
});

test('each completed refresh can be retried after a failure', async () => {
  let calls = 0;
  const { controller, values, errors } = harness(() => ++calls === 1 ? Promise.reject('temporary') : Promise.resolve('recovered'));
  controller.configure(null, 1000);
  await controller.refresh();
  assert.deepEqual(errors, ['temporary']);
  await controller.refresh();
  assert.deepEqual(values, ['recovered']);
  controller.stop();
});

test('configuration at request completion cannot get lost in a Promise-finalization gap', async () => {
  const old = deferred();
  const calls = [];
  const { controller, values } = harness(config => {
    calls.push(config);
    return config === 'old' ? old.promise : Promise.resolve('new result');
  });
  old.promise.then(() => { queueMicrotask(() => controller.configure('new', 1000)); });
  controller.configure('old', 1000);
  await flush();
  old.resolve('old result');
  await flush();
  assert.deepEqual(calls, ['old', 'new']);
  assert.equal(values.at(-1), 'new result');
  controller.stop();
});

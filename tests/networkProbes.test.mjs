import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { create } from 'zustand';
import { createNetworkProbeCoordinator, probePreferences, probeSummary } from '../src/utils/networkProbes.ts';
import { geoIPDisplay } from '../src/utils/diagnostics.ts';

const config = { ready: true, automatic: false, intervalMs: 5000, includeGeoip: false, dnsServer: '8.8.8.8' };
const activeCommands = ['get_network_status', 'get_ip_info', 'test_http_connectivity', 'test_dns'];
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const localIP = { ipv4: null, local_ipv4: '192.168.1.2', ipv6: '2001:db8::1', ipv6_interface: 'en0', public_ipv4_probe: { status: 'skipped' } };
const geoip = { status: 'success', ip: '203.0.113.8', country: 'Example', region: '', city: '', queried_at: 321, provider: 'mock', error: null, error_kind: null };
const publicIP = { ...localIP, ipv4: '203.0.113.8', ipv4_geoip: geoip, public_ipv4_probe: { status: 'success', checked_at: 123, cache_hit: true } };
const values = { get_local_ip_info_only: localIP, get_network_status: { status: 'normal', probes: [] }, get_ip_info: publicIP,
  test_http_connectivity: { success: true, latency_ms: 20, checked_at: 456 }, test_dns: { avg_latency_ms: 10, success_rate: 1 } };
function harness(implementation = async command => values[command]) {
  const calls = [], timers = [];
  const invoke = (command, args) => { calls.push({ command, args }); return implementation(command, args); };
  const schedule = (callback, ms) => {
    const timer = { callback, ms, active: true }; timers.push(timer);
    return () => { timer.active = false; };
  };
  const coordinator = createNetworkProbeCoordinator({ invoke, schedule, now: () => 1000 });
  return { coordinator, calls, timers, invoke, schedule,
    active: () => calls.filter(call => activeCommands.includes(call.command)),
    tick: async () => { timers.filter(t => t.active).forEach(t => t.callback()); await flush(); } };
}

test('startup, hydration, GeoIP/DNS/interval changes default to local commands only', async () => {
  const h = harness();
  h.coordinator.configure({ ...config, ready: false });
  await h.coordinator.refresh(); // unavailable settings cannot send manual/default probes
  await h.tick();
  h.coordinator.configure(config);
  h.coordinator.configure({ ...config, includeGeoip: true });
  h.coordinator.configure({ ...config, includeGeoip: true, dnsServer: '1.1.1.1', intervalMs: 1000 });
  await h.tick();
  assert.deepEqual(h.calls.map(c => c.command), ['get_local_ip_info_only', 'get_local_ip_info_only']);
  assert.deepEqual(h.active(), []);
  const data = h.coordinator.getSnapshot();
  assert.equal(data.loading, false); assert.equal(data.hasRun, false); assert.equal(data.lastCheckedAt, null);
  assert.equal(data.ipInfo.local_ipv4, localIP.local_ipv4); assert.equal(data.ipInfo.ipv4, undefined);
  assert.equal(probeSummary(data), '未检测');
  assert.equal(geoIPDisplay(null, { enabled: true, hasQueried: data.geoipQueried }).text, '未检测');
  h.coordinator.stop();
});

test('a manual click runs exactly one shared complete round and retains evidence after passive refresh', async () => {
  const h = harness();
  h.coordinator.configure({ ...config, includeGeoip: true });
  await flush();
  const first = h.coordinator.refresh(), second = h.coordinator.refresh();
  assert.equal(first, second);
  await first;
  assert.deepEqual(h.active(), [
    { command: 'get_network_status', args: undefined },
    { command: 'get_ip_info', args: { includeGeoip: true } },
    { command: 'test_http_connectivity', args: { url: null } },
    { command: 'test_dns', args: { server: '8.8.8.8' } },
  ]);
  await h.tick();
  const data = h.coordinator.getSnapshot();
  assert.equal(data.ipInfo.ipv4, publicIP.ipv4); assert.deepEqual(data.ipInfo.ipv4_geoip, geoip);
  assert.equal(data.ipInfo.public_ipv4_probe.checked_at, 123);
  assert.equal(data.lastCheckedAt, 1000); assert.equal(data.geoipQueried, true);
  assert.equal(h.active().length, 4);
  h.coordinator.stop();
});

test('a changed local IPv6 cannot inherit the previous address GeoIP', async () => {
  let current = localIP;
  const h = harness(async command => command === 'get_local_ip_info_only' ? current : command === 'get_ip_info' ? { ...publicIP, ipv6_geoip: { ...geoip, ip: localIP.ipv6 } } : values[command]);
  h.coordinator.configure({ ...config, includeGeoip: true });
  await h.coordinator.refresh();
  assert.equal(h.coordinator.getSnapshot().ipInfo.ipv6_geoip.ip, localIP.ipv6);
  current = { ...localIP, ipv6: '2001:db8::2' };
  await h.tick();
  assert.equal(h.coordinator.getSnapshot().ipInfo.ipv6_geoip, null);
  assert.equal(h.coordinator.getSnapshot().ipInfo.ipv4, publicIP.ipv4);
  h.coordinator.stop();
});

test('automatic probes require readiness and explicit opt-in, and unchanged preferences do not restart them', async () => {
  const h = harness();
  h.coordinator.configure({ ...config, ready: false, automatic: true });
  await h.tick(); assert.equal(h.active().length, 0);
  h.coordinator.configure({ ...config, automatic: true });
  await flush(); assert.equal(h.active().length, 4);
  h.coordinator.configure({ ...config, automatic: true });
  await flush(); assert.equal(h.active().length, 4);
  await h.tick(); assert.equal(h.active().length, 8);
  h.coordinator.configure(config);
  await h.tick(); assert.equal(h.active().length, 8);
  assert.equal(h.coordinator.getSnapshot().stale, true);
  h.coordinator.stop();
});

test('disable during slow auto work discards values/errors and does not start a replacement', async () => {
  const pending = deferred();
  const h = harness(command => command === 'get_ip_info' ? pending.promise : command === 'test_dns' ? Promise.reject('old DNS error') : Promise.resolve(values[command]));
  h.coordinator.configure({ ...config, automatic: true });
  await flush();
  h.coordinator.configure(config);
  const afterDisable = h.coordinator.getSnapshot();
  pending.resolve(publicIP); await flush(); await h.tick();
  const data = h.coordinator.getSnapshot();
  assert.equal(h.active().length, 4); assert.equal(data.hasRun, false); assert.equal(data.dnsError, null);
  assert.equal(data.lastCheckedAt, null); assert.equal(data.loading, false);
  assert.equal(h.coordinator.isResultCurrent(afterDisable.resultId), false);
  h.coordinator.stop();
});

test('partial rejection holds all four physical requests; manual retry after config change waits and uses latest config', async () => {
  const pending = deferred(); let ipCalls = 0;
  const h = harness(command => command === 'get_ip_info' && ++ipCalls === 1 ? pending.promise : command === 'test_dns' ? Promise.reject('DNS unavailable') : Promise.resolve(values[command]));
  h.coordinator.configure(config);
  const first = h.coordinator.refresh(); await flush();
  h.coordinator.configure({ ...config, includeGeoip: true, dnsServer: '1.1.1.1' });
  const second = h.coordinator.refresh();
  assert.equal(first, second); assert.equal(h.active().length, 4);
  pending.resolve({ ipv4: 'stale' }); await second;
  assert.equal(h.active().length, 8);
  assert.deepEqual(h.active().slice(-4).map(c => c.args), [undefined, { includeGeoip: true }, { url: null }, { server: '1.1.1.1' }]);
  assert.equal(h.coordinator.getSnapshot().ipInfo.ipv4, publicIP.ipv4);
  assert.equal(h.coordinator.getSnapshot().dnsError, 'DNS unavailable');
  h.coordinator.stop();
});

test('old result and pending notifications become invalid when settings change; stopped work cannot publish', async () => {
  const h = harness(); h.coordinator.configure(config); await h.coordinator.refresh();
  const original = h.coordinator.getSnapshot();
  assert.equal(h.coordinator.isResultCurrent(original.resultId), true);
  h.coordinator.configure({ ...config, includeGeoip: true });
  assert.equal(h.coordinator.isResultCurrent(original.resultId), false);
  assert.equal(h.coordinator.getSnapshot().stale, true);
  assert.equal(h.coordinator.getSnapshot().geoipQueried, false);
  assert.equal(h.coordinator.getSnapshot().ipInfo.ipv4_geoip, null);
  assert.match(probeSummary(h.coordinator.getSnapshot()), /设置已变更/);
  h.coordinator.stop();
  await h.coordinator.refresh(); assert.equal(h.active().length, 4);

  const pending = deferred();
  const slow = harness(command => command === 'get_ip_info' ? pending.promise : Promise.resolve(values[command]));
  slow.coordinator.configure(config); const task = slow.coordinator.refresh(); await flush();
  slow.coordinator.stop(); pending.resolve(publicIP); await task;
  assert.equal(slow.coordinator.getSnapshot().hasRun, false);
  assert.equal(slow.coordinator.getSnapshot().loading, false);
});

// Evaluate the production hook and store with injected React lifecycle and Tauri IO.
// The hook itself performs ownership, subscription and saved-settings gating.
function settingsStore(invoke) {
  const source = fs.readFileSync(new URL('../src/store/settingsStore.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\n/gm, '').replace('export const useSettingsStore', 'const useSettingsStore');
  return new Function('create', 'invoke', `${stripTypeScriptTypes(source)}; return useSettingsStore;`)(create, invoke);
}
function hookHarness(store, h) {
  let effects = [];
  const source = fs.readFileSync(new URL('../src/hooks/useNetworkData.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\n/gm, '').replace(/^export type .*;\n/gm, '').replaceAll('export const ', 'const ').replaceAll('export function ', 'function ');
  const hook = new Function('useState', 'useEffect', 'invoke', 'useSettingsStore', 'createNetworkProbeCoordinator', 'probePreferences',
    `${stripTypeScriptTypes(source)}; return useNetworkData;`)(
      initial => [typeof initial === 'function' ? initial() : initial, () => {}], callback => effects.push(callback),
      h.invoke, store, deps => createNetworkProbeCoordinator({ ...deps, schedule: h.schedule, now: () => 1000 }), probePreferences);
  return { mount(options) {
    effects = []; const result = hook(options); const cleanups = effects.map(effect => effect());
    return { result, unmount: () => cleanups.forEach(cleanup => cleanup?.()) };
  } };
}

test('production hook: App ownership, page mounts/unmounts and settings draft changes cause zero active probes', async () => {
  const store = settingsStore(async () => ({ ...store.getState().settings, show_geoip: true }));
  const h = harness(); const hooks = hookHarness(store, h);
  const owner = hooks.mount({ owner: true }); await store.getState().loadSettings(); await flush();
  for (let i = 0; i < 5; i++) {
    const page = hooks.mount(); await flush(); page.unmount();
  }
  store.getState().setSettings({ show_geoip: false, primary_dns: '1.1.1.1', refresh_interval_secs: 2 });
  await flush(); await h.tick();
  assert.equal(h.active().length, 0);
  await owner.result.refresh(); assert.equal(h.active().length, 4);
  owner.unmount();
});

test('production hook/store: failed load/save and uncommitted draft cannot enable auto; accepted save does', async () => {
  let loadFails = true, save = deferred();
  const store = settingsStore(async command => {
    if (command === 'get_settings') { if (loadFails) throw new Error('load failed'); return { ...store.getState().settings, auto_probe_enabled: false }; }
    if (command === 'update_settings') return save.promise;
  });
  const h = harness(); const hooks = hookHarness(store, h); const owner = hooks.mount({ owner: true });
  await store.getState().loadSettings(); await h.tick(); assert.equal(h.active().length, 0);
  loadFails = false; await store.getState().loadSettings();
  const draft = { ...store.getState().settings, auto_probe_enabled: true };
  const failedSave = store.getState().saveSettings(draft); await flush(); assert.equal(h.active().length, 0);
  save.resolve(false); await failedSave; await h.tick(); assert.equal(h.active().length, 0);
  save = deferred(); const acceptedSave = store.getState().saveSettings(draft);
  await flush(); assert.equal(h.active().length, 0);
  save.resolve(true); await acceptedSave; await flush(); assert.equal(h.active().length, 4);
  const disabling = store.getState().saveSettings({ ...draft, auto_probe_enabled: false });
  await disabling; await h.tick(); assert.equal(h.active().length, 4);
  owner.unmount();
});

test('production hook survives StrictMode stop/remount in manual mode without outbound startup traffic', async () => {
  const store = settingsStore(async () => ({ ...store.getState().settings }));
  await store.getState().loadSettings();
  const h = harness(); const hooks = hookHarness(store, h);
  const first = hooks.mount({ owner: true }); first.unmount();
  const second = hooks.mount({ owner: true }); await h.tick();
  assert.equal(h.active().length, 0);
  await second.result.refresh(); assert.equal(h.active().length, 4);
  second.unmount();
});

test('page code has no remaining direct active commands; repair diagnosis stays an explicit action', () => {
  for (const file of ['App.tsx', 'components/Dashboard/Dashboard.tsx', 'components/Dashboard/IPInfoCard.tsx', 'components/Dashboard/NetworkStatus.tsx', 'components/ConnectionManager/ConnectionManager.tsx', 'components/Settings/Settings.tsx']) {
    const source = fs.readFileSync(new URL(`../src/${file}`, import.meta.url), 'utf8');
    assert.doesNotMatch(source, /invoke(?:<[^\n]*>)?\(['"](?:get_ip_info|get_network_status|test_dns|test_http_connectivity|query_geoip|run_diagnostics)['"]/, file);
  }
  const emergency = fs.readFileSync(new URL('../src/components/EmergencyKit/EmergencyKit.tsx', import.meta.url), 'utf8');
  assert.match(emergency, /onClick=\{startDiagnosis\}/);
  assert.match(emergency, /const applyFix = async/);
});

test('failed public IP refresh clears previous active fields and preserves only passive local addresses', async () => {
  let fail = false;
  const h = harness(command => command === 'get_ip_info' && fail ? Promise.reject('public service unavailable') : Promise.resolve(values[command]));
  h.coordinator.configure(config); await h.coordinator.refresh();
  assert.equal(h.coordinator.getSnapshot().ipInfo.ipv4, publicIP.ipv4);
  fail = true; await h.coordinator.refresh();
  const data = h.coordinator.getSnapshot();
  assert.equal(data.ipInfo.ipv4, undefined); assert.equal(data.ipInfo.ipv4_geoip, undefined);
  assert.equal(data.ipInfo.public_ipv4_probe, undefined); assert.equal(data.ipInfo.local_ipv4, localIP.local_ipv4);
  assert.equal(data.ipError, 'public service unavailable');
  h.coordinator.stop();
});

test('a completed new round invalidates an old abnormal notification awaiting permission in the same settings generation', async () => {
  let status = 'abnormal'; const permission = deferred(), sent = [];
  const h = harness(async command => command === 'get_network_status' ? { status } : values[command]);
  h.coordinator.configure(config); await h.coordinator.refresh();
  const abnormalId = h.coordinator.getSnapshot().resultId;
  const source = stripTypeScriptTypes(fs.readFileSync(new URL('../src/utils/notify.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;\s*$/gm, '').replace('export async function notify', 'async function notify');
  const notify = new Function('sendNotification', 'isPermissionGranted', 'requestPermission', `${source}; return notify;`)(
    value => sent.push(value), () => permission.promise, async () => 'granted');
  const task = notify('abnormal', 'old round', () => h.coordinator.isResultCurrent(abnormalId));
  await flush(); status = 'normal'; await h.coordinator.refresh();
  assert.notEqual(h.coordinator.getSnapshot().resultId, abnormalId);
  assert.equal(h.coordinator.isResultCurrent(abnormalId), false);
  permission.resolve(true); await task; assert.deepEqual(sent, []);
  h.coordinator.stop();
});

test('disabled automatic mode stays passive through subsequent interval, GeoIP and DNS edits', async () => {
  const h = harness(); h.coordinator.configure({ ...config, automatic: true }); await flush();
  h.coordinator.configure(config);
  h.coordinator.configure({ ...config, intervalMs: 1000, includeGeoip: true, dnsServer: '1.1.1.1' });
  await h.tick(); await h.tick();
  assert.equal(h.active().length, 4);
  assert.equal(h.coordinator.getSnapshot().stale, true);
  h.coordinator.stop();
});

// Deferred actual acquisition owner with controlled HID API/EventTarget endpoints.
// This does not open hardware, decode descriptors or claim playable HID integration.
import assert from "node:assert/strict";
import test from "node:test";
import { HidInputOwner } from "./hid-input.mjs";

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function attempt(action) {
  const result = { state: "pending" };
  let value;
  try { value = action(); } catch (error) { value = Promise.reject(error); }
  result.done = Promise.resolve(value).then(
    value => { result.state = "fulfilled"; result.value = value; },
    error => { result.state = "rejected"; result.error = error; },
  );
  return result;
}
async function flush() { for (let i = 0; i < 48; i++) await Promise.resolve(); }
async function rejected(result, pattern) {
  await result.done;
  assert.equal(result.state, "rejected");
  assert.ok(result.error instanceof Error);
  assert.ok(result.error.message.length > 0);
  if (pattern !== undefined) {
    const messages = [];
    for (let cause = result.error; cause instanceof Error && messages.length < 8; cause = cause.cause) messages.push(cause.message);
    assert.match(messages.join("\n"), pattern);
  }
  return result.error;
}
class Events {
  listeners = new Map();
  addEventListener(kind, listener) {
    if (!this.listeners.has(kind)) this.listeners.set(kind, new Set());
    this.listeners.get(kind).add(listener);
  }
  removeEventListener(kind, listener) { this.listeners.get(kind)?.delete(listener); }
  emit(kind, fields = {}) {
    const event = { type: kind, target: this, ...fields };
    for (const listener of [...(this.listeners.get(kind) ?? [])]) listener(event);
  }
  count(kind) { return this.listeners.get(kind)?.size ?? 0; }
}
function device(name, faults = {}) {
  return Object.assign(new Events(), {
    productName: name, opened: faults.opened ?? false, opens: 0, closes: 0,
    async open() {
      this.opens++;
      if (faults.openGate) await faults.openGate.promise;
      if (faults.openError) throw faults.openError;
      this.opened = true;
    },
    async close() {
      this.closes++;
      if (faults.closeGate) await faults.closeGate.promise;
      if (faults.closeError) throw faults.closeError;
      this.opened = false;
    },
  });
}
function rig(settings = {}) {
  const hid = Object.assign(new Events(), {
    authorized: settings.authorized ?? [], selected: settings.selected ?? [], gets: 0, requests: [],
    getDevices() { this.gets++; return settings.getGate?.promise ?? Promise.resolve(this.authorized); },
    requestDevice(options) {
      this.requests.push(options);
      return settings.requestGate?.promise ?? Promise.resolve(this.selected);
    },
  });
  const reports = [], disconnects = [], errors = [];
  let calls = 0, sequence = settings.sequence ?? 9007199254740993n;
  const config = {
    hid,
    nextSequence() {
      calls++;
      settings.beforeSequence?.();
      if (settings.sequenceError) throw settings.sequenceError;
      if (Object.hasOwn(settings, "sequenceValue")) return settings.sequenceValue;
      return sequence++;
    },
    onReport(report) { reports.push(report); if (settings.reportError) throw settings.reportError; },
    onDisconnect(value) { disconnects.push(value); if (settings.disconnectError) throw settings.disconnectError; },
    onError(error) { errors.push(error); if (settings.errorCallbackError) throw settings.errorCallbackError; },
    ...(settings.options ?? {}),
  };
  return { hid, config, owner: new HidInputOwner(config), reports, disconnects, errors,
    get sequenceCalls() { return calls; }, nextShared() { return config.nextSequence(); } };
}
function report(hidDevice, fields = {}) {
  return { device: hidDevice, timeStamp: 1234.125, reportId: 0x7f,
    data: new DataView(Uint8Array.from([0x7f, 0, 255]).buffer), ...fields };
}

test("authorized devices retain full source identity and original report subviews before sharing acquisition sequence", async () => {
  const first = device("first"), second = device("second");
  const backing = Uint8Array.from([99, 0x7f, 0, 255, 99]);
  const h = rig({ authorized: [first, first, second], beforeSequence: () => backing.fill(88),
    options: { firstSource: 18446744073709551614n } });
  const snapshot = await h.owner.connectAuthorized();
  assert.equal(h.hid.gets, 1);
  assert.equal(h.hid.requests.length, 0, "authorized discovery does not open a permission chooser");
  assert.deepEqual(snapshot.map(entry => entry.source), [18446744073709551614n, 18446744073709551615n]);
  assert.equal(snapshot[0].device, first);
  assert.equal(snapshot[1].device, second);
  assert.ok(Object.isFrozen(snapshot) && snapshot.every(Object.isFrozen));
  assert.equal(first.opens, 1); assert.equal(second.opens, 1);
  await h.owner.connectAuthorized();
  assert.equal(first.opens, 1); assert.equal(first.count("inputreport"), 1);
  first.emit("inputreport", report(first, { data: new DataView(backing.buffer, 1, 3) }));
  assert.equal(h.sequenceCalls, 1);
  assert.deepEqual(h.reports[0], { kind: "hid", hostNs: 1234125000n, source: 18446744073709551614n,
    sequence: 9007199254740993n, reportId: 0x7f, data: Uint8Array.from([0x7f, 0, 255]) });
  assert.ok(Object.isFrozen(h.reports[0]));
  assert.notEqual(h.reports[0].data.buffer, backing.buffer);
  assert.deepEqual(Array.from(backing), [88, 88, 88, 88, 88], "native payload was copied before invoking the shared sequence supplier");
  assert.deepEqual(Array.from(h.reports[0].data), [0x7f, 0, 255]);
  assert.equal(h.nextShared(), 9007199254740994n, "the host can allocate an intervening keyboard/touch sequence");
  second.emit("inputreport", report(second, { timeStamp: 1234.25, reportId: 0, data: new DataView(new ArrayBuffer(0)) }));
  assert.deepEqual(h.reports[1], { kind: "hid", hostNs: 1234250000n, source: 18446744073709551615n,
    sequence: 9007199254740995n, reportId: 0, data: new Uint8Array() });
  const extra = device("identity exhausted"); h.hid.authorized = [extra];
  const exhaustion = await rejected(attempt(() => h.owner.connectAuthorized()), /source|identit/i);
  assert.equal(h.owner.failure, exhaustion);
  await h.owner.close();
  assert.equal(extra.opens, 0);
  assert.equal(first.closes, 1); assert.equal(second.closes, 1);
  assert.equal(snapshot[0].source, 18446744073709551614n, "older immutable metadata cannot be rewritten by cleanup");
  for (const bad of [
    { reportId: -1 }, { reportId: 256 }, { reportId: 1.5 }, { reportId: "1" },
    { timeStamp: -1 }, { timeStamp: NaN }, { timeStamp: "1234" },
    { data: new Uint8Array(1) }, { data: new DataView(new ArrayBuffer(5)) }, { device: device("foreign") },
  ]) {
    const d = device("bounded");
    const invalid = rig({ authorized: [d], options: { maxReportBytes: 4 } });
    await invalid.owner.connectAuthorized();
    d.emit("inputreport", report(d, bad));
    assert.equal(invalid.sequenceCalls, 0, "untrusted report fields must fail before consuming shared sequence");
    assert.equal(invalid.reports.length, 0);
    assert.equal(invalid.errors.length, 1);
    assert.equal(invalid.owner.failure, invalid.errors[0]);
    await invalid.owner.close();
    assert.equal(d.closes, 1);
  }
});

test("permission acquisition preserves the synchronous gesture and refuses busy invalid capacity or borrowed handles without fallback", async () => {
  const chooser = deferred(), d = device("chosen");
  const h = rig({ requestGate: chooser });
  const filters = [{ vendorId: 0xffffffff, productId: 0xffff, usagePage: 1, usage: 5 }, { vendorId: 65536 }];
  const choosing = attempt(() => h.owner.requestDevices(filters));
  assert.equal(h.hid.requests.length, 1, "requestDevice must be invoked before the caller loses its gesture");
  filters[0].vendorId = 9; filters.push({ vendorId: 1 });
  assert.deepEqual(h.hid.requests[0], { filters: [{ vendorId: 0xffffffff, productId: 0xffff, usagePage: 1, usage: 5 }, { vendorId: 65536 }] });
  await rejected(attempt(() => h.owner.connectAuthorized()), /busy|progress|pending/i);
  await rejected(attempt(() => h.owner.requestDevices()), /busy|progress|pending/i);
  assert.equal(h.hid.gets, 0); assert.equal(h.hid.requests.length, 1);
  assert.equal(h.owner.failure, null);
  chooser.resolve([d, d]); await choosing.done;
  assert.equal(choosing.state, "fulfilled");
  assert.equal(d.opens, 1);
  for (const invalid of [null, {}, Array(1), Array(17).fill({ vendorId: 1 }), [{}], [{ productId: 1 }], [{ usage: 1 }],
    [{ vendorId: -1 }], [{ vendorId: 0x100000000 }], [{ vendorId: 1, productId: 65536 }], [{ usagePage: 65536 }],
    [{ usagePage: 1.5 }], [{ vendorId: "1" }], [{ serialNumber: "private" }]]) {
    await rejected(attempt(() => h.owner.requestDevices(invalid)));
  }
  assert.equal(h.hid.requests.length, 1);
  assert.equal(h.owner.failure, null);
  await h.owner.requestDevices([]);
  assert.deepEqual(h.hid.requests[1], { filters: [] }, "an empty filter list is valid; an empty filter dictionary is not");
  assert.equal(d.opens, 1);
  await h.owner.close();
  assert.equal(d.closes, 1);
  await rejected(attempt(() => h.owner.requestDevices()), /closed/i);
  const a = device("a"), b = device("b");
  const capacity = rig({ authorized: [a, b], options: { maxDevices: 1 } });
  await rejected(attempt(() => capacity.owner.connectAuthorized()), /capacity|limit|maximum|at most/i);
  await capacity.owner.close();
  assert.equal(a.opens + b.opens, 0, "distinct selected capacity is checked before opening interfaces");
  const owned = device("owner"), borrowed = device("external", { opened: true });
  const borrow = rig({ authorized: [owned, borrowed] });
  await rejected(attempt(() => borrow.owner.connectAuthorized()), /device|interface|open/i);
  await borrow.owner.close();
  assert.equal(borrowed.opens, 0); assert.equal(borrowed.closes, 0);
  assert.equal(owned.closes, owned.opens, "only successfully owned openings may require cleanup");
  for (const options of [{ maxDevices: 0 }, { maxDevices: 17 }, { maxReportBytes: 0 }, { maxReportBytes: 1025 },
    { firstSource: 2n }, { firstSource: 3 }, { firstSource: 18446744073709551616n }, { nextSequence: null },
    { hid: null }, { hid: {} }, { onReport: null }, { onDisconnect: null }, { onError: null }]) {
    assert.throws(() => new HidInputOwner({ ...h.config, ...options }), Error);
  }
});

test("late opens disconnects and callback or close failures release only owned handles with no stale report resurrection", async () => {
  const discovery = deferred(), neverOpened = device("late discovery");
  const waiting = rig({ getGate: discovery });
  const acquisition = attempt(() => waiting.owner.connectAuthorized());
  const closingPromise = waiting.owner.close(), closing = attempt(() => closingPromise);
  assert.equal(waiting.owner.close(), closingPromise);
  assert.equal(waiting.owner.closed, true);
  await flush(); assert.equal(closing.state, "pending");
  discovery.resolve([neverOpened]); await acquisition.done; await closing.done;
  assert.equal(acquisition.state, "rejected"); assert.equal(closing.state, "fulfilled");
  assert.equal(neverOpened.opens, 0); assert.equal(neverOpened.closes, 0);
  const openGate = deferred(), late = device("late owned open", { openGate });
  const pending = rig({ authorized: [late] });
  const opening = attempt(() => pending.owner.connectAuthorized());
  await flush(); assert.equal(late.opens, 1);
  const stopped = attempt(() => pending.owner.close());
  assert.equal(pending.hid.count("disconnect"), 0);
  openGate.resolve(); await opening.done; await stopped.done;
  assert.equal(opening.state, "rejected"); assert.equal(stopped.state, "fulfilled");
  assert.equal(late.closes, 1); assert.equal(late.count("inputreport"), 0);
  assert.deepEqual(pending.owner.devices, []);
  const one = device("first accepted"), bad = device("second refused", { openError: new Error("open rejected") });
  const partial = rig({ authorized: [one, bad] });
  const error = await rejected(attempt(() => partial.owner.connectAuthorized()), /device|interface|open/i);
  assert.equal(partial.owner.failure, error);
  await partial.owner.close();
  assert.equal(one.opens, 1); assert.equal(one.closes, 1); assert.equal(bad.closes, 0);
  const d = device("disconnect"), h = rig({ authorized: [d] });
  await h.owner.connectAuthorized();
  const oldReport = [...d.listeners.get("inputreport")][0];
  h.hid.emit("disconnect", { device: device("unowned"), timeStamp: NaN });
  assert.equal(h.disconnects.length, 0);
  h.hid.emit("disconnect", { device: d, timeStamp: 2500.125 });
  assert.deepEqual(h.disconnects, [{ source: 3n, hostNs: 2500125000n }]);
  assert.deepEqual(h.owner.devices, []);
  assert.equal(d.count("inputreport"), 0);
  oldReport(report(d));
  assert.equal(h.sequenceCalls, 0); assert.equal(h.reports.length, 0);
  await flush();
  await h.owner.connectAuthorized();
  assert.equal(h.owner.devices[0].source, 4n, "retired identities cannot be recycled after disconnect");
  oldReport(report(d));
  assert.equal(h.sequenceCalls, 0, "a queued callback from the retired entry cannot reach the new owner for the same device");
  d.emit("inputreport", report(d, { timeStamp: 3000 }));
  assert.equal(h.reports.length, 1);
  assert.equal(h.reports[0].source, 4n);
  await h.owner.close();
  assert.equal(d.opens, 2); assert.equal(d.closes, 2);
  oldReport(report(d));
  assert.equal(h.reports.length, 1);
  const interrupted = device("disconnect during sequence");
  let reentrant;
  reentrant = rig({ authorized: [interrupted], beforeSequence() {
    reentrant.hid.emit("disconnect", { device: interrupted, timeStamp: 1234.25 });
  } });
  await reentrant.owner.connectAuthorized();
  interrupted.emit("inputreport", report(interrupted));
  assert.equal(reentrant.sequenceCalls, 1);
  assert.deepEqual(reentrant.disconnects, [{ source: 3n, hostNs: 1234250000n }]);
  assert.equal(reentrant.reports.length, 0, "removal during the shared sequence callback must prevent delivery from the retired entry");
  assert.equal(reentrant.errors.length, 0);
  await reentrant.owner.close();
  assert.equal(interrupted.closes, 1);
  for (const settings of [{ sequenceValue: 1 }, { sequenceValue: -1n }, { sequenceValue: 18446744073709551616n },
    { sequenceError: new Error("sequence failed") }, { reportError: new Error("consumer failed") }]) {
    const d = device("callback"); const h = rig({ authorized: [d], ...settings });
    await h.owner.connectAuthorized(); d.emit("inputreport", report(d));
    assert.equal(h.errors.length, 1); assert.equal(h.owner.failure, h.errors[0]);
    assert.equal(h.owner.closed, true);
    await h.owner.close(); assert.equal(d.closes, 1);
  }
  const d2 = device("disconnect callback"), callback = rig({ authorized: [d2], disconnectError: new Error("disconnect consumer failed") });
  await callback.owner.connectAuthorized(); callback.hid.emit("disconnect", { device: d2, timeStamp: 4 });
  assert.equal(callback.errors.length, 1); await callback.owner.close(); assert.equal(d2.closes, 1);
  const nativeCloseError = new Error("native handle close failed");
  const broken = device("cleanup", { closeError: nativeCloseError });
  const cleanup = rig({ authorized: [broken] }); await cleanup.owner.connectAuthorized();
  const cleanupError = await rejected(attempt(() => cleanup.owner.close()), /close|cleanup/i);
  assert.equal(cleanup.errors.length, 1);
  assert.equal(cleanup.owner.failure, cleanupError);
  assert.equal(broken.closes, 1);
  await rejected(attempt(() => cleanup.owner.close()), /close|cleanup/i);
  assert.equal(broken.closes, 1);
  assert.equal(cleanup.hid.count("disconnect"), 0);
});

test("optional shared source allocation preserves full identity, burns retired IDs and rejects collisions before native opens", async () => {
  let next = 9007199254740993n, allocations = 0;
  const allocate = () => { allocations++; return next++; };
  const first = device("first"), second = device("second");
  const h = rig({ authorized: [first], options: { nextSource: allocate } });
  await h.owner.connectAuthorized();
  assert.equal(h.owner.devices[0].source, 9007199254740993n);
  assert.equal(allocate(), 9007199254740994n, "another input owner shares the allocator without reusing the HID source");
  h.hid.authorized = [first, first, second];
  await h.owner.connectAuthorized();
  assert.deepEqual(h.owner.devices.map(value => value.source), [9007199254740993n, 9007199254740995n]);
  assert.equal(allocations, 3, "rediscovery and duplicate native objects do not allocate new identities");
  second.emit("inputreport", report(second));
  assert.equal(h.reports[0].source, 9007199254740995n);
  h.hid.emit("disconnect", { device: first, timeStamp: 1400 }); await flush();
  await h.owner.connectAuthorized();
  assert.equal(h.owner.devices.find(value => value.device === first).source, 9007199254740996n);
  await h.owner.close(); assert.equal(first.closes, 2); assert.equal(second.closes, 1);

  for (const values of [[3n, 3n], [4n, 3n], [3n, 18446744073709551616n], [3n, 2n], [3n, 4]]) {
    const one = device("unopened first"), two = device("unopened second");
    let index = 0;
    const invalid = rig({ authorized: [one, two], options: { nextSource: () => values[index++] } });
    const error = await rejected(attempt(() => invalid.owner.connectAuthorized()), /source|identity|allocator/i);
    assert.equal(invalid.owner.failure, error); await invalid.owner.close();
    assert.equal(one.opens, 0); assert.equal(two.opens, 0);
    assert.equal(one.closes, 0); assert.equal(two.closes, 0);
    assert.deepEqual(invalid.owner.devices, []);
  }
  const maximum = device("last allocatable source"), exhausted = device("exhausted");
  let source = 18446744073709551615n;
  const boundary = rig({ authorized: [maximum], options: { nextSource: () => source++ } });
  await boundary.owner.connectAuthorized();
  assert.equal(boundary.owner.devices[0].source, 18446744073709551615n);
  boundary.hid.authorized = [maximum, exhausted];
  await rejected(attempt(() => boundary.owner.connectAuthorized()), /source|identity|allocator/i);
  await boundary.owner.close(); assert.equal(maximum.closes, 1); assert.equal(exhausted.opens, 0);
  const legacy = rig({ authorized: [device("default allocation")] });
  await legacy.owner.connectAuthorized(); assert.equal(legacy.owner.devices[0].source, 3n); await legacy.owner.close();
  assert.throws(() => rig({ options: { nextSource: 3n } }));
});

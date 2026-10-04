// Deferred acquisition fixtures: actual owner, controlled browser endpoints only.
// No hardware access, playable control mapping or browser timing claim is made here.
import assert from "node:assert/strict";
import test from "node:test";
import { GamepadInputOwner } from "./gamepad-input.mjs";

const U64_MAX = 18446744073709551615n;
class Events {
  listeners = new Map();
  addEventListener(kind, listener) {
    if (!this.listeners.has(kind)) this.listeners.set(kind, new Set());
    this.listeners.get(kind).add(listener);
  }
  removeEventListener(kind, listener) { this.listeners.get(kind)?.delete(listener); }
  emit(kind, gamepad) {
    const event = { type: kind, target: this, gamepad };
    for (const listener of [...(this.listeners.get(kind) ?? [])]) listener(event);
  }
  count(kind) { return this.listeners.get(kind)?.size ?? 0; }
}
function pad(index, fields = {}) {
  return { index, id: "shared product description", mapping: "standard", connected: true,
    timestamp: 1234.125, axes: [0], buttons: [{ value: 0, pressed: false, touched: false }], ...fields };
}
function rig(settings = {}) {
  const events = settings.events ?? new Events();
  const samples = [], disconnects = [], errors = [];
  let pads = settings.pads ?? [], source = settings.source ?? 9007199254740993n;
  let sequence = settings.sequence ?? 9007199254741993n;
  let sourceCalls = 0, sequenceCalls = 0, reads = 0;
  const navigator = {
    getGamepads() {
      assert.equal(this, navigator, "the native method keeps its receiver");
      reads++;
      settings.onRead?.();
      if (settings.readError) throw settings.readError;
      return pads;
    },
  };
  const config = {
    navigator, eventTarget: events,
    nextSource() {
      sourceCalls++;
      return settings.nextSource ? settings.nextSource(sourceCalls) : source++;
    },
    nextSequence() {
      sequenceCalls++;
      return settings.nextSequence ? settings.nextSequence(sequenceCalls) : sequence++;
    },
    onSample(sample) { samples.push(sample); settings.onSample?.(sample); },
    onDisconnect(value) { disconnects.push(value); settings.onDisconnect?.(value); },
    onError(error) { errors.push(error); settings.onError?.(error); },
    ...(settings.options ?? {}),
  };
  const owner = new GamepadInputOwner(config);
  return { owner, config, navigator, events, samples, disconnects, errors,
    set pads(value) { pads = value; }, get pads() { return pads; },
    get reads() { return reads; }, get sourceCalls() { return sourceCalls; },
    get sequenceCalls() { return sequenceCalls; },
    nextSharedSource() { return config.nextSource(); },
    nextSharedSequence() { return config.nextSequence(); } };
}
function caught(action) {
  let result;
  assert.throws(action, error => { result = error; return error instanceof Error; });
  return result;
}
function terminal(h, action, pattern) {
  const error = caught(action);
  assert.ok(error.message.length > 0);
  if (pattern) assert.match(error.message, pattern);
  assert.equal(h.owner.failure, error);
  assert.equal(h.owner.closed, true);
  assert.deepEqual(h.owner.devices, []);
  assert.equal(h.events.count("gamepadconnected"), 0);
  assert.equal(h.events.count("gamepaddisconnected"), 0);
  assert.equal(caught(() => h.owner.poll()), error, "later polls retain the original failure");
  assert.deepEqual(h.errors, [error]);
  h.owner.close();
  h.owner.close();
  assert.deepEqual(h.errors, [error]);
  return error;
}

test("sparse multi-device acquisition preserves native timestamps, double values and full shared identities", () => {
  const first = pad(1, { mapping: "", axes: [-0, 0.12345678901234568, -1, 1],
    buttons: [{ value: 0.12345678901234566, pressed: true, touched: false },
      { value: 1, pressed: false, touched: true }] });
  const second = pad(4, { timestamp: 604800000.125, axes: [], buttons: [] });
  const slots = new Array(6);
  slots[0] = null; slots[1] = first; slots[4] = second; slots[5] = null;
  const h = rig({ pads: slots, source: U64_MAX - 1n, sequence: U64_MAX - 1n });
  assert.equal(h.reads, 0, "construction performs no private polling");
  assert.equal(h.owner.poll(), 2);
  assert.equal(h.reads, 1);
  assert.deepEqual(h.samples, [
    { kind: "gamepad", source: U64_MAX - 1n, index: 1, id: first.id, mapping: "",
      hostNs: 1234125000n, timestampMs: 1234.125, sequence: U64_MAX - 1n,
      axes: [-0, 0.12345678901234568, -1, 1],
      buttons: [{ value: 0.12345678901234566, pressed: true, touched: false },
        { value: 1, pressed: false, touched: true }] },
    { kind: "gamepad", source: U64_MAX, index: 4, id: second.id, mapping: "standard",
      hostNs: 604800000125000n, timestampMs: 604800000.125, sequence: U64_MAX,
      axes: [], buttons: [] },
  ]);
  assert.notEqual(h.samples[0].axes[1], Math.fround(h.samples[0].axes[1]));
  assert.notEqual(h.samples[0].buttons[0].value, Math.fround(h.samples[0].buttons[0].value));
  for (const sample of h.samples) {
    assert.ok(Object.isFrozen(sample) && Object.isFrozen(sample.axes) && Object.isFrozen(sample.buttons));
    assert.ok(sample.buttons.every(Object.isFrozen));
  }
  const devices = h.owner.devices;
  assert.deepEqual(devices, [
    { source: U64_MAX - 1n, index: 1, id: first.id, mapping: "" },
    { source: U64_MAX, index: 4, id: second.id, mapping: "standard" },
  ]);
  assert.ok(Object.isFrozen(devices) && devices.every(Object.isFrozen));
  h.owner.close();
});

test("complete snapshots precede allocator callbacks and equal native timestamps retain changed samples", () => {
  const first = pad(0), second = pad(1, { axes: [0.12345678901234568] });
  let timestampReads = 0, nativeTimestamp = 1234.125;
  Object.defineProperty(second, "timestamp", { get() { timestampReads++; return nativeTimestamp; } });
  const h = rig({ pads: [first, second], nextSequence(call) {
    if (call === 1) {
      second.axes[0] = 0.75;
      second.buttons[0].value = 0.5;
      second.buttons[0].touched = true;
      nativeTimestamp = 2000;
    }
    return BigInt(call);
  } });
  assert.equal(h.owner.poll(), 2);
  assert.equal(timestampReads, 1);
  assert.equal(h.samples[1].timestampMs, 1234.125);
  assert.equal(h.samples[1].hostNs, 1234125000n);
  assert.deepEqual(h.samples[1].axes, [0.12345678901234568]);
  assert.deepEqual(h.samples[1].buttons, [{ value: 0, pressed: false, touched: false }]);
  assert.notEqual(h.samples[1].axes, second.axes);
  assert.notEqual(h.samples[1].buttons[0], second.buttons[0]);
  nativeTimestamp = 1234.125;
  first.axes[0] = -0.25;
  assert.equal(h.owner.poll(), 2);
  assert.equal(timestampReads, 2);
  assert.deepEqual(h.samples[2].axes, [-0.25]);
  assert.deepEqual(h.samples[3].axes, [0.75]);
  assert.equal(h.samples[3].buttons[0].value, 0.5);
  assert.equal(h.samples[3].timestampMs, h.samples[1].timestampMs);
  assert.equal(h.samples[3].source, h.samples[1].source);
  assert.equal(h.samples[3].sequence, 4n);
  assert.equal(h.sourceCalls, 2);
  h.owner.close();
});

test("connection evidence retires exact objects and index reuse allocates fresh sources without synthetic releases", () => {
  const first = pad(0), replacement = pad(0);
  const h = rig({ pads: [first], source: 100n });
  assert.equal(h.owner.poll(), 1);
  const initialDevices = h.owner.devices;
  assert.equal(h.nextSharedSource(), 101n, "other acquisition owners can share the allocator");
  h.events.emit("gamepadconnected", replacement);
  assert.deepEqual(h.disconnects, [{ source: 100n, index: 0 }]);
  assert.equal(h.samples.length, 1);
  assert.equal(h.sourceCalls, 2, "connection notifications do not synthesize samples or allocate the next sample");
  h.pads = [replacement];
  assert.equal(h.owner.poll(), 1);
  assert.equal(h.samples[1].source, 102n);
  assert.equal(h.samples[1].id, h.samples[0].id);
  first.connected = false;
  h.events.emit("gamepaddisconnected", first);
  assert.equal(h.disconnects.length, 1, "a stale native object cannot retire the replacement");
  replacement.connected = false;
  h.events.emit("gamepaddisconnected", replacement);
  assert.deepEqual(h.disconnects, [{ source: 100n, index: 0 }, { source: 102n, index: 0 }]);
  assert.ok(h.disconnects.every(Object.isFrozen));
  assert.equal(h.samples.length, 2);
  assert.deepEqual(h.owner.devices, []);
  assert.equal(initialDevices[0].source, 100n, "previous public snapshots remain immutable");
  replacement.connected = true;
  assert.equal(h.owner.poll(), 1);
  assert.equal(h.samples[2].source, 103n);
  h.pads = [null];
  assert.equal(h.owner.poll(), 0);
  assert.deepEqual(h.disconnects.at(-1), { source: 103n, index: 0 });
  const inactive = pad(0, { connected: false });
  h.pads = [inactive];
  assert.equal(h.owner.poll(), 0);
  assert.equal(h.sourceCalls, 4);
  h.owner.close();
  assert.equal(h.samples.length, 3);
});

test("malformed later slots and timestamp or descriptor regressions refuse the entire poll before publication", () => {
  const changes = [
    value => { value.timestamp = 1234; },
    value => { value.timestamp = NaN; },
    value => { value.timestamp = -1; },
    value => { value.timestamp = "1234.125"; },
    value => { value.timestamp = 9223372036855; },
    value => { value.index = 0; },
    value => { value.connected = 1; },
    value => { value.id = "changed connection descriptor"; },
    value => { value.mapping = ""; },
    value => { value.axes.push(0); },
    value => { value.buttons.push({ value: 0, pressed: false, touched: false }); },
    value => { value.axes[0] = NaN; },
    value => { value.axes[0] = 1.0000000000000002; },
    value => { value.axes[0] = -1.0000000000000002; },
    value => { delete value.axes[0]; },
    value => { value.buttons[0].value = Infinity; },
    value => { value.buttons[0].value = -0.01; },
    value => { value.buttons[0].value = 1.01; },
    value => { value.buttons[0].pressed = 1; },
    value => { value.buttons[0].touched = undefined; },
    value => { value.buttons[0] = null; },
  ];
  for (const change of changes) {
    const first = pad(0), second = pad(1);
    const h = rig({ pads: [first, second] });
    assert.equal(h.owner.poll(), 2);
    first.timestamp = 2000;
    first.axes[0] = 0.5;
    change(second);
    terminal(h, () => h.owner.poll());
    assert.equal(h.samples.length, 2, "the valid first slot is not published before the bad second slot is checked");
    assert.equal(h.sequenceCalls, 2, "invalid native snapshots do not consume shared sequences");
    assert.equal(h.sourceCalls, 2);
    assert.deepEqual(h.disconnects, [], "failure teardown does not invent native release events");
  }
});

test("constructor and native storage limits reject excess before acquisition while exact boundaries remain usable", () => {
  const limits = { maxDevices: 16, maxSlots: 64, maxButtons: 128, maxAxes: 64 };
  for (const [name, maximum] of Object.entries(limits)) {
    for (const value of [0, -1, 1.5, maximum + 1, NaN, Infinity, "1", null]) {
      const events = new Events();
      assert.throws(() => rig({ events, options: { [name]: value } }));
      assert.equal(events.count("gamepadconnected"), 0);
      assert.equal(events.count("gamepaddisconnected"), 0);
    }
  }
  for (const name of ["nextSource", "nextSequence", "onSample", "onDisconnect", "onError"]) {
    const events = new Events();
    assert.throws(() => rig({ events, options: { [name]: null } }));
    assert.equal(events.count("gamepadconnected"), 0);
  }
  const boundary = new Array(64);
  for (let index = 48; index < 64; index++) {
    boundary[index] = pad(index, { id: "x".repeat(1024), timestamp: 0,
      axes: new Array(64).fill(-1),
      buttons: Array.from({ length: 128 }, () => ({ value: 1, pressed: true, touched: true })) });
  }
  const h = rig({ pads: boundary });
  assert.equal(h.owner.poll(), 16);
  assert.equal(h.owner.devices.length, 16);
  assert.equal(h.samples[15].index, 63);
  assert.equal(h.samples[15].hostNs, 0n);
  assert.equal(h.samples[15].axes.length, 64);
  assert.equal(h.samples[15].buttons.length, 128);
  h.owner.close();

  const tooManyButtons = Array.from({ length: 129 }, () => ({ value: 0, pressed: false, touched: false }));
  const cases = [
    { pads: new Array(65) },
    { pads: Array.from({ length: 17 }, (_, index) => pad(index)) },
    { pads: [pad(0, { axes: new Array(65).fill(0) })] },
    { pads: [pad(0, { buttons: tooManyButtons })] },
    { pads: [pad(0, { id: "x".repeat(1025) })] },
    { pads: [pad(0, { mapping: "vendor-defined" })] },
    { pads: [pad(0, { axes: new Float64Array([0]) })] },
    { pads: [pad(0, { buttons: {} })] },
    { pads: [undefined] },
    { pads: { 0: pad(0), length: 1 } },
    { pads: [pad(0), pad(1)], options: { maxDevices: 1 } },
    { pads: [null, pad(1)], options: { maxSlots: 1 } },
    { pads: [pad(0, { axes: [0, 0] })], options: { maxAxes: 1 } },
    { pads: [pad(0, { buttons: tooManyButtons.slice(0, 2) })], options: { maxButtons: 1 } },
  ];
  for (const settings of cases) {
    const limited = rig(settings);
    terminal(limited, () => limited.owner.poll());
    assert.equal(limited.samples.length, 0);
    assert.equal(limited.sourceCalls, 0);
    assert.equal(limited.sequenceCalls, 0);
  }
});

test("shared u64 allocators reject exhaustion, reuse and regression before any sample of the affected poll", () => {
  for (const source of [2n, -1n, U64_MAX + 1n, 3, undefined]) {
    const h = rig({ pads: [pad(0)], nextSource: () => source });
    terminal(h, () => h.owner.poll());
    assert.equal(h.samples.length, 0);
    assert.equal(h.sequenceCalls, 0);
  }
  for (const sequence of [-1n, U64_MAX + 1n, 0, undefined]) {
    const h = rig({ pads: [pad(0)], nextSequence: () => sequence });
    terminal(h, () => h.owner.poll());
    assert.equal(h.samples.length, 0);
  }
  const sourceFault = new Error("shared source exhausted");
  const sequenceFault = new Error("shared sequence exhausted");
  const noPrefix = [
    { nextSource: () => 3n },
    { nextSource: call => call === 1 ? 4n : 3n },
    { nextSource: call => { if (call === 2) throw sourceFault; return 3n; }, cause: sourceFault },
    { nextSequence: () => 0n },
    { nextSequence: call => call === 1 ? 9n : 8n },
    { nextSequence: call => { if (call === 2) throw sequenceFault; return 0n; }, cause: sequenceFault },
  ];
  for (const settings of noPrefix) {
    const h = rig({ pads: [pad(0), pad(1)], ...settings });
    const error = terminal(h, () => h.owner.poll());
    if (settings.cause) assert.equal(error.cause, settings.cause);
    assert.equal(h.samples.length, 0, "a later allocator refusal must not publish an earlier valid sample");
  }
  const exhausted = rig({ pads: [pad(0)], source: U64_MAX, sequence: 0n });
  assert.equal(exhausted.owner.poll(), 1);
  assert.equal(exhausted.samples[0].source, U64_MAX);
  assert.equal(exhausted.samples[0].sequence, 0n);
  assert.equal(exhausted.nextSharedSequence(), 1n);
  assert.equal(exhausted.owner.poll(), 1);
  assert.equal(exhausted.samples[1].sequence, 2n, "other adapters may consume the intervening shared sequence");
  exhausted.pads = [pad(0)];
  terminal(exhausted, () => exhausted.owner.poll());
  assert.equal(exhausted.samples.length, 2);

  for (const next of [U64_MAX, U64_MAX - 1n, U64_MAX + 1n]) {
    const same = pad(0);
    const h = rig({ pads: [same], nextSequence: call => call === 1 ? U64_MAX : next });
    assert.equal(h.owner.poll(), 1);
    same.axes[0] = 0.5;
    terminal(h, () => h.owner.poll());
    assert.equal(h.samples.length, 1, "equal timestamps do not excuse reused or invalid acquisition sequences");
  }
});

test("unsupported endpoints and native exceptions have explicit ownership cleanup and retain the first operational failure", () => {
  for (const options of [
    { navigator: null }, { navigator: {} }, { navigator: { getGamepads: 1 } },
    { eventTarget: null }, { eventTarget: { addEventListener() {} } },
  ]) {
    const events = new Events();
    assert.throws(() => rig({ events, options }));
    assert.equal(events.count("gamepadconnected"), 0);
    assert.equal(events.count("gamepaddisconnected"), 0);
  }
  const setupFailure = new Error("native addEventListener failed");
  const events = new Events();
  const add = events.addEventListener.bind(events);
  events.addEventListener = (kind, listener) => {
    add(kind, listener);
    if (kind === "gamepaddisconnected") throw setupFailure;
  };
  const setupErrors = [];
  const setup = caught(() => rig({ events, onError: error => setupErrors.push(error) }));
  assert.equal(setup.cause, setupFailure);
  assert.deepEqual(setupErrors, [setup]);
  assert.equal(events.count("gamepadconnected"), 0);
  assert.equal(events.count("gamepaddisconnected"), 0);

  const nativeFailure = new Error("getGamepads denied");
  const read = rig({ readError: nativeFailure });
  assert.equal(terminal(read, () => read.owner.poll()).cause, nativeFailure);
  assert.equal(read.reads, 1, "future fenced polls never call the browser again");
  assert.equal(read.sourceCalls, 0);
  const getterFailure = new Error("native timestamp getter failed");
  const broken = pad(1);
  Object.defineProperty(broken, "timestamp", { get() { throw getterFailure; } });
  const snapshot = rig({ pads: [pad(0), broken] });
  assert.equal(terminal(snapshot, () => snapshot.owner.poll()).cause, getterFailure);
  assert.equal(snapshot.samples.length, 0);
  assert.equal(snapshot.sequenceCalls, 0);

  const callbackFailure = new Error("consumer stopped accepting samples");
  const callback = rig({ pads: [pad(0), pad(1)], onSample() { throw callbackFailure; },
    onError() { throw new Error("notification failure must not replace the original failure"); } });
  assert.equal(terminal(callback, () => callback.owner.poll()).cause, callbackFailure);
  assert.equal(callback.samples.length, 1, "only the already invoked consumer callback is observable");

  const disconnected = pad(0);
  const disconnectFailure = new Error("disconnect consumer failed");
  const lifecycle = rig({ pads: [disconnected], onDisconnect() { throw disconnectFailure; } });
  lifecycle.owner.poll();
  disconnected.connected = false;
  assert.doesNotThrow(() => lifecycle.events.emit("gamepaddisconnected", disconnected));
  assert.equal(terminal(lifecycle, () => lifecycle.owner.poll()).cause, disconnectFailure);
  assert.deepEqual(lifecycle.disconnects, [{ source: 9007199254740993n, index: 0 }]);
  assert.equal(lifecycle.samples.length, 1);
});

test("close, callback reentry and lifecycle changes fence remaining emissions and detach every owned listener", () => {
  let busy;
  const h = rig({ pads: [pad(0), pad(1)], onSample() {
    busy = caught(() => h.owner.poll());
  } });
  assert.equal(h.owner.poll(), 2);
  assert.match(busy.message, /active|busy/i);
  assert.equal(h.owner.failure, null);
  assert.equal(h.owner.closed, false);
  assert.deepEqual(h.errors, []);
  h.owner.close();

  const closes = rig({ pads: [pad(0), pad(1)], onSample() { closes.owner.close(); } });
  const late = [...closes.events.listeners.get("gamepaddisconnected")][0];
  assert.equal(closes.owner.poll(), 1);
  assert.equal(closes.samples.length, 1);
  assert.equal(closes.owner.closed, true);
  assert.deepEqual(closes.owner.devices, []);
  assert.equal(closes.events.count("gamepadconnected"), 0);
  assert.equal(closes.events.count("gamepaddisconnected"), 0);
  assert.deepEqual(closes.disconnects, [], "explicit close synthesizes no release or disconnect");
  closes.owner.close();
  late({ gamepad: null });
  assert.deepEqual(closes.errors, []);
  assert.throws(() => closes.owner.poll(), /closed/i);

  const beforePublication = rig({ pads: [pad(0), pad(1)], nextSequence(call) {
    beforePublication.owner.close();
    return BigInt(call);
  } });
  assert.equal(beforePublication.owner.poll(), 0);
  assert.equal(beforePublication.samples.length, 0);
  assert.equal(beforePublication.sequenceCalls, 1);
  assert.equal(beforePublication.sourceCalls, 1);

  const first = pad(0), second = pad(1);
  const retires = rig({ pads: [first, second], onSample() {
    second.connected = false;
    retires.events.emit("gamepaddisconnected", second);
  } });
  assert.equal(retires.owner.poll(), 1);
  assert.equal(retires.samples.length, 1);
  assert.deepEqual(retires.disconnects, [{ source: 9007199254740994n, index: 1 }]);
  assert.deepEqual(retires.owner.devices.map(value => value.index), [0]);
  assert.equal(retires.owner.failure, null);
  retires.owner.close();

  const unhandled = rig({ pads: [pad(0), pad(1)], onSample() { unhandled.owner.poll(); } });
  const failed = terminal(unhandled, () => unhandled.owner.poll());
  assert.match(failed.cause.message, /active|busy/i);
  assert.equal(unhandled.samples.length, 1);

  const cleanupFailure = new Error("native removeEventListener failed");
  const eventTarget = new Events();
  const remove = eventTarget.removeEventListener.bind(eventTarget);
  const removed = [];
  eventTarget.removeEventListener = (kind, listener) => {
    removed.push(kind);
    remove(kind, listener);
    throw cleanupFailure;
  };
  const cleanup = rig({ events: eventTarget });
  assert.doesNotThrow(() => cleanup.owner.close());
  const error = terminal(cleanup, () => cleanup.owner.poll());
  assert.equal(error.cause, cleanupFailure);
  assert.deepEqual(removed, ["gamepadconnected", "gamepaddisconnected"]);
});

// Deferred actual acquisition component; only EventTarget/capture endpoints are scripted.
// Native histories are supplied as data; no hardware, rendering or judgment is executed.
import assert from "node:assert/strict";
import test from "node:test";
import { PointerInputOwner } from "./pointer-input.mjs";

const TYPES = ["pointerdown", "pointermove", "pointerup", "pointercancel", "lostpointercapture"];
const MOUSE = 18446744073709551614n, PEN = 18446744073709551615n;
class Target {
  constructor(faults = {}) { this.faults = faults; this.listeners = new Map(); this.adds = []; this.removes = []; this.captures = []; this.releases = []; }
  addEventListener(type, handler) {
    this.adds.push(type);
    if (this.faults.addAt === this.adds.length) throw new Error("native listener attachment failed");
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type).add(handler);
  }
  removeEventListener(type, handler) {
    this.removes.push(type); this.listeners.get(type)?.delete(handler);
    if (this.faults.removeError) throw this.faults.removeError;
  }
  setPointerCapture(id) { this.captures.push(id); if (this.faults.captureError) throw this.faults.captureError; }
  releasePointerCapture(id) { this.releases.push(id); if (this.faults.releaseError) throw this.faults.releaseError; }
  getBoundingClientRect() { assert.fail("acquisition must not read layout"); }
  emit(type, event) { for (const handler of [...(this.listeners.get(type) ?? [])]) handler.call(this, event); }
  count() { return [...this.listeners.values()].reduce((sum, entries) => sum + entries.size, 0); }
}
function event(fields = {}) {
  return { pointerType: "mouse", pointerId: -2147483648, timeStamp: 1234.125,
    offsetX: -0, offsetY: -2.25, buttons: 1,
    getPredictedEvents() { assert.fail("predicted samples are not acquisitions"); }, ...fields };
}
function rig(faults = {}) {
  const target = new Target(faults), batches = [], errors = [];
  let sourceCalls = 0, sequenceCalls = 0, sequence = faults.firstSequence ?? 9007199254740993n, current = true;
  const owner = new PointerInputOwner({ target,
    nextSource() { return [MOUSE, PEN][sourceCalls++]; },
    nextSequence() {
      const call = ++sequenceCalls; faults.beforeSequence?.(call);
      return faults.sequenceAt ? faults.sequenceAt(call) : sequence++;
    },
    onBatch(batch) { batches.push(batch); if (faults.batchError) throw faults.batchError; },
    onError(error) { errors.push(error); if (faults.errorCallbackError) throw faults.errorCallbackError; },
    isCurrent() { return current; },
  });
  return { target, owner, batches, errors, get sourceCalls() { return sourceCalls; },
    get sequenceCalls() { return sequenceCalls; }, nextShared() { return sequence++; },
    retire() { current = false; } };
}
function rows(h) { return h.batches.flat(); }
function child(fields = {}) {
  return { pointerType: "mouse", pointerId: -2147483648, isPrimary: true,
    timeStamp: 1300, clientX: 998, clientY: 1999, buttons: 0, ...fields };
}
function history(children, fields = {}) {
  const parent = event({ timeStamp: 1400, isPrimary: true, clientX: 1000, clientY: 2000,
    offsetX: 100, offsetY: 200, buttons: 0,
    getCoalescedEvents() { assert.equal(this, parent); return children; }, ...fields });
  return parent;
}

test("dispatched mouse masks retain exact signed IDs, timestamps, absolute positions and ordered independent button controls", () => {
  const h = rig();
  assert.equal(h.sourceCalls, 2);
  assert.deepEqual(h.owner.devices, [{ source: MOUSE, pointerType: "mouse" }, { source: PEN, pointerType: "pen" }]);
  assert.ok(Object.isFrozen(h.owner.devices) && h.owner.devices.every(Object.isFrozen));
  assert.deepEqual(h.target.adds.slice().sort(), TYPES.slice().sort());
  h.target.emit("pointerdown", event({ buttons: 0x80000005 }));
  assert.deepEqual(h.batches[0], [
    { kind: "pointer", pointerType: "mouse", hostNs: 1234125000n, source: MOUSE, sequence: 9007199254740993n,
      code: 0x80000000, control: 0, mode: 0, x: -0, y: -2.25 },
    { kind: "pointer-button", pointerType: "mouse", hostNs: 1234125000n, source: MOUSE, sequence: 9007199254740994n,
      code: 0x80000000, control: 1, state: 0 },
    { kind: "pointer-button", pointerType: "mouse", hostNs: 1234125000n, source: MOUSE, sequence: 9007199254740995n,
      code: 0x80000000, control: 3, state: 0 },
    { kind: "pointer-button", pointerType: "mouse", hostNs: 1234125000n, source: MOUSE, sequence: 9007199254740996n,
      code: 0x80000000, control: 32, state: 0 },
  ]);
  assert.ok(Object.isFrozen(h.batches[0]) && h.batches[0].every(Object.isFrozen));
  assert.equal(h.nextShared(), 9007199254740997n, "keyboard/HID/Gamepad can allocate an intervening sequence");
  h.target.emit("pointermove", event({ timeStamp: 1234.25, offsetX: 1.5, buttons: 0x80000002 }));
  assert.deepEqual(h.batches[1].map(row => [row.kind, row.control, row.state, row.sequence]), [
    ["pointer", 0, undefined, 9007199254740998n], ["pointer-button", 1, 1, 9007199254740999n],
    ["pointer-button", 2, 0, 9007199254741000n], ["pointer-button", 3, 1, 9007199254741001n],
  ]);
  assert.ok(h.batches[1].every(row => row.hostNs === 1234250000n));
  h.target.emit("pointerup", event({ timeStamp: 1234.5, buttons: 0 }));
  assert.deepEqual(h.batches[2].map(row => [row.control, row.state]), [[0, undefined], [2, 1], [32, 1]]);
  const afterUp = rows(h).length;
  const lost = { pointerType: "mouse", pointerId: -2147483648 };
  Object.defineProperty(lost, "timeStamp", { get() { assert.fail("already released capture has no new acquisition"); } });
  h.target.emit("lostpointercapture", lost);
  assert.equal(rows(h).length, afterUp); assert.deepEqual(h.target.captures, [-2147483648]);
  assert.deepEqual(h.target.releases, [-2147483648]);
  h.owner.close(); h.owner.close();
  assert.equal(rows(h).length, afterUp); assert.equal(h.target.count(), 0); assert.equal(h.owner.closed, true);
  assert.equal(h.owner.cleanupFailure, null); assert.deepEqual(h.errors, []);
});

test("aggregate pen masks release each held control once and ignore unrelated pointer types without reading their payload", () => {
  const h = rig({ firstSequence: 0n });
  const pen = (id, fields = {}) => event({ pointerType: "pen", pointerId: id, ...fields });
  h.target.emit("pointerdown", pen(11));
  h.target.emit("pointerdown", pen(12));
  assert.deepEqual(h.batches.map(batch => batch.map(row => [row.control, row.state])), [[[0, undefined], [1, 0]], [[0, undefined]]]);
  h.target.emit("pointermove", pen(11, { buttons: 3 }));
  assert.deepEqual(h.batches.at(-1).map(row => [row.control, row.state]), [[0, undefined], [2, 0]]);
  const cancel = { pointerType: "pen", pointerId: 11, timeStamp: 2000.125 };
  for (const key of ["offsetX", "offsetY", "buttons", "isPrimary"])
    Object.defineProperty(cancel, key, { get() { assert.fail(`cancel must not require ${key}`); } });
  h.target.emit("pointercancel", cancel);
  assert.deepEqual(h.batches.at(-1), [{ kind: "pointer-button", pointerType: "pen", hostNs: 2000125000n,
    source: PEN, sequence: 5n, code: 11, control: 2, state: 1 }]);
  h.target.emit("lostpointercapture", { pointerType: "pen", pointerId: 12, timeStamp: 2000.25 });
  assert.deepEqual(h.batches.at(-1), [{ kind: "pointer-button", pointerType: "pen", hostNs: 2000250000n,
    source: PEN, sequence: 6n, code: 12, control: 1, state: 1 }]);
  const count = h.batches.length;
  h.target.emit("lostpointercapture", { pointerType: "pen", pointerId: 11 });
  h.target.emit("lostpointercapture", { pointerType: "pen", pointerId: 12 });
  for (const pointerType of ["touch", "", "eraser"]) {
    const ignored = { pointerType };
    Object.defineProperty(ignored, "pointerId", { get() { assert.fail("unowned pointer type is not sampled"); } });
    h.target.emit("pointerdown", ignored);
  }
  assert.equal(h.batches.length, count); assert.equal(h.sequenceCalls, 7); assert.deepEqual(h.errors, []);
  h.owner.close(); assert.equal(h.target.count(), 0);
});

test("pointer event and identity bounds reject complete events without publishing partial masks or over-capacity acquisitions", () => {
  const bad = [{ pointerId: -2147483649 }, { pointerId: 2147483648 }, { pointerId: 1.5 }, { pointerId: "1" },
    { timeStamp: -1 }, { timeStamp: NaN }, { timeStamp: Infinity }, { timeStamp: "1" },
    { offsetX: NaN }, { offsetY: Infinity }, { offsetX: 3.5e38 }, { buttons: -1 }, { buttons: 4294967296 },
    { buttons: 1.5 }, { buttons: "1" }];
  for (const fields of bad) {
    const h = rig(); h.target.emit("pointerdown", event(fields));
    assert.equal(h.batches.length, 0); assert.equal(h.sequenceCalls, 0);
    assert.equal(h.errors.length, 1); assert.equal(h.owner.closed, true); assert.equal(h.target.count(), 0);
  }
  const full = rig({ firstSequence: 18446744073709551583n });
  full.target.emit("pointerdown", event({ buttons: 0xffffffff }));
  assert.equal(full.batches[0].length, 33);
  assert.deepEqual(full.batches[0].slice(1).map(row => row.control), Array.from({ length: 32 }, (_, index) => index + 1));
  assert.equal(full.batches[0].at(-1).sequence, 18446744073709551615n);
  full.owner.close(); assert.equal(full.batches.length, 1, "close never synthesizes the 32 held releases");
  for (const invalidSequence of [-1n, 0n, 18446744073709551616n, 1, "1"]) {
    const h = rig({ sequenceAt: call => call === 1 ? 0n : invalidSequence });
    h.target.emit("pointerdown", event({ buttons: 3 }));
    assert.equal(h.batches.length, 0); assert.equal(h.errors.length, 1); assert.equal(h.owner.closed, true);
  }
  const bounded = rig();
  for (const pointerType of ["mouse", "pen"]) for (let id = 0; id < 32; id++)
    bounded.target.emit("pointerdown", event({ pointerType, pointerId: (pointerType === "mouse" ? 0 : 1000) + id, buttons: 1 }));
  assert.equal(bounded.batches.length, 64); assert.deepEqual(bounded.errors, []);
  bounded.target.emit("pointerdown", event({ pointerId: 64 }));
  assert.equal(bounded.batches.length, 64, "held identity capacity is shared across mouse and pen types");
  assert.equal(bounded.errors.length, 1);
  assert.equal(bounded.owner.closed, true); assert.equal(bounded.target.count(), 0);
});

test("native and callback failures detach ownership and stale callbacks cannot affect a replacement or produce synthetic close input", () => {
  for (const sources of [[2n, 3n], [3n, 3n], [3n, 4], [3n, 18446744073709551616n]]) {
    const target = new Target(); let index = 0;
    assert.throws(() => new PointerInputOwner({ target, nextSource: () => sources[index++], nextSequence: () => 0n,
      onBatch() {}, onError() {}, isCurrent: () => true }));
    assert.equal(target.count(), 0);
  }
  const partial = new Target({ addAt: 3 });
  let source = 3n;
  assert.throws(() => new PointerInputOwner({ target: partial, nextSource: () => source++, nextSequence: () => 0n,
    onBatch() {}, onError() {}, isCurrent: () => true }));
  assert.equal(partial.count(), 0, "a failed constructor detaches every earlier listener");
  const capture = rig({ captureError: new Error("native capture failed") });
  capture.target.emit("pointerdown", event());
  assert.equal(capture.batches.length, 0); assert.equal(capture.errors.length, 1); assert.equal(capture.target.count(), 0);
  const delivery = rig({ batchError: new Error("batch consumer refused"), errorCallbackError: new Error("ignored error observer") });
  delivery.target.emit("pointerdown", event());
  assert.equal(delivery.batches.length, 1); assert.equal(delivery.errors.length, 1); assert.equal(delivery.owner.closed, true);
  const failedCleanup = rig({ removeError: new Error("native detach failed"), releaseError: new Error("native release failed") });
  failedCleanup.target.emit("pointerdown", event());
  failedCleanup.owner.close(); failedCleanup.owner.close();
  assert.ok(failedCleanup.owner.cleanupFailure instanceof Error);
  assert.equal(failedCleanup.target.removes.length, 5); assert.equal(failedCleanup.batches.length, 1);
  const old = rig(), retained = [...old.target.listeners.get("pointermove")][0];
  old.target.emit("pointerdown", event()); old.retire(); old.owner.close();
  const fresh = rig();
  retained(event({ buttons: 0, timeStamp: 2000 }));
  assert.equal(old.batches.length, 1); assert.equal(fresh.batches.length, 0); assert.equal(fresh.owner.closed, false);
  let retiring;
  retiring = rig({ beforeSequence: () => { retiring.retire(); retiring.owner.close(); } });
  retiring.target.emit("pointerdown", event({ buttons: 3 }));
  assert.equal(retiring.batches.length, 0, "ownership loss during an allocator callback discards the entire old acquisition");
  fresh.owner.close(); assert.equal(fresh.batches.length, 0);
});

test("coalesced mouse and pen histories snapshot once, preserve equal-time order and aggregate masks without using child offsets", () => {
  for (const pointerType of ["mouse", "pen"]) {
    const parentValues = { pointerType, pointerId: -2147483648, timeStamp: 1001, isPrimary: true,
      clientX: 1000, clientY: 2000, offsetX: 100, offsetY: 200, buttons: 0 };
    const childValues = [
      child({ pointerType, timeStamp: 1000.125, buttons: 0 }),
      child({ pointerType, timeStamp: 1000.25, clientX: 1001, clientY: 2004, buttons: 3 }),
      child({ pointerType, timeStamp: 1000.25, clientX: 1004, clientY: 1996, buttons: 0 }),
    ];
    const reads = [];
    const snapshotEndpoint = values => {
      const counts = new Map(), object = {};
      reads.push(counts);
      for (const key of Object.keys(values)) Object.defineProperty(object, key, { get() {
        counts.set(key, (counts.get(key) ?? 0) + 1); assert.equal(counts.get(key), 1); return values[key];
      } });
      return object;
    };
    const parent = snapshotEndpoint(parentValues), children = childValues.map(snapshotEndpoint);
    for (const value of children) for (const key of ["offsetX", "offsetY"])
      Object.defineProperty(value, key, { get() { assert.fail("coalesced coordinates use only the parent anchor and child client point"); } });
    let methodReads = 0, calls = 0;
    Object.defineProperty(parent, "getCoalescedEvents", { get() {
      assert.equal(++methodReads, 1);
      return function () { assert.equal(this, parent); assert.equal(++calls, 1); return children; };
    } });
    Object.defineProperty(parent, "getPredictedEvents", { get() { assert.fail("prediction is not read"); } });
    const h = rig({ firstSequence: 18446744073709551608n, beforeSequence(call) {
      if (call === 4) { parentValues.offsetX = 900; parentValues.buttons = 1; childValues[0].clientX = -900; children.length = 0; }
    } });
    h.target.emit("pointerdown", event({ pointerType, timeStamp: 1000, buttons: 1 }));
    h.target.emit("pointerdown", event({ pointerType, pointerId: 7, timeStamp: 1000, buttons: 1 }));
    h.target.emit("pointermove", parent);
    assert.deepEqual(h.errors, []); assert.equal(h.batches.length, 3);
    const batch = h.batches[2], source = pointerType === "mouse" ? MOUSE : PEN;
    assert.deepEqual(batch.map(row => [row.kind, row.control, row.state, row.hostNs, row.sequence]), [
      ["pointer", 0, undefined, 1000125000n, 18446744073709551611n],
      ["pointer", 0, undefined, 1000250000n, 18446744073709551612n],
      ["pointer-button", 2, 0, 1000250000n, 18446744073709551613n],
      ["pointer", 0, undefined, 1000250000n, 18446744073709551614n],
      ["pointer-button", 2, 1, 1000250000n, 18446744073709551615n],
    ], "another held pointer keeps control1 down throughout this history");
    assert.deepEqual(batch.filter(row => row.kind === "pointer").map(row => [row.x, row.y, row.mode]),
      [[98, 199, 0], [101, 204, 0], [104, 196, 0]]);
    assert.ok(batch.every(row => row.source === source && row.pointerType === pointerType && row.code === 0x80000000));
    assert.ok(Object.isFrozen(batch) && batch.every(Object.isFrozen));
    assert.ok(reads.every(counts => [...counts.values()].every(count => count === 1)));
    assert.deepEqual(h.target.captures, [-2147483648, 7]);
    assert.deepEqual(h.target.releases, [-2147483648], "capture follows final held state once, never individual historical edges");
    h.owner.close(); assert.equal(h.batches.length, 3);
  }
  for (const unavailable of [undefined, null, () => []]) {
    const h = rig(), parent = event({ buttons: 0, getCoalescedEvents: unavailable });
    for (const key of ["clientX", "clientY", "isPrimary"])
      Object.defineProperty(parent, key, { get() { assert.fail("single dispatched fallback needs no history-only fields"); } });
    h.target.emit("pointermove", parent);
    assert.equal(h.batches[0].length, 1); assert.equal(h.batches[0][0].hostNs, 1234125000n);
    assert.equal(h.batches[0][0].y, -2.25); assert.ok(Object.is(h.batches[0][0].x, -0));
    const down = event(), up = event({ buttons: 0 });
    for (const value of [down, up]) Object.defineProperty(value, "getCoalescedEvents", {
      get() { assert.fail("only movement can acquire a native history"); },
    });
    h.target.emit("pointerdown", down); h.target.emit("pointerup", up);
    assert.deepEqual(h.errors, []); h.owner.close();
  }
});

test("malformed complete histories refuse before sequences or publication even when the first child was valid", () => {
  const cases = [
    parent => { parent.getCoalescedEvents = 7; },
    parent => { parent.getCoalescedEvents = () => { throw new Error("history unavailable"); }; },
    parent => { parent.getCoalescedEvents = () => ({}); },
    parent => { parent.getCoalescedEvents = () => new Uint8Array(1); },
    parent => { parent.getCoalescedEvents = () => [child(), ,]; },
    (parent, children) => { children[1] = null; },
    (parent, children) => { children[1].pointerId = 1; },
    (parent, children) => { children[1].pointerType = "pen"; },
    (parent, children) => { children[1].isPrimary = false; },
    (parent, children) => { children[1].isPrimary = 1; },
    parent => { parent.isPrimary = undefined; },
    (parent, children) => { children[1].timeStamp = 1299; },
    (parent, children) => { children[1].timeStamp = 1400.0000001; },
    (parent, children) => { children[1].timeStamp = NaN; },
    (parent, children) => { children[1].timeStamp = -1; },
    (parent, children) => { children[1].buttons = 1; },
    (parent, children) => { children[1].buttons = 4294967296; },
    (parent, children) => { children[1].clientX = Infinity; },
    (parent, children) => { children[1].clientY = "1999"; },
    parent => { parent.clientX = NaN; },
    parent => { parent.offsetY = Infinity; },
    (parent, children) => { parent.offsetX = 3e38; children[1].clientX = 3e38; },
  ];
  for (const mutate of cases) {
    const h = rig(), children = [child(), child({ timeStamp: 1300.125 })], parent = history(children);
    mutate(parent, children); h.target.emit("pointermove", parent);
    assert.equal(h.batches.length, 0); assert.equal(h.sequenceCalls, 0); assert.equal(h.target.captures.length, 0);
    assert.equal(h.errors.length, 1); assert.equal(h.owner.closed, true);
  }
  const subNanosecond = rig();
  subNanosecond.target.emit("pointermove", history([child({ timeStamp: 1300.0000002 }), child({ timeStamp: 1300.0000001 })]));
  assert.equal(subNanosecond.sequenceCalls, 0); assert.equal(subNanosecond.batches.length, 0);
  const held = rig(); held.target.emit("pointerdown", event({ timeStamp: 1300, buttons: 1 }));
  held.target.emit("pointermove", history([child({ timeStamp: 1299.9999999, buttons: 1 }), child({ timeStamp: 1300, buttons: 1 })], { buttons: 1 }));
  assert.equal(held.batches.length, 1); assert.equal(held.sequenceCalls, 2);
  assert.equal(held.target.captures.length, 1); assert.equal(held.errors.length, 1);
});

test("coalesced sample and expanded DTO limits remain atomic and ownership loss never publishes an obsolete frozen history", () => {
  const samples = () => Array.from({ length: 256 }, (_, index) => child({ timeStamp: 1300 + index / 8,
    clientX: 1000 + index, clientY: 2000, buttons: index % 2 ? 0 : 7 }));
  const exact = rig({ firstSequence: 0n }); exact.target.emit("pointermove", history(samples()));
  assert.deepEqual(exact.errors, []); assert.equal(exact.batches.length, 1); assert.equal(exact.batches[0].length, 1024);
  assert.deepEqual(exact.batches[0].slice(0, 4).map(row => [row.control, row.state, row.sequence]),
    [[0, undefined, 0n], [1, 0, 1n], [2, 0, 2n], [3, 0, 3n]]);
  assert.deepEqual(exact.batches[0].slice(-4).map(row => [row.control, row.state, row.sequence]),
    [[0, undefined, 1020n], [1, 1, 1021n], [2, 1, 1022n], [3, 1, 1023n]]);
  assert.equal(exact.batches[0][0].x, 100); assert.equal(exact.batches[0][1020].x, 355);
  assert.equal(exact.batches[0][1020].hostNs, 1331875000n);
  assert.deepEqual(exact.target.captures, []); assert.deepEqual(exact.target.releases, []);
  exact.owner.close(); assert.equal(exact.batches.length, 1);
  for (const kind of ["257 children", "1025 DTOs"]) {
    const h = rig(), children = samples();
    if (kind === "257 children") children.push(child({ timeStamp: 1332 }));
    else children[255].buttons = 8;
    h.target.emit("pointermove", history(children, { buttons: kind === "1025 DTOs" ? 8 : 0 }));
    assert.equal(h.sequenceCalls, 0, kind); assert.equal(h.batches.length, 0, kind); assert.equal(h.target.captures.length, 0);
  }
  const full = rig();
  for (let pointerId = 0; pointerId < 64; pointerId++) full.target.emit("pointerdown", event({ pointerId, timeStamp: 1000 }));
  const before = full.sequenceCalls;
  full.target.emit("pointermove", history([child({ pointerId: 70, buttons: 1 }), child({ pointerId: 70, buttons: 0 })], { pointerId: 70 }));
  assert.equal(full.batches.length, 64); assert.equal(full.sequenceCalls, before, "transient history cannot exceed the shared held-identity capacity");
  assert.equal(full.errors.length, 1);
  for (const stage of ["method", "child getter", "sequence"]) {
    let h;
    h = rig({ beforeSequence: () => { if (stage === "sequence") { h.retire(); h.owner.close(); } } });
    const children = [child(), child({ timeStamp: 1300.25 })], parent = history(children);
    if (stage === "method") parent.getCoalescedEvents = () => { h.retire(); h.owner.close(); return children; };
    if (stage === "child getter") Object.defineProperty(children[1], "clientY", { get() { h.retire(); h.owner.close(); return 1999; } });
    h.target.emit("pointermove", parent);
    assert.equal(h.batches.length, 0); assert.equal(h.target.count(), 0); assert.equal(h.target.captures.length, 0);
    if (stage !== "sequence") assert.equal(h.sequenceCalls, 0);
  }
});

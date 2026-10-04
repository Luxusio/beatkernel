// Deferred actual acquisition component; only EventTarget/capture endpoints are scripted.
// No pointer hardware, coalesced input, rendering or judgment is exercised here.
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
    getCoalescedEvents() { assert.fail("this owner acquires only dispatched events"); },
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

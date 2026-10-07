// Deferred component fixtures: explicit setup ownership and literal Native rows.
// They exercise no browser pointer acquisition or judgment implementation.
import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { snapshotPointerSetup } from "./pointer-profile.mjs";

const MOUSE = 0x8877665544332211n;
const PEN = 18446744073709551615n;
function setup(rows = [[0x11, MOUSE, 1], [0x12, PEN, 32]], devices = [
  { source: MOUSE, pointerType: "mouse" }, { source: PEN, pointerType: "pen" },
]) {
  return { devices, bindingWords: new Uint32Array(rows.flatMap(([lane, source, control]) =>
    [lane, Number(source & 0xffffffffn), Number(source >> 32n), control])) };
}

test("pointer setup copies full-width descriptors and exact Native rows while only buttons establish press coverage", () => {
  const raw = setup([[0x13, MOUSE, 0], [0x11, MOUSE, 1], [0x12, PEN, 32]]);
  const saved = snapshotPointerSetup(raw);
  assert.deepEqual(saved.sources, [MOUSE, PEN]);
  assert.deepEqual(saved.devices, [{ source: MOUSE, pointerType: "mouse" }, { source: PEN, pointerType: "pen" }]);
  assert.deepEqual(saved.lanes, [0x11, 0x12], "position/displacement cannot cover a tap lane");
  assert.deepEqual(Array.from(saved.bindingWords), [
    0x13, 0x44332211, 0x88776655, 0,
    0x11, 0x44332211, 0x88776655, 1,
    0x12, 0xffffffff, 0xffffffff, 32,
  ]);
  const expected = [
    0x13, 1, 0x44332211, 0x88776655, 1, 0x574d4f55, 0,
    0x11, 1, 0x44332211, 0x88776655, 1, 0x574d4f55, 1,
    0x12, 1, 0xffffffff, 0xffffffff, 1, 0x5750454e, 32,
  ];
  assert.deepEqual(Array.from(saved.physicalWords), expected);
  assert.ok(Object.isFrozen(saved) && Object.isFrozen(saved.devices) && saved.devices.every(Object.isFrozen));
  assert.ok(Object.isFrozen(saved.sources) && Object.isFrozen(saved.lanes));
  assert.notEqual(saved.bindingWords.buffer, raw.bindingWords.buffer);
  assert.notEqual(saved.physicalWords.buffer, saved.bindingWords.buffer);
  raw.devices[0].source = 3n; raw.devices[1].pointerType = "mouse"; raw.devices.length = 0;
  raw.bindingWords.fill(0);
  assert.deepEqual(saved.sources, [MOUSE, PEN]); assert.equal(saved.devices[1].pointerType, "pen");
  assert.deepEqual(Array.from(saved.physicalWords), expected);
  saved.bindingWords.fill(0);
  assert.deepEqual(Array.from(saved.physicalWords), expected, "the returned ABI arrays own independent copies");
  assert.deepEqual(snapshotPointerSetup(setup([[0x11, 3n, 0]], [{ source: 3n, pointerType: "mouse" }])).lanes, []);
});

test("all 64 pointer identities and 256 rows retain exact bounds without aliases or truncated coverage", () => {
  const devices = Array.from({ length: 64 }, (_, index) => ({ source: PEN - 63n + BigInt(index),
    pointerType: index % 2 ? "pen" : "mouse" }));
  const rows = devices.flatMap(({ source }) => [[0x11, source, 0], [0x12, source, 1], [0x13, source, 2], [0x14, source, 3]]);
  const saved = snapshotPointerSetup(setup(rows, devices));
  assert.equal(saved.devices.length, 64); assert.equal(saved.sources.length, 64);
  assert.equal(new Set(saved.sources).size, 64);
  assert.equal(saved.bindingWords.length, 1024); assert.equal(saved.physicalWords.length, 1792);
  assert.deepEqual(saved.lanes, [0x12, 0x13, 0x14]);
  assert.deepEqual(Array.from(saved.physicalWords.slice(0, 7)), [0x11, 1, 0xffffffc0, 0xffffffff, 1, 0x574d4f55, 0]);
  assert.deepEqual(Array.from(saved.physicalWords.slice(-7)), [0x14, 1, 0xffffffff, 0xffffffff, 1, 0x5750454e, 3]);
  assert.throws(() => snapshotPointerSetup(setup([...rows, [0x15, devices[0].source, 4]], devices)));
  assert.throws(() => snapshotPointerSetup(setup(rows, [...devices, { source: 3n, pointerType: "mouse" }])));
  assert.throws(() => snapshotPointerSetup(setup([...rows.slice(0, 255), [0x19, devices[0].source, 0]], devices)),
    "one source/control cannot acquire a second lane, including position controls");
});

test("malformed pointer descriptors and foreign or invalid row storage refuse without changing the caller's setup", () => {
  const invalid = [null, {}, setup([], []), setup([], [{ source: MOUSE, pointerType: "mouse" }]),
    setup([[0x11, MOUSE, 1]], []), setup([[0x20, MOUSE, 1]]), setup([[0x11, MOUSE, 33]]),
    setup([[0x11, 3n, 1]]), setup([[0x11, MOUSE, 1], [0x12, MOUSE, 1]]),
    setup(undefined, [{ source: MOUSE, pointerType: "touch" }, { source: PEN, pointerType: "pen" }]),
    setup(undefined, [{ source: MOUSE, pointerType: "mouse" }, { source: MOUSE, pointerType: "pen" }]),
    { ...setup(), bindingWords: [0x11, 0x44332211, 0x88776655, 1] },
    { ...setup(), bindingWords: new Uint32Array(3) },
    { ...setup(), bindingWords: new Float32Array([17, 3, 0, 1]) },
    { ...setup(), bindingWords: runInNewContext("new Uint32Array([17,0x44332211,0x88776655,1])") },
  ];
  for (const source of [0n, 1n, 2n, -1n, PEN + 1n, Number(MOUSE), "3"])
    invalid.push(setup(undefined, [{ source, pointerType: "mouse" }, { source: PEN, pointerType: "pen" }]));
  const detached = setup(); structuredClone(detached.bindingWords.buffer, { transfer: [detached.bindingWords.buffer] });
  invalid.push(detached);
  if (typeof SharedArrayBuffer === "function") {
    const words = new Uint32Array(new SharedArrayBuffer(16)); words.set([0x11, 0x44332211, 0x88776655, 1]);
    invalid.push({ ...setup(), bindingWords: words });
  }
  if (typeof ArrayBuffer.prototype.resize === "function") {
    const words = new Uint32Array(new ArrayBuffer(16, { maxByteLength: 32 })); words.set([0x11, 0x44332211, 0x88776655, 1]);
    invalid.push({ ...setup(), bindingWords: words });
  }
  for (const input of invalid) assert.throws(() => snapshotPointerSetup(input));
  const raw = setup([[0x11, MOUSE, 1], [0x12, MOUSE, 1]]);
  const before = structuredClone(raw);
  assert.throws(() => snapshotPointerSetup(raw));
  assert.deepEqual(raw, before);
  raw.bindingWords[7] = 2;
  assert.deepEqual(snapshotPointerSetup(raw).lanes, [0x11, 0x12], "refusal consumes no caller configuration");
});

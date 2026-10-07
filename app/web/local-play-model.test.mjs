// Deferred source fixtures for the actual numeric local-player boundary.
import assert from "node:assert/strict";
import test from "node:test";
import { snapshotLocalPlan, localBindingWords } from "./local-play-model.mjs";

const MAX = 18446744073709551615n;
const HIGH = 9007199254740993n;
const KEYBOARD = 0x574b4559;
const TOUCH = 0x57544f55;
function planRows(rows) {
  return new Uint32Array(rows.flatMap(([player, source]) => source === null
    ? [player, 0, 0, 0] : [player, 1, Number(source & 0xffffffffn), Number(source >> 32n)]));
}
function exact(lane, source, kind = 1, namespace = 0x57475044, code = 0) {
  return [lane, 1, Number(source & 0xffffffffn), Number(source >> 32n), kind, namespace, code];
}
function rows(words, length) {
  return Array.from({ length: words.length / length }, (_, index) => Array.from(words.slice(index * length, (index + 1) * length)));
}

test("local plan snapshots preserve sparse member order, full source bits and caller-independent ownership", () => {
  const words = planRows([[0xffffffff, MAX], [7, 0n], [91, HIGH], [2, 1n]]);
  const expected = Array.from(words);
  const saved = snapshotLocalPlan(words);
  words.fill(0);
  assert.deepEqual(saved.members, [
    { player: 0xffffffff, source: MAX }, { player: 7, source: 0n },
    { player: 91, source: HIGH }, { player: 2, source: 1n },
  ]);
  assert.deepEqual(Array.from(saved.words), expected);
  assert.notEqual(saved.words.buffer, words.buffer);
  assert.ok(Object.isFrozen(saved)); assert.ok(Object.isFrozen(saved.members));
  assert.ok(saved.members.every(Object.isFrozen));
  const automatic = snapshotLocalPlan(planRows([[3, null]]));
  assert.deepEqual(automatic.members, [{ player: 3, source: null }]);
  const sixtyFour = snapshotLocalPlan(planRows(Array.from({ length: 64 }, (_, index) => [index + 1, BigInt(index)])));
  assert.equal(sixtyFour.members.length, 64); assert.equal(sixtyFour.members[63].source, 63n);
  for (const invalid of [null, [], new Float32Array(4), new Uint32Array(), new Uint32Array(5),
    new Uint32Array([0, 1, 0, 0]), new Uint32Array([1, 2, 0, 0]),
    new Uint32Array([1, 0, 1, 0]), new Uint32Array([1, 0, 0, 1]),
    planRows([[1, null], [2, 3n]]), planRows([[1, 3n], [1, 4n]]), planRows([[1, 3n], [2, 3n]]),
    planRows(Array.from({ length: 65 }, (_, index) => [index + 1, BigInt(index)]))]) {
    assert.throws(() => snapshotLocalPlan(invalid));
  }
  assert.deepEqual(Array.from(saved.words), expected);
});

test("one keyboard, one touch surface and disjoint full-width devices produce exact member bindings without source leakage", () => {
  const plan = snapshotLocalPlan(planRows([[99, MAX], [7, 1n], [0xffffffff, 2n], [31, HIGH]]));
  const physical = new Uint32Array([
    0x11, 0, 0, 0, 1, KEYBOARD, 2, 0x12, 0, 0, 0, 1, KEYBOARD, 3,
    ...exact(0x11, MAX, 1, 0x57475044, 0), ...exact(0x12, MAX, 1, 0x57475044, 0x30001),
    ...exact(0x11, HIGH, 0, 9, 1), ...exact(0x12, HIGH, 2, 0xffffffff, 0x80000000),
    ...exact(0x11, 123n, 0, 9, 2),
  ]);
  const admitted = localBindingWords(plan, physical, new Uint8Array([0x11, 0x12]), true);
  physical.fill(0);
  assert.equal(admitted.touchPlayer, 0xffffffff);
  const byPlayer = player => rows(admitted.words, 8).filter(row => row[0] === player);
  assert.deepEqual(byPlayer(7), [[7, 0x11, 1, 1, 0, 1, KEYBOARD, 2], [7, 0x12, 1, 1, 0, 1, KEYBOARD, 3]]);
  assert.deepEqual(byPlayer(0xffffffff), [[0xffffffff, 0x11, 1, 2, 0, 1, TOUCH, 0], [0xffffffff, 0x12, 1, 2, 0, 1, TOUCH, 0]]);
  assert.deepEqual(byPlayer(99), [[99, ...exact(0x11, MAX, 1, 0x57475044, 0)], [99, ...exact(0x12, MAX, 1, 0x57475044, 0x30001)]]);
  assert.deepEqual(byPlayer(31), [[31, ...exact(0x11, HIGH, 0, 9, 1)], [31, ...exact(0x12, HIGH, 2, 0xffffffff, 0x80000000)]]);
  assert.equal(admitted.words.length, 8 * 8);
  assert.ok(rows(admitted.words, 8).every(row => row[2] === 1));
  const auto = snapshotLocalPlan(planRows([[42, null]]));
  const originals = new Uint32Array([0x11, 0, 0, 0, 1, KEYBOARD, 2, ...exact(0x12, MAX, 0, 7, 4)]);
  const solo = localBindingWords(auto, originals, [0x11, 0x12], false);
  assert.equal(solo.touchPlayer, null);
  assert.deepEqual(rows(solo.words, 8), [[42, ...Array.from(originals.slice(0, 7))], [42, ...Array.from(originals.slice(7))]]);
  const contact = localBindingWords(auto, new Uint32Array(), [0x11, 0x12], true);
  assert.equal(contact.touchPlayer, 42); assert.equal(contact.words.length, 16);
});

test("invalid or ambiguous physical rows and missing member coverage refuse atomically even for otherwise unused devices", () => {
  const plan = snapshotLocalPlan(planRows([[7, 3n], [8, MAX]]));
  const good = new Uint32Array([...exact(0x11, 3n), ...exact(0x11, MAX)]);
  const retained = localBindingWords(plan, good, [0x11], false);
  const expected = Array.from(retained.words);
  const badRows = [[], Array.from(good.slice(0, 13)), [...good, 0], [...good, ...exact(0x11, 3n)],
    [...exact(0x11, 3n)], [...exact(0x11, 3n), ...exact(0x12, MAX)],
    [0x11, 0, 0, 0, 0, 7, 4, ...exact(0x11, MAX)],
    [0x11, 0, 1, 0, 1, KEYBOARD, 2, ...exact(0x11, MAX)]];
  for (const [index, value] of [[0, 0x10], [1, 2], [4, 3], [5, 65536], [6, 65536]]) {
    const unused = exact(0x11, 123n, 0, 7, 4); unused[index] = value;
    badRows.push([...good, ...unused]);
  }
  for (const words of badRows) assert.throws(() => localBindingWords(plan, new Uint32Array(words), [0x11], false));
  for (const value of [null, [], new Float32Array(14)]) assert.throws(() => localBindingWords(plan, value, [0x11], false));
  for (const lanes of [[0x11, 0x11], [0x10], [0x11, 0x12], new Array(19).fill(0x11), [undefined]]) {
    assert.throws(() => localBindingWords(plan, good, lanes, false));
  }
  for (const choice of [undefined, null, 0, "true"]) assert.throws(() => localBindingWords(plan, good, [0x11], choice));
  assert.deepEqual(Array.from(retained.words), expected);
  assert.equal(localBindingWords(plan, new Uint32Array(), [], false).words.length, 0);
});

test("the 256-row ceiling includes generated touch rows and remains separate from the 64-member roster bound", () => {
  const plan = snapshotLocalPlan(planRows([[7, 3n], [8, 2n]]));
  const physical = length => new Uint32Array(Array.from({ length }, (_, index) => exact(0x11, 3n, 1, 99, index)).flat());
  const boundary = localBindingWords(plan, physical(255), [0x11], true);
  assert.equal(boundary.words.length, 256 * 8); assert.equal(boundary.touchPlayer, 8);
  assert.throws(() => localBindingWords(plan, physical(256), [0x11], true));
  assert.throws(() => localBindingWords(snapshotLocalPlan(planRows([[7, 3n]])), physical(257), [0x11], false));
  assert.equal(localBindingWords(snapshotLocalPlan(planRows([[7, 3n]])), physical(256), [0x11], false).words.length, 2048);
  const members = Array.from({ length: 64 }, (_, index) => [index + 1, BigInt(index) + 3n]);
  const all = snapshotLocalPlan(planRows(members));
  const bindings = new Uint32Array(members.flatMap(([, source]) => exact(0x11, source)));
  const ready = localBindingWords(all, bindings, [0x11], false);
  assert.equal(ready.words.length, 64 * 8); assert.equal(ready.touchPlayer, null);
  assert.deepEqual(new Set(rows(ready.words, 8).map(row => row[0])), new Set(members.map(([player]) => player)));
});

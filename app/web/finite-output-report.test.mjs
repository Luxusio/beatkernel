// Independent report ABI and adoption oracles. Real Mixer/Worklet expiry is
// exercised separately by Rust and fresh-WASM export fixtures.
import assert from "node:assert/strict";
import test from "node:test";
import { reportWord, renderedCursor } from "./play-model.mjs";

const MAX = 18446744073709551615n;
function put(words, field, value) {
  words[field * 2] = Number(value & 0xffffffffn);
  words[field * 2 + 1] = Number(value >> 32n);
}
function terminal() {
  const words = new Uint32Array(58);
  for (const [field, value] of [[0, 1n], [1, 4n], [2, 2n], [3, 3n],
    [5, 1n], [6, 1n], [7, 3n], [12, 6n], [13, 3n], [14, 1n],
    [23, 1n], [24, 106n], [25, 1n], [26, 100n], [28, 2n]]) put(words, field, value);
  return { available: true, words };
}

test("appended expiry retains its exact u64 width and original report indices", () => {
  const report = terminal();
  assert.equal(report.words.byteLength, 232);
  assert.equal(reportWord(report.words, 28), 2n);
  assert.equal(reportWord(report.words, 13), 3n);
  assert.equal(reportWord(report.words, 14), 1n);
  assert.equal(renderedCursor(report, 100n), 3n);
  put(report.words, 28, MAX - 1n);
  put(report.words, 13, MAX);
  assert.equal(reportWord(report.words, 28), MAX - 1n);
  assert.equal(renderedCursor(report, 100n), 3n, "transport preserves wide counters; completion owns saturation proof");
});

test("unavailable, unlimited, unpaused and overflowing expiry cannot establish playback evidence", () => {
  for (const mutate of [
    value => { value.available = false; put(value.words, 0, 0n); },
    value => put(value.words, 6, 0n),
    value => put(value.words, 5, 0n),
    value => put(value.words, 13, 2n),
    value => put(value.words, 14, MAX),
    value => put(value.words, 28, MAX),
  ]) {
    const value = terminal(); mutate(value);
    assert.throws(() => renderedCursor(value, 100n));
  }
});

test("old and mixed report versions, detached, shared, resizable and offset backings are refused", () => {
  const detached = new Uint32Array(58);
  structuredClone(detached.buffer, { transfer: [detached.buffer] });
  const resizable = new ArrayBuffer(232, { maxByteLength: 464 });
  for (const words of [new Uint32Array(56), new Uint32Array(57), new Uint32Array(59),
    new Uint32Array(new ArrayBuffer(236), 0, 58), new Uint32Array(new ArrayBuffer(236), 4, 58),
    new Uint32Array(new SharedArrayBuffer(232)), new Uint32Array(resizable, 0, 58),
    detached, new Int32Array(58), Array(58).fill(0)]) {
    assert.throws(() => reportWord(words, 28));
    assert.throws(() => renderedCursor({ available: true, words }, 100n));
  }
});

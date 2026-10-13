// Diagnostic arithmetic only: actual native onset admission is a browser-fixture obligation.
import assert from "node:assert/strict";
import test from "node:test";
import { markerTime, markerFrame, advanceFrame } from "./worklet-native-frame-probe.mjs";

const rates = [44100, 48000];
const extents = [2, 3, 7, 17, 64, 128, 129, 257];
const invalidIntegers = [-1, 0.5, NaN, Infinity, -Infinity, Number.MAX_SAFE_INTEGER + 1,
  "128", 128n, null, undefined, true, {}, []];

function edge(frames, index) {
  const values = new Float32Array(frames);
  values.fill(1, index);
  return values;
}

function unchangedMarker(values, frames, target, expected) {
  const before = Array.from(values);
  assert.equal(markerFrame(values, frames, target), expected);
  assert.deepEqual(Array.from(values), before, "the diagnostic cannot rewrite native samples");
}

for (const rate of rates) {
  test(`markerTime lands inside the native onset interval at ${rate}Hz`, () => {
    for (const target of [1, 2, 63, 127, 128, 896, 1664, rate, 20 * 3600 * rate,
      7 * 24 * 3600 * rate]) {
      const time = markerTime(target, rate);
      assert.equal(typeof time, "number", `target ${target}`);
      assert.ok(Number.isFinite(time));
      const scaled = time * rate;
      assert.ok(scaled > target - 1 && scaled < target, `strict onset interval at ${target}`);
      assert.equal(Math.ceil(scaled), target);
      assert.equal(time, (target - 0.25) / rate, "the planned fractional scheduling offset is preserved");
    }
  });

  test(`every interior edge recovers its known native origin at ${rate}Hz`, () => {
    for (const origin of [0, rate, 20 * 3600 * rate, 7 * 24 * 3600 * rate]) {
      for (const frames of extents) {
        for (let index = 1; index < frames; index++) {
          unchangedMarker(edge(frames, index), frames, origin + index, origin);
        }
      }
    }
  });

  test(`independent known onsets agree across actual variable extents at ${rate}Hz`, () => {
    const origin = 7 * 24 * 3600 * rate;
    let cursor = markerFrame(edge(17, 5), 17, origin + 5);
    assert.equal(cursor, origin);
    for (const frames of [17, 0, 3, 129, 64, 257]) cursor = advanceFrame(cursor, frames);
    const secondOrigin = origin + 17 + 3 + 129 + 64 + 257;
    assert.equal(cursor, secondOrigin);
    assert.equal(markerFrame(edge(7, 6), 7, secondOrigin + 6), cursor);
    assert.notEqual(markerFrame(edge(7, 6), 7, secondOrigin + 7), cursor,
      "a contradictory second actual onset is exposed instead of rebasing the first");
  });
}

test("markerTime rejects invalid frame and sample-rate metadata without coercion", () => {
  for (const value of [0, ...invalidIntegers]) {
    assert.equal(markerTime(value, 48000), null, `invalid target ${String(value)}`);
    assert.equal(markerTime(128, value), null, `invalid rate ${String(value)}`);
  }
  assert.equal(markerTime(), null);
});

test("markerTime refuses safe-integer targets whose fractional interval is lost", () => {
  for (const rate of [1, ...rates, Number.MAX_SAFE_INTEGER]) {
    for (const target of [2 ** 52, Number.MAX_SAFE_INTEGER]) {
      assert.equal(markerTime(target, rate), null, `${target}/${rate} is ambiguous`);
    }
  }
  assert.equal(markerTime(2 ** 51, 48000), null,
    "division/multiplication can lose the strict upper boundary before integer safety runs out");
  const stillRepresentable = markerTime(2 ** 51, 44100);
  assert.ok(stillRepresentable !== null);
  assert.equal(Math.ceil(stillRepresentable * 44100), 2 ** 51);
  assert.ok(stillRepresentable * 44100 < 2 ** 51);
});

test("markerTime accepts the full positive safe-integer rate contract", () => {
  const time = markerTime(1, Number.MAX_SAFE_INTEGER);
  assert.ok(time !== null);
  assert.ok(time * Number.MAX_SAFE_INTEGER > 0 && time * Number.MAX_SAFE_INTEGER < 1);
  assert.equal(Math.ceil(time * Number.MAX_SAFE_INTEGER), 1);
});

test("integer-time scheduling has the pinned one-sample floating-point counterexamples", () => {
  for (const [target, rate, scaled] of [
    [1664, 44100, 1664.0000000000002],
    [896, 48000, 896.0000000000001],
  ]) {
    assert.equal((target / rate) * rate, scaled);
    assert.equal(Math.ceil((target / rate) * rate), target + 1);
    const fractionalTime = markerTime(target, rate);
    assert.ok(fractionalTime !== null);
    assert.equal(Math.ceil(fractionalTime * rate), target);
  }
});

test("a spatial interior edge cannot authenticate an arbitrary late/reset scheduled onset", () => {
  for (const target of [896, 1664]) {
    const lateResetArray = edge(128, 1);
    const spatialCandidate = markerFrame(lateResetArray, 128, target);
    assert.equal(spatialCandidate, target - 1);
    // If native clamping moved the actual onset to target+1, this identical array
    // belongs to origin target. The caller must establish actual onset independently.
    const knownActualOnset = target + 1;
    assert.equal(markerFrame(lateResetArray, 128, knownActualOnset), target);
    assert.notEqual(spatialCandidate, target,
      "accepting the shape alone would mis-certify the origin by one sample");
  }
});

test("markerFrame refuses empty, single-sample, constant and index-zero arrays", () => {
  for (const frames of [0, 1, 2, 17, 128, 257]) {
    unchangedMarker(new Float32Array(frames), frames, 1000, null);
    unchangedMarker(new Float32Array(frames).fill(1), frames, 1000, null);
  }
});

test("markerFrame refuses nonbinary, nonfinite, reversed and repeated transitions", () => {
  for (const values of [
    [0, 0.5, 1], [0, -1, 1], [0, 2, 1], [0, NaN, 1],
    [0, Infinity, 1], [0, -Infinity, 1], [1, 0, 0], [0, 1, 0],
    [0, 1, 0, 1], [0, 0, 1, 1, 0], [0, 0, 1, 0.5],
  ]) {
    const array = Float32Array.from(values);
    unchangedMarker(array, array.length, 1000, null);
  }
});

test("markerFrame requires the exact full Float32Array callback shape", () => {
  const values = edge(7, 3);
  for (const actualFrames of [0, 1, 6, 8, ...invalidIntegers]) {
    unchangedMarker(values, actualFrames, 1000, null);
  }
  for (const other of [[0, 1], new Float64Array([0, 1]), new Uint8Array([0, 1]),
    { 0: 0, 1: 1, length: 2 }, null, undefined]) {
    assert.equal(markerFrame(other, 2, 1000), null);
  }
  assert.equal(markerFrame(), null);
});

test("markerFrame validates onset metadata and refuses an origin before frame zero", () => {
  const values = edge(7, 3);
  for (const target of [0, 1, 2, ...invalidIntegers]) {
    unchangedMarker(values, values.length, target, null);
  }
  unchangedMarker(values, values.length, 3, 0);
  unchangedMarker(values, values.length, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER - 3);
});

test("markerFrame can read a full native view without changing it or neighboring storage", () => {
  const storage = Float32Array.from([9, 0, 0, 1, 1, 8]);
  const before = storage.slice();
  const view = storage.subarray(1, 5);
  unchangedMarker(view, 4, 102, 100);
  assert.deepEqual(storage, before);
});

test("advanceFrame uses actual extents including zero and long-duration sample positions", () => {
  assert.equal(advanceFrame(0, 0), 0);
  assert.equal(advanceFrame(0, 1), 1);
  for (const rate of rates) {
    for (const origin of [0, 20 * 3600 * rate, 7 * 24 * 3600 * rate]) {
      let cursor = origin;
      for (const frames of [0, 1, 3, 17, 64, 129, 257]) cursor = advanceFrame(cursor, frames);
      assert.equal(cursor, origin + 1 + 3 + 17 + 64 + 129 + 257);
      assert.equal(advanceFrame(cursor, 0), cursor);
    }
  }
});

test("advanceFrame rejects invalid operands and unsafe sums without coercion", () => {
  for (const value of invalidIntegers) {
    assert.equal(advanceFrame(value, 128), null);
    assert.equal(advanceFrame(1000, value), null);
  }
  assert.equal(advanceFrame(), null);
  assert.equal(advanceFrame(Number.MAX_SAFE_INTEGER, 0), Number.MAX_SAFE_INTEGER);
  assert.equal(advanceFrame(Number.MAX_SAFE_INTEGER - 128, 128), Number.MAX_SAFE_INTEGER);
  assert.equal(advanceFrame(0, Number.MAX_SAFE_INTEGER), Number.MAX_SAFE_INTEGER);
  assert.equal(advanceFrame(Number.MAX_SAFE_INTEGER, 1), null);
  assert.equal(advanceFrame(Number.MAX_SAFE_INTEGER - 128, 129), null);
  assert.equal(advanceFrame(Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER), null);
});

// Diagnostic arithmetic only: actual native onset admission is a browser-fixture obligation.
import assert from "node:assert/strict";
import test from "node:test";
import {
  markerTime, markerFrame, advanceFrame, nativePhase, phaseIdeal, phaseEvidence,
  nativeJointOffset,
} from "./worklet-native-frame-probe.mjs";

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

// These expectations come from the frozen event-time contract and rational
// phase sequence, not from another exported diagnostic helper. Synthetic
// twenty-hour/week origins exercise arithmetic; they are not native-age evidence.
function eventTimeIdeal(frame, startFrame, sampleRate, period) {
  const phase = Number((BigInt(frame) - BigInt(startFrame)) % BigInt(period));
  const cycleStart = frame - phase;
  const firstEventTime = (cycleStart - 0.25) / sampleRate;
  const nextEventTime = (cycleStart + period - 0.25) / sampleRate;
  return 1 + period * (frame / sampleRate - firstEventTime)
    / (nextEventTime - firstEventTime);
}

function phaseSamples(frame, startFrame, frames, period) {
  return Float32Array.from({ length: frames }, (_, index) =>
    1.25 + Number((BigInt(frame) + BigInt(index) - BigInt(startFrame)) % BigInt(period)));
}

function expectedEvidence(values, frame, startFrame, sampleRate, period) {
  let maxError = 0;
  for (let index = 0; index < values.length; index++) {
    maxError = Math.max(maxError,
      Math.abs(values[index] - eventTimeIdeal(frame + index, startFrame, sampleRate, period)));
  }
  const phaseAt = offset => Number((BigInt(frame) + BigInt(offset)
    - BigInt(startFrame)) % BigInt(period));
  return {
    checkedSamples: values.length,
    maxError,
    firstPhase: phaseAt(0),
    lastPhase: phaseAt(values.length - 1),
  };
}

function unchangedEvidence(values, actualFrames, frame, startFrame, sampleRate,
  horizonFrame, period, expected) {
  const before = values.slice();
  const evidence = phaseEvidence(values, actualFrames, frame, startFrame,
    sampleRate, horizonFrame, period);
  assert.deepEqual(evidence, expected);
  assert.deepEqual(values, before, "phase validation cannot rewrite native samples");
  return evidence;
}

const phasePeriods = [2, 3, 7, 17, 128, 257, 2 ** 20];
const invalidPeriods = [0, 1, 2 ** 20 + 1, ...invalidIntegers];
const invalidSamples = [NaN, Infinity, -Infinity, "1.25", 1n, null, undefined,
  true, {}, []];

test("nativePhase decodes quarter-offset samples and the terminal reset code", () => {
  for (const period of phasePeriods) {
    for (const phase of new Set([0, 1, Math.floor(period / 2), period - 1])) {
      assert.equal(nativePhase(phase + 1.25, period), phase);
      assert.equal(nativePhase(phase + 1, period), phase);
    }
    assert.equal(nativePhase(period + 1, period), 0, "terminal code wraps to phase zero");
    assert.equal(nativePhase(period + 1.25, period), 0,
      "decoding alone remains a modulo candidate, not an ideal-ramp check");
    assert.equal(nativePhase(0.75, period), 0);
    assert.equal(nativePhase(0, period), null);
    assert.equal(nativePhase(-1, period), null);
    assert.equal(nativePhase(period + 2, period), null);
  }
});

test("nativePhase refuses invalid metadata, nonfinite values and exact half ties", () => {
  for (const period of invalidPeriods) assert.equal(nativePhase(1.25, period), null);
  for (const value of invalidSamples) assert.equal(nativePhase(value, 128), null);
  for (const value of [0.5, 1.5, 2.5, 64.5, 128.5, 129.5]) {
    assert.equal(nativePhase(value, 128), null, `ambiguous half tie ${value}`);
  }
  assert.equal(nativePhase(1.5 - Number.EPSILON, 128), 0);
  assert.equal(nativePhase(1.5 + Number.EPSILON, 128), 1);
  assert.equal(nativePhase(129.5 - 2 ** -40, 128), 0);
  assert.equal(nativePhase(129.5 + 2 ** -40, 128), null);
  assert.equal(nativePhase(Number.MAX_VALUE, 128), null);
  assert.equal(nativePhase(), null);
});

for (const sampleRate of rates) {
  test(`phaseIdeal preserves quarter-offset first sample and resets at ${sampleRate}Hz`, () => {
    for (const startFrame of [1, 1024]) {
      for (const period of phasePeriods) {
        for (const offset of new Set([0, 1, Math.floor(period / 2), period - 1,
          period, period + 1, 2 * period - 1, 2 * period])) {
          const frame = startFrame + offset;
          const expected = eventTimeIdeal(frame, startFrame, sampleRate, period);
          assert.equal(phaseIdeal(frame, startFrame, sampleRate, period), expected);
          const rationalIdeal = 1.25 + offset % period;
          assert.ok(Math.abs(expected - rationalIdeal) < 0.000001,
            `quarter-offset ideal at period ${period}, offset ${offset}`);
        }
      }
    }
  });

  test(`phaseIdeal twenty-hour/week origins are arithmetic vectors at ${sampleRate}Hz`, () => {
    for (const startFrame of [20 * 3600 * sampleRate + 1,
      7 * 24 * 3600 * sampleRate + 1]) {
      for (const period of phasePeriods) {
        for (const offset of [0, period - 1, period, 2 * period - 1]) {
          const frame = startFrame + offset;
          const expected = eventTimeIdeal(frame, startFrame, sampleRate, period);
          assert.equal(phaseIdeal(frame, startFrame, sampleRate, period), expected);
          assert.ok(Math.abs(expected - (1.25 + offset % period)) < 0.01,
            "representable long-origin arithmetic retains a sub-quarter error margin");
        }
      }
    }
  });

  test(`phaseEvidence checks full variable extents and resets at ${sampleRate}Hz`, () => {
    for (const startFrame of [1, 1024, 20 * 3600 * sampleRate + 1,
      7 * 24 * 3600 * sampleRate + 1]) {
      for (const period of phasePeriods) {
        const horizonFrame = startFrame + period * (3 + Math.ceil(257 / period));
        for (const frames of [1, ...extents]) {
          for (const offset of new Set([0, period - 1, period, 2 * period - 1])) {
            const frame = startFrame + offset;
            const values = phaseSamples(frame, startFrame, frames, period);
            const expected = expectedEvidence(values, frame, startFrame, sampleRate, period);
            assert.ok(expected.maxError < 0.25);
            unchangedEvidence(values, frames, frame, startFrame, sampleRate,
              horizonFrame, period, expected);
          }
        }
      }
    }
  });
}

test("phaseIdeal has exactly 1.25 at the first sample and after each reset", () => {
  const startFrame = 1;
  const period = 17;
  for (const cycle of [0, 1, 2, 100]) {
    const frame = startFrame + cycle * period;
    assert.equal(phaseIdeal(frame, startFrame, 1, period), 1.25);
    assert.equal(phaseIdeal(frame + period - 1, startFrame, 1, period), period + 0.25);
  }
});

test("phase helpers accept the full positive safe-integer sample-rate contract", () => {
  for (const sampleRate of [1, Number.MAX_SAFE_INTEGER]) {
    const startFrame = 1;
    const period = 2;
    for (const frame of [1, 2, 3, 4, 5, 6]) {
      assert.equal(phaseIdeal(frame, startFrame, sampleRate, period),
        eventTimeIdeal(frame, startFrame, sampleRate, period));
      const values = phaseSamples(frame, startFrame, 1, period);
      unchangedEvidence(values, 1, frame, startFrame, sampleRate, 7, period,
        expectedEvidence(values, frame, startFrame, sampleRate, period));
    }
  }
});

test("phaseIdeal rejects malformed coordinates, periods, overflow and lost event precision", () => {
  for (const value of invalidIntegers) {
    assert.equal(phaseIdeal(value, 1, 48000, 128), null);
    assert.equal(phaseIdeal(128, value, 48000, 128), null);
    assert.equal(phaseIdeal(128, 1, value, 128), null);
  }
  for (const period of invalidPeriods) assert.equal(phaseIdeal(128, 1, 48000, period), null);
  assert.equal(phaseIdeal(0, 1, 48000, 128), null);
  assert.equal(phaseIdeal(128, 0, 48000, 128), null);
  assert.equal(phaseIdeal(128, 1, 0, 128), null);
  assert.equal(phaseIdeal(127, 128, 48000, 128), null);
  assert.equal(phaseIdeal(Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER - 1, 1, 2), null,
    "a safe frame does not make the following cycle boundary safe");
  for (const sampleRate of [1, ...rates, Number.MAX_SAFE_INTEGER]) {
    assert.equal(phaseIdeal(2 ** 52, 2 ** 52, sampleRate, 2), null,
      "the quarter-frame event must still have a strict representable interval");
  }
  assert.equal(phaseIdeal(2 ** 51, 2 ** 51, 48000, 2), null,
    "the pinned division/multiplication precision loss invalidates the first event");
  assert.equal(phaseIdeal(2 ** 51 - 2, 2 ** 51 - 2, 48000, 2), null,
    "a lost next event boundary also invalidates the active cycle");
  assert.equal(phaseIdeal(), null);
});

test("phaseEvidence reports first/last reset phases and the largest measured error", () => {
  const startFrame = 1;
  const period = 7;
  const frame = startFrame + period - 1;
  const values = phaseSamples(frame, startFrame, 4, period);
  assert.deepEqual(Array.from(values), [7.25, 1.25, 2.25, 3.25]);
  values[1] += 0.125;
  values[3] -= 0.0625;
  unchangedEvidence(values, 4, frame, startFrame, 1, startFrame + 3 * period,
    period, { checkedSamples: 4, maxError: 0.125, firstPhase: 6, lastPhase: 2 });
});

test("phaseEvidence refuses corruption at every sample, including middle and reset samples", () => {
  const startFrame = 1;
  const period = 17;
  const frame = startFrame + 13;
  const horizonFrame = startFrame + 40 * period;
  for (const frames of [1, 2, 3, 7, 17, 64, 128, 129, 257]) {
    const pristine = phaseSamples(frame, startFrame, frames, period);
    for (let index = 0; index < frames; index++) {
      for (const corruption of [pristine[index] + 1, NaN, Infinity, -Infinity]) {
        const values = pristine.slice();
        values[index] = corruption;
        unchangedEvidence(values, frames, frame, startFrame, 1,
          horizonFrame, period, null);
      }
    }
  }
});

test("phaseEvidence uses a strict quarter-sample error bound and refuses half ties", () => {
  const startFrame = 1;
  const period = 7;
  const pristine = phaseSamples(startFrame, startFrame, period, period);
  for (let index = 0; index < pristine.length; index++) {
    const onErrorBoundary = pristine.slice();
    onErrorBoundary[index] -= 0.25;
    unchangedEvidence(onErrorBoundary, period, startFrame, startFrame, 1,
      startFrame + 3 * period, period, null);
    const halfTie = pristine.slice();
    halfTie[index] += 0.25;
    unchangedEvidence(halfTie, period, startFrame, startFrame, 1,
      startFrame + 3 * period, period, null);
  }
  const withinMargin = Float32Array.from(pristine, value => value - 0.125);
  unchangedEvidence(withinMargin, period, startFrame, startFrame, 1,
    startFrame + 3 * period, period,
    { checkedSamples: period, maxError: 0.125, firstPhase: 0, lastPhase: period - 1 });
});

test("phaseEvidence refuses constant fallback contradictions and scalar callback shape", () => {
  const startFrame = 1;
  const period = 128;
  const horizonFrame = startFrame + 3 * period;
  for (const frames of [2, 3, 17, 128, 129, 257]) {
    for (const held of [0, 1, 1.25, 17.25, period + 1]) {
      unchangedEvidence(new Float32Array(frames).fill(held), frames, startFrame,
        startFrame, 1, horizonFrame, period, null);
    }
  }
  unchangedEvidence(new Float32Array([1.25]), 128, startFrame, startFrame,
    1, horizonFrame, period, null);
  unchangedEvidence(new Float32Array([1.25]), 1, startFrame, startFrame,
    1, horizonFrame, period,
    { checkedSamples: 1, maxError: 0, firstPhase: 0, lastPhase: 0 });
});

test("phaseEvidence requires exact full Float32Array shape without coercion", () => {
  const values = phaseSamples(1, 1, 7, 17);
  for (const actualFrames of [0, 1, 6, 8, ...invalidIntegers]) {
    unchangedEvidence(values, actualFrames, 1, 1, 1, 52, 17, null);
  }
  for (const other of [Array.from(values), Float64Array.from(values),
    Uint8Array.from(values), { ...values, length: 7 }, null, undefined]) {
    assert.equal(phaseEvidence(other, 7, 1, 1, 1, 52, 17), null);
  }
  unchangedEvidence(new Float32Array(0), 0, 1, 1, 1, 52, 17, null);
  assert.equal(phaseEvidence(), null);
});

test("phaseEvidence validates all frame, rate and horizon metadata", () => {
  const values = phaseSamples(1, 1, 7, 17);
  for (const value of invalidIntegers) {
    unchangedEvidence(values, 7, value, 1, 1, 52, 17, null);
    unchangedEvidence(values, 7, 1, value, 1, 52, 17, null);
    unchangedEvidence(values, 7, 1, 1, value, 52, 17, null);
    unchangedEvidence(values, 7, 1, 1, 1, value, 17, null);
  }
  for (const period of invalidPeriods) {
    unchangedEvidence(values, 7, 1, 1, 1, 52, period, null);
  }
  unchangedEvidence(values, 7, 0, 1, 1, 52, 17, null);
  unchangedEvidence(values, 7, 1, 0, 1, 52, 17, null);
  unchangedEvidence(values, 7, 1, 1, 0, 52, 17, null);
  unchangedEvidence(values, 7, 1, 1, 1, 0, 17, null);
  for (const horizonFrame of [1, 2, 17, 19, 51, 53]) {
    unchangedEvidence(values, 7, 1, 1, 1, horizonFrame, 17, null);
  }
});

test("phaseEvidence permits anchored horizon boundary slices and refuses crossing or hold", () => {
  const startFrame = 1;
  const period = 17;
  const horizonFrame = startFrame + 3 * period;
  const first = phaseSamples(startFrame, startFrame, 1, period);
  unchangedEvidence(first, 1, startFrame, startFrame, 1, horizonFrame, period,
    { checkedSamples: 1, maxError: 0, firstPhase: 0, lastPhase: 0 });
  const last = phaseSamples(horizonFrame - 1, startFrame, 1, period);
  unchangedEvidence(last, 1, horizonFrame - 1, startFrame, 1, horizonFrame, period,
    { checkedSamples: 1, maxError: 0, firstPhase: period - 1, lastPhase: period - 1 });
  unchangedEvidence(phaseSamples(horizonFrame - 2, startFrame, 2, period), 2,
    horizonFrame - 2, startFrame, 1, horizonFrame, period,
    { checkedSamples: 2, maxError: 0, firstPhase: period - 2, lastPhase: period - 1 });
  unchangedEvidence(phaseSamples(horizonFrame - 1, startFrame, 2, period), 2,
    horizonFrame - 1, startFrame, 1, horizonFrame, period, null);
  unchangedEvidence(new Float32Array([period + 1]), 1, horizonFrame, startFrame,
    1, horizonFrame, period, null);
  unchangedEvidence(new Float32Array([1.25]), 1, horizonFrame, startFrame,
    1, horizonFrame, period, null);
  unchangedEvidence(first, 1, startFrame - 1, startFrame, 1, horizonFrame, period, null);
});

test("phaseEvidence rejects safe-coordinate overflow and unrepresentable event boundaries", () => {
  const values = new Float32Array([1.25, 2.25]);
  unchangedEvidence(values, 2, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER - 4,
    1, Number.MAX_SAFE_INTEGER, 2, null);
  unchangedEvidence(values, 2, 2 ** 52, 2 ** 52, 1, 2 ** 52 + 6, 2, null);
  unchangedEvidence(values, 2, 2 ** 51, 2 ** 51, 48000, 2 ** 51 + 6, 2, null);
});

test("phaseEvidence preserves a native subarray and neighboring storage on success and failure", () => {
  const storage = Float32Array.from([99, 1.25, 2.25, 3.25, 4.25, 88]);
  const before = storage.slice();
  const view = storage.subarray(1, 5);
  unchangedEvidence(view, 4, 1, 1, 1, 52, 17,
    { checkedSamples: 4, maxError: 0, firstPhase: 0, lastPhase: 3 });
  unchangedEvidence(view, 4, 2, 1, 1, 52, 17, null);
  assert.deepEqual(storage, before, "neither native view nor adjacent samples can be changed");
});

test("one repeating phase is mathematically indistinguishable after a whole-period gap", () => {
  const startFrame = 1;
  const period = 17;
  const frame = startFrame + 4;
  const frames = 7;
  const horizonFrame = startFrame + 10 * period;
  const values = phaseSamples(frame, startFrame, frames, period);
  const expected = { checkedSamples: frames, maxError: 0, firstPhase: 4, lastPhase: 10 };
  unchangedEvidence(values, frames, frame, startFrame, 1, horizonFrame, period, expected);
  for (const skippedPeriods of [1, 2, 3]) {
    const alternativeFrame = frame + skippedPeriods * period;
    assert.deepEqual(phaseSamples(alternativeFrame, startFrame, frames, period), values);
    unchangedEvidence(values, frames, alternativeFrame, startFrame, 1,
      horizonFrame, period, expected);
    assert.notEqual(alternativeFrame, frame,
      "equal modulo evidence cannot authenticate the absolute callback origin");
  }
  unchangedEvidence(values, frames, frame + 1, startFrame, 1,
    horizonFrame, period, null);
});

// Synthetic residue arithmetic does not prove native precision, epoch or age.
// This oracle uses the general CRT sum and Euclidean inverses, independently of
// the helper's fixed-period reconstruction. No exported decoder supplies it.
const jointPeriods = [1048576n, 1048575n, 1048573n];
const jointModulus = jointPeriods.reduce((product, period) => product * period, 1n);
const maximumSafeOffset = BigInt(Number.MAX_SAFE_INTEGER);

function positiveMod(value, modulus) {
  return ((value % modulus) + modulus) % modulus;
}

function euclideanInverse(value, modulus) {
  let remainder = modulus;
  let nextRemainder = positiveMod(value, modulus);
  let coefficient = 0n;
  let nextCoefficient = 1n;
  while (nextRemainder !== 0n) {
    const quotient = remainder / nextRemainder;
    [remainder, nextRemainder] = [nextRemainder, remainder - quotient * nextRemainder];
    [coefficient, nextCoefficient] = [nextCoefficient, coefficient - quotient * nextCoefficient];
  }
  assert.equal(remainder, 1n, "the oracle requires coprime periods");
  return positiveMod(coefficient, modulus);
}

function exactJointOracle(residues) {
  let sum = 0n;
  for (let index = 0; index < jointPeriods.length; index++) {
    const period = jointPeriods[index];
    const complementaryProduct = jointModulus / period;
    sum += residues[index] * complementaryProduct
      * euclideanInverse(complementaryProduct, period);
  }
  return positiveMod(sum, jointModulus);
}

function jointResidues(offset) {
  return jointPeriods.map(period => BigInt(offset) % period);
}

function jointValues(residues) {
  return residues.map(residue => Number(residue) + 1.25);
}

function assertJointResidues(residues) {
  const exact = exactJointOracle(residues);
  const expected = exact <= maximumSafeOffset ? Number(exact) : null;
  const actual = nativeJointOffset(...jointValues(residues));
  assert.equal(actual, expected, `residues ${residues.join(",")}, exact offset ${exact}`);
  if (actual !== null) {
    assert.ok(Number.isSafeInteger(actual) && actual >= 0);
    for (let index = 0; index < jointPeriods.length; index++) {
      assert.equal(BigInt(actual) % jointPeriods[index], residues[index],
        `returned offset satisfies lane ${index}`);
    }
  }
  return actual;
}

function assertKnownJointOffset(offset) {
  const residues = jointResidues(offset);
  assert.equal(exactJointOracle(residues), BigInt(offset), "known offset independently reconstructed");
  assert.equal(assertJointResidues(residues), Number(offset));
}

test("nativeJointOffset reconstructs zero, first and each lane's maximum residue", () => {
  for (const offset of [0n, 1n, ...jointPeriods.map(period => period - 1n)]) {
    assertKnownJointOffset(offset);
  }
  const largestResidues = jointPeriods.map(period => period - 1n);
  assert.equal(exactJointOracle(largestResidues), jointModulus - 1n);
  assert.equal(assertJointResidues(largestResidues), null,
    "individually valid maximum residues can jointly exceed the certified domain");
});

test("nativeJointOffset agrees with a general exact CRT oracle and every congruence", () => {
  const residueChoices = jointPeriods.map(period => [0n, 1n, period / 2n, period - 1n]);
  for (const r0 of residueChoices[0]) {
    for (const r1 of residueChoices[1]) {
      for (const r2 of residueChoices[2]) assertJointResidues([r0, r1, r2]);
    }
  }
  let seed = 712367n;
  for (let vector = 0; vector < 64; vector++) {
    const residues = jointPeriods.map(period => {
      seed = (seed * 1664525n + 1013904223n) % (2n ** 32n);
      return seed % period;
    });
    assertJointResidues(residues);
  }
});

test("nativeJointOffset carries through mixed lane cycles and the P0P1 boundary", () => {
  const pairProduct = jointPeriods[0] * jointPeriods[1];
  const offsets = new Set();
  for (const period of jointPeriods) {
    for (const cycles of [1n, 2n, 7n, 31n]) {
      for (const delta of [-1n, 0n, 1n]) offsets.add(period * cycles + delta);
    }
  }
  for (const cycles of [1n, 2n, 17n, 8192n]) {
    for (const delta of [-1n, 0n, 1n]) offsets.add(pairProduct * cycles + delta);
  }
  offsets.add(pairProduct * 17n + jointPeriods[0] * 123n + jointPeriods[2] * 29n + 91n);
  for (const offset of offsets) assertKnownJointOffset(offset);
});

test("nativeJointOffset distinguishes offsets that alias in the first period", () => {
  for (const offset of [0n, 17n, jointPeriods[0] - 1n]) {
    const firstValues = jointValues(jointResidues(offset));
    assertKnownJointOffset(offset);
    for (const skippedCycles of [1n, 2n, 7n, 8170n]) {
      const alternativeOffset = offset + skippedCycles * jointPeriods[0];
      const alternativeValues = jointValues(jointResidues(alternativeOffset));
      assert.equal(alternativeValues[0], firstValues[0], "the first native scalar still aliases");
      assert.notEqual(alternativeValues[1], firstValues[1]);
      assert.notEqual(alternativeValues[2], firstValues[2]);
      assertKnownJointOffset(alternativeOffset);
      assert.notEqual(nativeJointOffset(...alternativeValues), nativeJointOffset(...firstValues));
    }
  }
});

for (const sampleRate of rates) {
  test(`nativeJointOffset twenty-hour/week offsets are arithmetic vectors at ${sampleRate}Hz`, () => {
    for (const seconds of [20n * 3600n, 7n * 24n * 3600n]) {
      const offset = seconds * BigInt(sampleRate);
      for (const delta of [0n, 1n, jointPeriods[0] - 1n, ...jointPeriods]) {
        assertKnownJointOffset(offset + delta);
      }
    }
  });
}

test("nativeJointOffset returns MAX_SAFE_INTEGER exactly and refuses the first unsafe offset", () => {
  for (const distance of [0n, 1n, ...jointPeriods, jointPeriods[0] * jointPeriods[1]]) {
    assertKnownJointOffset(maximumSafeOffset - distance);
  }
  const firstUnsafe = maximumSafeOffset + 1n;
  assert.equal(exactJointOracle(jointResidues(firstUnsafe)), firstUnsafe);
  for (const offset of [firstUnsafe, firstUnsafe + 1n,
    firstUnsafe + jointPeriods[0], jointModulus - 1n]) {
    assert.equal(assertJointResidues(jointResidues(offset)), null, `unsafe exact offset ${offset}`);
  }
});

test("nativeJointOffset rejects nonfinite and nonnumber values in every argument position", () => {
  const noCoercion = { valueOf() { throw new Error("a scalar must not be coerced"); } };
  for (let index = 0; index < jointPeriods.length; index++) {
    for (const value of [...invalidSamples, Symbol("phase"), () => 1.25,
      new Number(1.25), noCoercion]) {
      const values = [1.25, 1.25, 1.25];
      values[index] = value;
      assert.equal(nativeJointOffset(...values), null, `invalid scalar at lane ${index}`);
    }
  }
  assert.equal(nativeJointOffset(), null);
  assert.equal(nativeJointOffset(1.25), null);
  assert.equal(nativeJointOffset(1.25, 1.25), null);
});

test("nativeJointOffset rejects exact half ties and out-of-range values in every lane", () => {
  for (let index = 0; index < jointPeriods.length; index++) {
    const period = Number(jointPeriods[index]);
    for (const value of [0.5, 1.5, 17.5, period - 0.5, period + 0.5, period + 1.5,
      -Infinity, -1, -0, 0, 0.25, period + 1.75, period + 2, Number.MAX_VALUE]) {
      const values = [1.25, 1.25, 1.25];
      values[index] = value;
      assert.equal(nativeJointOffset(...values), null, `invalid ${value} at lane ${index}`);
    }
  }
});

test("nativeJointOffset preserves zero and nonzero offsets when terminal codes wrap", () => {
  for (const terminalFraction of [1, 1.25]) {
    for (let mask = 0; mask < 8; mask++) {
      const values = jointPeriods.map((period, index) =>
        mask & (1 << index) ? Number(period) + terminalFraction : 1.25);
      assert.equal(nativeJointOffset(...values), 0, `terminal mask ${mask}`);
    }
    for (let index = 0; index < jointPeriods.length; index++) {
      const offset = jointPeriods[index] * 7n;
      const values = jointValues(jointResidues(offset));
      assert.equal(values[index], 1.25);
      values[index] = Number(jointPeriods[index]) + terminalFraction;
      assert.equal(nativeJointOffset(...values), Number(offset), `terminal lane ${index}`);
    }
  }
  assert.equal(nativeJointOffset(0.75, 0.75, 0.75), 0,
    "accepted scalar decoding is a modulo candidate, not full native-ramp validation");
  for (const offset of [1n, 17n, jointPeriods[0] - 1n]) {
    const residues = jointResidues(offset);
    for (const fractionalCode of [0.75, 1, 1.25]) {
      assert.equal(nativeJointOffset(...residues.map(residue => Number(residue) + fractionalCode)),
        Number(offset));
    }
  }
});

test("nativeJointOffset consumes scalars without changing arrays or neighboring storage", () => {
  const offset = jointPeriods[0] * jointPeriods[1] + 17n;
  const storage = Float32Array.from([99, ...jointValues(jointResidues(offset)), 88]);
  const before = storage.slice();
  const values = storage.subarray(1, 4);
  assert.equal(nativeJointOffset(...values), Number(offset));
  assert.deepEqual(storage, before);
  assert.equal(nativeJointOffset(values), null, "an array is not the three-scalar interface");
  for (let index = 0; index < jointPeriods.length; index++) {
    for (const container of [Object.freeze([1.25]), new Float32Array([1.25]),
      new Float64Array([1.25]), values.subarray(index, index + 1)]) {
      const beforeContainer = Array.from(container);
      const argumentsByLane = [1.25, 1.25, 1.25];
      argumentsByLane[index] = container;
      assert.equal(nativeJointOffset(...argumentsByLane), null, `container at lane ${index}`);
      assert.deepEqual(Array.from(container), beforeContainer);
    }
  }
  assert.deepEqual(storage, before, "success and refusal preserve the view and its neighbors");
});

test("full-modulus aliases remain indistinguishable outside the certified joint domain", () => {
  assert.ok(jointModulus > maximumSafeOffset);
  for (const offset of [0n, 1n, 17n, jointPeriods[0] * jointPeriods[1] + 29n,
    maximumSafeOffset]) {
    const outsideDomain = offset + jointModulus;
    assert.ok(outsideDomain > maximumSafeOffset);
    const residues = jointResidues(offset);
    assert.deepEqual(jointResidues(outsideDomain), residues);
    assert.deepEqual(jointValues(jointResidues(outsideDomain)), jointValues(residues));
    assert.equal(exactJointOracle(residues), offset);
    assert.equal(nativeJointOffset(...jointValues(jointResidues(outsideDomain))), Number(offset),
      "identical residues cannot establish the absolute epoch or native context age");
  }
});

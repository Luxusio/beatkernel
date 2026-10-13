// Diagnostic arithmetic only: actual native onset admission is a browser-fixture obligation.
import assert from "node:assert/strict";
import test from "node:test";
import {
  markerTime, markerFrame, advanceFrame, nativePhase, phaseIdeal, phaseEvidence,
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

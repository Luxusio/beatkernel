// Scalar adapter vectors cannot establish actual native delivery or epoch.
import assert from "node:assert/strict";
import test from "node:test";
import {
  CLOCK_VERSION, CLOCK_ID, CLOCK_PERIODS, CLOCK_NAMES,
  clockGeometry, validClockLane,
} from "./native-audio-clock.mjs";

const periods = [1048576, 1048575, 1048573];
const rates = [44100, 48000];
const invalidScalars = [-1, 0.5, NaN, Infinity, -Infinity,
  Number.MAX_SAFE_INTEGER + 1, "128", 128n, null, undefined, true, {}, [], Symbol("frame")];

// Integer residue and specified event-time interpolation are an independent
// oracle; no production or diagnostic helper computes these expectations.
function eventIdeal(frame, origin, period, rate) {
  const residue = Number((BigInt(frame) - BigInt(origin)) % BigInt(period));
  const previousBoundary = frame - residue;
  const previousTime = (previousBoundary - 0.25) / rate;
  const nextTime = (previousBoundary + period - 0.25) / rate;
  return 1 + (frame / rate - previousTime) * period / (nextTime - previousTime);
}

function lane(frame, frames, origin, period, rate, firstIndex = 0) {
  const values = new Float32Array(frames);
  for (let index = firstIndex; index < frames; index++) {
    values[index] = eventIdeal(frame + index, origin, period, rate);
  }
  return values;
}

function futureHorizon(frame, frames, origin, period) {
  const cycles = Math.floor((frame + frames - 1 - origin) / period) + 1;
  return origin + Math.max(cycles, 1) * period;
}

function expectLane(values, frames, frame, origin, horizon, period, rate, firstIndex, expected) {
  const before = Array.from(values);
  const result = validClockLane(values, frames, frame, origin, horizon, period, rate, firstIndex);
  assert.equal(typeof result, "boolean", "admission has a scalar Boolean result");
  assert.equal(result, expected);
  assert.deepEqual(Array.from(values), before, "admission cannot rewrite native input");
}

test("fixed version, clock identity, periods and parameter names are immutable", () => {
  assert.equal(CLOCK_VERSION, 1);
  assert.equal(CLOCK_ID, 1);
  assert.deepEqual(CLOCK_PERIODS, periods);
  assert.deepEqual(CLOCK_NAMES, ["__beatkernel_clock_marker", "__beatkernel_clock_0",
    "__beatkernel_clock_1", "__beatkernel_clock_2"]);
  assert.ok(Object.isFrozen(CLOCK_PERIODS));
  assert.ok(Object.isFrozen(CLOCK_NAMES));
  assert.throws(() => { CLOCK_PERIODS[0] = 1; }, TypeError);
  assert.throws(() => { CLOCK_NAMES[0] = "other"; }, TypeError);
});

for (const rate of rates) {
  test(`geometry reserves complete associated cycles at ${rate}Hz`, () => {
    const origin = 1536;
    for (const maxFrames of [1, 2, 128, 4096, 174763, 262144, 524288, 1048576]) {
      const geometry = clockGeometry(origin, maxFrames, rate);
      const lowWater = Math.max(periods[2], 4 * maxFrames);
      const cycles = Math.ceil((lowWater + 2 * maxFrames) / periods[2]) + 1;
      assert.deepEqual(geometry, {
        origin, h0: origin + cycles * periods[0], h1: origin + cycles * periods[1],
        h2: origin + cycles * periods[2], lowWater, periods: cycles,
      });
      assert.ok(geometry.h0 > geometry.h1 && geometry.h1 > geometry.h2,
        "own horizons descend; sorting would reassociate native lanes");
      for (const horizon of [origin, geometry.h0, geometry.h1, geometry.h2]) {
        const represented = ((horizon - 0.25) / rate) * rate;
        assert.ok(represented > horizon - 1 && represented < horizon);
        assert.equal(Math.ceil(represented), horizon);
      }
    }
    assert.equal(clockGeometry(origin, 4096, rate).periods, 3);
    assert.equal(clockGeometry(origin, 1048576, rate).periods, 8);
  });

  test(`geometry arithmetic supports twenty-hour/week origins at ${rate}Hz`, () => {
    for (const origin of [20 * 3600 * rate, 7 * 24 * 3600 * rate]) {
      const geometry = clockGeometry(origin, 4096, rate);
      assert.equal(geometry.origin, origin);
      assert.equal(geometry.h2, origin + 3 * periods[2]);
      assert.equal(geometry.periods, 3);
    }
  });

  for (const period of periods) {
    test(`every interior initial prefix retains original indices for ${period}/${rate}`, () => {
      const origin = 4096;
      for (const frames of [2, 3, 7, 17, 64, 128, 129, 257]) {
        for (let firstIndex = 1; firstIndex < frames; firstIndex++) {
          const frame = origin - firstIndex;
          const values = lane(frame, frames, origin, period, rate, firstIndex);
          expectLane(values, frames, frame, origin, origin + period, period, rate, firstIndex, true);
        }
      }
    });

    test(`one, variable and reset-crossing full blocks validate for ${period}/${rate}`, () => {
      const origin = 1024;
      for (const offset of [0, 1, period - 2, period - 1, period, period + 1,
        2 * period - 3, 20 * 3600 * rate, 7 * 24 * 3600 * rate]) {
        for (const frames of [1, 2, 3, 17, 64, 128, 129, 257]) {
          const frame = origin + offset;
          const values = lane(frame, frames, origin, period, rate);
          const horizon = futureHorizon(frame, frames, origin, period);
          expectLane(values, frames, frame, origin, horizon, period, rate, 0, true);
        }
      }
    });

    test(`every original sample across a reset is checked for ${period}/${rate}`, () => {
      const origin = 1024, frames = 129, frame = origin + period - 64;
      const horizon = origin + 2 * period;
      const original = lane(frame, frames, origin, period, rate);
      expectLane(original, frames, frame, origin, horizon, period, rate, 0, true);
      for (let index = 0; index < frames; index++) {
        const corrupt = original.slice();
        corrupt[index] += 1;
        expectLane(corrupt, frames, frame, origin, horizon, period, rate, 0, false);
      }
      for (const badValue of [NaN, Infinity, -Infinity, -1, 0, period + 2]) {
        const corrupt = original.slice();
        corrupt[frames >> 1] = badValue;
        expectLane(corrupt, frames, frame, origin, horizon, period, rate, 0, false);
      }
    });

    test(`terminal horizon excludes its reset sample for ${period}/${rate}`, () => {
      const origin = 1024, horizon = origin + 3 * period;
      for (const frames of [1, 3, 17, 128, 257]) {
        const frame = horizon - frames;
        expectLane(lane(frame, frames, origin, period, rate), frames, frame, origin,
          horizon, period, rate, 0, true);
        expectLane(lane(frame + 1, frames, origin, period, rate), frames, frame + 1, origin,
          horizon, period, rate, 0, false);
      }
      expectLane(lane(horizon, 1, origin, period, rate), 1, horizon, origin,
        horizon, period, rate, 0, false);
    });
  }
}

test("geometry refuses malformed domain, oversized capacity and unrepresentable event times", () => {
  for (const bad of [0, ...invalidScalars]) {
    assert.equal(clockGeometry(bad, 4096, 48000), null);
    assert.equal(clockGeometry(1536, bad, 48000), null);
    assert.equal(clockGeometry(1536, 4096, bad), null);
  }
  for (const capacity of [1048577, Number.MAX_SAFE_INTEGER]) {
    assert.equal(clockGeometry(1536, capacity, 48000), null);
  }
  for (const origin of [2 ** 52, Number.MAX_SAFE_INTEGER - 1, Number.MAX_SAFE_INTEGER]) {
    for (const rate of [1, ...rates, Number.MAX_SAFE_INTEGER]) {
      assert.equal(clockGeometry(origin, 4096, rate), null,
        "integer safety does not guarantee a represented quarter-early event");
    }
  }
  assert.equal(clockGeometry(), null);
});

test("geometry checks intermediate native resets even when origin and all final horizons represent", () => {
  const representable = (frame, rate) => {
    const scaled = ((frame - 0.25) / rate) * rate;
    return scaled > frame - 1 && scaled < frame && Math.ceil(scaled) === frame;
  };
  for (const [rate, origin, badIntermediate] of [
    [44100, 2251799808685251, 2251799809733827],
    [48000, 2251799808685253, 2251799809733828],
  ]) {
    assert.ok(representable(origin, rate));
    for (const period of periods) assert.ok(representable(origin + 3 * period, rate));
    assert.equal(representable(badIntermediate, rate), false);
    assert.equal(clockGeometry(origin, 4096, rate), null,
      "a valid endpoint cannot hide an ambiguous intermediate reset");
  }
});

test("initial prefix stays exact zero at every original position", () => {
  const origin = 512, frames = 17, firstIndex = 9, frame = origin - firstIndex;
  for (const period of periods) {
    const original = lane(frame, frames, origin, period, 48000, firstIndex);
    expectLane(original, frames, frame, origin, origin + period, period, 48000, firstIndex, true);
    for (let index = 0; index < firstIndex; index++) {
      for (const value of [2 ** -149, 0.5, 1, -1, NaN, Infinity]) {
        const corrupt = original.slice(); corrupt[index] = value;
        expectLane(corrupt, frames, frame, origin, origin + period, period, 48000, firstIndex, false);
      }
    }
    for (const wrongIndex of [0, firstIndex - 1, firstIndex + 1, frames]) {
      expectLane(original, frames, frame, origin, origin + period, period, 48000, wrongIndex, false);
    }
  }
});

test("small-cycle oracles cover every phase and repeated reset without changing fixed production periods", () => {
  const origin = 16, rate = 1;
  for (const period of [2, 3, 5, 7, 17, 257]) {
    const horizon = origin + 10 * period;
    for (let offset = 0; offset < 3 * period; offset++) {
      for (const frames of [1, 2, 3, period + 1]) {
        const frame = origin + offset;
        expectLane(lane(frame, frames, origin, period, rate), frames, frame, origin,
          horizon, period, rate, 0, true);
      }
    }
  }
});

test("original Float32 shape is required; scalar phase lanes are never expanded", () => {
  const origin = 1024, frames = 17, period = periods[0], horizon = origin + period;
  const values = lane(origin, frames, origin, period, 48000);
  for (const input of [null, undefined, Array.from(values), new Float64Array(values),
    new Int32Array(values), new Uint8Array(values), new DataView(values.buffer),
    { length: frames, 0: values[0] }, new Float32Array([values[0]]), values.subarray(1)]) {
    assert.equal(validClockLane(input, frames, origin, origin, horizon, period, 48000, 0), false);
  }
  for (const wrongFrames of [0, 1, frames - 1, frames + 1, ...invalidScalars]) {
    expectLane(values, wrongFrames, origin, origin, horizon, period, 48000, 0, false);
  }
  expectLane(new Float32Array(), 0, origin, origin, horizon, period, 48000, 0, false);
  assert.equal(validClockLane(), false);
});

test("metadata rejects coercion, unsafe extent and malformed own horizon", () => {
  const origin = 1024, frames = 3, period = periods[0], horizon = origin + period;
  const values = lane(origin, frames, origin, period, 48000);
  const valid = [values, frames, origin, origin, horizon, period, 48000, 0];
  for (const slot of [2, 3, 4, 5, 6, 7]) {
    for (const bad of invalidScalars) {
      const args = valid.slice(); args[slot] = bad;
      assert.equal(validClockLane(...args), false, `invalid scalar in argument ${slot}`);
    }
  }
  for (const [slot, bad] of [[3, 0], [4, origin], [4, origin - 1], [4, horizon + 1],
    [5, 0], [5, 1], [5, 2 ** 20 + 1], [6, 0], [7, frames], [7, frames + 1]]) {
    const args = valid.slice(); args[slot] = bad;
    assert.equal(validClockLane(...args), false);
  }
  expectLane(values, frames, origin - 1, origin, horizon, period, 48000, 0, false);
  expectLane(values, frames, Number.MAX_SAFE_INTEGER - 1, origin,
    Number.MAX_SAFE_INTEGER, period, 48000, 0, false);
  expectLane(values, frames, origin, origin, 2 ** 52 + origin, period, 48000, 0, false);
});

test("represented ideal requires strict quarter margin and rejects every half tie", () => {
  const origin = 8, period = periods[0], rate = 1, horizon = origin + period;
  assert.equal(eventIdeal(origin, origin, period, rate), 1.25);
  for (const value of [1.0000001192092896, 1.125, 1.25, 1.375, 1.4999998807907104]) {
    expectLane(new Float32Array([value]), 1, origin, origin, horizon, period, rate, 0, true);
  }
  for (const value of [1, 1.5, 0.5, 2.5, 0.75, 1.75, period + 1]) {
    expectLane(new Float32Array([value]), 1, origin, origin, horizon, period, rate, 0, false);
  }
  const frames = 17, frame = origin + 20;
  const original = lane(frame, frames, origin, period, rate);
  for (let index = 0; index < frames; index++) {
    const exactTie = original.slice(); exactTie[index] = frame + index - origin + 1.5;
    expectLane(exactTie, frames, frame, origin, horizon, period, rate, 0, false);
    const exactMargin = original.slice(); exactMargin[index] = frame + index - origin + 1;
    expectLane(exactMargin, frames, frame, origin, horizon, period, rate, 0, false);
  }
});

test("held/default middle values refuse; a stateless lane alone cannot certify epoch", () => {
  const origin = 1024, period = periods[2], frames = 128, frame = origin + 256;
  const horizon = origin + period;
  const original = lane(frame, frames, origin, period, 48000);
  for (const held of [new Float32Array(frames), new Float32Array(frames).fill(1),
    new Float32Array(frames).fill(original[0]), new Float32Array(frames).fill(period + 1)]) {
    expectLane(held, frames, frame, origin, horizon, period, 48000, 0, false);
  }
  const heldMiddle = original.slice(); heldMiddle.fill(original[0], 1, frames - 1);
  assert.equal(heldMiddle[0], original[0]);
  assert.equal(heldMiddle[frames - 1], original[frames - 1]);
  expectLane(heldMiddle, frames, frame, origin, horizon, period, 48000, 0, false);
  // Integer rate makes whole-period alias values exactly identical. Pure/native
  // certificate state owns chronology and age, not this stateless Boolean scan.
  const aliasValues = lane(frame + period, frames, origin, period, 1);
  expectLane(aliasValues, frames, frame, origin, horizon, period, 1, 0, true);
});

test("original native view and neighboring storage remain unchanged", () => {
  const origin = 1024, frames = 17, period = periods[1], frame = origin + period - 8;
  const native = lane(frame, frames, origin, period, 48000);
  const storage = new Float32Array(frames + 2);
  storage[0] = -77; storage[frames + 1] = 91; storage.set(native, 1);
  const view = new Float32Array(storage.buffer, Float32Array.BYTES_PER_ELEMENT, frames);
  const before = storage.slice();
  expectLane(view, frames, frame, origin, origin + 2 * period, period, 48000, 0, true);
  view[frames >> 1] = NaN;
  const corruptBefore = storage.slice();
  expectLane(view, frames, frame, origin, origin + 2 * period, period, 48000, 0, false);
  assert.deepEqual(storage, corruptBefore);
  assert.deepEqual(storage.subarray(0, 1), before.subarray(0, 1));
  assert.deepEqual(storage.subarray(frames + 1), before.subarray(frames + 1));
});

test("hot scans create no Float32 arrays, views, slice copies or BigInts", () => {
  const origin = 1024, period = periods[0], frame = origin + period - 64, frames = 129;
  const values = lane(frame, frames, origin, period, 48000);
  const before = Array.from(values);
  const originalFloat32 = globalThis.Float32Array;
  const originalBigInt = globalThis.BigInt;
  const originalSlice = Float32Array.prototype.slice;
  const originalSubarray = Float32Array.prototype.subarray;
  const reject = () => { throw new Error("callback allocation/copy guard"); };
  try {
    globalThis.Float32Array = new Proxy(originalFloat32, { construct: reject, apply: reject });
    globalThis.BigInt = reject;
    originalFloat32.prototype.slice = reject;
    originalFloat32.prototype.subarray = reject;
    assert.equal(validClockLane(values, frames, frame, origin,
      origin + 2 * period, period, 48000, 0), true);
    values[64] = NaN;
    assert.equal(validClockLane(values, frames, frame, origin,
      origin + 2 * period, period, 48000, 0), false);
  } finally {
    globalThis.Float32Array = originalFloat32;
    globalThis.BigInt = originalBigInt;
    originalFloat32.prototype.slice = originalSlice;
    originalFloat32.prototype.subarray = originalSubarray;
    values[64] = before[64];
  }
  assert.deepEqual(Array.from(values), before);
});

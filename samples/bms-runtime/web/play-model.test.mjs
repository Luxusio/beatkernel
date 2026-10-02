// Deferred portable helpers; no WebAudio, Worker, WASM, or wall-clock acquisition.
import assert from "node:assert/strict";
import test from "node:test";
import {
  millisecondsToNanos, secondsToNanos, frameNanos, startProjection, presentationPoint, reportWord, renderedCursor,
  KEY_BINDINGS, bindingsFor,
} from "./play-model.mjs";

const U64_MAX = 18446744073709551615n;
const I64_MAX = 9223372036854775807n;
function setWord(words, index, value) {
  words[index * 2] = Number(value & 0xffffffffn);
  words[index * 2 + 1] = Number(value >> 32n);
}
function report(start = 9007199254740993n, available = true) {
  const words = new Uint32Array(56);
  setWord(words, 0, BigInt(available));
  setWord(words, 2, 257n);
  setWord(words, 3, 9007199254741999n);
  setWord(words, 4, 129n);
  setWord(words, 25, 1n);
  setWord(words, 26, start);
  return { available, words };
}

test("Window timestamps retain fractional milliseconds and long lifetimes within signed nanoseconds", () => {
  for (const [input, expected] of [
    [0, 0n], [0.125, 125000n], [1.5, 1500000n],
    [72000000, 72000000000000n], [604800000, 604800000000000n],
    [9007199254.75, 9007199254750000n],
    [9223372036854.5, 9223372036854500000n],
  ]) assert.equal(millisecondsToNanos(input), expected);
  for (const invalid of [-1, NaN, Infinity, "1", null, 1n, Number.MAX_SAFE_INTEGER + 1,
    9223372036854.875, 9223372036855]) assert.throws(() => millisecondsToNanos(invalid));
});

test("frame conversion keeps full u64 width, exact flooring and signed output headroom", () => {
  assert.equal(frameNanos(1n, 44100), 22675n);
  assert.equal(frameNanos(44100n, 44100), 1000000000n);
  assert.equal(frameNanos(48000n * 604800n, 48000), 604800000000000n);
  assert.equal(frameNanos(9007199254740993n, 1000000000), 9007199254740993n);
  assert.equal(frameNanos(I64_MAX, 1000000000), I64_MAX);
  assert.equal(frameNanos(U64_MAX, 0xffffffff), 4294967297000000000n);
  for (const [frame, rate] of [[-1n, 48000], [U64_MAX + 1n, 48000], [1, 48000],
    [0n, 0], [0n, 0x100000000], [1n, 1.5], [1n, NaN], [I64_MAX + 1n, 1000000000]]) {
    assert.throws(() => frameNanos(frame, rate));
  }
});

test("context seconds are split before nanosecond scaling and reject signed overflow", () => {
  assert.equal(secondsToNanos(0.125), 125000000n);
  assert.equal(secondsToNanos(72000), 72000000000000n);
  assert.equal(secondsToNanos(604800), 604800000000000n);
  assert.equal(secondsToNanos(2 ** 33 + 2 ** -18), 8589934592000003814n);
  assert.equal(secondsToNanos(9223372036.5), 9223372036500000000n);
  for (const invalid of [-1, NaN, Infinity, "1", null, 9223372037]) {
    assert.throws(() => secondsToNanos(invalid));
  }
});

test("start projection uses the Window bracket and splits long context seconds before scaling", () => {
  assert.equal(startProjection({ beforeMs: 1000, afterMs: 1002, contextTime: 2, sampleRate: 48000 }, 144000n), 2001000000n);
  assert.equal(startProjection({ beforeMs: 10, afterMs: 10.125, contextTime: 0, sampleRate: 4 }, 1n), 260062500n);
  // This binary fraction is exactly representable, even at a 272-year context.
  // Multiplying the complete Number by 1e3/1e9 first loses fractional precision.
  assert.equal(startProjection({ beforeMs: 1000, afterMs: 1002,
    contextTime: 2 ** 33 + 2 ** -18, sampleRate: 48000 }, 8589934592n * 48000n), 1000996186n);
  for (const clock of [
    null,
    { beforeMs: 2, afterMs: 1, contextTime: 0, sampleRate: 48000 },
    { beforeMs: 0, afterMs: NaN, contextTime: 0, sampleRate: 48000 },
    { beforeMs: -1, afterMs: 1, contextTime: 0, sampleRate: 48000 },
    { beforeMs: 1, afterMs: 2, contextTime: -1, sampleRate: 48000 },
    { beforeMs: 0, afterMs: 0, contextTime: 1, sampleRate: 48000 },
    { beforeMs: 9223372036854.5, afterMs: 9223372036854.5, contextTime: 0, sampleRate: 1 },
  ]) assert.throws(() => startProjection(clock, 1n));
});

test("report words preserve unsigned high bits and an armed unavailable report carries no cursor", () => {
  const words = new Uint32Array(56);
  setWord(words, 0, U64_MAX);
  setWord(words, 27, 0x8000000000000000n);
  assert.equal(reportWord(words, 0), U64_MAX);
  assert.equal(reportWord(words, 27), 0x8000000000000000n);
  for (const index of [-1, 28, 0.5, NaN]) assert.throws(() => reportWord(words, index));
  for (const invalid of [new Uint32Array(55), new Int32Array(56), Array(56).fill(0), null]) {
    assert.throws(() => reportWord(invalid, 0));
  }
  const start = 9007199254740993n;
  assert.equal(renderedCursor(report(start, false), start), null);
  const actual = report(start);
  setWord(actual.words, 15, 7n); // Existing late-command telemetry is retained, not invented failure.
  assert.equal(renderedCursor(actual, start), 9007199254742128n);
});

test("report corruption, command rejection and wrong start cannot fabricate completed playback", () => {
  const start = 9007199254740993n;
  for (const index of [0, 5, 6, 11, 23, 25, 27]) {
    const invalid = report(start);
    setWord(invalid.words, index, 2n);
    assert.throws(() => renderedCursor(invalid, start), `flag ${index}`);
  }
  for (let index = 16; index <= 22; index++) {
    const failed = report(start);
    setWord(failed.words, index, 1n);
    assert.throws(() => renderedCursor(failed, start), `actual command error ${index}`);
  }
  for (const mutate of [
    value => { value.available = false; },
    value => { value.available = "true"; },
    value => setWord(value.words, 27, 1n),
    value => setWord(value.words, 25, 0n),
    value => setWord(value.words, 26, start + 1n),
    value => setWord(value.words, 4, 258n),
    value => { setWord(value.words, 3, U64_MAX); setWord(value.words, 4, 1n); },
  ]) {
    const invalid = report(start);
    mutate(invalid);
    assert.throws(() => renderedCursor(invalid, start));
  }
  assert.throws(() => renderedCursor(report(start), Number(start)));
  assert.throws(() => renderedCursor(null, start));
});

test("explicit keyboard bindings preserve both sides and reject unknown or duplicate lanes", () => {
  assert.equal(KEY_BINDINGS.length, 18);
  assert.equal(new Set(KEY_BINDINGS.map(row => row[0])).size, 18);
  assert.equal(new Set(KEY_BINDINGS.map(row => row[1])).size, 18);
  assert.equal(new Set(KEY_BINDINGS.map(row => row[2])).size, 18);
  assert.deepEqual(bindingsFor(new Uint8Array([0x16, 0x11, 0x26, 0x21, 0x27])), [
    [0x16, "ShiftLeft", 1], [0x11, "KeyZ", 2], [0x26, "ShiftRight", 10],
    [0x21, "KeyN", 11], [0x27, "Slash", 18],
  ]);
  assert.deepEqual(bindingsFor([0x29, 0x17]), [[0x29, "Period", 17], [0x17, "Space", 9]]);
  assert.deepEqual(bindingsFor([]), []);
  for (const invalid of [null, new Uint32Array([0x11]), [0x11, 0x11], [0x10], [0x2a], ["17"], Array(19).fill(0x11)]) {
    assert.throws(() => bindingsFor(invalid));
  }
});

test("presentation uses only fresh reported output and rounds a fractional start conservatively", () => {
  const actual = { contextTime: 1, performanceTime: 1500 };
  assert.equal(presentationPoint(actual, 1n, 44100, 1500), 999977324n);
  assert.equal(presentationPoint(actual, 1n, 44100, 2500), 999977324n,
    "the full freshness allowance does not extrapolate presentation");
  assert.equal(presentationPoint(actual, 1n, 44100, 2500.125), null);
  assert.equal(presentationPoint(actual, 1n, 44100, 1499), null);
  assert.equal(presentationPoint(actual, 44100n, 44100, 1500), 0n);
  assert.equal(presentationPoint(actual, 44101n, 44100, 1500), null);
  assert.equal(presentationPoint(actual, 0n, 48000, 1500, 0), 1000000000n);
  assert.equal(presentationPoint(actual, 0n, 48000, 1501, 0), null);
  for (const timestamp of [{ contextTime: 0, performanceTime: 1 },
    { contextTime: 1, performanceTime: 0 }, { contextTime: 0, performanceTime: 0 }]) {
    assert.equal(presentationPoint(timestamp, 0n, 48000, 1500), null);
  }
  assert.equal(presentationPoint({ contextTime: 2 ** 33 + 2 ** -18, performanceTime: 5000 },
    8589934592n * 48000n, 48000, 5000), 3814n);
  assert.equal(presentationPoint({ contextTime: 604800.125, performanceTime: 604800125 },
    604800n * 48000n, 48000, 604800125), 125000000n);
});

test("malformed presentation data and unrepresentable grids are errors even before output exists", () => {
  const actual = { contextTime: 1, performanceTime: 1 };
  for (const timestamp of [null, {}, { ...actual, contextTime: -1 },
    { ...actual, contextTime: NaN }, { ...actual, performanceTime: Infinity },
    { ...actual, contextTime: "1" }, { ...actual, performanceTime: -1 },
    { ...actual, contextTime: 9223372037 }]) {
    assert.throws(() => presentationPoint(timestamp, 0n, 48000, 1));
  }
  for (const [start, rate, now, age] of [[-1n, 48000, 1, 1000], [U64_MAX + 1n, 48000, 1, 1000],
    [0, 48000, 1, 1000], [0n, 0, 1, 1000], [0n, 1.5, 1, 1000],
    [0n, 48000, -1, 1000], [0n, 48000, NaN, 1000], [0n, 48000, 1, -1],
    [0n, 48000, 1, Infinity], [I64_MAX, 1, 1, 1000]]) {
    assert.throws(() => presentationPoint({ contextTime: 0, performanceTime: 0 }, start, rate, now, age));
  }
});

// Deferred portable helpers; no WebAudio, Worker, WASM, or wall-clock acquisition.
import assert from "node:assert/strict";
import test from "node:test";
import {
  millisecondsToNanos, secondsToNanos, frameNanos, audioScheduleFromFrame, startProjection, committedStartProjection,
  presentationPoint, presentationPair, reportWord, renderedCursor,
  KEY_BINDINGS, KEY_CHOICES, snapshotBindings, bindingsFor,
  parseTimingMilliseconds, timingFromMilliseconds, validateTiming,
  ORIGINAL_PCM_SAMPLES, PLAY_PCM_SAMPLES, startFromSeconds, validateStart, validateEnd, sectionFromSeconds,
  audioOutputFromFields, audioLimitsFromFields, replayOutputFromMetadata,
} from "./play-model.mjs";

const U64_MAX = 18446744073709551615n;
const I64_MAX = 9223372036854775807n;

test("raw context frames project on the actual armed grid with conservative lookahead and exact long-duration flooring", () => {
  for (const [context, start, rate, expected] of [
    [0n, 960n, 48000, 0n], [0n, 961n, 48000, 0n], [1n, 960n, 48000, 20833n],
    [5n, 6n, 3, 0n], [6n, 6n, 3, 333333333n], [0n, 0n, 51, 39215686n],
    [100n, 90n, 50, 220000000n], [26671680000n, 0n, 44100, 604800020000000n],
    [9007199254740993n, 9007199254740990n, 1000000000, 20000003n],
    [U64_MAX - 1n, U64_MAX, 1, 0n],
    [U64_MAX - 960n, U64_MAX - 960n, 48000, 20000000n],
    [I64_MAX - 20000000n, 0n, 1000000000, I64_MAX],
  ]) assert.equal(audioScheduleFromFrame(context, start, rate), expected);
  assert.equal(audioScheduleFromFrame(44100n, 44100n, 44100), 20000000n);
  assert.equal(audioScheduleFromFrame(48000n, 48000n, 48000), 20000000n);
});

test("raw frame scheduling refuses lossy observations and overflow before pre-start clamping or signed conversion", () => {
  for (const value of [undefined, null, 0, "0", -1n, U64_MAX + 1n]) {
    assert.throws(() => audioScheduleFromFrame(value, 0n, 48000));
    assert.throws(() => audioScheduleFromFrame(0n, value, 48000));
  }
  for (const rate of [undefined, null, 0, -1, 1.5, NaN, Infinity, "48000", 48000n, 0x100000000]) {
    assert.throws(() => audioScheduleFromFrame(0n, 0n, rate));
  }
  assert.throws(() => audioScheduleFromFrame(U64_MAX, U64_MAX, 1), "lookahead overflow cannot be hidden by clamping");
  assert.throws(() => audioScheduleFromFrame(U64_MAX - 959n, U64_MAX, 48000));
  assert.throws(() => audioScheduleFromFrame(I64_MAX - 19999999n, 0n, 1000000000));
  assert.throws(() => audioScheduleFromFrame(9223372036n, 0n, 1));
  assert.equal(audioScheduleFromFrame(0n, U64_MAX, 1), 0n, "only the relative scheduled timestamp requires i64 headroom");
});

test("live section snapshots preserve exact original-song decimal endpoints and optional unlimited bounds", () => {
  for (const [start, end, expected] of [
    ["0", "", { startNs: 0n, endNs: undefined }],
    ["1", "1.000000001", { startNs: 1000000000n, endNs: 1000000001n }],
    ["604800.000000001", "604800.000000002", { startNs: 604800000000001n, endNs: 604800000000002n }],
    ["9223372034.854775806", "9223372034.854775807",
      { startNs: I64_MAX - 2000000001n, endNs: I64_MAX - 2000000000n }],
  ]) {
    const section = sectionFromSeconds(start, end);
    assert.deepEqual(section, expected);
    assert.ok(Object.isFrozen(section));
    assert.notEqual(sectionFromSeconds(start, end), section);
    assert.throws(() => { section.endNs = 0n; }, TypeError);
    assert.deepEqual(section, expected);
  }
  assert.equal(validateEnd(I64_MAX), undefined);
  assert.equal(validateEnd(I64_MAX - 1n, I64_MAX), I64_MAX,
    "binding metadata can use the full i64 range independently of the text field's existing reserve");
  assert.equal(validateEnd(0n, 1n), 1n);
});

test("live end admission rejects lossy types, reversed or empty sections and malformed text without an unlimited fallback", () => {
  for (const start of [undefined, null, 0, "0", -1n, I64_MAX + 1n]) {
    assert.throws(() => validateEnd(start));
    assert.throws(() => validateEnd(start, 1n));
  }
  for (const end of [null, 1, "1", 1.5, NaN, Infinity, false, -1n, 0n, 1n, I64_MAX + 1n]) {
    assert.throws(() => validateEnd(1n, end));
  }
  for (const end of [undefined, null, 1, " ", "\n", "0", "1", "0.999999999", "+2", "-2", "2 ",
    "2\n", "2.", ".2", "2e0", "２", "2.0000000001", "9223372034.854775808", "0".repeat(21)]) {
    assert.throws(() => sectionFromSeconds("1", end), String(end));
  }
  for (const start of [undefined, null, "", " 0", "0\n", "-0", "+0", "0.0000000001"]) {
    assert.throws(() => sectionFromSeconds(start, ""));
  }
  assert.deepEqual(sectionFromSeconds("1", ""), { startNs: 1000000000n, endNs: undefined });
});

test("recorded replay endpoints preserve exact original starts and upward frame boundaries in immutable snapshots", () => {
  const unlimited = replayOutputFromMetadata(0n, undefined, undefined, 48000);
  assert.deepEqual(unlimited, { endNs: undefined, endFrame: undefined });
  assert.ok(Object.isFrozen(unlimited));
  assert.notEqual(replayOutputFromMetadata(0n, undefined, undefined, 48000), unlimited);
  for (const [start, end, frame, rate] of [
    [0n, 1n, 1n, 1],
    [604800000000001n, 604800000000002n, 4411n, 44100],
    [0n, 1n, 429496734n, 4294967295],
    [1n, 9007199154740994n, 9007199254740993n, 1000000000],
    [100000000n, I64_MAX, I64_MAX, 1000000000],
    [0n, 4294967296900000000n, U64_MAX, 4294967295],
  ]) {
    const endpoint = replayOutputFromMetadata(start, end, frame, rate);
    assert.deepEqual(endpoint, { endNs: end, endFrame: frame });
    assert.ok(Object.isFrozen(endpoint));
    assert.throws(() => { endpoint.endFrame = 1n; }, TypeError);
    assert.equal(endpoint.endFrame, frame);
  }
});

test("replay metadata refuses half-pairs, lossy scalar types, mismatched grids and unrepresentable physical endpoints", () => {
  for (const start of [undefined, null, 0, "0", -1n, I64_MAX + 1n]) {
    assert.throws(() => replayOutputFromMetadata(start, undefined, undefined, 48000));
  }
  for (const rate of [undefined, null, 0, -1, 1.5, NaN, Infinity, "48000", 48000n, 4294967296]) {
    assert.throws(() => replayOutputFromMetadata(0n, undefined, undefined, rate));
  }
  for (const [end, frame] of [[1n, undefined], [undefined, 4801n], [null, null], [1, 4801n],
    ["1", 4801n], [1n, 4801], [1n, "4801"], [0n, 4800n], [-1n, 4800n],
    [I64_MAX + 1n, 1n], [1n, 0n], [1n, -1n], [1n, U64_MAX + 1n], [1n, 4800n], [1n, 4802n]]) {
    assert.throws(() => replayOutputFromMetadata(0n, end, frame, 48000));
  }
  assert.throws(() => replayOutputFromMetadata(0n, I64_MAX, I64_MAX + 100000000n, 1000000000),
    "a representable logical end can overflow after preroll");
  assert.throws(() => replayOutputFromMetadata(0n, I64_MAX - 100000000n, 9223372037n, 1),
    "upward frame rounding itself can exceed the signed timestamp boundary");
  assert.deepEqual(replayOutputFromMetadata(I64_MAX, undefined, undefined, 1), { endNs: undefined, endFrame: undefined });
});

test("audio capacities preserve independent bounded budgets in a fresh immutable snapshot", () => {
  const defaults = { queueCapacity: "4096", maxVoices: "4096", pendingCapacity: "4096", maxFrames: "4096", maxCommandsPerRender: "4096" };
  const captured = audioLimitsFromFields(defaults);
  assert.deepEqual(captured, { queueCapacity: 4096, maxVoices: 4096, pendingCapacity: 4096, maxFrames: 4096, maxCommandsPerRender: 4096 });
  assert.ok(Object.isFrozen(captured));
  assert.notEqual(audioLimitsFromFields(defaults), captured);
  defaults.queueCapacity = "1";
  assert.equal(captured.queueCapacity, 4096);
  assert.deepEqual(audioLimitsFromFields({ queueCapacity: "65536", maxVoices: "00001", pendingCapacity: "4096",
    maxFrames: "00257", maxCommandsPerRender: "1" }), {
    queueCapacity: 65536, maxVoices: 1, pendingCapacity: 4096, maxFrames: 257, maxCommandsPerRender: 1,
  }, "a render budget is independent of queue allocation and frame capacity is not a fixed quantum");
  assert.deepEqual(audioLimitsFromFields({ queueCapacity: "1", maxVoices: "4096", pendingCapacity: "1",
    maxFrames: "1", maxCommandsPerRender: "65536" }), {
    queueCapacity: 1, maxVoices: 4096, pendingCapacity: 1, maxFrames: 1, maxCommandsPerRender: 65536,
  });
});

test("audio capacity text refuses malformed or unknown fields and each exact ceiling without clamping", () => {
  const fields = { queueCapacity: "4096", maxVoices: "4096", pendingCapacity: "4096", maxFrames: "4096", maxCommandsPerRender: "4096" };
  for (const [name, maximum] of [["queueCapacity", 65536], ["maxVoices", 4096], ["pendingCapacity", 4096],
    ["maxFrames", 4096], ["maxCommandsPerRender", 65536]]) {
    for (const value of [undefined, null, 1, 1n, "", "0", "-0", "+1", " 1", "1 ", "1\n", "1.0", "1e3",
      "１２８", "000001", String(maximum + 1)]) {
      assert.throws(() => audioLimitsFromFields({ ...fields, [name]: value }), `${name}: ${String(value)}`);
    }
    const missing = { ...fields };
    delete missing[name];
    assert.throws(() => audioLimitsFromFields(missing));
  }
  const hidden = { ...fields };
  Object.defineProperty(hidden, "silentCapacity", { value: "1" });
  for (const malformed of [undefined, null, [], Object.values(fields), Object.create(fields),
    { ...fields, unknown: "1" }, { ...fields, [Symbol("extra")]: "1" }, hidden]) {
    assert.throws(() => audioLimitsFromFields(malformed));
  }
  assert.equal(audioLimitsFromFields(fields).queueCapacity, 4096, "refusal cannot rewrite the caller's retained draft");
});

test("output fields preserve category defaults, exact admitted latency units and automatic versus requested rates", () => {
  const automatic = audioOutputFromFields("interactive", "", "");
  assert.deepEqual(automatic, { latencyHint: "interactive" });
  assert.equal(Object.hasOwn(automatic, "sampleRate"), false);
  assert.ok(Object.isFrozen(automatic));
  for (const latency of ["interactive", "balanced", "playback"]) {
    assert.deepEqual(audioOutputFromFields(latency, "invalid inactive draft", "48000"), { latencyHint: latency, sampleRate: 48000 });
  }
  for (const [text, seconds] of [["0", 0], ["0.000001", 0.000000001], ["0.5", 0.0005],
    ["12.345678", 0.012345678], ["60000", 60], ["00000000000000.000001", 0.000000001]]) {
    const options = audioOutputFromFields("custom", text, "0000048000");
    assert.deepEqual(options, { latencyHint: seconds, sampleRate: 48000 });
    assert.ok(Object.isFrozen(options));
  }
  assert.deepEqual(audioOutputFromFields("playback", "-broken", "1"), { latencyHint: "playback", sampleRate: 1 });
  assert.deepEqual(audioOutputFromFields("balanced", "60001", "4294967295"), { latencyHint: "balanced", sampleRate: 4294967295 });
});

test("output field admission rejects malformed values without clamping rates or falling back from custom latency", () => {
  for (const latency of [undefined, null, "", "low", "Interactive", "interactive\n", 0]) {
    assert.throws(() => audioOutputFromFields(latency, "10", "48000"));
  }
  for (const milliseconds of [undefined, null, 1, "", "+0", "-0", "-1", " 1", "1\n", "1.", ".1",
    "1e3", "Infinity", "1.0000001", "60000.000001", "0".repeat(22)]) {
    assert.throws(() => audioOutputFromFields("custom", milliseconds, "48000"), String(milliseconds));
  }
  for (const rate of [undefined, null, 48000, "0", "-1", "+48000", " 48000", "48000\n", "48000.0",
    "48e3", "４８０００", "4294967296", "00000048000"]) {
    assert.throws(() => audioOutputFromFields("interactive", "ignored", rate), String(rate));
  }
  assert.deepEqual(audioOutputFromFields("custom", "1", ""), { latencyHint: 0.001 });
});

test("section seconds preserve exact original-song nanoseconds and bounded playback PCM capacity", () => {
  for (const [text, expected] of [["0", 0n], ["0.000000001", 1n], ["1.125000001", 1125000001n],
    ["604800.000000001", 604800000000001n], ["9223372034.854775807", I64_MAX - 2000000000n]]) {
    assert.equal(startFromSeconds(text), expected);
    assert.equal(validateStart(expected), expected);
  }
  for (const text of [undefined, null, 1, "", "-1", "+1", " 1", "1 ", "1\n", ".1", "1.", "1e3",
    "1.0000000001", "9223372034.854775808", "00000000000", "0".repeat(21)]) {
    assert.throws(() => startFromSeconds(text), String(text));
  }
  assert.equal(validateStart(), 0n);
  assert.equal(validateStart(I64_MAX), I64_MAX, "binary protocol preserves the full signed range; text reserves existing preview lookahead");
  for (const value of [null, "0", 0, -1n, I64_MAX + 1n]) assert.throws(() => validateStart(value));
  assert.equal(ORIGINAL_PCM_SAMPLES, 3844, "full two-digit base62 original resource capacity");
  assert.equal(PLAY_PCM_SAMPLES, 7940, "3844 original assets plus the bounded 4096 selected suffixes");
});

test("judge timing text preserves exact nanoseconds across signed decimal milliseconds and the full i64 range", () => {
  for (const [text, expected] of [
    ["0", 0n], ["-0", 0n], ["+0.000000", 0n], ["0.000001", 1n], ["-0.000001", -1n],
    ["+1.000001", 1000001n], ["-1.234567", -1234567n], ["000050.000001", 50000001n],
    ["0.1", 100000n], ["0.00001", 10n], ["604800000", 604800000000000n],
    ["9223372036854.775807", I64_MAX], ["+9223372036854.775807", I64_MAX],
    ["-9223372036854.775808", -I64_MAX - 1n],
  ]) assert.equal(parseTimingMilliseconds(text), expected, text);
  for (const text of [undefined, null, 50, 1n, "", " 1", "1 ", "1\n", "+", "-", ".1", "1.",
    "1e3", "0x10", "NaN", "Infinity", "--1", "1.0000000", "0".repeat(22),
    "9223372036854.775808", "-9223372036854.775809"]) {
    assert.throws(() => parseTimingMilliseconds(text), String(text));
  }
});

test("timing profiles keep signed offset separate from nonnegative windows and snapshot validated BigInts immutably", () => {
  const defaults = validateTiming();
  assert.deepEqual(defaults, { earlyNs: 50000000n, lateNs: 50000000n, offsetNs: 0n });
  assert.ok(Object.isFrozen(defaults));
  assert.deepEqual(timingFromMilliseconds("1.000001", "2.000002", "-0.000003"), {
    earlyNs: 1000001n, lateNs: 2000002n, offsetNs: -3n,
  });
  assert.deepEqual(timingFromMilliseconds("9223372036854.775807", "0", "-9223372036854.775808"), {
    earlyNs: I64_MAX, lateNs: 0n, offsetNs: -I64_MAX - 1n,
  });
  for (const values of [["-0.000001", "50", "0"], ["50", "-1", "0"], ["50", "50", "0.0000001"]]) {
    assert.throws(() => timingFromMilliseconds(...values));
  }
  const supplied = { earlyNs: 0n, lateNs: I64_MAX, offsetNs: I64_MAX };
  const captured = validateTiming(supplied);
  supplied.earlyNs = 99n;
  assert.equal(captured.earlyNs, 0n);
  assert.notEqual(captured, supplied);
  assert.ok(Object.isFrozen(captured));
  for (const bad of [null, {}, [], { ...defaults, earlyNs: 50 }, { ...defaults, lateNs: "50" },
    { ...defaults, offsetNs: undefined }, { ...defaults, earlyNs: -1n }, { ...defaults, lateNs: -1n },
    { ...defaults, earlyNs: I64_MAX + 1n }, { ...defaults, lateNs: I64_MAX + 1n },
    { ...defaults, offsetNs: I64_MAX + 1n }, { ...defaults, offsetNs: -I64_MAX - 2n }]) {
    assert.throws(() => validateTiming(bad));
  }
});
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
    [9223372036854.5, 9_223_372_036_854_500_000n],
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

test("keyboard choices preserve all original physical IDs and snapshots own frozen mappings including explicit unbound lanes", () => {
  const defaults = [
    [0x16, "ShiftLeft", 1], [0x11, "KeyZ", 2], [0x12, "KeyS", 3], [0x13, "KeyX", 4],
    [0x14, "KeyD", 5], [0x15, "KeyC", 6], [0x18, "KeyF", 7], [0x19, "KeyV", 8], [0x17, "Space", 9],
    [0x26, "ShiftRight", 10], [0x21, "KeyN", 11], [0x22, "KeyJ", 12], [0x23, "KeyM", 13],
    [0x24, "KeyK", 14], [0x25, "Comma", 15], [0x28, "KeyL", 16], [0x29, "Period", 17], [0x27, "Slash", 18],
  ];
  assert.deepEqual(KEY_BINDINGS, defaults);
  assert.deepEqual(KEY_CHOICES.slice(0, 18), defaults.map(([, code, id]) => [code, id]));
  assert.equal(new Set(KEY_CHOICES.map(([code]) => code)).size, KEY_CHOICES.length);
  assert.equal(new Set(KEY_CHOICES.map(([, id]) => id)).size, KEY_CHOICES.length);
  assert.ok(KEY_CHOICES.every(([code, id]) => typeof code === "string" && code !== "Escape" && Number.isInteger(id) && id > 0 && id <= 65535));
  assert.ok(Object.isFrozen(KEY_CHOICES) && KEY_CHOICES.every(Object.isFrozen));
  for (const code of ["KeyA", "Digit0", "Semicolon", "ArrowLeft", "Numpad0"]) {
    assert.ok(KEY_CHOICES.some(([name, id]) => name === code && id > 18), code);
  }
  assert.deepEqual(snapshotBindings(defaults.map(([lane, code]) => [lane, code])), defaults);
  const draft = [[0x16, "KeyA"], [0x11, ""], [0x12, "KeyB"]];
  const snapshot = snapshotBindings(draft);
  assert.deepEqual(snapshot, [[0x16, "KeyA", 19], [0x12, "KeyB", 20]]);
  assert.ok(Object.isFrozen(snapshot) && snapshot.every(Object.isFrozen));
  draft[0][1] = "ShiftLeft";
  draft.push([0x13, "KeyQ"]);
  assert.deepEqual(bindingsFor(new Uint8Array([0x12, 0x16]), snapshot), [[0x12, "KeyB", 20], [0x16, "KeyA", 19]]);
  assert.throws(() => bindingsFor([0x11], snapshot), "unbound actual lane must not fall back to its default");
  assert.deepEqual(snapshotBindings([]), []);
  assert.deepEqual(snapshotBindings(defaults.map(([lane]) => [lane, ""])), []);
  assert.deepEqual(bindingsFor([], []), []);
});

test("binding snapshots reject ambiguous malformed or sparse drafts and custom triples cannot forge physical IDs", () => {
  const sparse = Array(1);
  const missingCode = [0x11, "KeyA"]; delete missingCode[1];
  for (const value of [null, {}, new Uint8Array([0x11]), Array(19).fill([0x11, "KeyA"]), sparse,
    [missingCode], [[0x11]], [[0x11, "KeyA", 19]], [["17", "KeyA"]], [[0x10, "KeyA"]],
    [[0x11, "KeyA"], [0x11, ""]], [[0x11, ""], [0x11, ""]], [[0x11, "KeyA"], [0x12, "KeyA"]],
    [[0x11, "Escape"]], [[0x11, "KeyUnrecognized"]], [[0x11, "KeyA\n"]], [[0x11, null]]]) {
    assert.throws(() => snapshotBindings(value));
  }
  for (const custom of [[[0x11, "KeyA", 2]], [[0x11, "KeyZ", 19]], [[0x11, "KeyA", "19"]],
    [[0x11, "KeyA", 19], [0x12, "KeyA", 19]], [[0x11, "", 0]], Array(1)]) {
    assert.throws(() => bindingsFor([0x11], custom));
  }
  assert.throws(() => bindingsFor([0x11, 0x11], snapshotBindings([[0x11, "KeyA"]])));
  assert.throws(() => bindingsFor([0x11], snapshotBindings([[0x12, "KeyA"]])));
  assert.deepEqual(bindingsFor([0x11]), [[0x11, "KeyZ", 2]], "failed custom snapshots cannot alter stable defaults");
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

test("presentation pairs preserve original fractional Window time separately from output and receipt time", () => {
  const evidence = { contextTime: 1.5, performanceTime: 2050.125 };
  const expected = { outputNs: 500000000n, hostNs: 2050125000n };
  assert.deepEqual(presentationPair(evidence, 48000n, 48000, 2200), expected);
  assert.deepEqual(presentationPair(evidence, 48000n, 48000, 3050.125), expected,
    "neither coordinate advances with the observation's receipt time");
  assert.equal(presentationPoint(evidence, 48000n, 48000, 2200), expected.outputNs);
  assert.deepEqual(presentationPair({ contextTime: 1, performanceTime: 9007199254.75 },
    1n, 44100, 9007199255), { outputNs: 999977324n, hostNs: 9007199254750000n });
  assert.deepEqual(presentationPair({ contextTime: 604800.125, performanceTime: 604800000.125 },
    604800n * 48000n, 48000, 604800001), { outputNs: 125000000n, hostNs: 604800000125000n });
  for (const timestamp of [{ contextTime: 0, performanceTime: 2050 },
    { contextTime: 1.5, performanceTime: 0 }, { contextTime: 0.5, performanceTime: 2050 },
    { contextTime: 1.5, performanceTime: 999 }, { contextTime: 1.5, performanceTime: 2201 }]) {
    assert.equal(presentationPair(timestamp, 48000n, 48000, 2200), null);
  }
  assert.throws(() => presentationPair({ contextTime: 1, performanceTime: 9223372036855 },
    0n, 48000, 9223372036855), "original host time must fit signed nanoseconds");
});

test("committed starts round upward onto the exact shared frame without retiming the accepted target", () => {
  const clock = { beforeMs: 1000, afterMs: 1002, contextTime: 2, sampleRate: 48000 };
  assert.deepEqual(committedStartProjection(clock, 1501000000n, 1002), {
    startFrame: 120000n, origin: 1501000000n, uncertaintyNs: 1000000n, roundingNs: 0n,
  });
  assert.deepEqual(committedStartProjection(clock, 1501000001n, 1002), {
    startFrame: 120001n, origin: 1501020833n, uncertaintyNs: 1000000n, roundingNs: 20832n,
  });
  const grid441 = { beforeMs: 1000, afterMs: 1000, contextTime: 2, sampleRate: 44100 };
  assert.deepEqual(committedStartProjection(grid441, 1100000001n, 1000), {
    startFrame: 92611n, origin: 1100022675n, uncertaintyNs: 0n, roundingNs: 22674n,
  });
  for (const [snapshot, target, now] of [
    [clock, 1501000001n, 1002],
    [grid441, 1100000001n, 1000],
    [{ beforeMs: 604800000, afterMs: 604800000, contextTime: 2 ** 33 + 2 ** -18,
      sampleRate: 1000000000 }, 604800500000001n, 604800000],
  ]) {
    const chosen = committedStartProjection(snapshot, target, now);
    assert.equal(chosen.origin, startProjection(snapshot, chosen.startFrame));
    assert.equal(chosen.roundingNs, chosen.origin - target);
    assert.ok(chosen.origin >= target);
    assert.ok(startProjection(snapshot, chosen.startFrame - 1n) < target);
    assert.ok(chosen.roundingNs <= (1000000000n + BigInt(snapshot.sampleRate) - 1n) / BigInt(snapshot.sampleRate));
  }
});

test("committed starts enforce conservative lead and combined uncertainty at exact boundaries", () => {
  const clock = { beforeMs: 1000, afterMs: 1002, contextTime: 2, sampleRate: 48000 };
  const peer = 99_000_000n;
  const boundary = committedStartProjection(clock, 1202000000n, 1002, peer);
  assert.equal(boundary.uncertaintyNs, 100000000n);
  assert.throws(() => committedStartProjection(clock, 1201999999n, 1002, peer), "one ns too little conservative lead");
  assert.throws(() => committedStartProjection(clock, 1500000000n, 1002, peer + 1n), "peer plus bracket exceeds cap");
  const fresh = { beforeMs: 1000, afterMs: 1000, contextTime: 1, sampleRate: 48000 };
  assert.doesNotThrow(() => committedStartProjection(fresh, 1500000000n, 1100));
  assert.throws(() => committedStartProjection(fresh, 1500000000n, 1100.125));
  assert.throws(() => committedStartProjection(fresh, 1500000000n, 999.875), "future bracket cannot be used");
  const oddBracket = { beforeMs: 0, afterMs: 0.000001, contextTime: 0, sampleRate: 1000000000 };
  assert.equal(committedStartProjection(oddBracket, 100000002n, 0.000001).uncertaintyNs, 1n);
});

test("malformed or overflowing committed clocks cannot arm an earlier or unrepresentable frame", () => {
  const clock = { beforeMs: 1000, afterMs: 1000, contextTime: 2, sampleRate: 48000 };
  for (const target of [-1n, I64_MAX + 1n, 1500000000, null]) {
    assert.throws(() => committedStartProjection(clock, target, 1000));
  }
  for (const [peer, lead, cap] of [[-1n, 1n, 1n], [U64_MAX + 1n, 1n, I64_MAX],
    [0, 1n, 1n], [0n, 0n, 1n], [0n, I64_MAX + 1n, 1n], [0n, 1n, 0n], [0n, 1n, I64_MAX + 1n]]) {
    assert.throws(() => committedStartProjection(clock, 1500000000n, 1000, peer, lead, cap));
  }
  for (const snapshot of [null, { ...clock, beforeMs: 1001 }, { ...clock, afterMs: NaN },
    { ...clock, contextTime: -1 }, { ...clock, sampleRate: 0 },
    { ...clock, contextTime: 2 ** 33, sampleRate: 0xffffffff },
    { ...clock, contextTime: 9223372036.75, sampleRate: 48000 }]) {
    assert.throws(() => committedStartProjection(snapshot, 2000000000n, 1000));
  }
});

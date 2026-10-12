import test from "node:test";
import assert from "node:assert/strict";
import {
  playbackSpeedFromFraction,
  playbackOutputFromMetadata,
  replaySpeedFromFraction,
  replayOutputFromMetadata,
  validateLiveColdPhase,
} from "./play-model.mjs";

const I64_MAX = 9223372036854775807n;
const U64_MAX = 18446744073709551615n;

test("cold live admission checks the whole signed preroll at a nonzero original start", () => {
  for (const [fraction, expected] of [["1/2", 9n], ["1/1", 7n], ["2/1", 4n], ["3/2", 6n]]) {
    const speed = playbackSpeedFromFraction(fraction);
    assert.equal(validateLiveColdPhase(10n, speed, 3n), expected, fraction);
    assert.deepEqual(speed, playbackSpeedFromFraction(fraction));
  }
  assert.equal(validateLiveColdPhase(10n, playbackSpeedFromFraction("1/2"), 1n), 10n,
    "negative half nanosecond truncates toward zero before adding the original start");
  assert.equal(validateLiveColdPhase(604800000000001n, playbackSpeedFromFraction("3/2"), 3n),
    604799999999997n);
  assert.equal(validateLiveColdPhase(2000000000n), 1900000000n);
});

test("cold live phase admits exact signed minimum and refuses the next unrepresentable value", () => {
  assert.equal(validateLiveColdPhase(0n, playbackSpeedFromFraction("4611686018427387904/1"), 2n),
    -9223372036854775808n);
  assert.throws(() => validateLiveColdPhase(0n, playbackSpeedFromFraction("4611686018427387905/1"), 2n), Error);
  const max = playbackSpeedFromFraction("9223372036854775807/1");
  assert.equal(validateLiveColdPhase(I64_MAX - 1n, max, 2n), -9223372036854775808n);
  assert.throws(() => validateLiveColdPhase(I64_MAX - 2n, max, 2n), Error);
  assert.equal(validateLiveColdPhase(0n, max, 1n), -9223372036854775807n);
  assert.equal(validateLiveColdPhase(I64_MAX, max, 0n), I64_MAX);
  assert.throws(() => validateLiveColdPhase(0n, max), Error,
    "valid fraction syntax cannot authorize an unrepresentable fixed-preroll phase");
});

test("cold admission retains full-width ratios and validates selected start instead of banning large rates", () => {
  assert.equal(validateLiveColdPhase(0n, playbackSpeedFromFraction("9223372036854775807/18446744073709551615")), -49999999n);
  assert.equal(validateLiveColdPhase(0n, playbackSpeedFromFraction("1/18446744073709551615")), 0n);
  assert.equal(validateLiveColdPhase(0n, playbackSpeedFromFraction("92233720368/1")), -9223372036800000000n);
  assert.throws(() => validateLiveColdPhase(0n, playbackSpeedFromFraction("92233720369/1")), Error);
  assert.equal(validateLiveColdPhase(100000000n, playbackSpeedFromFraction("92233720369/1")), -9223372036800000000n);
});

test("cold admission rejects nonexact setup inputs without mutating a normalized snapshot", () => {
  const selected = playbackSpeedFromFraction("3/2");
  for (const start of [-1n, I64_MAX + 1n, 0, "0", null]) {
    assert.throws(() => validateLiveColdPhase(start, selected), Error);
  }
  for (const preroll of [-1n, I64_MAX + 1n, 0, "0", null]) {
    assert.throws(() => validateLiveColdPhase(0n, selected, preroll), Error);
  }
  for (const speed of ["3/2", null, {}, { ...selected, numerator: 3 },
    { numerator: 6n, denominator: 4n, fraction: "6/4" }, { ...selected, fraction: "2/1" }]) {
    assert.throws(() => validateLiveColdPhase(0n, speed), Error);
  }
  assert.deepEqual(selected, { numerator: 3n, denominator: 2n, fraction: "3/2" });
  assert.ok(Object.isFrozen(selected));
});

test("general playback speed defaults to a fresh immutable normal snapshot", () => {
  const first = playbackSpeedFromFraction();
  assert.deepEqual(first, { numerator: 1n, denominator: 1n, fraction: "1/1" });
  assert.ok(Object.isFrozen(first));
  assert.notEqual(first, playbackSpeedFromFraction());
  assert.throws(() => { first.numerator = 2n; }, TypeError);
  assert.equal(first.numerator, 1n);
});

test("live fractions normalize all four selected rates without changing caller text", () => {
  for (const [input, numerator, denominator, fraction] of [
    ["00002/00004", 1n, 2n, "1/2"],
    ["24/24", 1n, 1n, "1/1"],
    ["18/9", 2n, 1n, "2/1"],
    ["21/14", 3n, 2n, "3/2"],
  ]) {
    const selected = playbackSpeedFromFraction(input);
    assert.deepEqual(selected, { numerator, denominator, fraction });
    assert.ok(Object.isFrozen(selected));
    assert.equal(typeof selected.numerator, "bigint");
    assert.equal(typeof selected.denominator, "bigint");
  }
});

test("general parser admits exact integer bounds and the forty-character boundary", () => {
  assert.deepEqual(playbackSpeedFromFraction("9223372036854775807/18446744073709551615"), {
    numerator: I64_MAX, denominator: U64_MAX,
    fraction: "9223372036854775807/18446744073709551615",
  });
  assert.deepEqual(playbackSpeedFromFraction("9223372036854775806/18446744073709551612"), {
    numerator: 1n, denominator: 2n, fraction: "1/2",
  });
  assert.equal(playbackSpeedFromFraction("0".repeat(37) + "1/1").fraction, "1/1");
  assert.throws(() => playbackSpeedFromFraction("0".repeat(38) + "1/1"), Error);
});

test("live speed rejects malformed, signed, non-ASCII, zero and oversized components", () => {
  for (const value of [
    "", "1", "1/", "/1", "1/2/3", "1.0/2", "1e2/1", "1 /2", " 1/2", "1/2 ",
    "1/2\n", "+1/2", "-1/2", "1/-2", "1/+2", "０１/２", "١/٢", "0/1", "1/0",
    "000/1", "1/000", "9223372036854775808/1", "1/18446744073709551616",
    null, false, 1, 1n, {}, ["1/1"],
  ]) assert.throws(() => playbackSpeedFromFraction(value), Error, String(value));
});

test("live finite endpoints use output preroll and literal four-rate frame grids", () => {
  const start = 2000000000n;
  const end = 2004000000n;
  for (const [fraction, frame] of [["1/2", 11n], ["1/1", 7n], ["2/1", 5n], ["3/2", 6n]]) {
    const speed = playbackSpeedFromFraction(fraction);
    const endpoint = playbackOutputFromMetadata(start, end, frame, 1000, speed, 3000000n);
    assert.deepEqual(endpoint, { endNs: end, endFrame: frame }, fraction);
    assert.ok(Object.isFrozen(endpoint));
    assert.throws(() => playbackOutputFromMetadata(start, end, frame - 1n, 1000, speed, 3000000n), Error);
    assert.throws(() => playbackOutputFromMetadata(start, end, frame + 1n, 1000, speed, 3000000n), Error);
  }
});

test("three-halves maps to whole inverse nanoseconds before physical frame ceiling", () => {
  const speed = playbackSpeedFromFraction("3/2");
  assert.deepEqual(playbackOutputFromMetadata(0n, 4000000n, 5666666n, 1000000000, speed, 3000000n),
    { endNs: 4000000n, endFrame: 5666666n });
  assert.throws(() => playbackOutputFromMetadata(0n, 4000000n, 5666667n, 1000000000, speed, 3000000n), Error);
  assert.deepEqual(playbackOutputFromMetadata(0n, 4000000n, 6n, 1000, speed, 3000000n),
    { endNs: 4000000n, endFrame: 6n });
});

test("positive sub-nanosecond inverse spans retain their genuine zero output fence", () => {
  const double = playbackSpeedFromFraction("2/1");
  assert.deepEqual(playbackOutputFromMetadata(0n, 1n, 0n, 48000, double, 0n), { endNs: 1n, endFrame: 0n });
  assert.throws(() => playbackOutputFromMetadata(0n, 1n, 1n, 48000, double, 0n), Error);
  assert.throws(() => playbackOutputFromMetadata(0n, 1n, 0n, 48000, double), Error);
  assert.deepEqual(playbackOutputFromMetadata(0n, 3n, 1n, 1000000000, double, 0n), { endNs: 3n, endFrame: 1n });
  assert.throws(() => playbackOutputFromMetadata(0n, 0n, 0n, 48000, double, 0n), Error);
});

test("maximum fraction components stay exact instead of passing through Number", () => {
  const nearHalf = playbackSpeedFromFraction("9223372036854775807/18446744073709551615");
  assert.deepEqual(playbackOutputFromMetadata(0n, 1n, 2n, 1000000000, nearHalf, 0n), { endNs: 1n, endFrame: 2n });
  const fastest = playbackSpeedFromFraction("9223372036854775807/1");
  assert.deepEqual(playbackOutputFromMetadata(0n, 1n, 0n, 1000000000, fastest, 0n), { endNs: 1n, endFrame: 0n });
  assert.throws(() => playbackOutputFromMetadata(0n, 1n, U64_MAX, 1000000000,
    playbackSpeedFromFraction("1/18446744073709551615"), 0n), Error);
});

test("original-song maximum start is independent of output duration", () => {
  assert.deepEqual(playbackOutputFromMetadata(I64_MAX - 4n, I64_MAX, 5n, 1000000000,
    playbackSpeedFromFraction("2/1"), 3n), { endNs: I64_MAX, endFrame: 5n });
  assert.deepEqual(playbackOutputFromMetadata(I64_MAX, undefined, undefined, 1),
    { endNs: undefined, endFrame: undefined });
});

test("output mapped duration and final physical grid must both fit signed nanoseconds", () => {
  const half = playbackSpeedFromFraction("1/2");
  assert.equal(playbackOutputFromMetadata(0n, 4611686018427387903n, 9223372036854775806n,
    1000000000, half, 0n).endFrame, 9223372036854775806n);
  assert.throws(() => playbackOutputFromMetadata(0n, 4611686018427387904n, 9223372036854775808n,
    1000000000, half, 0n), Error);
  assert.equal(playbackOutputFromMetadata(0n, I64_MAX, I64_MAX, 1000000000, undefined, 0n).endFrame, I64_MAX);
  assert.throws(() => playbackOutputFromMetadata(0n, I64_MAX, I64_MAX + 1n, 1000000000, undefined, 1n), Error);
  assert.equal(playbackOutputFromMetadata(0n, 9223372036000000000n, 9223372036n, 1, undefined, 0n).endFrame, 9223372036n);
  assert.throws(() => playbackOutputFromMetadata(0n, 9223372036000000001n, 9223372037n, 1, undefined, 0n), Error);
});

test("general output requires a normalized model snapshot rather than a mode string", () => {
  const speed = playbackSpeedFromFraction("3/2");
  for (const bad of [
    "3/2", null, [], {}, { ...speed, numerator: 3 }, { ...speed, denominator: 0n },
    { numerator: 6n, denominator: 4n, fraction: "6/4" },
    { ...speed, fraction: "6/4" }, { ...speed, fraction: "2/1" },
    { ...speed, mode: "live" }, { ...speed, [Symbol("extra")]: true },
  ]) {
    assert.throws(() => playbackOutputFromMetadata(0n, undefined, undefined, 48000, bad), Error);
    assert.throws(() => playbackOutputFromMetadata(0n, 4000000n, 6n, 1000, bad, 3000000n), Error);
  }
});

test("general output rejects invalid metadata, sample Hz and output preroll before adoption", () => {
  const speed = playbackSpeedFromFraction("3/2");
  for (const hz of [0, -1, 1.5, NaN, Infinity, 4294967296, 48000n, "48000"]) {
    assert.throws(() => playbackOutputFromMetadata(0n, undefined, undefined, hz, speed), Error);
  }
  for (const start of [-1n, I64_MAX + 1n, 0, "0", null]) {
    assert.throws(() => playbackOutputFromMetadata(start, undefined, undefined, 48000, speed), Error);
  }
  for (const preroll of [-1n, I64_MAX + 1n, 0, "0", null]) {
    assert.throws(() => playbackOutputFromMetadata(0n, undefined, undefined, 48000, speed, preroll), Error);
  }
  for (const [end, frame] of [
    [undefined, 6n], [4000000n, undefined], [0n, 6n], [-1n, 6n],
    [I64_MAX + 1n, 6n], [4000000, 6n], [4000000n, 6],
    [4000000n, -1n], [4000000n, U64_MAX + 1n],
  ]) assert.throws(() => playbackOutputFromMetadata(0n, end, frame, 1000, speed, 3000000n), Error);
  assert.deepEqual(speed, { numerator: 3n, denominator: 2n, fraction: "3/2" });
});

test("unlimited live output keeps a fresh frozen endpoint while validating speed and preroll", () => {
  const speed = playbackSpeedFromFraction("3/2");
  const endpoint = playbackOutputFromMetadata(0n, undefined, undefined, 48000, speed, 0n);
  assert.deepEqual(endpoint, { endNs: undefined, endFrame: undefined });
  assert.ok(Object.isFrozen(endpoint));
  assert.notEqual(endpoint, playbackOutputFromMetadata(0n, undefined, undefined, 48000, speed, 0n));
  assert.throws(() => { endpoint.endFrame = 1n; }, TypeError);
});

test("general four-argument normal output and original replay API retain literal compatibility", () => {
  const expected = { endNs: 3000000000n, endFrame: 31n };
  assert.deepEqual(playbackOutputFromMetadata(0n, 3000000000n, 31n, 10), expected);
  assert.deepEqual(replayOutputFromMetadata(0n, 3000000000n, 31n, 10), expected);
  assert.deepEqual(playbackOutputFromMetadata(0n, 3000000000n, 31n, 10,
    playbackSpeedFromFraction("8/8")), expected);
  assert.throws(() => playbackOutputFromMetadata(0n, 3000000000n, 16n, 10), Error);
  assert.deepEqual(replaySpeedFromFraction("21/14"), { numerator: 3n, denominator: 2n, fraction: "3/2" });
  assert.deepEqual(replayOutputFromMetadata(0n, 4000000n, 6n, 1000,
    replaySpeedFromFraction("3/2"), 3000000n), { endNs: 4000000n, endFrame: 6n });
});

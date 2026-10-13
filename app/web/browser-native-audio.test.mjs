// Actual freshly generated production audio WASM, never a JS clock/Mixer mock.
// Run after the coordinator rebuilds audio-pkg:
// node --experimental-vm-modules --test app/web/browser-native-audio.test.mjs
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, runInContext } from "node:vm";
import { pathToFileURL } from "node:url";

const packageUrl = process.env.BEATKERNEL_AUDIO_PACKAGE
  ? pathToFileURL(`${process.env.BEATKERNEL_AUDIO_PACKAGE.replace(/\/$/, "")}/`)
  : new URL("./audio-pkg/", import.meta.url);
const bindingUrl = new URL("beatkernel_bms_runtime.js", packageUrl);
const glue = await readFile(bindingUrl, "utf8");
const bytes = await readFile(new URL("beatkernel_bms_runtime_bg.wasm", packageUrl));
const context = createContext({ WebAssembly, TextEncoder, TextDecoder, Uint8Array,
  Uint32Array, Float32Array, Float64Array, ArrayBuffer, DataView, URL, Request, Response, console });
const binding = new SourceTextModule(glue, { context, identifier: bindingUrl.href,
  initializeImportMeta(meta) { meta.url = bindingUrl.href; } });
await binding.link(specifier => { throw new Error(`unexpected production binding import ${specifier}`); });
await binding.evaluate();
context.actualAudioModule = new WebAssembly.Module(bytes);
binding.namespace.initSync(runInContext("({ module: actualAudioModule })", context));
const { BrowserAudio } = binding.namespace;
assert.equal(typeof BrowserAudio, "function", "actual production audio owner is required");
const REQUIRED = Object.freeze({ native_clock_start: 8, native_clock_extend: 6,
  native_frame: 8, native_clock_ready: 0, arm_words: 4 });
for (const [name, count] of Object.entries(REQUIRED)) {
  assert.equal(typeof BrowserAudio.prototype[name], "function", `fresh numeric export ${name} is required`);
  assert.equal(BrowserAudio.prototype[name].length, count, `frozen scalar arity of ${name}`);
}

// This test oracle uses a general product/Bezout CRT, independent of the
// production fixed mixed-radix formula. BigInt belongs to this test only.
const P = Object.freeze([1048576n, 1048575n, 1048573n]);
const PRODUCT = P.reduce((a, b) => a * b, 1n);
const MAX = 9007199254740991n;
const mod = (value, divisor) => (value % divisor + divisor) % divisor;
function inverse(value, modulus) {
  let [oldR, r, oldS, s] = [value, modulus, 1n, 0n];
  while (r !== 0n) {
    const q = oldR / r;
    [oldR, r] = [r, oldR - q * r];
    [oldS, s] = [s, oldS - q * s];
  }
  assert.equal(oldR, 1n, "oracle moduli must be coprime");
  return mod(oldS, modulus);
}
const CRT_WEIGHTS = P.map(period => {
  const other = PRODUCT / period;
  return other * inverse(other, period);
});
function oracle(phases) {
  return mod(phases.reduce((sum, phase, index) => sum + BigInt(phase) * CRT_WEIGHTS[index], 0n), PRODUCT);
}
function residues(offset) { return P.map(period => Number(mod(BigInt(offset), period))); }
function words(value) {
  value = BigInt(value);
  return [Number(value & 0xffffffffn), Number(value >> 32n)];
}
function horizons(origin, count = 3n) { return P.map(period => BigInt(origin) + BigInt(count) * period); }
function maxHorizons(origin) {
  origin = BigInt(origin);
  return P.map(period => origin + ((MAX - origin) / period) * period);
}
function audio({ rate = 48000, channels = 1, frames = 4096 } = {}) {
  const owner = new BrowserAudio(rate, channels, 16384, 65536, 4, 16, 4, 16, frames, 16);
  assert.equal(owner.status(), 0, "actual builder configuration");
  return owner;
}
function start(owner, origin, ownHorizons = horizons(origin)) {
  return owner.native_clock_start(...words(origin), ...ownHorizons.flatMap(words));
}
function extend(owner, ownHorizons) {
  return owner.native_clock_extend(...ownHorizons.flatMap(words));
}
function native(owner, offset, frames, firstIndex = 0) {
  offset = BigInt(offset);
  const first = residues(offset), last = residues(offset + BigInt(frames - firstIndex - 1));
  assert.equal(oracle(first), offset, "independent exact first candidate");
  assert.equal(oracle(last), offset + BigInt(frames - firstIndex - 1), "independent exact last candidate");
  return owner.native_frame(...first, ...last, firstIndex, frames);
}
function certify(owner, origin, ownHorizons = horizons(origin)) {
  assert.equal(start(owner, origin, ownHorizons), 0);
  assert.equal(owner.native_clock_ready(), false);
  assert.equal(native(owner, 0n, 128, 64), Number(BigInt(origin) - 64n));
  assert.equal(owner.native_clock_ready(), false, "partial certificate alone is not ready");
  assert.equal(native(owner, 64n, 128), Number(BigInt(origin) + 64n));
  assert.equal(owner.native_clock_ready(), true, "following contiguous full block is ready");
}
function report(owner) {
  assert.equal(owner.report_fields, 29, "unchanged actual report declaration");
  return Array.from({ length: 29 }, (_, index) =>
    BigInt(owner.report_word(index, false)) | (BigInt(owner.report_word(index, true)) << 32n));
}
function pcm(owner, length = owner.output_len()) {
  return new Float32Array(BrowserAudio.memory().buffer, owner.output_ptr(), length);
}
function render(owner, frame, frames) { return owner.render(...words(frame), frames); }
function arm(owner, target, actual) { return owner.arm_words(...words(target), ...words(actual)); }
function withAudio(body, options) {
  const owner = audio(options);
  try { return body(owner); } finally { owner.free(); }
}
const ORIGIN = 1000000n;

test("fresh scalar exports are direct numeric glue with no input array conversion", () => {
  const exports = WebAssembly.Module.exports(context.actualAudioModule);
  for (const [name, arity] of Object.entries(REQUIRED)) {
    assert.ok(exports.some(entry => entry.kind === "function" && entry.name === `browseraudio_${name}`),
      `actual compiled WASM export ${name}`);
    const match = glue.match(new RegExp(`\\n    ${name}\\(([^)]*)\\) \\{([^}]*)\\}`, "u"));
    assert.ok(match, `generated public body for ${name}`);
    assert.equal(match[1].split(",").filter(value => value.trim() !== "").length, arity);
    assert.match(match[2], new RegExp(`wasm\\.browseraudio_${name}\\(`, "u"));
    assert.doesNotMatch(match[2], /malloc|realloc|passArray|WASM_VECTOR_LEN|BigInt|new\s|\.set\(|\.slice\(|\.subarray\(/u,
      `scalar glue of ${name} must not import/copy callback arrays or allocate a result wrapper`);
  }
  // Setup sample import remains a real owned-array import; this distinction
  // prevents stable memory identity being misreported as an allocator proof.
  assert.match(glue, /insert_sample[\s\S]*?passArrayF32ToWasm0/u);
});

test("actual clock can authenticate during sample setup and preserves report29", () => withAudio(owner => {
  certify(owner, ORIGIN);
  assert.deepEqual(report(owner), Array(29).fill(0n));
  assert.equal(owner.insert_sample(1n, 48000, 1, Float32Array.of(0.25, -0.5)), 0);
  assert.equal(owner.finish(), 0);
  assert.equal(owner.channels(), 1);
  assert.equal(owner.max_frames(), 4096);
  assert.equal(owner.output_len(), 4096);
  assert.ok(pcm(owner).every(value => value === 0), "authentication has not rendered PCM");
  assert.equal(native(owner, 192n, 1), Number(ORIGIN + 192n));
  assert.deepEqual(report(owner), Array(29).fill(0n), "numeric decoding alone is not a render report");
}));

test("setup absence and duplicate clock configuration refuse without replacing origin", () => withAudio(owner => {
  assert.equal(owner.native_frame(0, 0, 0, 0, 0, 0, 1, 2), -1);
  assert.equal(owner.status(), 2);
  assert.equal(extend(owner, horizons(ORIGIN)), 2);
  assert.equal(owner.native_clock_ready(), false);
  assert.equal(start(owner, ORIGIN), 0);
  assert.equal(start(owner, ORIGIN + 100n), 2);
  assert.equal(native(owner, 0n, 128, 64), Number(ORIGIN - 64n), "original origin survives duplicate setup");
  assert.equal(native(owner, 64n, 128), Number(ORIGIN + 64n));
  assert.equal(owner.native_clock_ready(), true);
}));

test("unsafe, zero, missing and nonintegral horizon geometry cannot consume setup", () => {
  const cases = [
    { origin: 0n, h: horizons(0n), status: 1 },
    { origin: MAX + 1n, h: horizons(MAX + 1n), status: 7 },
    { origin: ORIGIN, h: [ORIGIN, ...horizons(ORIGIN).slice(1)], status: 1 },
    { origin: ORIGIN, h: [ORIGIN - 1n, ...horizons(ORIGIN).slice(1)], status: 1 },
    { origin: ORIGIN, h: [horizons(ORIGIN)[0] + 1n, ...horizons(ORIGIN).slice(1)], status: 1 },
    { origin: ORIGIN, h: [MAX + 1n, ...horizons(ORIGIN).slice(1)], status: 7 },
  ];
  for (const entry of cases) withAudio(owner => {
    assert.equal(start(owner, entry.origin, entry.h), entry.status);
    assert.equal(owner.status(), entry.status);
    certify(owner, ORIGIN);
  });
});

test("descending horizons keep fixed own-lane association", () => withAudio(owner => {
  const ownHorizons = horizons(ORIGIN);
  assert.ok(ownHorizons[0] > ownHorizons[1] && ownHorizons[1] > ownHorizons[2]);
  certify(owner, ORIGIN, ownHorizons);
  assert.equal(native(owner, P[2] - 3n, 3), Number(ORIGIN + P[2] - 3n));
  assert.equal(native(owner, P[2], 4), Number(ORIGIN + P[2]), "different own wraps still identify one exact base");
}));

test("exact scalar CRT agrees with a general oracle across 4096 sorted vectors", () => withAudio(owner => {
  certify(owner, 128n, maxHorizons(128n));
  const vectors = new Set([192n, ...P.flatMap(period => [period - 1n, period, period + 1n]),
    (1n << 32n) - 1n, 1n << 32n, (1n << 32n) + 1n,
    48000n * 20n * 3600n, 48000n * 7n * 24n * 3600n, MAX - 4n * P[0]]);
  let seed = 0x391cf733n;
  for (let index = 0; index < 4096; index++) {
    seed = (seed * 6364136223846793005n + 1442695040888963407n) & ((1n << 64n) - 1n);
    vectors.add(192n + seed % (MAX - 8n * P[0]));
  }
  const ordered = [...vectors].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  for (const offset of ordered) {
    assert.equal(native(owner, offset, 1), Number(128n + offset), `exact candidate at ${offset}`);
    assert.equal(owner.status(), 0);
  }
}));

test("20-hour/week coordinates and original high words remain exact arithmetic coverage", () => {
  for (const rate of [44100, 48000]) for (const seconds of [20n * 3600n, 7n * 24n * 3600n]) {
    const origin = BigInt(rate) * seconds + 128n;
    withAudio(owner => {
      certify(owner, origin);
      assert.equal(owner.finish(), 0);
      const frame = origin + 192n;
      assert.equal(native(owner, 192n, 17), Number(frame));
      assert.equal(arm(owner, frame, frame), 0);
      assert.equal(render(owner, frame, 17), 0);
      const values = report(owner);
      assert.equal(values[23], 1n);
      assert.equal(values[24], frame + 17n);
      assert.equal(values[25], 1n);
      assert.equal(values[26], frame);
      assert.equal(values[26] + values[1] + values[2], values[24]);
    }, { rate });
  }
});

test("safe Number limit is exact and unsafe absolute end does not mutate native state", () => withAudio(owner => {
  const origin = MAX - 8n * P[0];
  certify(owner, origin, maxHorizons(origin));
  const before = owner.native_clock_ready();
  assert.equal(native(owner, MAX - origin + 1n, 1), -1);
  assert.equal(owner.status(), 7);
  assert.equal(owner.native_clock_ready(), before);
  assert.equal(native(owner, 192n, 1), Number(origin + 192n), "overflow leaves the old cursor usable");
}));

test("unsafe reconstructed CRT domain is refused rather than rounded or aliased", () => withAudio(owner => {
  certify(owner, 128n, maxHorizons(128n));
  for (const candidate of [MAX + 1n, PRODUCT - 1n]) {
    const phase = residues(candidate);
    assert.equal(oracle(phase), candidate);
    assert.equal(owner.native_frame(...phase, ...phase, 0, 1), -1);
    assert.equal(owner.status(), 7);
  }
  assert.deepEqual(residues(PRODUCT), residues(0n), "full-modulus aliases alone cannot certify age");
  assert.equal(native(owner, 192n, 1), 320);
}));

test("zero, wrong indices and out-of-period phases fail atomically", () => {
  const validFirst = residues(192n), validLast = residues(199n);
  const cases = [
    [...validFirst, ...validLast, 0, 0],
    [...validFirst, ...validLast, 1, 8],
    [...validFirst, ...validLast, 8, 8],
    [Number(P[0]), validFirst[1], validFirst[2], ...validLast, 0, 8],
    [validFirst[0], Number(P[1]), validFirst[2], ...validLast, 0, 8],
    [validFirst[0], validFirst[1], Number(P[2]), ...validLast, 0, 8],
    [...validFirst, Number(P[0]), validLast[1], validLast[2], 0, 8],
    [...validFirst, validLast[0], Number(P[1]), validLast[2], 0, 8],
    [...validFirst, validLast[0], validLast[1], Number(P[2]), 0, 8],
  ];
  for (const args of cases) withAudio(owner => {
    certify(owner, ORIGIN);
    assert.equal(owner.native_frame(...args), -1);
    assert.equal(owner.status(), 1);
    assert.equal(owner.native_clock_ready(), true);
    assert.equal(native(owner, 192n, 8), Number(ORIGIN + 192n));
  });
});

test("initial one-frame/index-zero admission cannot become readiness", () => {
  for (const [index, frames] of [[0, 1], [0, 128], [128, 128], [1, 1]]) withAudio(owner => {
    assert.equal(start(owner, ORIGIN), 0);
    assert.equal(owner.native_frame(0, 0, 0, 0, 0, 0, index, frames), -1);
    assert.equal(owner.status(), 1);
    assert.equal(owner.native_clock_ready(), false);
    assert.equal(native(owner, 0n, 128, 64), Number(ORIGIN - 64n));
    assert.equal(native(owner, 64n, 1), Number(ORIGIN + 64n));
    assert.equal(owner.native_clock_ready(), true, "authenticated one-frame callback is supported");
  });
});

test("first active offset and positive origin cannot hide callback-base underflow", () => {
  withAudio(owner => {
    assert.equal(start(owner, ORIGIN), 0);
    assert.equal(native(owner, 1n, 128, 64), -1, "initial active sample must be offset zero");
    assert.equal(owner.status(), 1, "initial phase-zero domain is an argument constraint");
    assert.equal(owner.native_clock_ready(), false);
    assert.equal(native(owner, 0n, 128, 64), Number(ORIGIN - 64n));
    assert.equal(native(owner, 64n, 128), Number(ORIGIN + 64n));
  });
  withAudio(owner => {
    assert.equal(start(owner, 1n), 0);
    assert.equal(native(owner, 0n, 3, 2), -1, "origin minus original edge must be an unsigned context base");
    assert.equal(owner.status(), 7);
    assert.equal(owner.native_clock_ready(), false);
    assert.equal(native(owner, 0n, 2, 1), 0, "failed underflow leaves a valid origin certificate usable");
    assert.equal(native(owner, 1n, 1), 2);
    assert.equal(owner.native_clock_ready(), true);
  });
});

test("second-block readiness requires continuity and incorrect endpoints preserve prior certificate", () => withAudio(owner => {
  assert.equal(start(owner, ORIGIN), 0);
  assert.equal(native(owner, 0n, 128, 64), Number(ORIGIN - 64n));
  assert.equal(native(owner, 65n, 1), -1, "second block cannot certify a gap");
  assert.equal(owner.status(), 6);
  assert.equal(owner.native_clock_ready(), false);
  assert.equal(owner.native_frame(...residues(64n), ...residues(192n), 0, 128), -1);
  assert.equal(owner.status(), 6, "last phase must match the actual original extent");
  assert.equal(native(owner, 64n, 128), Number(ORIGIN + 64n));
  assert.equal(owner.native_clock_ready(), true);
}));

test("post-ready overlaps/backtracks refuse and forward gaps retain native coordinates", () => withAudio(owner => {
  certify(owner, ORIGIN);
  for (const offset of [0n, 64n, 191n]) {
    assert.equal(native(owner, offset, 1), -1);
    assert.equal(owner.status(), 6);
  }
  assert.equal(native(owner, 256n, 9), Number(ORIGIN + 256n), "clock preserves a real forward gap for strict core admission");
  assert.equal(native(owner, 265n, 1), Number(ORIGIN + 265n));
}));

test("common committed horizon includes the last active frame but refuses its endpoint", () => withAudio(owner => {
  const own = horizons(ORIGIN), offset = own[2] - ORIGIN;
  certify(owner, ORIGIN, own);
  assert.equal(native(owner, offset - 2n, 3), -1, "block includes an active sample at common horizon");
  assert.equal(owner.status(), 6);
  assert.equal(native(owner, offset - 2n, 2), Number(own[2] - 2n), "active frames strictly before horizon are valid");
  assert.equal(native(owner, offset, 1), -1);
  assert.equal(owner.status(), 6);
  assert.equal(extend(owner, horizons(ORIGIN, 4n)), 6, "observed horizon expiry cannot be revived");
}));

test("whole integral extension preserves origin and rejects partial/nonintegral/unsafe changes atomically", () => withAudio(owner => {
  const old = horizons(ORIGIN), next = horizons(ORIGIN, 4n);
  assert.equal(start(owner, ORIGIN, old), 0);
  assert.equal(extend(owner, next), 2, "no extension before native readiness");
  assert.equal(native(owner, 0n, 128, 64), Number(ORIGIN - 64n));
  assert.equal(native(owner, 64n, 128), Number(ORIGIN + 64n));
  for (const [candidate, status] of [
    [[next[0], old[1], next[2]], 1],
    [[next[0] + 1n, next[1], next[2]], 1],
    [[old[0] - P[0], next[1], next[2]], 1],
    [[MAX + 1n, next[1], next[2]], 7],
  ]) {
    assert.equal(extend(owner, candidate), status);
    assert.equal(owner.status(), status);
  }
  assert.equal(extend(owner, next), 0);
  const oldCommonOffset = old[2] - ORIGIN;
  assert.equal(native(owner, oldCommonOffset, 9), Number(old[2]), "whole extension makes the old boundary available without rebasing");
  assert.equal(extend(owner, horizons(ORIGIN, 5n)), 0);
  assert.equal(native(owner, next[2] - ORIGIN, 1), Number(next[2]));
}));

test("actual sample PCM and natural finite-end reports stay on scalar callback arm coordinates", () => withAudio(owner => {
  certify(owner, ORIGIN);
  const sample = Float32Array.of(0.25, -0.5, 0.75, -0.25, 0.125, -0.125, 0.5, -0.75);
  assert.equal(owner.insert_sample(1n, 48000, 1, sample), 0);
  assert.equal(owner.finish_at(5n), 0);
  assert.equal(owner.enqueue(0, 1n, 1n, 0n, 1, 0n, 1n), 0);
  const callback = ORIGIN + 192n, target = callback + 3n;
  assert.equal(native(owner, 192n, 128), Number(callback));
  assert.equal(arm(owner, target, callback), 0);
  assert.equal(render(owner, callback, 128), 0);
  assert.deepEqual(Array.from(pcm(owner, 8)), [0, 0, 0, ...sample.slice(0, 5)]);
  assert.ok(pcm(owner, 128).slice(8).every(value => value === 0), "natural finite end silences remaining block");
  const crossed = report(owner);
  assert.deepEqual(crossed.slice(0, 8), [1n, 0n, 125n, 0n, 5n, 1n, 1n, 5n]);
  assert.equal(crossed[24], callback + 128n);
  assert.equal(crossed[26], target);
  assert.equal(crossed[26] + crossed[1] + crossed[2], crossed[24]);
  assert.equal(crossed[27], 0n);
  assert.equal(native(owner, 320n, 64), Number(callback + 128n));
  assert.equal(render(owner, callback + 128n, 64), 0);
  const ended = report(owner);
  assert.deepEqual(ended.slice(1, 8), [125n, 64n, 5n, 0n, 1n, 1n, 5n]);
  assert.equal(ended[24], callback + 192n);
  assert.equal(ended[26], target);
  assert.ok(pcm(owner, 64).every(value => value === 0));
}));

test("failed scalar native admission leaves existing PCM/report intact and does not render", () => withAudio(owner => {
  certify(owner, ORIGIN);
  assert.equal(owner.insert_sample(1n, 48000, 1, Float32Array.of(0.25, -0.5, 0.75, -0.25)), 0);
  assert.equal(owner.finish(), 0);
  assert.equal(owner.enqueue(0, 1n, 1n, 0n, 1, 0n, 1n), 0);
  const frame = ORIGIN + 192n;
  assert.equal(native(owner, 192n, 2), Number(frame));
  assert.equal(arm(owner, frame, frame), 0);
  assert.equal(render(owner, frame, 2), 0);
  const beforeReport = report(owner), beforePcm = Array.from(pcm(owner));
  assert.equal(owner.native_frame(Number(P[0]), 0, 0, 0, 0, 0, 0, 1), -1);
  assert.equal(owner.status(), 1);
  assert.deepEqual(report(owner), beforeReport);
  assert.deepEqual(Array.from(pcm(owner)), beforePcm);
  assert.equal(native(owner, 194n, 2), Number(frame + 2n));
  assert.equal(render(owner, frame + 2n, 2), 0);
  assert.deepEqual(Array.from(pcm(owner, 2)), [0.75, -0.25]);
}));

test("actual core fences an armed native forward gap and clears the entire output", () => withAudio(owner => {
  certify(owner, ORIGIN);
  assert.equal(owner.insert_sample(1n, 48000, 1, new Float32Array(64).fill(0.25)), 0);
  assert.equal(owner.finish(), 0);
  assert.equal(owner.enqueue(0, 1n, 1n, 0n, 1, 0n, 1n), 0);
  const frame = ORIGIN + 192n;
  assert.equal(native(owner, 192n, 8), Number(frame));
  assert.equal(arm(owner, frame, frame), 0);
  assert.equal(render(owner, frame, 8), 0);
  assert.ok(pcm(owner, 8).every(value => value === 0.25));
  assert.equal(native(owner, 224n, 8), Number(ORIGIN + 224n), "pure clock exposes actual gap without fabricating continuity");
  assert.equal(render(owner, ORIGIN + 224n, 8), 6);
  assert.equal(report(owner)[27], 1n);
  assert.ok(pcm(owner).every(value => value === 0));
  assert.equal(arm(owner, ORIGIN + 232n, ORIGIN + 232n), 5);
  assert.equal(render(owner, ORIGIN + 232n, 8), 5);
  assert.equal(owner.enqueue(0, 2n, 1n, 0n, 1, 0n, 1n), 5);
}));

test("scalar arm keeps setup, stale, duplicate and u64 endpoint-overflow statuses", () => {
  withAudio(owner => {
    assert.equal(arm(owner, 100n, 100n), 2, "arm requires actual finished Mixer");
    assert.equal(owner.finish(), 0);
    assert.equal(arm(owner, 99n, 100n), 6);
    assert.equal(report(owner)[25], 0n, "failed arm preserves unarmed state");
    assert.equal(arm(owner, 101n, 100n), 0);
    assert.equal(arm(owner, 102n, 100n), 2);
    assert.equal(report(owner)[26], 101n);
  });
  withAudio(owner => {
    assert.equal(owner.finish_at(2n), 0);
    const last = (1n << 64n) - 1n;
    assert.equal(arm(owner, last, last), 7);
    assert.equal(report(owner)[25], 0n);
    assert.equal(arm(owner, 100n, 100n), 0);
  });
});

test("old BigInt arm/render API remains available without native clock setup", () => withAudio(owner => {
  assert.equal(owner.finish(), 0);
  assert.equal(owner.arm(123n, 123n), 0);
  assert.equal(render(owner, 123n, 3), 0);
  const values = report(owner);
  assert.equal(values[24], 126n);
  assert.equal(values[26], 123n);
  assert.equal(values[27], 0n);
  assert.equal(owner.native_clock_ready(), false);
}));

test("steady actual native, extension, scalar arm and render preserve setup memory identity", () => withAudio(owner => {
  certify(owner, ORIGIN);
  assert.equal(owner.insert_sample(1n, 48000, 1, new Float32Array(4096).fill(0.125)), 0);
  assert.equal(owner.finish(), 0);
  assert.equal(owner.enqueue(0, 1n, 1n, 0n, 1, 0n, 1n), 0);
  const memory = BrowserAudio.memory(), buffer = memory.buffer;
  const pointer = owner.output_ptr(), length = owner.output_len();
  const view = new Float32Array(buffer, pointer, length);
  assert.equal(extend(owner, horizons(ORIGIN, 4n)), 0);
  assert.strictEqual(memory.buffer, buffer);
  const startFrame = ORIGIN + 192n;
  assert.equal(arm(owner, startFrame, startFrame), 0);
  for (let index = 0; index < 2048; index++) {
    const offset = 192n + BigInt(index) * 128n;
    assert.equal(native(owner, offset, 128), Number(ORIGIN + offset));
    assert.equal(render(owner, ORIGIN + offset, 128), 0);
    assert.strictEqual(memory.buffer, buffer, `no observed memory growth at steady block ${index}`);
    assert.equal(owner.output_ptr(), pointer);
    assert.equal(owner.output_len(), length);
    assert.equal(view.byteLength, length * 4);
  }
  const values = report(owner);
  assert.equal(values[24], startFrame + 2048n * 128n);
  assert.equal(values[26], startFrame);
  assert.equal(values[27], 0n);
  // This proves stable output storage / observed memory size, not absence of
  // every possible allocator call or performance on an actual audio device.
}));

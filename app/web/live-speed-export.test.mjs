// Requires fresh, separate production main/audio WASM packages. No JS game,
// producer, report or clock replaces either Rust owner in these fixtures.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, runInContext } from "node:vm";
import { pathToFileURL } from "node:url";
import { keyboardBindingWords, encodeKeyboardEvent, encodeTouchEvent, touchBindingWords } from "./physical-input.mjs";
import { snapshotLocalPlan, localBindingWords } from "./local-play-model.mjs";

async function packageInstance(environment, directory) {
  const base = process.env[environment]
    ? pathToFileURL(`${process.env[environment].replace(/\/$/, "")}/`)
    : new URL(directory, import.meta.url);
  const url = new URL("beatkernel_bms_runtime.js", base);
  const context = createContext({ WebAssembly, TextEncoder, TextDecoder, Uint8Array,
    Uint32Array, Float32Array, Float64Array, ArrayBuffer, DataView, URL, Request, Response, console });
  const module = new SourceTextModule(await readFile(url, "utf8"), { context,
    identifier: url.href, initializeImportMeta(meta) { meta.url = url.href; } });
  await module.link(specifier => { throw new Error(`unexpected production binding import ${specifier}`); });
  await module.evaluate();
  context.actualModule = new WebAssembly.Module(await readFile(new URL("beatkernel_bms_runtime_bg.wasm", base)));
  module.namespace.initSync(runInContext("({ module: actualModule })", context));
  return module.namespace;
}
const main = await packageInstance("BEATKERNEL_PREPARATION_PACKAGE", "./pkg/");
const audioBindings = await packageInstance("BEATKERNEL_AUDIO_PACKAGE", "./audio-pkg/");
const { BrowserLibrary, BrowserGame, BrowserLocalGame, BrowserReplay } = main;
const { BrowserAudio } = audioBindings;
assert.equal(typeof Object.getOwnPropertyDescriptor(BrowserAudio.prototype, "report_fields")?.get,
  "function", "fresh audio WASM must declare its own report format");
for (const name of ["new_and_speed", "new_section_and_speed", "new_physical_and_speed", "new_physical_contact_and_speed"])
  assert.equal(typeof BrowserGame[name], "function", `fresh solo export ${name} required`);
assert.equal(typeof BrowserLocalGame.new_physical_and_speed, "function", "fresh local export required");
for (const owner of [BrowserGame, BrowserLocalGame])
  assert.equal(typeof Object.getOwnPropertyDescriptor(owner.prototype, "pre_origin_inputs")?.get,
    "function", "fresh main WASM must export the actual audio-origin discard counter");

const encode = text => new TextEncoder().encode(text);
const CHART = "#BPM 120\n#RANK 2\n#WAV01 key.wav\n#00001:01\n#00011:01\n";
const LIMITS = [1024, 4096, 8];
const KEYS = Uint32Array.from([0x11, 4]);
const PHYSICAL = keyboardBindingWords(KEYS);
const PLAN = snapshotLocalPlan(Uint32Array.from([7, 0, 0, 0]));
const LOCAL = localBindingWords(PLAN, PHYSICAL, [0x11], false);
const POLICY = "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1";
const RATES = [
  { fraction: "1/2", n: 1n, d: 2n, end: 4800n, song: 10000000n, pcm: [0.25, 0.0625, -0.125, -0.125] },
  { fraction: "1/1", n: 1n, d: 1n, end: 3600n, song: 20000000n, pcm: [0.25, -0.125] },
  { fraction: "2/1", n: 2n, d: 1n, end: 3000n, song: 40000000n, pcm: [0.25] },
  { fraction: "3/2", n: 3n, d: 2n, end: 3200n, song: 30000000n, pcm: [0.25, -0.125] },
];
function wav(hz) {
  const bytes = new Uint8Array(48), view = new DataView(bytes.buffer);
  bytes.set(encode("RIFF")); view.setUint32(4, 40, true); bytes.set(encode("WAVEfmt "), 8);
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, hz, true); view.setUint32(28, hz * 2, true);
  view.setUint16(32, 2, true); view.setUint16(34, 16, true);
  bytes.set(encode("data"), 36); view.setUint32(40, 4, true);
  view.setInt16(44, 8192, true); view.setInt16(46, -4096, true); return bytes;
}
function library(hz = 24000, chart = CHART) {
  const owner = new BrowserLibrary(32, 1024 * 1024, 8 * 1024 * 1024, 256);
  const bytes = encode(chart), sound = wav(hz);
  owner.declare_file("pack/chart.bms", bytes.length); owner.add_file("pack/chart.bms", bytes);
  owner.declare_file("pack/key.wav", sound.length); owner.add_file("pack/key.wav", sound);
  return owner;
}
function ready(job, owner) {
  for (let i = 0; i < 512; i++) if (job.advance(owner)) {
    assert.equal(job.stage, "ready"); return job.take_prepared(owner);
  }
  assert.fail("actual preparation exceeded fixture bound");
}
function prepare(owner, hz = 24000, policy = false, start = 0n) {
  const job = policy
    ? owner.begin_prepare_chart_with_policy_at("pack/chart.bms", hz, 2, 0n, start,
      ...LIMITS, POLICY, "rank-first", "groove", 0n)
    : owner.begin_prepare_chart_at("pack/chart.bms", hz, 2, 0n, start, ...LIMITS);
  try { return ready(job, owner); } finally { job.free(); }
}
function solo(prepared, speed, { preroll = 100000000n, end = 50000000n, variant = "section" } = {}) {
  const args = [prepared, 0n, preroll, 50000000n, 50000000n, 0n];
  if (variant === "plain") return BrowserGame.new_and_speed(...args, KEYS, speed);
  if (variant === "physical" || variant === "contact")
    return BrowserGame[variant === "contact" ? "new_physical_contact_and_speed" : "new_physical_and_speed"](
      ...args, PHYSICAL, end, 4096, 1024, speed);
  return BrowserGame.new_section_and_speed(...args, KEYS, end, speed);
}
function local(prepared, speed, { preroll = 100000000n, end = 50000000n } = {}) {
  return BrowserLocalGame.new_physical_and_speed(prepared, 0n, preroll,
    50000000n, 50000000n, 0n, PLAN.words, LOCAL.words, end, false, 4096, 1024, speed);
}
function audioFor(game, hz = 24000) {
  const audio = new BrowserAudio(hz, 2, 1024 * 1024, 8 * 1024 * 1024, 8, 4096, 8, 4096, 128, 4096);
  try {
    assert.equal(audio.status(), 0);
    for (;;) {
      const sample = game.next_sample(); if (!sample) break;
      try { assert.equal(audio.insert_sample(sample.id, sample.rate, sample.channels, sample.take_pcm_array()), 0); }
      finally { sample.free(); }
    }
    assert.equal(audio.finish_at(game.playback_end_frame), 0); assert.equal(audio.arm(0n, 0n), 0);
    return audio;
  } catch (error) { audio.free(); throw error; }
}
function words(audio) {
  assert.equal(audio.report_fields, 29, "actual current audio WASM report declaration");
  const result = new Uint32Array(58);
  for (let i = 0; i < 29; i++) { result[2 * i] = audio.report_word(i, false); result[2 * i + 1] = audio.report_word(i, true); }
  return result;
}
function drain(game, audio, commands) {
  for (let i = 0; i < 128; i++) {
    const batch = game.commands(4096); if (!batch) return;
    assert.ok(batch.commands.length > 0);
    for (const command of batch.commands) {
      commands.push({ kind: command.kind, at: command.at, value: command.value, denominator: command.denominator });
      assert.equal(audio.enqueue(command.kind, command.voice, command.sample, command.at,
        command.gain, command.value, command.denominator), 0);
    }
    game.acknowledge(batch.sequence, batch.commands.length, true);
  }
  assert.fail("actual command admission exceeded fixture bound");
}
function render(audio, frame, frames) {
  assert.equal(audio.render(frame, 0, frames), 0);
  return Array.from(new Float32Array(BrowserAudio.memory().buffer, audio.output_ptr(), frames * 2));
}

test("actual current audio WASM appends terminal expiry without applying or reopening playback", () => {
  const audio = new BrowserAudio(1000, 1, 1024, 4096, 1, 4, 1, 4, 8, 4);
  try {
    assert.equal(audio.report_fields, 29);
    assert.equal(audio.status(), 0);
    assert.equal(audio.finish_at(1n), 0);
    assert.equal(audio.arm(100n, 100n), 0);
    assert.equal(audio.render(100, 0, 2), 0);
    const before = words(audio);
    assert.equal(before[12], 1, "actual endpoint marker");
    assert.equal(before[56], 0);
    assert.equal(audio.enqueue(2, 0n, 0n, 1000000n, 1, 3n, 2n), 0);
    assert.equal(audio.render(102, 0, 2), 0);
    const after = words(audio);
    assert.equal(after[26], 1, "consumed");
    assert.equal(after[28], 0, "never applied");
    assert.equal(after[56], 1, "appended actual expiry");
    assert.equal(after[57], 0);
    assert.deepEqual(Array.from(before.slice(6, 10)), [0, 0, 1, 0], "crossing block starts at zero and plays exactly one frame");
    assert.deepEqual(Array.from(after.slice(6, 10)), [1, 0, 0, 0], "terminal silent block starts at endpoint and plays no frames");
    assert.equal(before[6] + before[8], 1, "crossing playback cursor is endpoint");
    assert.equal(after[6] + after[8], 1, "silent playback cursor stays at endpoint");
    for (const field of [5, 6, 7, 8, 10, 14, 15, 16, 17, 18, 19, 20, 21, 22])
      assert.deepEqual(after.slice(field * 2, field * 2 + 2), before.slice(field * 2, field * 2 + 2), `frozen field ${field}`);
    assert.deepEqual(Array.from(new Float32Array(BrowserAudio.memory().buffer, audio.output_ptr(), 2)), [0, 0]);
  } finally { audio.free(); }
});
function present(game, audio, ns) {
  game.admit_output(words(audio), ns);
  game.observe_presentation(ns, ns); game.close_input_prefix(ns);
  return game.service_audio(ns, ns);
}
function capture(game, isLocal) { if (isLocal) game.configure_capture(7, 1024 * 1024, 1024); else game.configure_capture(1024 * 1024, 1024); }

test("real WASM consumes original failed-browser observation pair without BeforeOrigin or discarded capture", () => {
  for (const count of [1, 2, 4]) {
    const owner = library();
    let game;
    if (count === 1) game = solo(prepare(owner), "3/2", { variant: "physical" });
    else {
      const plan = snapshotLocalPlan(Uint32Array.from(Array.from({ length: count }, (_, i) => [7 + i, 1, i + 1, 0]).flat()));
      const memberPhysical = Uint32Array.from(Array.from({ length: count }, (_, i) =>
        [0x11, 1, i + 1, 0, 1, 0x574b4559, 4]).flat());
      const bindings = localBindingWords(plan, memberPhysical, [0x11], false);
      game = BrowserLocalGame.new_physical_and_speed(prepare(owner), 0n, 100000000n,
        50000000n, 50000000n, 0n, plan.words, bindings.words, 50000000n, false, 4096, 1024, "3/2");
    }
    try {
      capture(game, count !== 1);
      game.activate(1717990000n);
      game.observe_presentation(9816157n, 1760030000n);
      game.observe_presentation(22571157n, 1772805000n);
      game.queue_input_blob(encodeKeyboardEvent({ hostNs: 1719645000n, key: 4, down: true, sequence: 21n }), 1772805000n);
      game.queue_input_blob(encodeKeyboardEvent({ hostNs: 1772805000n, key: 4, down: true, sequence: 25n }), 1772805000n);
      game.close_input_prefix(1772805000n);
      assert.equal(game.service_audio(1772805000n, 30000000n), 1, "negative original input is consumed without a Runtime report");
      assert.equal(game.pending_inputs(), 0);
      assert.equal(game.pre_origin_inputs, 1n);
      assert.equal(typeof game.pre_origin_inputs, "bigint");
      assert.equal(game.service_audio(1772805000n, 30000000n), 0);
      assert.equal(game.pre_origin_inputs, 1n, "repeated service cannot count the same input again");
      game.stop();
      const bytes = count === 1 ? game.take_replay() : game.take_replay(7);
      assert.ok(bytes);
      const saved = recorded(bytes).records.filter(row => row.kind === 0);
      assert.equal(saved.length, 1, "negative Down never mutates held state or enters replay");
      assert.equal(saved[0].song, -116143264n, "original output 22571157 minus preroll is multiplied once with signed truncation");
      const view = new DataView(saved[0].physical.buffer, saved[0].physical.byteOffset, saved[0].physical.byteLength);
      // Physical codec's original-clock fields are preserved by the Rust capture.
      assert.equal(view.getBigUint64(27, true), 25n);
      assert.equal(view.getUint16(4, true), 1);
      assert.equal(view.getUint8(35), 1, "encoded keyboard retains native metadata");
      assert.equal(view.getUint32(36, true), 0x574b4559);
      assert.equal(view.getUint8(58), 1, "original HOST point follows the complete native metadata");
      assert.equal(view.getUint32(59, true), 0x57494e);
      assert.equal(view.getBigInt64(63, true), 1772805000n);
      assert.equal(view.getBigInt64(50, true), 1772805000n, "native acquisition time remains raw HOST");
    } finally { game.free(); owner.free(); }
  }
});

test("real contact owner discards negative touch before contact ownership and never lends its projection to accepted input", () => {
  const owner = library(24000, "#BPM 60000\n#WAV01 key.wav\n#00011:00010000\n");
  const touchWords = touchBindingWords([0x11]);
  const physical = Uint32Array.from([...PHYSICAL, ...touchWords]);
  const game = BrowserGame.new_physical_contact_and_speed(prepare(owner), 0n, 100000000n,
    50000000n, 50000000n, 0n, physical, 50000000n, 4096, 1024, "1/1");
  const event = (hostNs, sequence, x) => ({ kind: "touch", hostNs, sequence,
    contact: 7n, phase: 0, code: 0, x, y: 50, pressure: null, width: 100, height: 100 });
  try {
    game.configure_touch_regions(touchWords, Float32Array.from([0, 0, 480, 720]), 8);
    capture(game, false); game.activate(0n);
    game.observe_presentation(0n, 10000000n);
    game.observe_presentation(104000000n, 114000000n);
    // The first sample would acquire the lane at projected x=240. The next
    // genuine Down uses the same physical contact at x=720 outside that lane.
    game.queue_input_blob_on_surface(encodeTouchEvent(event(9000000n, 1n, 25)), 100, 100, 960, 720, 114000000n);
    game.queue_input_blob_on_surface(encodeTouchEvent(event(111000000n, 2n, 75)), 100, 100, 960, 720, 114000000n);
    game.close_input_prefix(111000000n);
    assert.equal(game.service_audio(114000000n, 105000000n), 1);
    assert.equal(game.pre_origin_inputs, 1n);
    assert.equal(game.pending_inputs(), 0);
    assert.equal(game.hits, 0n, "accepted outside touch cannot borrow discarded inside coordinates");
    assert.equal(gauge(game).pressed, 0, "discarded contact cannot acquire or retain a lane");
    game.queue_input_blob(encodeKeyboardEvent({ hostNs: 112000000n, key: 4, down: true, sequence: 3n }), 114000000n);
    game.close_input_prefix(114000000n);
    assert.equal(game.service_audio(114000000n, 106000000n), 1);
    assert.equal(game.hits, 1n, "genuine keyboard Down remains independent of discarded touch state");
    game.stop();
    const saved = recorded(game.take_replay()).records.filter(row => row.kind === 0);
    assert.ok(saved.every(row => new DataView(row.physical.buffer, row.physical.byteOffset,
      row.physical.byteLength).getBigUint64(27, true) !== 1n));
    const keyboard = saved.find(row => new DataView(row.physical.buffer, row.physical.byteOffset,
      row.physical.byteLength).getBigUint64(27, true) === 3n);
    assert.ok(keyboard);
    assert.equal(keyboard.song, 2000000n);
    const view = new DataView(keyboard.physical.buffer, keyboard.physical.byteOffset, keyboard.physical.length);
    assert.equal(view.getUint16(4, true), 1);
    assert.equal(view.getUint8(35), 1);
    assert.equal(view.getUint8(58), 1);
    assert.equal(view.getUint32(59, true), 0x57494e);
    assert.equal(view.getBigInt64(63, true), 112000000n, "accepted original HOST provenance remains exact");
    assert.equal(view.getBigInt64(50, true), 112000000n, "native acquisition timestamp remains exact too");
  } finally { game.free(); owner.free(); }
});
function takeReplay(game, isLocal) { return isLocal ? game.take_replay(7) : game.take_replay(); }
function score(game, isLocal) { return isLocal ? [game.hits(7), game.misses(7), game.combo(7), game.max_combo(7)] : [game.hits, game.misses, game.combo, game.max_combo]; }

// Independent inspection of actual BKREPLAY v1 bytes, not a replacement replay
// producer. Literal field order comes from the durable codec's public format.
function recorded(bytes) {
  assert.ok(bytes instanceof Uint8Array); const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  const take = count => { assert.ok(at + count <= bytes.length); const out = bytes.subarray(at, at + count); at += count; return out; };
  const u8 = () => take(1)[0];
  const u32 = () => { const p = at; take(4); return view.getUint32(p, true); };
  const u64 = () => { const p = at; take(8); return view.getBigUint64(p, true); };
  const i64 = () => { const p = at; take(8); return view.getBigInt64(p, true); };
  const blob = () => { const length = u64(); assert.ok(length <= BigInt(bytes.length)); return take(Number(length)); };
  assert.equal(new TextDecoder().decode(take(8)), "BKREPLAY"); assert.equal(u32(), 1);
  u32(); const domain = u32(); u64(); blob(); blob(); blob(); const options = blob();
  const calibration = u8(); assert.ok(calibration <= 1); if (calibration) blob();
  const count = u64(); assert.ok(count <= 1024n); const records = [];
  for (let i = 0n; i < count; i++) {
    assert.equal(u64(), i); const song = i64(), kind = u8(); assert.ok(kind <= 1);
    if (kind === 0) { const control = u32(); records.push({ song, kind, control, physical: blob() }); }
    else records.push({ song, kind });
  }
  assert.equal(at, bytes.length); return { domain, options, records };
}
function sourceSpeed(options, n, d) {
  const prefix = encode("bms-speed-setup/v1:");
  if (n === 1n && d === 1n) {
    assert.notDeepEqual(Array.from(options.subarray(0, prefix.length)), Array.from(prefix)); return;
  }
  assert.deepEqual(Array.from(options.subarray(0, prefix.length)), Array.from(prefix));
  const view = new DataView(options.buffer, options.byteOffset, options.byteLength);
  assert.equal(view.getUint32(prefix.length, true) + prefix.length + 20, options.length);
  assert.equal(view.getBigInt64(prefix.length + 4, true), n);
  assert.equal(view.getBigUint64(prefix.length + 12, true), d);
}
function gauge(game) {
  game.visual_registration(1n, 1n, 1024 * 1024, 4096);
  const packet = game.visual_frame(1n, 0), v = new DataView(packet.buffer, packet.byteOffset, packet.byteLength);
  assert.equal(game.acknowledge_visual(1n, 1n, 1n), true);
  const pressed = v.getUint32(40 + 4 + 8 + 1 + 4 + 4 + 8, true);
  let at = 40 + 4 + 8 + 1; assert.equal(v.getUint32(at, true), 1); at += 4 + 4 + 8 + 4 + 4 + 3;
  assert.equal(v.getUint8(at++), 1); at += 32 + 64;
  for (let i = 0; i < 3; i++) { const tag = v.getUint8(at++); assert.ok(tag <= 1); if (tag) at += 8; }
  assert.equal(v.getUint8(at++), 1);
  return { pressed, level: v.getBigUint64(at, true), failure: v.getUint8(at + 8), clear: v.getBigUint64(at + 9, true) };
}

for (const isLocal of [false, true]) for (const fixture of RATES) {
  test(`actual ${isLocal ? "local" : "solo"} ${fixture.fraction} executes rate, PCM, original capture and finite authority`, () => {
    const owner = library(); const game = (isLocal ? local : solo)(prepare(owner), fixture.fraction, { variant: "physical" });
    let audio;
    try {
      assert.equal(game.speed_numerator, fixture.n); assert.equal(game.speed_denominator, fixture.d);
      assert.equal(game.playback_end_frame, fixture.end); assert.equal(game.end_ns, 50000000n);
      capture(game, isLocal); game.activate(0n); audio = audioFor(game);
      game.feed_audio(0n, 256); const commands = [], pcm = []; drain(game, audio, commands);
      game.observe_presentation(0n, 0n);
      let frame = 0;
      while (frame < 2424) { const n = Math.min(128, 2424 - frame); pcm.push(...render(audio, frame, n)); frame += n; }
      game.admit_output(words(audio), 101000000n); game.observe_presentation(101000000n, 101000000n);
      game.queue_input_blob(encodeKeyboardEvent({ hostNs: 100000000n, key: 4, down: true, sequence: 1n }), 101000000n);
      game.queue_input_blob(encodeKeyboardEvent({ hostNs: 100500000n, key: 4, down: false, sequence: 2n }), 101000000n);
      game.close_input_prefix(101000000n); assert.equal(game.service_audio(101000000n, 101000000n), 2);
      assert.deepEqual(score(game, isLocal), [1n, 0n, 1n, 1n]);
      const actualGauge = gauge(game);
      assert.equal(actualGauge.pressed, 0, "Up releases an actually held Down");
      assert.deepEqual(actualGauge, { pressed: 0, level: 21000000n, failure: 0, clear: 80000000n });
      drain(game, audio, commands);
      assert.ok(commands.some(c => c.kind === 0 && c.at === 101000000n), "keysound remains raw output time");
      let complete = false;
      while (frame < Number(fixture.end) + 128) {
        const target = frame < 2880 ? 2880 : Number(fixture.end) + 128;
        const n = Math.min(128, target - frame); pcm.push(...render(audio, frame, n)); frame += n;
        if (frame === 2880) {
          const before = game.song_ns; game.admit_output(words(audio), undefined);
          assert.equal(game.song_ns, before, "render-ahead alone cannot move the original song");
          present(game, audio, 120000000n); assert.equal(game.song_ns, fixture.song);
        } else present(game, audio, (BigInt(frame) * 1000000000n + 23999n) / 24000n);
        drain(game, audio, commands); complete = game.evaluate_completion(); if (complete) break;
      }
      assert.equal(complete, true); assert.equal(game.song_ns, 50000000n); assert.equal(game.failed, false);
      assert.deepEqual(score(game, isLocal), [1n, 0n, 1n, 1n]);
      assert.ok(pcm.slice(0, 4800).every(x => x === 0));
      assert.deepEqual(pcm.slice(4800, 4800 + fixture.pcm.length * 2), fixture.pcm.flatMap(x => [x, x]));
      const rates = commands.filter(c => c.kind === 2);
      assert.deepEqual(rates, fixture.n === fixture.d ? [] : [{ kind: 2, at: 0n, value: fixture.n, denominator: fixture.d }]);
      assert.equal(audio.report_word(13, false), commands.length, "real consumed command count");
      assert.equal(audio.report_word(14, false), commands.length, "real applied command count");
      game.stop();
      const bytes = takeReplay(game, isLocal); const decoded = recorded(bytes); sourceSpeed(decoded.options, fixture.n, fixture.d);
      const inputs = decoded.records.filter(r => r.kind === 0); assert.equal(inputs.length, 2);
      assert.equal(inputs[0].song, 0n); assert.equal(inputs[1].song, fixture.n * 500000n / fixture.d);
      for (const [i, r] of inputs.entries()) {
        const view = new DataView(r.physical.buffer, r.physical.byteOffset, r.physical.length);
        assert.equal(view.getBigUint64(27, true), BigInt(i + 1));
        assert.equal(view.getUint32(23, true), decoded.domain);
        assert.equal(view.getUint8(58), 1, "original HOST point survives normalization");
        assert.equal(view.getUint32(59, true), 0x57494e);
        assert.equal(view.getBigInt64(63, true), i === 0 ? 100000000n : 100500000n);
      }
      assert.equal(decoded.records.at(-1).song, 50000000n);
      assert.equal(takeReplay(game, isLocal), undefined, "capture is consumed once");
      const originalBytes = Array.from(bytes);
      const job = owner.begin_prepare_replay_chart("pack/chart.bms", bytes, 24000, 2, ...LIMITS);
      let replay;
      try { replay = BrowserReplay.new_with_speed(ready(job, owner), 0n, "2/1"); }
      finally { job.free(); }
      let replayAudio;
      try {
        assert.equal(replay.speed_numerator, 2n); assert.equal(replay.speed_denominator, 1n);
        assert.equal(replay.playback_end_frame, 600n, "output rate is independent of recorded source rate");
        replayAudio = audioFor(replay); const replayCommands = []; drain(replay, replayAudio, replayCommands);
        let done = false;
        for (let f = 0; f < 728 && !done;) {
          const size = Math.min(128, 728 - f); render(replayAudio, f, size); f += size;
          done = replay.observe_output(words(replayAudio), (BigInt(f) * 1000000000n + 23999n) / 24000n);
          if (!done) drain(replay, replayAudio, replayCommands);
        }
        assert.equal(done, true); assert.equal(replay.song_ns, 50000000n);
        assert.deepEqual([replay.hits, replay.misses, replay.combo, replay.max_combo], [1n, 0n, 1n, 1n],
          "reconstruction does not multiply original captured operations by source speed");
        assert.deepEqual(Array.from(bytes), originalBytes);
      } finally { replayAudio?.free(); replay.free(); }
    } finally { audio?.free(); game.free(); owner.free(); }
  });
}

test("all real policy/physical constructor variants normalize the selected identity and preserve defaults", () => {
  const owner = library();
  try {
    for (const policy of [false, true]) for (const variant of ["plain", "section", "physical", "contact", "local"]) {
      const game = variant === "local" ? local(prepare(owner, 24000, policy), "0006/0004")
        : solo(prepare(owner, 24000, policy), "0006/0004", { variant });
      try {
        assert.equal(game.speed_numerator, 3n); assert.equal(game.speed_denominator, 2n);
        capture(game, variant === "local"); game.stop(); sourceSpeed(recorded(takeReplay(game, variant === "local")).options, 3n, 2n);
      } finally { game.free(); }
    }
    const old = new BrowserGame(prepare(owner), 0n, 100000000n, 50000000n, 50000000n, 0n, KEYS);
    const selected = solo(prepare(owner), "0002/0002", { variant: "plain" });
    try {
      for (const game of [old, selected]) { assert.equal(game.speed_numerator, 1n); assert.equal(game.speed_denominator, 1n); capture(game, false); game.stop(); }
      assert.deepEqual(Array.from(old.take_replay()), Array.from(selected.take_replay()), "NORMAL exact old record bytes");
    } finally { old.free(); selected.free(); }
    const legacyLocal = BrowserLocalGame.new_physical(prepare(owner), 0n, 100000000n,
      50000000n, 50000000n, 0n, PLAN.words, LOCAL.words, 50000000n, false, 4096, 1024);
    try { assert.equal(legacyLocal.speed_numerator, 1n); assert.equal(legacyLocal.speed_denominator, 1n); assert.equal(legacyLocal.playback_end_frame, 3600n); }
    finally { legacyLocal.free(); }
  } finally { owner.free(); }
});

test("actual 1kHz finite endpoints are literal 11/7/5/6 frames, with exact maximal BigInt components", () => {
  const owner = library(1000);
  try {
    for (const [fraction, endpoint] of [["1/2", 11n], ["1/1", 7n], ["2/1", 5n], ["3/2", 6n]])
      for (const create of [solo, local]) {
        const game = create(prepare(owner, 1000), fraction, { preroll: 3000000n, end: 4000000n });
        try { assert.equal(game.playback_end_frame, endpoint); } finally { game.free(); }
      }
    const maximum = solo(prepare(owner, 1000), "9223372036854775807/18446744073709551615", { preroll: 0n, end: 4000000n });
    try {
      assert.equal(maximum.speed_numerator, 9223372036854775807n); assert.equal(maximum.speed_denominator, 18446744073709551615n);
      capture(maximum, false); maximum.stop();
      sourceSpeed(recorded(maximum.take_replay()).options, 9223372036854775807n, 18446744073709551615n);
    }
    finally { maximum.free(); }
  } finally { owner.free(); }
});

test("genuine zero-output and one-frame quantized completion need presentation and close the original frontier", () => {
  // The accepted inverse scheduler truncates source-span/rate to integer ns
  // before physical frame ceil. 2,000,001ns / 2 becomes 1,000,000 output ns:
  // one real 1kHz frame maps to original 2,000,000ns, leaving a 1ns frontier.
  for (const [fraction, endpoint, originalEnd] of [["9223372036854775807/1", 0n, 1n], ["2/1", 1n, 2000001n]]) {
    const owner = library(1000), game = solo(prepare(owner, 1000), fraction, { preroll: 0n, end: originalEnd }); let audio;
    try {
      assert.equal(game.playback_end_frame, endpoint); game.activate(0n); audio = audioFor(game, 1000);
      game.observe_presentation(0n, 0n);
      const commands = []; game.feed_audio(0n, 256); drain(game, audio, commands);
      render(audio, 0, 1); game.admit_output(words(audio), undefined); game.close_input_prefix(0n);
      assert.equal(game.evaluate_completion(), false, "actual render marker without presentation is insufficient");
      present(game, audio, 1000000n); drain(game, audio, commands);
      if (endpoint === 1n) assert.equal(game.song_ns, 2000000n, "integer mapping remains one original ns short");
      assert.equal(game.evaluate_completion(), true); assert.equal(game.song_ns, originalEnd);
      if (endpoint === 0n) { assert.equal(commands.length, 0); assert.equal(audio.report_word(13, false), 0); }
    } finally { audio?.free(); game.free(); owner.free(); }
  }
});

test("original key-pair constructors judge actual typed Down/Up and capture original operations at all four rates", () => {
  const owner = library();
  try {
    for (const fixture of RATES) {
      const game = solo(prepare(owner), fixture.fraction); let audio;
      try {
        capture(game, false); game.activate(0n); audio = audioFor(game);
        game.feed_audio(0n, 256); const commands = []; drain(game, audio, commands);
        game.observe_presentation(0n, 0n);
        for (let frame = 0; frame < 2424;) {
          const size = Math.min(128, 2424 - frame); render(audio, frame, size); frame += size;
        }
        game.admit_output(words(audio), 101000000n);
        game.observe_presentation(101000000n, 101000000n);
        // These legacy pair bindings use USB keyboard controls. Native Window
        // blobs belong to new_physical_and_speed, covered independently above.
        game.queue_input(100000000n, 4, true, 1n, 101000000n);
        game.queue_input(100500000n, 4, false, 2n, 101000000n);
        game.close_input_prefix(101000000n);
        assert.equal(game.service_audio(101000000n, 101000000n), 2);
        assert.deepEqual(score(game, false), [1n, 0n, 1n, 1n]);
        assert.deepEqual(gauge(game), { pressed: 0, level: 21000000n, failure: 0, clear: 80000000n });
        drain(game, audio, commands);
        assert.ok(commands.some(c => c.kind === 0 && c.at === 101000000n));
        render(audio, 2424, 24);
        assert.equal(audio.report_word(13, false), commands.length);
        assert.equal(audio.report_word(14, false), commands.length);
        game.stop(); const decoded = recorded(game.take_replay());
        sourceSpeed(decoded.options, fixture.n, fixture.d);
        const inputs = decoded.records.filter(r => r.kind === 0); assert.equal(inputs.length, 2);
        assert.deepEqual(inputs.map(r => r.song), [0n, fixture.n * 500000n / fixture.d]);
        for (const [index, input] of inputs.entries()) {
          const view = new DataView(input.physical.buffer, input.physical.byteOffset, input.physical.byteLength);
          assert.equal(view.getBigUint64(27, true), BigInt(index + 1));
          assert.equal(view.getUint32(23, true), decoded.domain);
          assert.equal(view.getUint8(35), 0, "typed key API does not invent native metadata");
          assert.equal(view.getUint8(36), 1, "original HOST provenance is retained");
          assert.equal(view.getUint32(37, true), 0x57494e);
          assert.equal(view.getBigInt64(41, true), index === 0 ? 100000000n : 100500000n);
          assert.equal(input.physical.at(-1), index, "actual Down and Up states survive capture");
        }
      } finally { audio?.free(); game.free(); }
    }
  } finally { owner.free(); }
});

test("malformed fractions consume failed preparation once and fresh solo/local retry remains admissible", () => {
  const owner = library();
  try {
    for (const fraction of ["", "0/1", "1/0", "-1/1", "+1/1", "1", "1/2/3", "1/2 ", " 1/2", "１/2", "1/٢", "1.0/2", "9223372036854775808/1", "1/18446744073709551616"])
      for (const create of [solo, local]) {
        const prepared = prepare(owner); assert.throws(() => create(prepared, fraction), undefined, fraction);
        assert.throws(() => create(prepared, "1/1"), "failed consuming constructor cannot reuse ownership");
        const retry = create(prepare(owner), "6/4"); try { assert.equal(retry.speed_numerator, 3n); } finally { retry.free(); }
      }
  } finally { owner.free(); }
});

test("cancelled preparation and Stop preserve genuine resources for fresh selected-speed retries", () => {
  const owner = library();
  try {
    const job = owner.begin_prepare_chart("pack/chart.bms", 24000, 2, 0n, ...LIMITS);
    try { job.cancel(); assert.throws(() => job.take_prepared(owner)); assert.throws(() => job.advance(owner)); }
    finally { job.free(); }
    const stopped = solo(prepare(owner), "3/2");
    try { stopped.activate(0n); stopped.stop(); assert.equal(stopped.failed, true); assert.throws(() => stopped.queue_input(0n, 4, true, 1n, 0n)); }
    finally { stopped.free(); }
    const retry = local(prepare(owner), "1/2"); try { assert.equal(retry.sample_count(), 1); assert.equal(retry.speed_denominator, 2n); } finally { retry.free(); }
  } finally { owner.free(); }
});

test("live selected-speed constructors refuse actual replay preparation and permit fresh live retry", () => {
  const owner = library();
  try {
    const source = solo(prepare(owner), "3/2"); let bytes;
    try { capture(source, false); source.stop(); bytes = source.take_replay(); } finally { source.free(); }
    for (const create of [solo, local]) {
      const job = owner.begin_prepare_replay_chart("pack/chart.bms", bytes, 24000, 2, ...LIMITS);
      let prepared; try { prepared = ready(job, owner); } finally { job.free(); }
      assert.throws(() => create(prepared, "3/2"), /live|replay/i);
      assert.throws(() => create(prepared, "1/1"), "rejected replay preparation is consumed once");
      const retry = create(prepare(owner), "3/2"); try { assert.equal(retry.speed_numerator, 3n); } finally { retry.free(); }
    }
  } finally { owner.free(); }
});

test("an original chart without BGM still admits the selected origin rate before input sounds", () => {
  const owner = library(24000, "#BPM 120\n#RANK 2\n#WAV01 key.wav\n#00011:01\n");
  try {
    for (const create of [solo, local]) {
      const game = create(prepare(owner), "3/2"); let audio;
      try {
        game.activate(0n); audio = audioFor(game); game.feed_audio(0n, 256);
        const commands = []; drain(game, audio, commands);
        assert.deepEqual(commands, [{ kind: 2, at: 0n, value: 3n, denominator: 2n }]);
        render(audio, 0, 1);
        assert.equal(audio.report_word(13, false), 1); assert.equal(audio.report_word(14, false), 1);
      } finally { audio?.free(); game.free(); }
    }
  } finally { owner.free(); }
});

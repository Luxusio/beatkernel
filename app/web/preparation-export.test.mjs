// Actual generated WASM bindings only. Run after the coordinator regenerates
// the production package: node --experimental-vm-modules --test <this file>.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, runInContext } from "node:vm";
import { pathToFileURL } from "node:url";
import { keyboardBindingWords } from "./physical-input.mjs";
import { localBindingWords, snapshotLocalPlan } from "./local-play-model.mjs";

const packageUrl = process.env.BEATKERNEL_PREPARATION_PACKAGE
  ? pathToFileURL(`${process.env.BEATKERNEL_PREPARATION_PACKAGE.replace(/\/$/, "")}/`)
  : new URL("./pkg/", import.meta.url);
const bindingUrl = new URL("beatkernel_bms_runtime.js", packageUrl);
const context = createContext({ WebAssembly, TextEncoder, TextDecoder, Uint8Array, Uint32Array,
  Float32Array, Float64Array, ArrayBuffer, DataView, URL, Request, Response, console });
const binding = new SourceTextModule(await readFile(bindingUrl, "utf8"), { context,
  identifier: bindingUrl.href, initializeImportMeta(meta) { meta.url = bindingUrl.href; } });
await binding.link(specifier => { throw new Error(`unexpected preparation binding import ${specifier}`); });
await binding.evaluate();
context.preparationModule = new WebAssembly.Module(await readFile(new URL("beatkernel_bms_runtime_bg.wasm", packageUrl)));
const wasm = binding.namespace.initSync(runInContext("({ module: preparationModule })", context));
// Match the production Worklet split: audio has its own actual WASM instance,
// rather than enabling browser-audio in the main Worker package.
const audioPackageUrl = process.env.BEATKERNEL_AUDIO_PACKAGE
  ? pathToFileURL(`${process.env.BEATKERNEL_AUDIO_PACKAGE.replace(/\/$/, "")}/`)
  : new URL("./audio-pkg/", import.meta.url);
const audioBindingUrl = new URL("beatkernel_bms_runtime.js", audioPackageUrl);
const audioContext = createContext({ WebAssembly, TextEncoder, TextDecoder, Uint8Array, Uint32Array,
  Float32Array, Float64Array, ArrayBuffer, DataView, URL, Request, Response, console });
const audioBinding = new SourceTextModule(await readFile(audioBindingUrl, "utf8"), { context: audioContext,
  identifier: audioBindingUrl.href, initializeImportMeta(meta) { meta.url = audioBindingUrl.href; } });
await audioBinding.link(specifier => { throw new Error(`unexpected audio binding import ${specifier}`); });
await audioBinding.evaluate();
audioContext.audioModule = new WebAssembly.Module(await readFile(new URL("beatkernel_bms_runtime_bg.wasm", audioPackageUrl)));
audioBinding.namespace.initSync(runInContext("({ module: audioModule })", audioContext));
const audioNamespace = audioBinding.namespace;
const { BrowserLibrary, BrowserGame, BrowserLocalGame, BrowserReplay } = binding.namespace;
assert.equal(typeof BrowserLibrary, "function", "generated current BrowserLibrary is required");
assert.equal(typeof BrowserLibrary.prototype.begin_prepare_chart, "function", "do not silently run an old generated package");
assert.equal(typeof binding.namespace.BrowserSample.prototype.take_pcm_array, "function");

const CHART = "#BPM 120\n#RANK 2\n#WAV01 key.wav\n#WAV02 key.wav\n#00011:0102\n";
const PCM = [0.25, 0.25, -0.125, -0.125];
const textBytes = text => new TextEncoder().encode(text);
const limits = [1024, 4096, 8];
function wav() {
  const bytes = new Uint8Array(48), view = new DataView(bytes.buffer);
  bytes.set(textBytes("RIFF")); view.setUint32(4, 40, true); bytes.set(textBytes("WAVEfmt "), 8);
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, 24000, true); view.setUint32(28, 48000, true);
  view.setUint16(32, 2, true); view.setUint16(34, 16, true); bytes.set(textBytes("data"), 36);
  view.setUint32(40, 4, true); view.setInt16(44, 8192, true); view.setInt16(46, -4096, true);
  return bytes;
}
function library(chart = CHART, hydrated = true) {
  const owner = new BrowserLibrary(32, 1024 * 1024, 8 * 1024 * 1024, 256);
  const bytes = textBytes(chart);
  owner.declare_file("pack/chart.bms", bytes.byteLength); owner.add_file("pack/chart.bms", bytes);
  owner.declare_file("pack/key.wav", wav().byteLength);
  if (hydrated) owner.add_file("pack/key.wav", wav());
  return owner;
}
function originalJob(owner, seed = 0n) { return owner.begin_prepare_chart("pack/chart.bms", 24000, 2, seed, ...limits); }
function ready(job, owner) {
  const stages = [];
  for (let index = 0; index < 512; index++) {
    assert.equal(typeof job.stage, "string"); stages.push(job.stage);
    assert.ok(job.completed_assets <= job.total_assets);
    if (job.advance(owner)) {
      assert.equal(job.stage, "ready");
      return { stages, parses: job.parse_count, compiles: job.compile_count, reads: job.read_count,
        decodes: job.decode_count, shared: job.shared_asset_count };
    }
  }
  assert.fail("bounded actual preparation did not reach Ready");
}
function prepare(owner, seed = 0n) {
  const job = originalJob(owner, seed);
  try { const metrics = ready(job, owner); return { prepared: job.take_prepared(owner), metrics }; }
  finally { job.free(); }
}
function game(prepared) {
  return new BrowserGame(prepared, 0n, 100000000n, 50000000n, 50000000n, 0n, Uint32Array.from([0x11, 4]));
}
function takeSample(game) {
  const sample = game.next_sample(); assert.ok(sample, "actual prepared PCM sample required"); return sample;
}

test("direct PCM export owns a transferable JS buffer and shares the old take latch", () => {
  const owner = library();
  try {
    const cold = prepare(owner);
    assert.equal(cold.metrics.decodes, 1, "one codec invocation serves both canonical aliases");
    const consumer = game(cold.prepared);
    try {
      const first = takeSample(consumer);
      try {
        assert.equal(first.rate, 24000); assert.equal(first.channels, 2);
        const pcm = first.take_pcm_array();
        assert.ok(pcm instanceof Float32Array);
        assert.deepEqual(Array.from(pcm), PCM);
        assert.notEqual(pcm.buffer, wasm.memory.buffer, "JS export must never expose a WASM memory view");
        assert.throws(() => first.take_pcm()); assert.throws(() => first.take_pcm_array());
        pcm[0] = 0.75;
        const transferred = structuredClone(pcm, { transfer: [pcm.buffer] });
        assert.equal(pcm.buffer.byteLength, 0);
        assert.equal(transferred[0], 0.75);
      } finally { first.free(); }
      const second = takeSample(consumer);
      try {
        const legacy = second.take_pcm();
        assert.deepEqual(Array.from(legacy), PCM, "independent JS mutation cannot alter an alias's Rust PCM");
        assert.throws(() => second.take_pcm_array()); assert.throws(() => second.take_pcm());
      } finally { second.free(); }
    } finally { consumer.free(); }
    const warm = prepare(owner);
    assert.equal(warm.metrics.parses, 0); assert.equal(warm.metrics.compiles, 0); assert.equal(warm.metrics.decodes, 0);
    assert.ok(warm.metrics.shared >= 2);
    const next = game(warm.prepared);
    try { const sample = takeSample(next); try { assert.deepEqual(Array.from(sample.take_pcm_array()), PCM); } finally { sample.free(); } }
    finally { next.free(); }
  } finally { owner.free(); }
});

// These fixtures use the production WASM Mixer adapter, including its actual
// command counters and finite marker. Presentation is an explicitly observed
// position in that rendered output; no JS replay or report owner replaces it.
const REPLAY_SPEED_CHART = "#BPM 120\n#RANK 2\n#WAV01 key.wav\n#00001:01\n#00011:01\n";
function speedRecording(owner, end = 50000000n, soundPrefix = false) {
  const source = BrowserGame.new_section(prepare(owner).prepared, 0n, soundPrefix ? 0n : 100000000n,
    50000000n, 50000000n, 0n, Uint32Array.from([0x11, 4]), end);
  try {
    source.configure_capture(1024 * 1024, 1024);
    if (soundPrefix) {
      // The canonical planner intentionally omits every BGM command from an
      // empty stopped log. Capture a real early Advance instead: it includes
      // the zero-time BGM while remaining well before the note's miss window.
      source.activate(0n);
      const audio = speedAudio(source);
      try {
        source.feed_audio(0n, 256);
        admitSpeedCommands(source, audio, []);
        renderSpeedBlock(audio, 0, 1);
        source.admit_output(actualAudioWords(audio), 1000n);
        source.observe_presentation(0n, 0n);
        source.observe_presentation(1000n, 1000n);
        source.close_input_prefix(1000n);
        assert.equal(source.service_audio(1000n, 1000n), 0);
        assert.equal(source.song_ns, 1000n);
        assert.deepEqual([source.hits, source.misses, source.combo, source.max_combo], [0n, 0n, 0n, 0n]);
      } finally { audio.free(); }
    }
    source.stop();
    const recording = source.take_replay();
    assert.ok(recording instanceof Uint8Array);
    assert.ok(recording.byteLength > 0);
    assert.equal(source.take_replay(), undefined);
    return recording;
  } finally { source.free(); }
}
function speedPrepared(owner, recording) {
  const job = owner.begin_prepare_replay_chart("pack/chart.bms", recording, 24000, 2, ...limits);
  try { ready(job, owner); return job.take_prepared(owner); }
  finally { job.free(); }
}
function speedAudio(replay, maxFrames = 128) {
  const { BrowserAudio } = audioNamespace;
  assert.equal(typeof BrowserAudio, "function", "production audio binding must be present");
  const audio = new BrowserAudio(24000, 2, 1024 * 1024, 8 * 1024 * 1024,
    8, 4096, 8, 4096, maxFrames, 4096);
  try {
    assert.equal(audio.status(), 0);
    for (;;) {
      const sample = replay.next_sample();
      if (!sample) break;
      try {
        assert.equal(audio.insert_sample(sample.id, sample.rate, sample.channels, sample.take_pcm_array()), 0);
      } finally { sample.free(); }
    }
    assert.equal(audio.finish_at(replay.playback_end_frame), 0);
    assert.equal(audio.arm(0n, 0n), 0);
    return audio;
  } catch (error) { audio.free(); throw error; }
}
function actualAudioWords(audio) {
  const words = new Uint32Array(56);
  for (let field = 0; field < 28; field++) {
    words[field * 2] = audio.report_word(field, false);
    words[field * 2 + 1] = audio.report_word(field, true);
  }
  return words;
}
function admitSpeedCommands(replay, audio, commands) {
  for (let turn = 0; turn < 128; turn++) {
    const batch = replay.commands(4096);
    if (!batch) return;
    assert.ok(batch.commands.length > 0);
    for (const command of batch.commands) {
      commands.push({ kind: command.kind, at: command.at, value: command.value,
        denominator: command.denominator });
      assert.equal(audio.enqueue(command.kind, command.voice, command.sample,
        command.at, command.gain, command.value, command.denominator), 0);
    }
    replay.acknowledge(batch.sequence, batch.commands.length, true);
  }
  assert.fail("actual replay command admission exceeded its bounded fixture");
}
function renderSpeedBlock(audio, frame, frames) {
  assert.equal(audio.render(frame, 0, frames), 0);
  const samples = frames * audio.channels();
  assert.ok(samples <= audio.output_len(), "actual callback extent fits the setup-owned output buffer");
  return Array.from(new Float32Array(speedAudioMemory(), audio.output_ptr(), samples));
}
function speedAudioMemory() { return audioNamespace.BrowserAudio.memory().buffer; }

test("actual WASM replay executes four literal rates with original-song presentation and finite output", () => {
  assert.equal(typeof BrowserReplay.new_with_speed, "function", "fresh selected-speed bindings are required");
  const owner = library(REPLAY_SPEED_CHART);
  try {
    const recording = speedRecording(owner, 50000000n, true), originalBytes = Array.from(recording);
    const cases = [
      { fraction: "1/2", numerator: 1n, denominator: 2n, end: 4800n,
        at120ms: 10000000n, pcm: [0.25, 0.0625, -0.125, -0.125] },
      { fraction: "1/1", numerator: 1n, denominator: 1n, end: 3600n,
        at120ms: 20000000n, pcm: [0.25, -0.125] },
      { fraction: "2/1", numerator: 2n, denominator: 1n, end: 3000n,
        at120ms: 40000000n, pcm: [0.25] },
      { fraction: "3/2", numerator: 3n, denominator: 2n, end: 3200n,
        at120ms: 30000000n, pcm: [0.25, -0.125] },
    ];
    for (const fixture of cases) {
      const replay = BrowserReplay.new_with_speed(speedPrepared(owner, recording), 100000000n, fixture.fraction);
      const audio = speedAudio(replay);
      try {
        assert.equal(replay.speed_numerator, fixture.numerator);
        assert.equal(replay.speed_denominator, fixture.denominator);
        assert.equal(replay.end_ns, 50000000n);
        assert.equal(replay.playback_end_frame, fixture.end);
        assert.equal(replay.recorded_until_ns, 1000n, "sound prefix retains its genuine original early Advance");
        const commands = [], pcm = [];
        admitSpeedCommands(replay, audio, commands);
        let frame = 0, complete = false;
        while (frame < Number(fixture.end) + 128) {
          // Split at the two literal clock boundaries, independent of any
          // production rate helper or end-frame computation.
          const frames = Math.min(128, frame < 2400 ? 2400 - frame
            : frame < 2880 ? 2880 - frame : Number(fixture.end) + 128 - frame);
          pcm.push(...renderSpeedBlock(audio, frame, frames));
          frame += frames;
          const words = actualAudioWords(audio);
          if (frame === 2880) {
            const before = replay.song_ns;
            assert.equal(replay.observe_output(words, undefined), false);
            assert.equal(replay.song_ns, before, "render-ahead without presentation cannot advance song time");
            assert.equal(replay.observe_output(words, 120000000n), false);
            assert.equal(replay.song_ns, fixture.at120ms);
          } else {
            // ceil is needed because a genuine presented endpoint must reach
            // the exact end of a frame at rates not dividing one second.
            const presented = (BigInt(frame) * 1000000000n + 23999n) / 24000n;
            complete = replay.observe_output(words, presented);
            if (frame === 2400) assert.equal(replay.song_ns, 0n, "unscaled output preroll crosses original start");
          }
          if (complete) break;
          admitSpeedCommands(replay, audio, commands);
        }
        assert.equal(complete, true, fixture.fraction);
        assert.equal(replay.song_ns, 50000000n, "genuine finite fence reaches original section end");
        assert.equal(replay.failed, false);
        assert.deepEqual([replay.hits, replay.misses, replay.combo, replay.max_combo], [0n, 0n, 0n, 0n],
          "speed must not invent judging for an original early stopped prefix");
        assert.ok(pcm.slice(0, 4800).every(value => value === 0), "100ms preroll stays in output time");
        const expected = fixture.pcm.flatMap(value => [value, value]);
        assert.deepEqual(pcm.slice(4800, 4800 + expected.length), expected, fixture.fraction);
        assert.ok(pcm.slice(4800 + expected.length).every(value => value === 0));
        const controls = commands.filter(command => command.kind === 2);
        assert.deepEqual(controls, fixture.fraction === "1/1" ? [] : [{ kind: 2, at: 0n,
          value: fixture.numerator, denominator: fixture.denominator }]);
        assert.equal(commands.filter(command => command.kind === 0).length, 1);
        assert.ok(commands.some(command => command.kind === 0 && command.at === 100000000n));
        assert.deepEqual(Array.from(recording), originalBytes, "consuming playback preserves canonical captured bytes");
      } finally { audio.free(); replay.free(); }
    }
  } finally { owner.free(); }
});

test("actual WASM normal constructor and reduced selected normal preserve commands and metadata", () => {
  const owner = library(REPLAY_SPEED_CHART);
  try {
    const recording = speedRecording(owner, 50000000n, true);
    const old = new BrowserReplay(speedPrepared(owner, recording), 100000000n);
    const selected = BrowserReplay.new_with_speed(speedPrepared(owner, recording), 100000000n, "0002/0002");
    try {
      for (const replay of [old, selected]) {
        assert.equal(replay.speed_numerator, 1n); assert.equal(replay.speed_denominator, 1n);
        assert.equal(replay.end_ns, 50000000n); assert.equal(replay.playback_end_frame, 3600n);
      }
      const a = old.commands(4096), b = selected.commands(4096);
      assert.deepEqual(structuredClone(a), structuredClone(b));
      assert.ok(a.commands.every(command => command.kind !== 2));
      old.acknowledge(a.sequence, a.commands.length, true);
      selected.acknowledge(b.sequence, b.commands.length, true);
      assert.equal(old.commands(4096), null); assert.equal(selected.commands(4096), null);
    } finally { old.free(); selected.free(); }
  } finally { owner.free(); }
});

test("actual WASM string speed preserves maximal exact components and rejects malformed or overflowed fractions", () => {
  const owner = library(REPLAY_SPEED_CHART);
  try {
    const recording = speedRecording(owner);
    const maximal = BrowserReplay.new_with_speed(speedPrepared(owner, recording), 0n,
      "9223372036854775807/18446744073709551615");
    try {
      assert.equal(maximal.speed_numerator, 9223372036854775807n);
      assert.equal(maximal.speed_denominator, 18446744073709551615n);
      assert.equal(maximal.playback_end_frame, 2400n);
    } finally { maximal.free(); }
    for (const fraction of ["", "0/1", "1/0", "-1/1", "+1/1", "1", "1/2/3", "1/2 ", " 1/2",
      "１/2", "1/٢", "1.0/2", "9223372036854775808/1", "1/18446744073709551616"]) {
      const prepared = speedPrepared(owner, recording);
      assert.throws(() => BrowserReplay.new_with_speed(prepared, 0n, fraction), undefined, fraction);
      assert.throws(() => new BrowserReplay(prepared, 0n), "failed consuming constructor cannot reuse prepared ownership");
      const retry = BrowserReplay.new_with_speed(speedPrepared(owner, recording), 0n, "6/4");
      try { assert.equal(retry.speed_numerator, 3n); assert.equal(retry.speed_denominator, 2n); }
      finally { retry.free(); }
    }
  } finally { owner.free(); }
});

test("actual WASM zero quantized fence requires real output presentation and never emits out-of-fence controls", () => {
  const owner = library(REPLAY_SPEED_CHART);
  try {
    const recording = speedRecording(owner);
    const replay = BrowserReplay.new_with_speed(speedPrepared(owner, recording), 0n, "9223372036854775807/1");
    const audio = speedAudio(replay);
    try {
      assert.equal(replay.playback_end_frame, 0n);
      assert.equal(replay.commands(4096), null);
      assert.equal(replay.observe_output(actualAudioWords(audio), undefined), false);
      renderSpeedBlock(audio, 0, 1);
      const words = actualAudioWords(audio);
      const before = replay.song_ns;
      assert.equal(replay.observe_output(words, undefined), false);
      assert.equal(replay.song_ns, before);
      assert.equal(replay.observe_output(words, 0n), true);
      assert.equal(replay.song_ns, 50000000n);
      assert.equal(audio.report_word(13, false), 0, "no rate or sound command was consumed beyond the fence");
      assert.equal(audio.report_word(14, false), 0);
    } finally { audio.free(); replay.free(); }
  } finally { owner.free(); }
});

test("actual WASM selected-speed constructor refuses live prepared resources and permits fresh replay retry", () => {
  const owner = library(REPLAY_SPEED_CHART);
  try {
    const live = prepare(owner).prepared;
    assert.throws(() => BrowserReplay.new_with_speed(live, 0n, "3/2"), /replay/i);
    const recording = speedRecording(owner);
    const replay = BrowserReplay.new_with_speed(speedPrepared(owner, recording), 100000000n, "3/2");
    try { assert.equal(replay.speed_numerator, 3n); assert.equal(replay.sample_count(), 1); }
    finally { replay.free(); }
  } finally { owner.free(); }
});

test("actual WASM selected speeds preserve nonempty captured judge operations and their canonical bytes", () => {
  const owner = library(REPLAY_SPEED_CHART);
  try {
    const source = BrowserGame.new_section(prepare(owner).prepared, 0n, 0n,
      50000000n, 50000000n, 0n, Uint32Array.from([0x11, 4]), 200000000n);
    let recording;
    try {
      source.configure_capture(1024 * 1024, 1024);
      source.activate(0n);
      const audio = speedAudio(source);
      try {
        source.feed_audio(0n, 256);
        admitSpeedCommands(source, audio, []);
        source.observe_presentation(0n, 0n);
        for (let frame = 0; frame < 2400;) {
          const frames = Math.min(128, 2400 - frame);
          renderSpeedBlock(audio, frame, frames); frame += frames;
        }
        source.admit_output(actualAudioWords(audio), 100000000n);
        source.observe_presentation(100000000n, 100000000n);
        source.close_input_prefix(100000000n);
        assert.equal(source.service_audio(100000000n, 100000000n), 0);
        assert.equal(source.song_ns, 100000000n);
        assert.deepEqual([source.hits, source.misses, source.combo, source.max_combo], [0n, 1n, 0n, 0n],
          "genuine output-authoritative Advance misses the original zero-time note");
        source.stop(); recording = source.take_replay();
        assert.ok(recording instanceof Uint8Array);
      } finally { audio.free(); }
    } finally { source.free(); }
    const originalBytes = Array.from(recording);
    for (const [fraction, endpoint] of [["1/2", 9600n], ["1/1", 4800n], ["2/1", 2400n], ["3/2", 3200n]]) {
      const replay = BrowserReplay.new_with_speed(speedPrepared(owner, recording), 0n, fraction);
      const audio = speedAudio(replay);
      try {
        assert.equal(replay.recorded_until_ns, 100000000n);
        assert.equal(replay.end_ns, 200000000n);
        assert.equal(replay.playback_end_frame, endpoint);
        admitSpeedCommands(replay, audio, []);
        let completed = false;
        for (let frame = 0; frame < Number(endpoint) + 128 && !completed;) {
          const frames = Math.min(128, Number(endpoint) + 128 - frame);
          renderSpeedBlock(audio, frame, frames); frame += frames;
          completed = replay.observe_output(actualAudioWords(audio),
            (BigInt(frame) * 1000000000n + 23999n) / 24000n);
          if (!completed) admitSpeedCommands(replay, audio, []);
        }
        assert.equal(completed, true, fraction);
        assert.equal(replay.song_ns, 200000000n);
        assert.deepEqual([replay.hits, replay.misses, replay.combo, replay.max_combo], [0n, 1n, 0n, 0n], fraction);
        assert.deepEqual(Array.from(recording), originalBytes);
      } finally { audio.free(); replay.free(); }
    }
  } finally { owner.free(); }
});

test("the canonical dependency query and preparation continue the same parsed original plan", () => {
  const owner = library(CHART, false);
  try {
    assert.equal(owner.query_parse_count, 0n);
    assert.equal(owner.query_compile_count, 0n);
    assert.deepEqual(Array.from(owner.referenced_asset_paths("pack/chart.bms", 0n, 8)), ["pack/key.wav"]);
    assert.equal(owner.query_parse_count, 1n, "query's actual parsing is counted independently of the continuation");
    assert.equal(owner.query_compile_count, 1n);
    owner.add_file("pack/key.wav", wav());
    const job = originalJob(owner);
    try {
      const metrics = ready(job, owner);
      assert.equal(metrics.parses, 0, "dependency planning already parsed this exact admitted chart");
      assert.equal(metrics.compiles, 0, "preparation must continue the query's compiled original");
      assert.equal(metrics.decodes, 1);
      job.take_prepared(owner).free();
    } finally { job.free(); }
    assert.equal(owner.query_parse_count, 1n);
    assert.equal(owner.query_compile_count, 1n);
    assert.deepEqual(Array.from(owner.referenced_asset_paths("pack/chart.bms", 0n, 8)), ["pack/key.wav"]);
    assert.equal(owner.query_parse_count, 1n, "warm queries must not repeat hidden parsing");
    assert.equal(owner.query_compile_count, 1n);
    owner.declare_file("pack/additional.wav", 1);
    assert.deepEqual(Array.from(owner.referenced_asset_paths("pack/chart.bms", 0n, 8)), ["pack/key.wav"]);
    assert.equal(owner.query_parse_count, 2n);
    assert.equal(owner.query_compile_count, 2n);
    const changed = originalJob(owner);
    try {
      const metrics = ready(changed, owner);
      assert.equal(metrics.parses, 0, "new inventory query establishes a fresh plan used by preparation");
      assert.equal(metrics.compiles, 0);
      changed.take_prepared(owner).free();
    } finally { changed.free(); }
  } finally { owner.free(); }
});

test("preparation jobs authenticate the library and inventory while declaration hydration stays admissible", () => {
  const owner = library(CHART, false), foreign = library();
  try {
    const hydration = originalJob(owner);
    try {
      owner.add_file("pack/key.wav", wav());
      ready(hydration, owner);
      const prepared = hydration.take_prepared(owner); prepared.free();
      assert.throws(() => hydration.take_prepared(owner)); assert.throws(() => hydration.advance(owner));
    } finally { hydration.free(); }
    const wrongOwner = originalJob(owner);
    try { assert.throws(() => wrongOwner.advance(foreign)); assert.throws(() => wrongOwner.take_prepared(foreign)); }
    finally { wrongOwner.free(); }
    const drift = originalJob(owner);
    try {
      owner.declare_file("pack/new.wav", 1);
      assert.throws(() => drift.advance(owner)); assert.throws(() => drift.take_prepared(owner));
    } finally { drift.free(); }
    const readyDrift = originalJob(owner);
    try {
      ready(readyDrift, owner);
      owner.declare_file("pack/newer.wav", 1);
      assert.throws(() => readyDrift.take_prepared(owner), "Ready is not admission after inventory drift");
    } finally { readyDrift.free(); }
  } finally { owner.free(); foreign.free(); }
});

test("ready cancellation preserves a previously admitted original and synchronous wrappers remain compatible", () => {
  const owner = library();
  try {
    const first = owner.prepare_chart("pack/chart.bms", 24000, 2, 0n, ...limits); first.free();
    const staged = originalJob(owner);
    try { ready(staged, owner); staged.cancel(); assert.throws(() => staged.take_prepared(owner)); assert.throws(() => staged.advance(owner)); }
    finally { staged.free(); }
    const retry = prepare(owner);
    assert.equal(retry.metrics.parses, 0); assert.equal(retry.metrics.decodes, 0);
    assert.equal(retry.prepared.note_count, 2); assert.equal(retry.prepared.sample_count, 2);
    retry.prepared.free();
    assert.throws(() => owner.prepare_chart("pack/chart.bms", 24000, 2, 0n, 16, 16, 1), "shared originals still obey narrower logical sample capacity");
    const afterFailure = prepare(owner); assert.equal(afterFailure.metrics.decodes, 0); afterFailure.prepared.free();
  } finally { owner.free(); }
});

test("actual section, policy, local and replay preparations share originals but create fresh consumers", () => {
  const owner = library();
  try {
    const first = prepare(owner, 3n); const capture = game(first.prepared);
    let recording;
    try {
      capture.configure_capture(1024 * 1024, 1024);
      capture.stop(); // Export the owner's real fenced, incomplete prefix.
      recording = capture.take_replay();
      assert.ok(recording instanceof Uint8Array); assert.ok(recording.byteLength > 0);
    } finally { capture.free(); }
    const factories = [
      [() => originalJob(owner, 3n), 0n, "live"],
      [() => owner.begin_prepare_chart_at("pack/chart.bms", 24000, 2, 3n, 1n, ...limits), 1n, "section"],
      [() => owner.begin_prepare_chart_with_policy_at("pack/chart.bms", 24000, 2, 3n, 0n, ...limits,
        "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1", "rank-first", "groove", 0n), 0n, "policy"],
      [() => originalJob(owner, 3n), 0n, "local"],
      [() => owner.begin_prepare_replay_chart("pack/chart.bms", recording, 24000, 2, ...limits), 0n, "replay"],
    ];
    for (const [make, start, mode] of factories) {
      const job = make();
      try {
        const metrics = ready(job, owner);
        assert.equal(metrics.decodes, 0, `${mode} must not decode identical original PCM again`);
        const prepared = job.take_prepared(owner);
        assert.equal(prepared.start_ns, start);
        if (mode === "local") {
          const plan = snapshotLocalPlan(Uint32Array.from([7, 0, 0, 0]));
          const physical = keyboardBindingWords(Uint32Array.from([0x11, 4]));
          const bindings = localBindingWords(plan, physical, [0x11], false);
          const consumer = BrowserLocalGame.new_physical(prepared, 0n, 100000000n, 50000000n, 50000000n, 0n,
            plan.words, bindings.words, undefined, false, 4096, 1024);
          try { assert.deepEqual(Array.from(consumer.players), [7]); const sample = takeSample(consumer); sample.free(); }
          finally { consumer.free(); }
        } else if (mode === "replay") {
          const consumer = new BrowserReplay(prepared, 100000000n);
          try { const sample = takeSample(consumer); sample.free(); } finally { consumer.free(); }
        } else {
          const consumer = game(prepared);
          try { const sample = takeSample(consumer); sample.free(); } finally { consumer.free(); }
        }
      } finally { job.free(); }
    }
    const unhydrated = library("#BPM 121\n#WAV01 key.wav\n#00011:01\n", false);
    try {
      const invalid = unhydrated.begin_prepare_replay_chart("pack/chart.bms", recording, 24000, 2, ...limits);
      try {
        assert.throws(() => ready(invalid, unhydrated), error => /identity|record|setup|fingerprint/i.test(String(error)),
          "genuine recording must fail chart identity validation before trying declared-only media");
        assert.equal(invalid.read_count, 0); assert.equal(invalid.decode_count, 0);
        assert.throws(() => invalid.take_prepared(unhydrated));
      } finally { invalid.free(); }
    } finally { unhydrated.free(); }
  } finally { owner.free(); }
});

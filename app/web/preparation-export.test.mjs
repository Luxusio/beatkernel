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

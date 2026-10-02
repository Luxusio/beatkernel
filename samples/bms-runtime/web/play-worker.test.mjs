// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/play-worker.test.mjs
// Actual Worker and numeric helpers; only generated WASM owners and browser APIs are mocked.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

const FileType = globalThis.File ?? NodeFile;
const ORIGIN = 9007199254740993n;
const START = 9007199254741999n;
const SCORE = { song_ns: 123456789012345n, hits: 17n, misses: 3n, combo: 9n };
const pairs = () => new Uint32Array([0x11, 2, 0x12, 3]);
const command = (voice = 7n) => ({ kind: 0, voice, sample: 19n, at: 100000001n, gain: 0.5, value: 0n, denominator: 1n });
const batch = sequence => ({ sequence, commands: [command(sequence), command(sequence + 1n)] });

function deferred() {
  let resolve;
  const promise = new Promise(yes => { resolve = yes; });
  return { promise, resolve };
}

async function flushJobs() {
  for (let index = 0; index < 32; index++) await Promise.resolve();
}

function selectedFile(path, acquire) {
  const bytes = new TextEncoder().encode("#BPM 120");
  const file = new FileType([bytes], path.split("/").at(-1));
  file.arrayBuffer = acquire ?? (() => Promise.resolve(bytes.buffer));
  return { file, path };
}

function renderReport({ available = true, cursor = 9007199254742999n, frames = 257n, start = START } = {}) {
  const words = new Uint32Array(56);
  const put = (index, value) => {
    words[index * 2] = Number(value & 0xffffffffn);
    words[index * 2 + 1] = Number(value >> 32n);
  };
  put(0, available ? 1n : 0n);
  put(2, frames);
  put(3, cursor - 129n);
  put(4, 129n);
  put(25, 1n);
  put(26, start);
  return { available, words };
}

async function workerHarness(options = {}) {
  const messages = [];
  const transfers = [];
  const libraries = [];
  const views = [];
  const games = [];
  const preparedOwners = [];
  const timers = new Map();
  let timerId = 0;
  let receive;
  class BrowserLibrary {
    files = [];
    preparations = [];
    frees = 0;
    constructor(...limits) { this.limits = limits; libraries.push(this); }
    add_file(path) { this.files.push(path); }
    chart_paths() { return this.files.filter(path => /\.bms$/i.test(path)); }
    prepare_chart(path, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.includes(path));
      this.preparations.push({ path, args });
      const prepared = {
        path, title: "Actual prepared metadata", artist: "Fixture", duration_ns: 604800000000000n,
        note_count: 23, sample_count: 2, image_count: 1,
        lanes: new Uint8Array(options.lanes ?? [0x11, 0x12]), moved: false, frees: 0,
        free() { assert.equal(this.moved, false); assert.equal(++this.frees, 1); },
      };
      preparedOwners.push(prepared);
      return prepared;
    }
    free() { assert.equal(++this.frees, 1); }
  }
  class BrowserView {
    static async create() {
      if (options.viewGate) await options.viewGate.promise;
      const view = new BrowserView();
      views.push(view);
      return view;
    }
    current = null;
    extents = [];
    positions = [];
    draws = 0;
    gameDraws = [];
    resize(...extent) { this.extents.push(extent); }
    set_chart(prepared) {
      assert.equal(prepared.moved, false);
      prepared.moved = true;
      this.current = prepared;
    }
    seek(ns) { this.positions.push(ns); }
    draw() { this.draws++; }
    draw_game(game) { assert.equal(game.frees, 0); this.gameDraws.push(game); }
    needs_redraw() { return false; }
  }
  class BrowserGame {
    constructor(prepared, ...args) {
      assert.equal(prepared.moved, false);
      prepared.moved = true; // The generated consuming constructor owns even its Err argument.
      if (options.constructError) throw new Error(options.constructError);
      this.prepared = prepared;
      this.args = args;
      this.score = { ...SCORE };
      this.calls = [];
      this.frees = 0;
      this.stops = 0;
      this.samples = [
        { id: 19n, rate: 44100, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) },
        { id: 18446744073709551615n, rate: 96000, pcm: new Float32Array([1, -1]) },
      ].map(value => ({
        ...value, channels: 2, takes: 0, frees: 0,
        take_pcm() {
          assert.equal(++this.takes, 1);
          if (options.takeError) throw new Error(options.takeError);
          return this.pcm;
        },
        free() { assert.equal(++this.frees, 1); if (options.sampleFreeError) throw new Error(options.sampleFreeError); },
      }));
      this.sampleIndex = 0;
      this.batches = [...(options.batches ?? [])];
      games.push(this);
    }
    live() { assert.equal(this.frees, 0, "binding must not be read after free"); }
    get song_ns() { this.live(); return this.score.song_ns; }
    get hits() { this.live(); return this.score.hits; }
    get misses() { this.live(); return this.score.misses; }
    get combo() { this.live(); return this.score.combo; }
    get failed() { this.live(); return false; }
    sample_count() { this.live(); return this.samples.length; }
    next_sample() { this.live(); this.calls.push(["sample"]); return this.samples[this.sampleIndex++] ?? null; }
    activate(host) { this.live(); this.calls.push(["activate", host]); }
    input(...args) {
      this.live();
      this.calls.push(["input", ...args]);
      options.input?.(this, args);
    }
    advance(...args) { this.live(); this.calls.push(["advance", ...args]); options.advance?.(this, args); }
    observe_output(words, presentedNs) {
      this.live();
      this.calls.push(["output", words.slice(), presentedNs]);
      return options.observeOutput?.(this, words, presentedNs) ?? false;
    }
    commands(max) { this.live(); this.calls.push(["commands", max]); return this.batches.shift() ?? null; }
    acknowledge(...args) { this.live(); this.calls.push(["ack", ...args]); options.ack?.(this, args); }
    stop() {
      this.live();
      assert.equal(++this.stops, 1);
      if (options.stopError) throw new Error(options.stopError);
    }
    free() {
      assert.equal(this.stops, 1);
      assert.equal(++this.frees, 1);
      if (options.freeError) throw new Error(options.freeError);
    }
  }
  const self = {
    isSecureContext: true, navigator: { gpu: {} },
    postMessage(value, transfer = []) {
      transfers.push([...transfer]);
      messages.push(structuredClone(value, { transfer }));
    },
    addEventListener(name, callback) { assert.equal(name, "message"); receive = callback; },
  };
  const context = createContext({
    self, File: FileType, TextEncoder, Uint8Array, Uint32Array, Float32Array, ArrayBuffer,
    performance: { now() { throw new Error("Worker timestamps cannot replace Window provenance"); } },
    setTimeout(callback) { const id = ++timerId; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  const wasm = new SyntheticModule(["default", "BrowserLibrary", "BrowserView", "BrowserGame"], function () {
    this.setExport("default", async () => { if (options.initGate) await options.initGate.promise; });
    this.setExport("BrowserLibrary", BrowserLibrary);
    this.setExport("BrowserView", BrowserView);
    this.setExport("BrowserGame", BrowserGame);
  }, { context });
  const helper = new SourceTextModule(await readFile(new URL("./host_model.mjs", import.meta.url), "utf8"), { context });
  const playHelper = new SourceTextModule(await readFile(new URL("./play-model.mjs", import.meta.url), "utf8"), { context });
  const worker = new SourceTextModule(await readFile(new URL("./worker.js", import.meta.url), "utf8"), { context });
  await worker.link(specifier => {
    if (specifier === "./pkg/beatkernel_bms_runtime.js") return wasm;
    if (specifier === "./host_model.mjs") return helper;
    if (specifier === "./play-model.mjs") return playHelper;
    throw new Error(`Unexpected import: ${specifier}`);
  });
  await worker.evaluate();
  return {
    messages, transfers, libraries, preparedOwners, views, games, timers,
    post(request) { receive({ data: request }); },
    async send(request) { receive({ data: request }); await flushJobs(); },
    async tick() {
      const entry = timers.entries().next().value;
      assert.ok(entry, "expected presentation callback");
      timers.delete(entry[0]); entry[1](); await flushJobs();
    },
    of(kind) { return messages.filter(value => value.kind === kind); },
  };
}

function startRequest(fields = {}) {
  return { kind: "play-start", playId: 7, rpcId: 1, libraryId: 1, path: "song/chart.bms", rate: 48000,
    seed: "18446744073709551615", keyPairs: pairs(), ...fields };
}

async function catalogWorker(options = {}) {
  const h = await workerHarness(options);
  await h.send({ kind: "init", canvas: { transferred: true } });
  assert.equal(h.of("ready").length, 1);
  await h.send({ kind: "import", id: 1, files: [selectedFile("song/chart.bms")] });
  await h.send({ kind: "accept-library", id: 1 });
  await h.send({ kind: "select", id: 2, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
  await h.send({ kind: "resize", width: 640, height: 480 });
  return h;
}

async function started(options = {}) {
  const h = await catalogWorker(options);
  await h.send(startRequest());
  assert.equal(h.of("play-reply").at(-1).result.kind, "prepared");
  h.rpcId = 1;
  h.rpc = async (kind, fields = {}) => {
    const rpcId = ++h.rpcId;
    await h.send({ kind, playId: 7, rpcId, ...fields });
    const reply = h.of("play-reply").find(value => value.rpcId === rpcId);
    assert.ok(reply, `missing reply ${rpcId}`);
    return reply;
  };
  return h;
}

async function active(options = {}) {
  const h = await started(options);
  const reply = await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
  assert.equal(reply.result, null);
  return h;
}

function step(fields = {}) {
  return { kind: "play-step", playId: 7, tickId: 1, events: [], watermark: ORIGIN, audioNs: 100000000n, ...fields };
}

function assertReleased(h, score = SCORE) {
  const game = h.games[0];
  assert.equal(game.stops, 1);
  assert.equal(game.frees, 1);
  const last = h.of("play-error").at(-1) ?? h.of("play-stopped").at(-1);
  assert.equal(last.songNs, score.song_ns);
  assert.equal(last.hits, score.hits);
  assert.equal(last.misses, score.misses);
  assert.equal(last.combo, score.combo);
  assert.equal(h.libraries[0].frees, 0, "accepted library remains available after gameplay");
}

test("prepared ownership, original-rate PCM transfers and setup batches retain their exact identities", async () => {
  const first = batch(9007199254740993n);
  const second = batch(9007199254740994n);
  const h = await started({ batches: [first, second] });
  const game = h.games[0];
  const metadata = h.of("play-reply")[0].result;
  assert.deepEqual(metadata, { kind: "prepared", samples: 2, title: "Actual prepared metadata", artist: "Fixture", notes: 23, lanes: [0x11, 0x12] });
  assert.deepEqual(h.libraries[0].preparations[1].args, [48000, 2, 18446744073709551615n, 64 * 1024 * 1024, 256 * 1024 * 1024, 1296]);
  assert.deepEqual(game.args.slice(0, 5), [0n, 100000000n, 50000000n, 50000000n, 0n]);
  assert.deepEqual(Array.from(game.args[5]), Array.from(pairs()));
  for (const [index, expected] of [[0, [19n, 44100, [0.25, -0.25, 0.5, -0.5]]], [1, [18446744073709551615n, 96000, [1, -1]]]]) {
    const reply = await h.rpc("play-sample");
    assert.equal(reply.result.id, expected[0]);
    assert.equal(reply.result.rate, expected[1]);
    assert.equal(reply.result.channels, 2);
    assert.deepEqual(Array.from(reply.result.pcm), expected[2]);
    assert.equal(game.samples[index].pcm.buffer.byteLength, 0, "the transferable buffer left its Worker owner");
    assert.equal(game.samples[index].takes, 1);
    assert.equal(game.samples[index].frees, 1);
  }
  assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
  assert.deepEqual((await h.rpc("play-commands")).result, first);
  await h.rpc("play-ack", { sequence: first.sequence, admitted: 2, success: true });
  assert.equal(game.calls.filter(value => value[0] === "commands").length, 1, "setup ACK does not consume the next batch");
  assert.equal(h.of("play-commands").length, 0);
  assert.deepEqual((await h.rpc("play-commands")).result, second);
  await h.rpc("play-ack", { sequence: second.sequence, admitted: 2, success: true });
  assert.equal((await h.rpc("play-commands")).result, null);
  await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
  assert.deepEqual(game.calls.find(value => value[0] === "activate"), ["activate", ORIGIN]);
  await h.tick();
  assert.equal(h.views[0].gameDraws[0], game);
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  assert.equal(h.preparedOwners[1].frees, 0, "consumed preparation is not explicitly freed twice");
  await h.tick();
  assert.equal(h.views[0].draws, 1);
  assert.equal(h.views[0].current, h.preparedOwners[0], "accepted preview survives gameplay");
});

test("Window input provenance, pre-origin count and actual rendered cursor survive an outstanding audio batch", async () => {
  const h = await active({ batches: [batch(11n), batch(12n)] });
  const game = h.games[0];
  const events = [
    { hostNs: ORIGIN - 1n, key: 2, down: true, sequence: 9007199254740993n },
    { hostNs: ORIGIN, key: 2, down: false, sequence: 9007199254740994n },
    { hostNs: ORIGIN + 9n, key: 3, down: true, sequence: 9007199254740994n },
  ];
  await h.send(step({ events, watermark: ORIGIN + 10n, audioNs: 987654321n }));
  assert.deepEqual(game.calls.filter(value => ["input", "advance"].includes(value[0])), [
    ["input", ORIGIN, 2, false, 9007199254740994n, 987654321n],
    ["input", ORIGIN + 9n, 3, true, 9007199254740994n, 987654321n],
    ["advance", ORIGIN + 10n, 987654321n],
  ]);
  assert.equal(h.of("play-step-done")[0].preOriginInputs, 1);
  assert.equal(h.of("play-step-done")[0].hits, SCORE.hits);
  assert.deepEqual(h.of("play-commands")[0].batch, batch(11n));
  const pulls = game.calls.filter(value => value[0] === "commands").length;
  const unavailable = renderReport({ available: false });
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: unavailable, presentedNs: null });
  assert.deepEqual(game.calls.find(value => value[0] === "output"), ["output", unavailable.words, null]);
  assert.deepEqual(h.of("play-render-done")[0], { kind: "play-render-done", playId: 7, renderId: 1, completed: false });
  const actual = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 2, report: actual, presentedNs: 9007199254742999n });
  assert.deepEqual(game.calls.filter(value => value[0] === "output")[1], ["output", actual.words, 9007199254742999n]);
  await h.send(step({ tickId: 2, watermark: ORIGIN + 20n }));
  assert.equal(game.calls.filter(value => value[0] === "commands").length, pulls);
  assert.equal(h.of("play-render-done").length, 2);
  await h.send({ kind: "play-ack", playId: 7, sequence: 11n, admitted: 2, success: true });
  assert.deepEqual(game.calls.find(value => value[0] === "ack"), ["ack", 11n, 2, true]);
  assert.deepEqual(h.of("play-commands")[1].batch, batch(12n));
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
});

test("a malformed later input rejects the whole bounded step before any binding mutation", async () => {
  const valid = { hostNs: ORIGIN, key: 2, down: true, sequence: 10n };
  const invalid = [
    { key: 99 }, { key: 2.5 }, { down: 1 }, { hostNs: -1n }, { hostNs: ORIGIN - 1n },
    { hostNs: 9223372036854775808n }, { hostNs: Number(ORIGIN) }, { sequence: 9n }, { sequence: 18446744073709551616n },
  ];
  const cases = invalid.map(fields => step({ events: [valid, { ...valid, hostNs: ORIGIN + 1n, ...fields }], watermark: ORIGIN + 2n }));
  cases.push(step({ events: [valid], watermark: ORIGIN - 1n }), step({ events: Array(257).fill(valid) }),
    step({ audioNs: -1n }), step({ tickId: 0 }), step({ watermark: 0 }));
  for (const request of cases) {
    const h = await active();
    await h.send(request);
    assert.equal(h.games[0].calls.filter(value => ["input", "advance", "output", "commands"].includes(value[0])).length, 0);
    assert.equal(h.of("play-step-done").length, 0);
    assert.equal(h.of("play-error").length, 1);
    assertReleased(h);
  }
});

test("partial binding failures and rejected admitted prefixes retain real score and never retry", async () => {
  const partial = { song_ns: 9007199254742999n, hits: 18n, misses: 4n, combo: 0n };
  const h = await active({ input(game) {
    game.score = { ...partial };
    if (game.calls.filter(value => value[0] === "input").length === 2) throw new Error("actual queue rejected committed prefix");
  } });
  await h.send(step({ events: [0n, 1n, 2n].map(offset => ({ hostNs: ORIGIN + offset, key: 2, down: true, sequence: offset })), watermark: ORIGIN + 3n }));
  assert.equal(h.games[0].calls.filter(value => value[0] === "input").length, 2);
  assert.equal(h.games[0].calls.filter(value => value[0] === "advance").length, 0);
  assert.match(h.of("play-error")[0].message, /committed prefix/);
  assertReleased(h, partial);
  await h.send(step({ tickId: 2 }));
  assert.equal(h.of("play-error").length, 1);

  const rejected = await active({ batches: [batch(55n)], ack(game) {
    game.score = { ...partial };
    throw new Error("remote rejected after one admitted command");
  } });
  await rejected.send(step());
  await rejected.send({ kind: "play-ack", playId: 7, sequence: 55n, admitted: 1, success: false });
  assert.deepEqual(rejected.games[0].calls.filter(value => value[0] === "ack"), [["ack", 55n, 1, false]]);
  assert.equal(rejected.games[0].calls.filter(value => value[0] === "commands").length, 1);
  assertReleased(rejected, partial);
});

test("sample and constructor failures release their consumed owners and preserve the original error", async () => {
  const h = await started({ takeError: "original sample transfer failure", sampleFreeError: "secondary wrapper cleanup failure" });
  const reply = await h.rpc("play-sample");
  assert.match(reply.error, /original sample transfer failure/);
  assert.doesNotMatch(reply.error, /secondary wrapper/);
  assert.equal(h.games[0].samples[0].takes, 1);
  assert.equal(h.games[0].samples[0].frees, 1);
  assertReleased(h);

  const constructor = await catalogWorker({ constructError: "consuming constructor failed" });
  await constructor.send(startRequest());
  assert.equal(constructor.games.length, 0);
  assert.equal(constructor.preparedOwners[1].moved, true);
  assert.equal(constructor.preparedOwners[1].frees, 0);
  assert.equal(constructor.of("play-error")[0].hits, null);

  const missingLane = await catalogWorker({ lanes: [0x11, 0x13] });
  await missingLane.send(startRequest());
  assert.equal(missingLane.games.length, 0);
  assert.equal(missingLane.preparedOwners[1].moved, false);
  assert.equal(missingLane.preparedOwners[1].frees, 1);
});

test("stop cancels reserved asynchronous setup before any late preparation or owner creation", async () => {
  for (const gateName of ["initGate", "viewGate"]) {
    const gate = deferred();
    const h = await workerHarness({ [gateName]: gate });
    await h.send({ kind: "init", canvas: {} });
    h.post(startRequest());
    h.post({ kind: "play-stop", playId: 7 });
    await flushJobs();
    const stopped = h.of("play-stopped")[0];
    assert.equal(stopped.playId, 7);
    for (const field of ["songNs", "hits", "misses", "combo"]) assert.equal(stopped[field], null);
    assert.equal(h.of("play-reply")[0].rpcId, 1);
    assert.match(h.of("play-reply")[0].error, /stopped/i);
    gate.resolve();
    await flushJobs();
    assert.equal(h.games.length, 0);
    assert.equal(h.preparedOwners.length, 0);
    assert.equal(h.of("play-error").length, 0);
  }
  const ready = await catalogWorker();
  ready.post(startRequest());
  ready.post({ kind: "play-stop", playId: 7 });
  await flushJobs();
  assert.equal(ready.games.length, 0);
  assert.equal(ready.libraries[0].preparations.length, 1, "only the earlier preview was prepared");
  await ready.send(startRequest());
  assert.equal(ready.games.length, 0, "a stopped identity cannot be resurrected");
  await ready.send(startRequest({ playId: 8 }));
  assert.equal(ready.games.length, 1);
  await ready.send({ kind: "play-stop", playId: 8 });
  assertReleased(ready);
});

test("live owners reject library and preview mutations while stale play identities do nothing", async () => {
  const h = await active();
  const game = h.games[0];
  let reads = 0;
  await h.send({ kind: "import", id: 8, files: [selectedFile("new.bms", () => { reads++; throw new Error("must not acquire while playing"); })] });
  await h.send({ kind: "accept-library", id: 8 });
  await h.send({ kind: "select", id: 9, libraryId: 1, path: "song/chart.bms", rate: 48000, seed: "0" });
  await h.send({ kind: "seek", id: 10, selectedId: 2, ns: "100" });
  assert.equal(reads, 0);
  assert.equal(h.libraries[0].preparations.length, 2);
  assert.equal(h.views[0].positions.length, 0);
  assert.equal(h.of("import-error").length, 2);
  assert.equal(h.of("selection-error").length, 1);
  assert.equal(h.of("seek-error").length, 1);
  const count = h.messages.length;
  await h.send(startRequest({ playId: 99 }));
  await h.send({ kind: "play-stop", playId: 6 });
  await h.send(step({ playId: 99 }));
  assert.equal(h.messages.length, count);
  assert.equal(game.frees, 0);
  await h.send({ kind: "resize", width: 800, height: 600 });
  await h.tick();
  assert.equal(h.views[0].gameDraws[0], game);
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  await h.tick();
  assert.equal(h.views[0].draws, 1);
});

test("RPC, tick and report fences prevent repeated consumption and reject faulty Mixer evidence", async () => {
  const repeated = await started();
  await repeated.rpc("play-sample");
  await repeated.send({ kind: "play-sample", playId: 7, rpcId: 2 });
  assert.equal(repeated.games[0].sampleIndex, 1);
  assertReleased(repeated);
  for (const next of [step(), step({ tickId: 2, watermark: ORIGIN - 1n })]) {
    const h = await active();
    await h.send(step());
    await h.send(next);
    assert.equal(h.games[0].calls.filter(value => value[0] === "advance").length, 1);
    assert.equal(h.of("play-step-done").length, 1);
    assertReleased(h);
  }
  const render = await active();
  await render.send({ kind: "play-render", playId: 7, renderId: 2, report: renderReport(), presentedNs: null });
  await render.send({ kind: "play-render", playId: 7, renderId: 2, report: renderReport(), presentedNs: null });
  assert.equal(render.games[0].calls.filter(value => value[0] === "output").length, 1);
  assert.equal(render.of("play-render-done").length, 1);
  assertReleased(render);
  const unknownSample = renderReport();
  unknownSample.words[36] = 1;
  const terminal = renderReport();
  terminal.words[54] = 1;
  for (const faulty of [renderReport({ start: START + 1n }), unknownSample, terminal]) {
    const h = await active();
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: faulty, presentedNs: null });
    assert.equal(h.games[0].calls.filter(value => value[0] === "output").length, 0);
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  const bounded = await active({ input() { throw new Error("x".repeat(5000)); } });
  await bounded.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }] }));
  assert.equal(bounded.of("play-error")[0].message.length, 4096);
  assertReleased(bounded);
});

test("actual completion result is correlated without disposing gameplay before its explicit stop", async () => {
  const h = await active({ observeOutput() { return true; } });
  const report = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report, presentedNs: 9223372036854775807n });
  assert.deepEqual(h.of("play-render-done")[0], { kind: "play-render-done", playId: 7, renderId: 1, completed: true });
  assert.equal(h.games[0].frees, 0);
  assert.equal(h.of("play-stopped").length, 0);
  await h.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: false, sequence: 1n }] }));
  assert.equal(h.games[0].calls.filter(value => value[0] === "input").length, 1,
    "captured input may join before the host releases this owner");
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  for (const fault of ["stopError", "freeError"]) {
    const broken = await active({ [fault]: "actual gameplay disposal failed", observeOutput() { return true; } });
    await broken.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
    assert.equal(broken.of("play-render-done")[0].completed, true);
    await broken.send({ kind: "play-stop", playId: 7 });
    assert.equal(broken.of("play-stopped").length, 0);
    assert.equal(broken.of("play-error")[0].released, false);
    assert.match(broken.of("play-error")[0].message, /disposal failed/);
    assertReleased(broken);
  }
});

test("malformed presentation and contradictory or failed completion cannot publish a successful receipt", async () => {
  for (const presentedNs of [undefined, -1n, 9223372036854775808n, 1, "1"]) {
    const h = await active();
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs });
    assert.equal(h.games[0].calls.filter(value => value[0] === "output").length, 0);
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  for (const result of ["true", 1]) {
    const h = await active({ observeOutput() { return result; } });
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  const outstanding = await active({ batches: [batch(81n)], observeOutput() { return true; } });
  await outstanding.send(step());
  await outstanding.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
  assert.equal(outstanding.of("play-render-done").length, 0);
  assertReleased(outstanding);

  const retained = { song_ns: 888n, hits: 21n, misses: 5n, combo: 2n };
  const failed = await active({ observeOutput(game) {
    game.score = { ...retained };
    throw new Error("actual completion rejected the output domain");
  } });
  await failed.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
  assert.equal(failed.of("play-render-done").length, 0);
  assert.match(failed.of("play-error")[0].message, /output domain/);
  assert.equal(failed.of("play-error")[0].released, true);
  assertReleased(failed, retained);
});

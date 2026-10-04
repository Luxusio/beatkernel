// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/*.test.mjs
// Loads the actual Worker and helper sources; no generated WASM or browser.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

const FileType = globalThis.File ?? NodeFile;

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function flushJobs() {
  // Advance only promise jobs; Worker presentation timers are entirely fake.
  for (let index = 0; index < 32; index++) await Promise.resolve();
}

function selectedFile(path, content = "#BPM 120", acquire) {
  const bytes = new TextEncoder().encode(content);
  const file = new FileType([bytes], path.split("/").at(-1));
  file.arrayBuffer = acquire ?? (() => Promise.resolve(bytes.buffer));
  return { file, path };
}

async function workerHarness(options = {}) {
  const messages = [];
  const libraries = [];
  const views = [];
  const games = [];
  const preparedOwners = [];
  const calls = [];
  const timers = new Map();
  let now = 0;
  let timerId = 0;
  let receive;
  function makePrepared(path, start = 0n) {
    const prepared = {
      title: `Prepared ${path}`, artist: "Fixture", duration_ns: 604800000000000n,
      note_count: 3, sample_count: 2, image_count: 1, lanes: [0x11], path, moved: false, frees: 0,
      free() { assert.equal(this.moved, false); assert.equal(++this.frees, 1); },
    };
    if (!options.omitPreparedStart) prepared.start_ns = Object.hasOwn(options, "preparedStart") ? options.preparedStart : start;
    preparedOwners.push(prepared);
    return prepared;
  }
  class BrowserLibrary {
    constructor(...limits) {
      this.limits = limits;
      this.files = [];
      this.preparations = [];
      this.frees = 0;
      libraries.push(this);
    }
    add_file(path, bytes) {
      assert.equal(this.frees, 0);
      this.files.push({ path, bytes: Array.from(bytes) });
    }
    chart_paths() {
      return this.files.map(entry => entry.path).filter(path => /\.(bms|bme|bml|pms)$/i.test(path));
    }
    prepare_chart(path, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.some(entry => entry.path === path), "prepare from the owning library");
      this.preparations.push({ path, args });
      if (path === options.rejectPreparation) throw new Error("sample rate mismatch");
      return makePrepared(path);
    }
    prepare_chart_at(path, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.some(entry => entry.path === path));
      this.preparations.push({ path, args, method: "prepare_chart_at" });
      calls.push(["prepare-section", ...args]);
      return makePrepared(path, args[3]);
    }
    prepare_replay_chart(path, bytes, rate) {
      calls.push(["prepare-replay", Array.from(bytes)]);
      const prepared = this.prepare_chart(path, rate);
      if (!options.omitPreparedStart) prepared.start_ns = options.replayStart ?? 0n;
      return prepared;
    }
    free() { this.frees++; assert.equal(this.frees, 1); }
  }
  class BrowserView {
    static async create(canvas) {
      if (options.createError) throw new Error(options.createError);
      const view = new BrowserView();
      view.canvas = canvas;
      views.push(view);
      return view;
    }
    current = null;
    replacements = [];
    extents = [];
    positions = [];
    draws = 0;
    resize(width, height) { this.extents.push([width, height]); }
    set_chart(prepared) {
      assert.equal(prepared.moved, false);
      assert.equal(prepared.frees, 0);
      prepared.moved = true;
      if (this.current) this.current.releasedByView = true;
      this.current = prepared;
      this.replacements.push(prepared);
    }
    seek(ns) { this.positions.push(ns); }
    draw() { this.draws++; }
    draw_game(game) { assert.equal(game.frees, 0); this.draws++; }
    draw_replay(game) { assert.equal(game.frees, 0); this.draws++; }
    needs_redraw() { return options.needsRedraw ?? false; }
  }
  class BrowserGame {
    constructor(prepared, ...constructorArgs) {
      if (!options.gameplay) throw new Error("Preview fixtures must not create gameplay owners");
      assert.equal(prepared.moved, false);
      prepared.moved = true;
      this.prepared = prepared;
      this.constructorArgs = constructorArgs;
      this.frees = 0;
      this.stops = 0;
      this.added = [];
      this.snapshots = 0;
      this.song_ns = -100000000n;
      this.hits = 17n;
      this.misses = 3n;
      this.combo = 4n;
      this.max_combo = 9n;
      this.recorded_until_ns = 1000000000n;
      games.push(this);
      calls.push(["new-game"]);
    }
    add_saved_opponent(bytes, own, label) {
      assert.equal(this.frees, 0);
      calls.push(["add-opponent", Array.from(bytes), own, label]);
      if (options.addError) throw new Error(options.addError);
      this.added.push({ bytes: Array.from(bytes), own, label });
      return this.added.length - 1;
    }
    saved_opponents() {
      assert.equal(this.frees, 0);
      this.snapshots++;
      calls.push(["snapshot", this.song_ns]);
      if (options.snapshotError) throw new Error(options.snapshotError);
      return options.snapshot?.(this) ?? [];
    }
    disable_saved_opponent_hud() {
      assert.equal(this.frees, 0);
      calls.push(["disable-opponent-hud"]);
    }
    update_peer_hud() { assert.fail("Preview and solo fixtures must not publish peer HUD state"); }
    disable_peer_hud() { assert.fail("Preview and solo fixtures must not own peer HUD state"); }
    configure_capture(...limits) { calls.push(["capture", ...limits]); }
    sample_count() { return options.sampleCount ?? 0; }
    next_sample() { return undefined; }
    commands() { return null; }
    activate(at) { calls.push(["activate", at]); }
    input(...values) { calls.push(["input", ...values]); }
    advance(host, audio) { calls.push(["advance", host, audio]); this.song_ns = options.songNs ?? host; }
    observe_output() { return false; }
    observe_presentation() {}
    stop() { this.stops++; calls.push(["stop"]); }
    take_replay() { calls.push(["take-replay"]); return Uint8Array.from([66, 75, 82]); }
    free() { this.frees++; assert.equal(this.frees, 1); calls.push(["free"]); }
  }
  const self = {
    isSecureContext: true,
    navigator: { gpu: {} },
    performance: { timeOrigin: 0, now: () => now },
    postMessage(value) { messages.push(value); },
    addEventListener(name, callback) {
      assert.equal(name, "message");
      receive = callback;
    },
  };
  const context = createContext({
    self, File: FileType, TextEncoder, TextDecoder, Uint8Array, Uint32Array, Float32Array, ArrayBuffer,
    performance: self.performance,
    setTimeout(callback) { const id = ++timerId; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  const wasm = new SyntheticModule(["default", "BrowserLibrary", "BrowserView", "BrowserGame", "BrowserReplay"], function () {
    this.setExport("default", async () => {
      if (options.initError) throw new Error(options.initError);
    });
    this.setExport("BrowserLibrary", BrowserLibrary);
    this.setExport("BrowserView", BrowserView);
    this.setExport("BrowserGame", BrowserGame);
    this.setExport("BrowserReplay", BrowserGame);
  }, { context });
  const network = new SyntheticModule(["BrowserMultiplayerOwner"], function () {
    this.setExport("BrowserMultiplayerOwner", class {
      static open() { throw new Error("Preview fixtures must not open multiplayer connections"); }
    });
  }, { context });
  const helpers = new SourceTextModule(await readFile(new URL("./host_model.mjs", import.meta.url), "utf8"), { context });
  const playHelpers = new SourceTextModule(await readFile(new URL("./play-model.mjs", import.meta.url), "utf8"), { context });
  const opponentHelpers = new SourceTextModule(await readFile(new URL("./saved-opponents.mjs", import.meta.url), "utf8"), { context });
  const physicalHelpers = new SourceTextModule(await readFile(new URL("./physical-input.mjs", import.meta.url), "utf8"), { context });
  const localHelpers = new SourceTextModule(await readFile(new URL("./local-play-model.mjs", import.meta.url), "utf8"), { context });
  const hidProfileHelpers = new SourceTextModule(await readFile(new URL("./hid-profile.mjs", import.meta.url), "utf8"), { context });
  const gamepadProfileHelpers = new SourceTextModule(await readFile(new URL("./gamepad-profile.mjs", import.meta.url), "utf8"), { context });
  const commandClient = new SourceTextModule(await readFile(new URL("./audio-command-client.mjs", import.meta.url), "utf8"), { context });
  const worker = new SourceTextModule(await readFile(new URL("./worker.js", import.meta.url), "utf8"), { context });
  await worker.link(specifier => {
    if (specifier === "./pkg/beatkernel_bms_runtime.js") return wasm;
    if (specifier === "./host_model.mjs") return helpers;
    if (specifier === "./play-model.mjs") return playHelpers;
    if (specifier === "./multiplayer-owner.mjs") return network;
    if (specifier === "./saved-opponents.mjs") return opponentHelpers;
    if (specifier === "./physical-input.mjs") return physicalHelpers;
    if (specifier === "./local-play-model.mjs") return localHelpers;
    if (specifier === "./hid-profile.mjs") return hidProfileHelpers;
    if (specifier === "./gamepad-profile.mjs") return gamepadProfileHelpers;
    if (specifier === "./audio-command-client.mjs") return commandClient;
    throw new Error(`Unexpected Worker import: ${specifier}`);
  });
  await worker.evaluate();
  return {
    messages, libraries, views, timers, games, preparedOwners, calls,
    setNow(value) { assert.ok(value >= now); now = value; },
    async send(request) { receive({ data: request }); await flushJobs(); },
    async tick() {
      const next = timers.entries().next().value;
      assert.ok(next, "expected a scheduled presentation callback");
      timers.delete(next[0]);
      next[1]();
      await flushJobs();
    },
    of(kind) { return messages.filter(message => message.kind === kind); },
  };
}

async function readyWorker(options) {
  const worker = await workerHarness(options);
  await worker.send({ kind: "init", canvas: { transferred: true } });
  assert.equal(worker.of("ready").length, 1);
  assert.equal(worker.of("fatal").length, 0);
  return worker;
}

function opponentFile(name, values = [66, 75, 82, 1], read) {
  const bytes = Uint8Array.from(values);
  const file = new FileType([bytes], name);
  let reads = 0;
  file.arrayBuffer = () => { reads++; return read ? read() : Promise.resolve(bytes.slice().buffer); };
  return { file, bytes, get reads() { return reads; } };
}
function opponentChoice(selected, sourceKey, own = true) {
  return { file: selected.file, sourceKey, own, label: selected.file.name };
}
async function gameWorker(options = {}) {
  const worker = await readyWorker({ ...options, gameplay: true });
  await worker.send({ kind: "import", id: 1, files: [selectedFile("song/chart.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  return worker;
}
function startGame(worker, opponents, extra = {}) {
  return worker.send({ kind: "play-start", playId: 1, rpcId: 1, libraryId: 1,
    path: "song/chart.bms", rate: 48000, seed: "0", keyPairs: new Uint32Array([0x11, 4]),
    opponents, ...extra });
}

test("section start routes fresh preparations and exact source metadata before capture while zero remains compatible", async () => {
  for (const startNs of [undefined, 0n, 1125000001n, 604800000000001n]) {
    const worker = await gameWorker();
    await startGame(worker, [], { startNs, seed: "18446744073709551615", recordReplay: true });
    const entry = worker.libraries[0].preparations[0];
    const game = worker.games[0];
    if (startNs) {
      assert.equal(entry.method, "prepare_chart_at");
      assert.deepEqual(entry.args, [48000, 2, 18446744073709551615n, startNs, 64 * 1024 * 1024, 256 * 1024 * 1024, 1296]);
    } else {
      assert.equal(entry.method, undefined);
      assert.deepEqual(entry.args, [48000, 2, 18446744073709551615n, 64 * 1024 * 1024, 256 * 1024 * 1024, 1296]);
    }
    assert.equal(worker.of("play-reply").at(-1).result.startNs, startNs ?? 0n);
    assert.equal(game.prepared.start_ns, startNs ?? 0n);
    assert.deepEqual(game.constructorArgs.slice(0, 2), [0n, 100000000n], "original-song start belongs to the prepared owner, not the output-origin argument");
    assert.ok(worker.calls.findIndex(call => call[0] === "new-game") < worker.calls.findIndex(call => call[0] === "capture"));
    await worker.send({ kind: "play-stop", playId: 1 });
    await startGame(worker, [], { playId: 2, startNs: 2000000001n });
    assert.equal(worker.libraries[0].preparations.length, 2);
    assert.notEqual(worker.games[1].prepared, game.prepared, "restart constructs from the library again");
    assert.equal(worker.games[1].prepared.start_ns, 2000000001n);
    assert.equal(game.frees, 1);
    await worker.send({ kind: "play-stop", playId: 2 });
  }
  const legacy = await gameWorker({ omitPreparedStart: true });
  await startGame(legacy, []);
  assert.equal(legacy.of("play-reply").at(-1).result.startNs, 0n);
  await legacy.send({ kind: "play-stop", playId: 1 });
  const full = await gameWorker({ sampleCount: 5392 });
  await startGame(full, [], { startNs: 1n });
  assert.equal(full.of("play-reply").at(-1).result.samples, 5392);
  assert.equal(full.libraries[0].preparations[0].args.at(-1), 1296);
  await full.send({ kind: "play-stop", playId: 1 });
});

test("invalid requested starts fail before acquisition and mismatched prepared starts release unconsumed owners", async () => {
  for (const startNs of [null, "1", 1, -1n, 9223372036854775808n]) {
    const worker = await gameWorker();
    const unread = opponentFile("must-not-read.bkr");
    await startGame(worker, [opponentChoice(unread, "file:1")], { startNs });
    assert.equal(worker.libraries[0].preparations.length, 0);
    assert.equal(worker.games.length, 0);
    assert.equal(unread.reads, 0);
    assert.equal(worker.of("play-error").length, 1);
  }
  for (const options of [{ omitPreparedStart: true }, { preparedStart: 0n }, { preparedStart: 1 },
    { preparedStart: -1n }, { preparedStart: 9223372036854775808n }]) {
    const worker = await gameWorker(options);
    await startGame(worker, [], { startNs: 1000000000n, recordReplay: true });
    assert.equal(worker.preparedOwners.length, 1);
    assert.equal(worker.preparedOwners[0].moved, false);
    assert.equal(worker.preparedOwners[0].frees, 1);
    assert.equal(worker.games.length, 0);
    assert.equal(worker.calls.some(call => call[0] === "capture"), false);
    assert.equal(worker.of("play-error").length, 1);
    assert.equal(worker.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
  }
  const oversized = await gameWorker({ sampleCount: 5393 });
  await startGame(oversized, [], { startNs: 1n });
  assert.equal(oversized.of("play-error").length, 1);
  assert.equal(oversized.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
  assert.equal(oversized.games[0].frees, 1);
});

test("cancelled section preparation cannot revive and replay retains its actual recorded start independently", async () => {
  const cancelled = await gameWorker();
  const pending = startGame(cancelled, [], { startNs: 9000000001n });
  const stopping = cancelled.send({ kind: "play-stop", playId: 1 });
  await Promise.all([pending, stopping]);
  assert.equal(cancelled.libraries[0].preparations.length, 0);
  assert.equal(cancelled.games.length, 0);
  assert.equal(cancelled.of("play-stopped").length, 1);
  await startGame(cancelled, [], { playId: 2, startNs: 4000000001n });
  assert.equal(cancelled.games[0].prepared.start_ns, 4000000001n);
  await cancelled.send({ kind: "play-stop", playId: 2 });
  const worker = await gameWorker({ replayStart: 604800000000001n });
  const replay = opponentFile("section-prefix.bkr");
  await startGame(worker, undefined, { mode: "replay", replayFile: replay.file, startNs: "invalid live draft" });
  assert.equal(replay.reads, 1);
  assert.equal(worker.calls.filter(call => call[0] === "prepare-replay").length, 1);
  assert.equal(worker.calls.some(call => call[0] === "prepare-section" || call[0] === "capture"), false);
  assert.equal(worker.of("play-reply").at(-1).result.startNs, 604800000000001n);
  assert.equal(worker.games[0].prepared.start_ns, 604800000000001n);
  assert.deepEqual(worker.games[0].constructorArgs, [100000000n]);
  await worker.send({ kind: "play-stop", playId: 1 });
});

test("live judge timing forwards exact validated constructor values while omitted timing preserves defaults", async () => {
  const defaults = await gameWorker();
  await startGame(defaults, []);
  assert.deepEqual(defaults.games[0].constructorArgs.slice(0, 5), [0n, 100000000n, 50000000n, 50000000n, 0n]);
  await defaults.send({ kind: "play-stop", playId: 1 });
  const configured = await gameWorker();
  const timing = { earlyNs: 12345678n, lateNs: 87654321n, offsetNs: -12500001n };
  const preparing = startGame(configured, [], { timing, recordReplay: true });
  timing.earlyNs = 0n;
  timing.offsetNs = 900n;
  await preparing;
  assert.deepEqual(configured.games[0].constructorArgs.slice(0, 5), [0n, 100000000n, 12345678n, 87654321n, -12500001n]);
  assert.deepEqual(Array.from(configured.games[0].constructorArgs[5]), [0x11, 4]);
  assert.equal(configured.of("play-error").length, 0);
  assert.ok(configured.calls.findIndex(call => call[0] === "new-game") < configured.calls.findIndex(call => call[0] === "capture"));
  await configured.send({ kind: "play-stop", playId: 1 });
});

test("bad live timing fails before chart or opponent acquisition and replay uses only its recorded constructor", async () => {
  const baseline = { earlyNs: 50000000n, lateNs: 50000000n, offsetNs: 0n };
  for (const timing of [null, {}, { ...baseline, earlyNs: -1n }, { ...baseline, lateNs: -1n },
    { ...baseline, earlyNs: 50 }, { ...baseline, offsetNs: "0" },
    { ...baseline, offsetNs: 9223372036854775808n }, { ...baseline, offsetNs: -9223372036854775809n }]) {
    const worker = await gameWorker();
    const unread = opponentFile("must-not-read.bkr", [1], () => { throw new Error("timing preflight must precede acquisition"); });
    await startGame(worker, [opponentChoice(unread, "file:1")], { timing });
    assert.equal(unread.reads, 0);
    assert.equal(worker.libraries[0].preparations.length, 0);
    assert.equal(worker.games.length, 0);
    assert.equal(worker.of("play-error").length, 1);
    assert.equal(worker.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
  }
  const worker = await gameWorker();
  const replay = opponentFile("recorded-profile.bkr");
  await startGame(worker, undefined, { mode: "replay", replayFile: replay.file,
    timing: { earlyNs: "invalid live draft", lateNs: -1n, offsetNs: null } });
  assert.equal(replay.reads, 1);
  assert.equal(worker.of("play-error").length, 0);
  assert.deepEqual(worker.games[0].constructorArgs, [100000000n]);
  assert.equal(worker.of("play-reply").at(-1).result.mode, "replay");
  assert.equal(worker.calls.some(call => call[0] === "capture"), false);
  await worker.send({ kind: "play-stop", playId: 1 });
});

test("live preparation reads selected immutable Files sequentially and admits actual bindings before capture or activation", async () => {
  const firstRead = deferred();
  const secondRead = deferred();
  const first = opponentFile("own.bkr", [66, 75, 82, 1], () => firstRead.promise);
  const second = opponentFile("other.bkr", [66, 75, 82, 2], () => secondRead.promise);
  const worker = await gameWorker();
  await startGame(worker, [opponentChoice(first, "file:1"), opponentChoice(second, "record:2", false)], { recordReplay: true });
  assert.equal(first.reads, 1);
  assert.equal(second.reads, 0);
  assert.equal(worker.games[0].added.length, 0);
  assert.equal(worker.calls.some(call => call[0] === "capture"), false);
  assert.equal(worker.of("play-reply").length, 0);
  firstRead.resolve(first.bytes.slice().buffer); await flushJobs();
  assert.equal(second.reads, 1);
  assert.deepEqual(worker.games[0].added, [{ bytes: [66, 75, 82, 1], own: true, label: "own.bkr" }]);
  secondRead.resolve(second.bytes.slice().buffer); await flushJobs();
  const prepared = worker.of("play-reply").at(-1).result;
  assert.equal(prepared.kind, "prepared");
  assert.equal(prepared.opponentCount, 2);
  assert.deepEqual(worker.games[0].added[1], { bytes: [66, 75, 82, 2], own: false, label: "other.bkr" });
  const operations = worker.calls.map(call => call[0]);
  assert.ok(operations.lastIndexOf("add-opponent") < operations.indexOf("capture"));
  assert.equal(first.bytes.byteLength, 4);
  assert.equal(second.bytes.byteLength, 4);
  await worker.send({ kind: "play-activate", playId: 1, rpcId: 2, hostNs: 1000000000n, startFrame: 48000n });
  assert.ok(worker.calls.findIndex(call => call[0] === "capture") < worker.calls.findIndex(call => call[0] === "activate"));
  await worker.send({ kind: "play-stop", playId: 1 });
  assert.equal(worker.games[0].stops, 1);
  assert.equal(worker.games[0].frees, 1);
  assert.equal(worker.of("play-stopped")[0].replayError, null);
});

test("invalid selection, changed read extent and incompatible bytes fail explicitly while cancelled reads cannot admit into a newer game", async () => {
  const invalid = await gameWorker();
  let forbiddenReads = 0;
  await startGame(invalid, [{ file: { size: 4, arrayBuffer() { forbiddenReads++; } }, sourceKey: "fake", own: true, label: "fake" }]);
  assert.equal(forbiddenReads, 0);
  assert.equal(invalid.games.length, 0);
  assert.equal(invalid.libraries[0].preparations.length, 0);
  assert.equal(invalid.of("play-error").length, 1);

  for (const failure of ["extent", "layout", "incompatible"]) {
    const worker = await gameWorker(failure === "incompatible" ? { addError: "actual binding rejected incompatible chart" } : {});
    const selected = opponentFile("bad.bkr", [1, 2, 3, 4], failure === "extent"
      ? () => Promise.resolve(new ArrayBuffer(3))
      : failure === "layout" ? () => Promise.resolve(new Uint8Array(4)) : undefined);
    await startGame(worker, [opponentChoice(selected, "file:1")], { recordReplay: true });
    assert.equal(selected.reads, 1);
    assert.equal(worker.of("play-error").length, 1);
    assert.equal(worker.of("play-reply").some(reply => reply.result?.kind === "prepared"), false);
    assert.equal(worker.calls.some(call => call[0] === "capture" || call[0] === "activate"), false);
    assert.equal(worker.games[0].stops, 1);
    assert.equal(worker.games[0].frees, 1);
  }
  const pendingRead = deferred();
  const pending = await gameWorker();
  const unread = opponentFile("pending.bkr", [1, 2, 3, 4], () => pendingRead.promise);
  await startGame(pending, [opponentChoice(unread, "pending")]);
  await pending.send({ kind: "play-activate", playId: 1, rpcId: 2, hostNs: 1000000000n, startFrame: 48000n });
  assert.equal(pending.calls.some(call => call[0] === "activate"), false);
  assert.equal(pending.of("play-error").length, 1);
  pendingRead.resolve(unread.bytes.slice().buffer); await flushJobs();
  assert.deepEqual(pending.games[0].added, []);
  assert.equal(pending.games[0].frees, 1);
  const gate = deferred();
  const oldFile = opponentFile("old.bkr", [1, 2, 3, 4], () => gate.promise);
  const worker = await gameWorker();
  await startGame(worker, [opponentChoice(oldFile, "old")], { recordReplay: true });
  const previous = worker.games[0];
  await worker.send({ kind: "play-stop", playId: 1 });
  assert.equal(previous.frees, 1);
  await startGame(worker, [], { playId: 2 });
  const current = worker.games[1];
  gate.resolve(oldFile.bytes.slice().buffer); await flushJobs();
  assert.deepEqual(previous.added, []);
  assert.deepEqual(current.added, []);
  assert.equal(current.frees, 0);
  assert.equal(worker.of("play-reply").filter(reply => reply.playId === 1 && reply.result?.kind === "prepared").length, 0);
  assert.equal(worker.of("play-reply").find(reply => reply.playId === 2).result.opponentCount, 0);
  await worker.send({ kind: "play-stop", playId: 2 });
});

test("actual comparison snapshots are throttled independently and comparison faults do not stop local capture or solo and replay paths", async () => {
  const controls = { songNs: 999999990n, snapshot: game => [{ kind: "own", label: "own.bkr",
    songNs: game.song_ns, recordedUntilNs: 500000000n, hits: 1n, misses: 0n, combo: 1n, maxCombo: 1n }] };
  const worker = await gameWorker(controls);
  const selected = opponentFile("own.bkr");
  await startGame(worker, [opponentChoice(selected, "file:1")], { recordReplay: true });
  await worker.send({ kind: "play-activate", playId: 1, rpcId: 2, hostNs: 1000000000n, startFrame: 48000n });
  const step = (tickId, time) => worker.send({ kind: "play-step", playId: 1, tickId,
    events: [], watermark: BigInt(time), audioNs: BigInt(time) });
  await step(1, 1000000000);
  assert.equal(worker.games[0].snapshots, 1);
  assert.equal(worker.of("play-opponents").length, 0, "successful comparison prefixes stay in the Worker HUD");
  assert.ok(worker.calls.some(call => call[0] === "snapshot" && call[1] === 999999990n));
  worker.setNow(249);
  await step(2, 1000000001);
  assert.equal(worker.games[0].snapshots, 1);
  worker.setNow(250);
  await step(3, 1000000002);
  assert.equal(worker.games[0].snapshots, 2);
  worker.games[0].saved_opponents = () => { throw new Error("comparison prefix failure"); };
  worker.setNow(500);
  await step(4, 1000000003);
  assert.match(worker.of("play-opponents").at(-1).error, /comparison prefix failure/);
  assert.equal(worker.of("play-opponents").at(-1).opponents, null);
  worker.setNow(750);
  await step(5, 1000000004);
  assert.equal(worker.of("play-error").length, 0);
  assert.equal(worker.games[0].frees, 0);
  assert.equal(worker.of("play-step-done").length, 5);
  assert.equal(worker.of("play-step-done").at(-1).hits, 17n);
  assert.equal(worker.of("play-opponents").filter(value => value.error !== null).length, 1);
  await worker.send({ kind: "play-stop", playId: 1 });
  assert.equal(worker.of("play-stopped").at(-1).replayError, null);
  assert.equal(worker.of("play-stopped").at(-1).savedOpponents.opponents, null);
  assert.match(worker.of("play-stopped").at(-1).savedOpponents.error, /comparison prefix failure/);
  assert.equal(worker.calls.filter(call => call[0] === "disable-opponent-hud").length, 1);
  const publicationCount = worker.of("play-opponents").length;
  await startGame(worker, [], { playId: 2 });
  await worker.send({ kind: "play-activate", playId: 2, rpcId: 2, hostNs: 2000000000n, startFrame: 96000n });
  await worker.send({ kind: "play-step", playId: 1, tickId: 6, events: [], watermark: 2000000000n, audioNs: 2000000000n });
  await worker.send({ kind: "play-step", playId: 2, tickId: 1, events: [], watermark: 2000000000n, audioNs: 2000000000n });
  assert.equal(worker.games[1].snapshots, 0);
  assert.equal(worker.of("play-opponents").length, publicationCount);
  await worker.send({ kind: "play-stop", playId: 2 });
  const replay = opponentFile("replay.bkr");
  await startGame(worker, undefined, { playId: 3, mode: "replay", replayFile: replay.file });
  assert.equal(worker.games[2].added.length, 0);
  assert.equal(worker.games[2].snapshots, 0);
  await worker.send({ kind: "play-stop", playId: 3 });
});

test("overlapping imports retain only the latest pending request and free the stale candidate", async () => {
  const worker = await readyWorker();
  const pending = deferred();
  let reads = 0;
  let skippedReads = 0;
  let newerReads = 0;
  await worker.send({ kind: "import", id: 1, files: [selectedFile("old/a.bms", "old", () => { reads++; return pending.promise; })] });
  assert.equal(reads, 1);
  assert.equal(worker.libraries.length, 1);
  await worker.send({ kind: "import", id: 2, files: [selectedFile("skipped/b.bms", "skip", () => {
    skippedReads++;
    return Promise.resolve(new TextEncoder().encode("skip").buffer);
  })] });
  await worker.send({ kind: "import", id: 3, files: [selectedFile("new/c.bms", "new", () => {
    newerReads++;
    return Promise.resolve(new TextEncoder().encode("new").buffer);
  })] });
  assert.equal(newerReads, 0);
  assert.equal(skippedReads, 0);
  assert.equal(worker.libraries.length, 1);
  assert.equal(worker.of("catalog").length, 0);
  pending.resolve(new TextEncoder().encode("old").buffer);
  await flushJobs();
  assert.equal(newerReads, 1);
  assert.equal(skippedReads, 0);
  assert.equal(worker.libraries.length, 2);
  assert.equal(worker.libraries[0].frees, 1);
  assert.equal(worker.libraries[0].files.length, 0);
  assert.equal(worker.libraries[1].frees, 0);
  assert.deepEqual(worker.libraries[1].files.map(entry => entry.path), ["new/c.bms"]);
  assert.deepEqual(worker.of("catalog").map(message => message.id), [3]);
  assert.equal(worker.of("import-error").length, 0);
  await worker.send({ kind: "accept-library", id: 3 });
  await worker.send({ kind: "select", id: 4, libraryId: 3, path: "new/c.bms", rate: 48000, seed: "0" });
  assert.equal(worker.views[0].current.path, "new/c.bms");
});

test("invalid import metadata acquires no bytes and preserves the admitted library", async () => {
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 10, files: [selectedFile("song/chart.bms")] });
  await worker.send({ kind: "accept-library", id: 10 });
  let reads = 0;
  const acquire = () => { reads++; throw new Error("must not read invalid import"); };
  await worker.send({ kind: "import", id: 11, files: [
    selectedFile("bad/a.bms", "x", acquire),
    selectedFile("bad/./a.bms", "x", acquire),
  ] });
  assert.equal(reads, 0);
  assert.equal(worker.libraries.length, 1);
  assert.equal(worker.libraries[0].frees, 0);
  assert.deepEqual(worker.of("catalog").map(message => message.id), [10]);
  assert.equal(worker.of("import-error")[0].id, 11);
  await worker.send({ kind: "select", id: 12, libraryId: 10, path: "song/chart.bms", rate: 44100, seed: "18446744073709551615" });
  assert.equal(worker.views[0].current.path, "song/chart.bms");
  assert.equal(worker.libraries[0].preparations[0].args[2], 18446744073709551615n);
});

test("an ignored catalog followed by a failed import preserves the accepted library until matching acknowledgement", async () => {
  const worker = await readyWorker();
  await worker.send({ kind: "import", id: 1, files: [selectedFile("accepted/old.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "accepted/old.bms", rate: 48000, seed: "0" });
  const accepted = worker.libraries[0];
  const previousView = worker.views[0].current;

  // Main has moved on to import B before it receives proposal A's catalog.
  await worker.send({ kind: "import", id: 3, files: [selectedFile("ignored/a.bms")] });
  const ignored = worker.libraries[1];
  assert.deepEqual(worker.of("catalog").map(message => message.id), [1, 3]);
  await worker.send({ kind: "accept-library", id: 999 });
  await worker.send({ kind: "accept-library", id: 1 });
  assert.equal(accepted.frees, 0);
  assert.equal(ignored.frees, 0);
  await worker.send({ kind: "select", id: 4, libraryId: 3, path: "ignored/a.bms", rate: 48000, seed: "0" });
  assert.equal(worker.of("selection-error").at(-1).id, 4);
  assert.equal(worker.views[0].current, previousView);
  assert.equal(ignored.preparations.length, 0);

  await worker.send({ kind: "import", id: 5, files: [selectedFile("failed/b.bms", "x", () => Promise.reject(new Error("file read failed")))] });
  const failed = worker.libraries[2];
  assert.equal(ignored.frees, 1);
  assert.equal(failed.frees, 1);
  assert.equal(accepted.frees, 0);
  assert.equal(worker.of("import-error").at(-1).id, 5);
  assert.equal(worker.views[0].current, previousView);
  assert.equal(previousView.releasedByView, undefined);
  await worker.send({ kind: "accept-library", id: 3 });
  assert.equal(ignored.frees, 1);
  assert.equal(accepted.frees, 0);
  await worker.send({ kind: "select", id: 6, libraryId: 1, path: "accepted/old.bms", rate: 48000, seed: "0" });
  assert.equal(worker.views[0].current.path, "accepted/old.bms");
  assert.equal(accepted.preparations.length, 2);
  assert.equal(worker.of("selected").at(-1).libraryId, 1);

  // Only acknowledgement of the current proposal may release the old owner.
  await worker.send({ kind: "import", id: 7, files: [selectedFile("accepted/new.bms")] });
  const replacement = worker.libraries[3];
  await worker.send({ kind: "accept-library", id: 3 });
  assert.equal(accepted.frees, 0);
  assert.equal(replacement.frees, 0);
  await worker.send({ kind: "accept-library", id: 7 });
  assert.equal(accepted.frees, 1);
  assert.equal(replacement.frees, 0);
  await worker.send({ kind: "accept-library", id: 7 });
  await worker.send({ kind: "accept-library", id: 1 });
  assert.equal(accepted.frees, 1);
  assert.equal(replacement.frees, 0);
  await worker.send({ kind: "select", id: 8, libraryId: 7, path: "accepted/new.bms", rate: 48000, seed: "0" });
  assert.equal(worker.views[0].current.path, "accepted/new.bms");
  assert.equal(replacement.preparations.length, 1);
  assert.equal(worker.of("fatal").length, 0);
});

test("preparation failure retains the old view and its selected identity", async () => {
  const worker = await readyWorker({ rejectPreparation: "bad.bms" });
  await worker.send({ kind: "import", id: 1, files: [selectedFile("good.bms"), selectedFile("bad.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "good.bms", rate: 48000, seed: "7" });
  const original = worker.views[0].current;
  await worker.send({ kind: "select", id: 3, libraryId: 1, path: "bad.bms", rate: 48000, seed: "7" });
  assert.equal(worker.views[0].current, original);
  assert.equal(original.frees, 0);
  assert.equal(original.releasedByView, undefined);
  assert.equal(worker.views[0].replacements.length, 1);
  assert.deepEqual(worker.of("selected").map(message => message.id), [2]);
  assert.equal(worker.of("selection-error")[0].id, 3);
  assert.match(worker.of("selection-error")[0].message, /sample rate mismatch/);
  await worker.send({ kind: "seek", id: 4, selectedId: 2, ns: "604800000000001" });
  assert.deepEqual(worker.views[0].positions, [604800000000001n]);
  assert.equal(worker.of("position")[0].selectedId, 2);
});

test("WASM or GPU initialization failure never reports readiness or admits later work", async () => {
  for (const options of [{ initError: "missing WASM" }, { createError: "GPU unavailable" }]) {
    const worker = await workerHarness(options);
    await worker.send({ kind: "init", canvas: {} });
    assert.equal(worker.of("ready").length, 0);
    assert.equal(worker.of("fatal").length, 1);
    let reads = 0;
    await worker.send({ kind: "import", id: 1, files: [selectedFile("a.bms", "x", () => { reads++; })] });
    assert.equal(reads, 0);
    assert.equal(worker.libraries.length, 0);
    assert.equal(worker.timers.size, 0);
    assert.equal(worker.of("fatal").length, 1);
  }
});

test("surface retries are bounded and zero extent cancels the pending callback", async () => {
  const worker = await readyWorker({ needsRedraw: true });
  await worker.send({ kind: "resize", width: 960, height: 720 });
  await worker.send({ kind: "import", id: 1, files: [selectedFile("a.bms")] });
  await worker.send({ kind: "accept-library", id: 1 });
  await worker.send({ kind: "select", id: 2, libraryId: 1, path: "a.bms", rate: 48000, seed: "0" });
  assert.equal(worker.timers.size, 1);
  for (let index = 0; index < 4; index++) await worker.tick();
  assert.equal(worker.views[0].draws, 4);
  assert.equal(worker.timers.size, 0);
  assert.equal(worker.of("render-wait").length, 1);
  assert.equal(worker.of("drawn").length, 0);
  await worker.send({ kind: "seek", id: 3, selectedId: 2, ns: "1" });
  assert.equal(worker.timers.size, 1);
  await worker.send({ kind: "resize", width: 0, height: 720 });
  assert.equal(worker.timers.size, 0);
  assert.equal(worker.views[0].draws, 4);
  await worker.send({ kind: "resize", width: 960, height: 720 });
  assert.equal(worker.timers.size, 1);
});

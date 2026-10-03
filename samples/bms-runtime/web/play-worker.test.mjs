// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/play-worker.test.mjs
// Actual Worker and numeric helpers; only generated WASM owners and browser APIs are mocked.
import assert from "node:assert/strict";
import { File as NodeFile } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";
import { encodeKeyboardEvent } from "./physical-input.mjs";

const FileType = globalThis.File ?? NodeFile;
const ORIGIN = 9007199254740993n;
const START = 9007199254741999n;
const SCORE = { song_ns: 123456789012345n, hits: 17n, misses: 3n, combo: 9n, max_combo: 15n };
const pairs = () => new Uint32Array([0x11, 2, 0x12, 3]);
const command = (voice = 7n) => ({ kind: 0, voice, sample: 19n, at: 100000001n, gain: 0.5, value: 0n, denominator: 1n });
const batch = sequence => ({ sequence, commands: [command(sequence), command(sequence + 1n)] });

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
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
  const replays = [];
  const sectionConstructions = [];
  const physicalConstructions = [];
  const preparedOwners = [];
  const timers = new Map();
  const timerDelays = new Map();
  const networks = [];
  const networkSessions = [];
  let networkNow = 1000;
  let timerId = 0;
  let receive;
  function makePrepared(path) {
    const prepared = {
      path, title: "Actual prepared metadata", artist: "Fixture", duration_ns: 604800000000000n,
      start_ns: 0n,
      note_count: 23, sample_count: 2, image_count: 1,
      lanes: new Uint8Array(options.lanes ?? [0x11, 0x12]), moved: false, frees: 0,
      free() { assert.equal(this.moved, false); assert.equal(++this.frees, 1); },
    };
    preparedOwners.push(prepared);
    return prepared;
  }
  class BrowserLibrary {
    files = [];
    preparations = [];
    replayPreparations = [];
    frees = 0;
    constructor(...limits) { this.limits = limits; libraries.push(this); }
    add_file(path) { this.files.push(path); }
    chart_paths() { return this.files.filter(path => /\.bms$/i.test(path)); }
    prepare_chart(path, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.includes(path));
      this.preparations.push({ path, args });
      return makePrepared(path);
    }
    prepare_chart_at(path, rate, channels, seed, startNs, ...limits) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.includes(path));
      this.preparations.push({ path, args: [rate, channels, seed, startNs, ...limits] });
      const prepared = makePrepared(path);
      prepared.start_ns = startNs;
      return prepared;
    }
    prepare_replay_chart(path, bytes, ...args) {
      assert.equal(this.frees, 0);
      assert.ok(this.files.includes(path));
      assert.ok(bytes instanceof Uint8Array);
      this.replayPreparations.push({ path, bytes: bytes.slice(), args });
      if (options.prepareReplayError) throw new Error(options.prepareReplayError);
      const prepared = makePrepared(path);
      if (Object.hasOwn(options, "replayStart")) prepared.start_ns = options.replayStart;
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
    replayDraws = [];
    resize(...extent) { this.extents.push(extent); }
    set_chart(prepared) {
      assert.equal(prepared.moved, false);
      prepared.moved = true;
      this.current = prepared;
    }
    seek(ns) { this.positions.push(ns); }
    draw() { this.draws++; }
    draw_game(game) { assert.equal(game.frees, 0); this.gameDraws.push(game); }
    draw_replay(replay) { assert.equal(replay.frees, 0); this.replayDraws.push(replay); }
    needs_redraw() { return false; }
  }
  class BrowserGame {
    static new_physical(prepared, ...args) {
      physicalConstructions.push({ prepared, args });
      if (options.physicalConstructError) {
        assert.equal(prepared.moved, false);
        prepared.moved = true;
        throw new Error(options.physicalConstructError);
      }
      const owner = new BrowserGame(prepared, ...args.slice(0, 6));
      owner.physical = true;
      return owner;
    }
    static new_section(prepared, ...args) {
      sectionConstructions.push({ prepared, args });
      if (options.sectionConstructError) {
        assert.equal(prepared.moved, false);
        prepared.moved = true;
        throw new Error(options.sectionConstructError);
      }
      const owner = new BrowserGame(prepared, ...args.slice(0, -1));
      owner.constructedEnd = args.at(-1);
      return owner;
    }
    constructor(prepared, ...args) {
      assert.equal(prepared.moved, false);
      prepared.moved = true; // The generated consuming constructor owns even its Err argument.
      if (options.constructError) throw new Error(options.constructError);
      this.prepared = prepared;
      this.args = args;
      this.score = { ...SCORE };
      this.calls = [];
      this.endpointReads = { end: 0, frame: 0 };
      this.frees = 0;
      this.stops = 0;
      this.disposals = [];
      this.replayTakes = 0;
      this.replayBytes = null;
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
    get end_ns() {
      this.live(); this.endpointReads.end++;
      return options.gameEndGetter ? options.gameEndGetter(this) : options.gameEnd;
    }
    get playback_end_frame() {
      this.live(); this.endpointReads.frame++;
      return options.gameFrameGetter ? options.gameFrameGetter(this) : options.gameEndFrame;
    }
    get song_ns() { this.live(); return this.score.song_ns; }
    get hits() { this.live(); return this.score.hits; }
    get misses() { this.live(); return this.score.misses; }
    get combo() { this.live(); return this.score.combo; }
    get max_combo() { this.live(); return this.score.max_combo; }
    get failed() { this.live(); return false; }
    competition_identity() {
      this.live(); this.calls.push(["identity"]);
      if (options.identityError) throw new Error(options.identityError);
      return options.identityBytes ?? Uint8Array.from([66, 75, 82, 0, 255]);
    }
    sample_count() { this.live(); return this.samples.length; }
    configure_capture(...limits) {
      this.live();
      this.calls.push(["capture", ...limits]);
      if (options.captureError) throw new Error(options.captureError);
    }
    take_replay() {
      this.live();
      assert.equal(this.stops, 1, "capture export requires a stopped owner");
      assert.equal(++this.replayTakes, 1);
      this.disposals.push("take");
      if (options.replayError) throw new Error(options.replayError);
      // Opaque binding output: actual codec/parity is covered by the Rust fixtures.
      this.replayBytes = options.replayBytes ? options.replayBytes() : Uint8Array.from([66, 75, 82, 255, 0, 1]);
      return this.replayBytes;
    }
    next_sample() { this.live(); this.calls.push(["sample"]); return this.samples[this.sampleIndex++] ?? null; }
    activate(host) { this.live(); this.calls.push(["activate", host]); }
    input(...args) {
      this.live();
      assert.notEqual(this.physical, true, "physical owners must not fall back to the legacy key method");
      this.calls.push(["input", ...args]);
      options.input?.(this, args);
    }
    input_blob(bytes, audioNs) {
      this.live();
      assert.equal(this.physical, true);
      assert.ok(bytes instanceof Uint8Array);
      this.calls.push(["blob", bytes.slice(), audioNs]);
      options.inputBlob?.(this, bytes, audioNs);
    }
    advance(...args) { this.live(); this.calls.push(["advance", ...args]); options.advance?.(this, args); }
    observe_output(words, presentedNs) {
      this.live();
      this.calls.push(["output", words.slice(), presentedNs]);
      return options.observeOutput?.(this, words, presentedNs) ?? false;
    }
    observe_presentation(outputNs, hostNs) {
      this.live();
      this.calls.push(["presentation", outputNs, hostNs]);
      options.observePresentation?.(this, outputNs, hostNs);
    }
    commands(max) { this.live(); this.calls.push(["commands", max]); return this.batches.shift() ?? null; }
    acknowledge(...args) { this.live(); this.calls.push(["ack", ...args]); options.ack?.(this, args); }
    stop() {
      this.live();
      assert.equal(++this.stops, 1);
      this.disposals.push("stop");
      if (options.stopError) throw new Error(options.stopError);
    }
    free() {
      assert.equal(this.stops, 1);
      assert.equal(++this.frees, 1);
      this.disposals.push("free");
      if (options.freeError) throw new Error(options.freeError);
    }
  }
  if (options.missingSectionConstructor) BrowserGame.new_section = undefined;
  if (options.missingPhysicalConstructor) BrowserGame.new_physical = undefined;
  if (options.missingInputBlob) BrowserGame.prototype.input_blob = undefined;
  class BrowserReplay extends BrowserGame {
    constructor(prepared, ...args) {
      super(prepared, ...args);
      assert.equal(games.pop(), this);
      this.endpointReads = { end: 0, frame: 0 };
      replays.push(this);
    }
    get end_ns() {
      this.live(); this.endpointReads.end++;
      return options.replayEndGetter ? options.replayEndGetter(this) : options.replayEnd;
    }
    get playback_end_frame() {
      this.live(); this.endpointReads.frame++;
      return options.replayFrameGetter ? options.replayFrameGetter(this) : options.replayEndFrame;
    }
    get recorded_until_ns() { this.live(); return options.recordedUntil === undefined ? SCORE.song_ns : options.recordedUntil; }
    activate() { assert.fail("replay must not activate a live transport"); }
    input() { assert.fail("replay must not accept live input"); }
    advance() { assert.fail("replay must not synthesize live advances"); }
    observe_presentation() { assert.fail("replay must not discipline a live input clock"); }
    configure_capture() { assert.fail("replay must not recapture a recording"); }
    take_replay() { assert.fail("replay playback must not re-export its input bytes"); }
    competition_identity() { assert.fail("replay playback must stay local"); }
  }
  class BrowserMultiplayer {
    constructor(identity, host, preroll) {
      this.identity = [...identity]; this.host = host; this.preroll = preroll;
      this.closes = 0; this.frees = 0; networkSessions.push(this);
      if (options.networkConstructError) throw new Error(options.networkConstructError);
    }
    close() { assert.equal(++this.closes, 1); }
    free() { assert.equal(++this.frees, 1); }
  }
  class BrowserMultiplayerOwner {
    static async open(url, config) {
      const owner = {
        url, config, origin: config.now(), closed: false, closes: 0, readyCalls: 0,
        submissions: [], ack: deferred(), ackCalls: 0,
        request_ready() { assert.equal(this.closed, false); this.readyCalls++; },
        submit(value, final) {
          assert.equal(this.closed, false);
          const gate = deferred();
          this.submissions.push({ value: structuredClone(value), final, gate });
          return gate.promise;
        },
        wait_final_ack() { this.ackCalls++; return this.ack.promise; },
        emit(event) { config.onEvent(event); },
        disconnect(error = new Error("peer disconnected")) {
          if (!this.closed) {
            this.closed = true; this.closes++;
            config.session.close(); config.session.free();
          }
          config.onClose(error);
        },
        close() {
          if (this.closed) return;
          this.disconnect(Object.assign(new Error("owner closed"), { code: "closed" }));
        },
      };
      networks.push(owner);
      try {
        if (options.networkOpenGate) await options.networkOpenGate.promise;
        if (options.networkOpenError) throw new Error(options.networkOpenError);
        return owner;
      } catch (error) { owner.disconnect(error); throw error; }
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
    self, File: FileType, TextEncoder, Uint8Array, Uint32Array, Float32Array, ArrayBuffer, URL, AbortController, AbortSignal,
    performance: { timeOrigin: 10000, now() {
      if (!options.allowNetworkClock) throw new Error("Solo Worker timestamps cannot replace Window provenance");
      return networkNow;
    } },
    setTimeout(callback, delay = 0) {
      const id = ++timerId; timers.set(id, callback); timerDelays.set(id, delay); return id;
    },
    clearTimeout(id) { timers.delete(id); timerDelays.delete(id); },
  });
  self.performance = context.performance;
  const wasm = new SyntheticModule(["default", "BrowserLibrary", "BrowserView", "BrowserGame", "BrowserReplay", "BrowserMultiplayer"], function () {
    this.setExport("default", async () => { if (options.initGate) await options.initGate.promise; });
    this.setExport("BrowserLibrary", BrowserLibrary);
    this.setExport("BrowserView", BrowserView);
    this.setExport("BrowserGame", BrowserGame);
    this.setExport("BrowserReplay", BrowserReplay);
    this.setExport("BrowserMultiplayer", BrowserMultiplayer);
  }, { context });
  const network = new SyntheticModule(["BrowserMultiplayerOwner"], function () {
    this.setExport("BrowserMultiplayerOwner", BrowserMultiplayerOwner);
  }, { context });
  const helper = new SourceTextModule(await readFile(new URL("./host_model.mjs", import.meta.url), "utf8"), { context });
  const playHelper = new SourceTextModule(await readFile(new URL("./play-model.mjs", import.meta.url), "utf8"), { context });
  const opponentHelper = new SourceTextModule(await readFile(new URL("./saved-opponents.mjs", import.meta.url), "utf8"), { context });
  const physicalHelper = new SourceTextModule(await readFile(new URL("./physical-input.mjs", import.meta.url), "utf8"), { context });
  const worker = new SourceTextModule(await readFile(new URL("./worker.js", import.meta.url), "utf8"), { context });
  await worker.link(specifier => {
    if (specifier === "./pkg/beatkernel_bms_runtime.js") return wasm;
    if (specifier === "./host_model.mjs") return helper;
    if (specifier === "./play-model.mjs") return playHelper;
    if (specifier === "./multiplayer-owner.mjs") return network;
    if (specifier === "./saved-opponents.mjs") return opponentHelper;
    if (specifier === "./physical-input.mjs") return physicalHelper;
    throw new Error(`Unexpected import: ${specifier}`);
  });
  await worker.evaluate();
  return {
    messages, transfers, libraries, preparedOwners, views, games, replays, sectionConstructions, physicalConstructions, timers, networks, networkSessions,
    setNetworkNow(value) { assert.ok(value >= networkNow); networkNow = value; },
    post(request) { receive({ data: request }); },
    async send(request) { receive({ data: request }); await flushJobs(); },
    async tick() {
      const entry = timers.entries().next().value;
      assert.ok(entry, "expected presentation callback");
      timers.delete(entry[0]); entry[1](); await flushJobs();
    },
    async expireNetwork() {
      const entry = [...timers].find(([id]) => timerDelays.get(id) === 2000);
      assert.ok(entry, "expected finite final drain deadline");
      timers.delete(entry[0]); timerDelays.delete(entry[0]); entry[1](); await flushJobs();
    },
    of(kind) { return messages.filter(value => value.kind === kind); },
  };
}

function startRequest(fields = {}) {
  return { kind: "play-start", playId: 7, rpcId: 1, libraryId: 1, path: "song/chart.bms", rate: 48000,
    seed: "18446744073709551615", keyPairs: pairs(), ...fields };
}

function replayFile(acquire = null, size = 6) {
  const bytes = Uint8Array.from([66, 75, 82, 0, 255, 1]);
  const file = new FileType([bytes], "original-recording.bkr");
  let reads = 0;
  Object.defineProperty(file, "size", { value: size });
  file.arrayBuffer = () => { reads++; return acquire ? acquire() : Promise.resolve(bytes.slice().buffer); };
  return { file, bytes, get reads() { return reads; } };
}
function replayRequest(file, fields = {}) {
  return { kind: "play-start", playId: 7, rpcId: 1, libraryId: 1,
    path: "song/chart.bms", rate: 48000, mode: "replay", replayFile: file, ...fields };
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
  await h.send(options.startRequest ?? startRequest(options.recordReplay === undefined ? {} : { recordReplay: options.recordReplay }));
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
  const game = h.replays[0] ?? h.games[0];
  assert.equal(game.stops, 1);
  assert.equal(game.frees, 1);
  const last = h.of("play-error").at(-1) ?? h.of("play-stopped").at(-1);
  assert.equal(last.songNs, score.song_ns);
  assert.equal(last.hits, score.hits);
  assert.equal(last.misses, score.misses);
  assert.equal(last.combo, score.combo);
  assert.equal(h.libraries[0].frees, 0, "accepted library remains available after gameplay");
}

test("explicit physical keyboard ownership uses native bindings and canonical blobs with the original acquisition clock and finite setup", async () => {
  for (const finite of [false, true]) {
    const keyPairs = pairs();
    const h = await active({ missingSectionConstructor: true,
      startRequest: startRequest({ inputMode: "physical", keyPairs, recordReplay: true,
        timing: { earlyNs: 7n, lateNs: 9n, offsetNs: -3n }, ...(finite ? { endNs: 1n } : {}) }),
      gameEnd: finite ? 1n : undefined, gameEndFrame: finite ? 4801n : undefined,
    });
    const game = h.games[0], metadata = h.of("play-reply")[0].result;
    assert.equal(metadata.inputMode, "physical");
    assert.equal(h.physicalConstructions.length, 1);
    assert.equal(h.sectionConstructions.length, 0);
    const construction = h.physicalConstructions[0];
    assert.equal(construction.prepared, h.preparedOwners[1]);
    assert.deepEqual(construction.args.slice(0, 5), [0n, 100000000n, 7n, 9n, -3n]);
    assert.deepEqual(Array.from(construction.args[5]), [
      0x11, 0, 0, 0, 1, 0x574b4559, 2, 0x12, 0, 0, 0, 1, 0x574b4559, 3,
    ]);
    assert.deepEqual(construction.args.slice(6), [finite ? 1n : undefined, 4096, 1024]);
    assert.equal(Object.hasOwn(metadata, "endFrame"), finite);
    if (finite) assert.equal(metadata.endFrame, 4801n);
    keyPairs[1] = 99;
    assert.equal(construction.args[5][6], 2);
    const down = { hostNs: ORIGIN, key: 2, down: true, sequence: 1n };
    const up = { hostNs: ORIGIN + 1n, key: 2, down: false, sequence: 2n };
    await h.send(step({ events: [{ hostNs: ORIGIN - 1n, key: 2, down: true, sequence: 0n }, down, up], watermark: ORIGIN + 1n }));
    const blobs = game.calls.filter(row => row[0] === "blob");
    assert.equal(blobs.length, 2, "fully validated pre-origin input remains ignored rather than retimestamped");
    assert.deepEqual(blobs.map(row => Array.from(row[1])), [Array.from(encodeKeyboardEvent(down)), Array.from(encodeKeyboardEvent(up))]);
    assert.deepEqual(blobs.map(row => row[2]), [100000000n, 100000000n]);
    assert.equal(game.calls.filter(row => row[0] === "input").length, 0);
    assert.deepEqual(game.calls.find(row => row[0] === "advance"), ["advance", ORIGIN + 1n, 100000000n]);
    assert.equal(h.of("play-step-done")[0].preOriginInputs, 1);
    await h.send({ kind: "play-stop", playId: 7 });
    assertReleased(h);
    assert.equal(h.of("play-stopped")[0].replayComplete, false);
    assert.deepEqual(game.disposals, ["stop", "take", "free"]);
    assert.equal(h.preparedOwners[1].frees, 0);
  }
  const empty = await active({ lanes: [], startRequest: startRequest({ inputMode: "physical", keyPairs: new Uint32Array() }) });
  assert.equal(empty.physicalConstructions[0].args[5].length, 0, "an empty chart does not acquire invented keyboard bindings");
  await empty.send(step());
  assert.equal(empty.games[0].calls.filter(row => row[0] === "blob").length, 0);
  await empty.send({ kind: "play-stop", playId: 7 });
  assertReleased(empty);
});

test("physical capability and whole-batch admission fail before consumption while encoded batches and committed failures never retry", async () => {
  for (const inputMode of [null, "", "legacy", "hid", 0]) {
    const gate = deferred(), h = await workerHarness({ initGate: gate });
    await h.send({ kind: "init", canvas: {} });
    await h.send(startRequest({ inputMode }));
    assert.equal(h.of("play-error").length, 1);
    assert.equal(h.preparedOwners.length, 0);
    assert.equal(h.games.length, 0);
    gate.resolve(); await flushJobs();
    assert.equal(h.games.length, 0);
  }
  for (const options of [{ missingPhysicalConstructor: true }, { missingInputBlob: true }]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ inputMode: "physical" }));
    assert.equal(h.physicalConstructions.length, 0);
    assert.equal(h.games.length, 0);
    assert.equal(h.of("play-error").length, 1);
    assert.ok(h.preparedOwners.slice(1).every(owner => !owner.moved && owner.frees === 1));
  }
  const selected = replayFile(), replay = await catalogWorker();
  await replay.send(replayRequest(selected.file, { inputMode: "physical" }));
  assert.equal(selected.reads, 0, "recorded playback refuses a live acquisition route before reading its recording");
  assert.equal(replay.games.length + replay.replays.length, 0);
  assert.equal(replay.of("play-error").length, 1);
  const refused = await catalogWorker({ physicalConstructError: "consuming physical setup refused" });
  await refused.send(startRequest({ inputMode: "physical" }));
  assert.equal(refused.physicalConstructions.length, 1);
  assert.equal(refused.games.length, 0);
  assert.equal(refused.preparedOwners[1].moved, true);
  assert.equal(refused.preparedOwners[1].frees, 0);
  assert.match(refused.of("play-error")[0].message, /consuming physical setup refused/);
  const malformed = await active({ startRequest: startRequest({ inputMode: "physical" }) });
  await malformed.send(step({ events: [
    { hostNs: ORIGIN, key: 2, down: true, sequence: 1n },
    { hostNs: ORIGIN + 1n, key: 3, down: false, sequence: 18446744073709551616n },
  ], watermark: ORIGIN + 1n }));
  assert.equal(malformed.games[0].calls.filter(row => ["input", "blob", "advance"].includes(row[0])).length, 0);
  assert.equal(malformed.of("play-step-done").length, 0);
  assertReleased(malformed);
  const staged = await active({ startRequest: startRequest({ inputMode: "physical" }) });
  const owner = staged.games[0];
  const second = { hostNs: ORIGIN + 1n, down: true, sequence: 2n,
    get key() { return owner.calls.some(row => row[0] === "blob") ? 65536 : 3; } };
  await staged.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }, second], watermark: ORIGIN + 1n }));
  assert.equal(staged.of("play-error").length, 0);
  assert.equal(owner.calls.filter(row => row[0] === "blob").length, 2, "every packet is encoded before the first runtime call");
  assert.deepEqual(Array.from(owner.calls.filter(row => row[0] === "blob")[1][1]),
    Array.from(encodeKeyboardEvent({ hostNs: ORIGIN + 1n, key: 3, down: true, sequence: 2n })));
  await staged.send({ kind: "play-stop", playId: 7 });
  assertReleased(staged);
  const partial = await active({ startRequest: startRequest({ inputMode: "physical" }), inputBlob(game) {
    game.score.hits = 18n;
    throw new Error("actual common input rejected after committed score");
  } });
  await partial.send(step({ events: [
    { hostNs: ORIGIN, key: 2, down: true, sequence: 1n },
    { hostNs: ORIGIN + 1n, key: 3, down: true, sequence: 2n },
  ], watermark: ORIGIN + 1n }));
  assert.equal(partial.games[0].calls.filter(row => row[0] === "blob").length, 1);
  assert.equal(partial.games[0].calls.filter(row => row[0] === "advance").length, 0);
  assertReleased(partial, { ...SCORE, hits: 18n });
  await partial.send(step({ tickId: 2 }));
  assert.equal(partial.games[0].calls.filter(row => row[0] === "blob").length, 1);
});

test("finite live ownership uses the consuming static constructor and snapshots actual endpoint metadata before capture and activation", async () => {
  const startNs = 604800000000001n, endNs = startNs + 1n;
  const timing = { earlyNs: 7000001n, lateNs: 9000002n, offsetNs: -3n };
  for (const finite of [false, true]) {
    const h = await started({
      startRequest: startRequest({ startNs, rate: 44100, timing, recordReplay: true, ...(finite ? { endNs } : {}) }),
      gameEndGetter(owner) { assert.equal(owner.endpointReads.end, 1); return finite ? endNs : undefined; },
      gameFrameGetter(owner) { assert.equal(owner.endpointReads.frame, 1); return finite ? 4411n : undefined; },
      observeOutput(owner) { owner.score.song_ns = endNs; return true; },
    });
    const game = h.games[0], metadata = h.of("play-reply")[0].result;
    assert.equal(h.sectionConstructions.length, finite ? 1 : 0);
    assert.deepEqual(h.libraries[0].preparations[1].args,
      [44100, 2, 18446744073709551615n, startNs, 64 * 1024 * 1024, 256 * 1024 * 1024, 1296]);
    assert.deepEqual(game.args.slice(0, 5), [0n, 100000000n, 7000001n, 9000002n, -3n]);
    assert.deepEqual(Array.from(game.args[5]), Array.from(pairs()));
    if (finite) {
      const construction = h.sectionConstructions[0];
      assert.equal(construction.prepared, h.preparedOwners[1]);
      assert.equal(construction.args.length, 7);
      assert.equal(construction.args[6], endNs, "end is the final static binding argument, not an input offset");
      assert.equal(game.constructedEnd, endNs);
      assert.equal(metadata.endNs, endNs);
      assert.equal(metadata.endFrame, 4411n);
    } else {
      assert.equal(Object.hasOwn(metadata, "endNs"), false);
      assert.equal(Object.hasOwn(metadata, "endFrame"), false);
    }
    assert.equal(metadata.startNs, startNs);
    assert.deepEqual(game.calls, [["capture", 64 * 1024 * 1024, 1000000]]);
    await h.rpc("play-sample"); await h.rpc("play-sample");
    assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
    assert.equal((await h.rpc("play-commands")).result, null);
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    await h.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }] }));
    assert.deepEqual(game.calls.find(row => row[0] === "input"), ["input", ORIGIN, 2, true, 1n, 100000000n]);
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 100022676n, presentedHostNs: ORIGIN });
    assert.equal(h.of("play-render-done").at(-1).completed, true);
    assert.equal(game.stops, 0, "the returned completion proof still waits for the explicit owner stop");
    await h.send({ kind: "play-stop", playId: 7, completed: true });
    assertReleased(h, { ...SCORE, song_ns: endNs });
    assert.equal(h.of("play-stopped")[0].replayComplete, true);
    assert.deepEqual(game.disposals, ["stop", "take", "free"]);
    assert.deepEqual(game.endpointReads, { end: 1, frame: 1 });
    assert.equal(h.preparedOwners[1].frees, 0);
  }
  const replay = await started({ startRequest: replayRequest(replayFile().file, { endNs: "invalid live end", startNs: null }) });
  assert.equal(replay.sectionConstructions.length, 0);
  assert.equal(replay.games.length, 0);
  assert.equal(Object.hasOwn(replay.of("play-reply")[0].result, "endNs"), false);
  await replay.send({ kind: "play-stop", playId: 7 });
  assertReleased(replay);
});

test("live end preflight, consuming-constructor failures and contradictory actual getters cannot retry an unlimited owner", async () => {
  for (const endNs of [null, 10n, 9n, -1n, 11, "11", 9223372036854775808n]) {
    const initGate = deferred();
    const h = await workerHarness({ initGate });
    await h.send({ kind: "init", canvas: {} });
    await h.send(startRequest({ startNs: 10n, endNs }));
    assert.match(h.of("play-reply")[0].error, /section end/i);
    assert.equal(h.of("ready").length, 0);
    assert.equal(h.preparedOwners.length, 0);
    initGate.resolve(); await flushJobs();
    assert.equal(h.games.length, 0);
    assert.equal(h.sectionConstructions.length, 0);
  }
  for (const options of [
    {}, { gameEnd: 1n }, { gameEnd: 2n, gameEndFrame: 4801n },
    { gameEnd: 1n, gameEndFrame: 4800n }, { gameEnd: 1n, gameEndFrame: 4801 },
    { gameEndGetter() { throw new Error("actual live end getter failed"); } },
    { gameEnd: 1n, gameFrameGetter() { throw new Error("actual live frame getter failed"); } },
  ]) {
    const h = await catalogWorker(options);
    await h.send(startRequest({ endNs: 1n, recordReplay: true }));
    assert.equal(h.sectionConstructions.length, 1);
    assert.equal(h.games.length, 1);
    const game = h.games[0];
    assert.equal(game.calls.length, 0, "metadata admission precedes capture, samples and runtime operations");
    assert.equal(h.of("play-reply").filter(row => row.result?.kind === "prepared").length, 0);
    assert.equal(h.of("play-error").length, 1);
    assertReleased(h);
    assert.deepEqual(game.disposals, ["stop", "free"]);
    assert.equal(h.preparedOwners[1].frees, 0);
    assert.ok(game.endpointReads.end <= 1 && game.endpointReads.frame <= 1);
  }
  for (const missing of [false, true]) {
    const h = await catalogWorker(missing ? { missingSectionConstructor: true } : { sectionConstructError: "section constructor refused" });
    await h.send(startRequest({ endNs: 1n }));
    assert.equal(h.games.length, 0);
    assert.equal(h.sectionConstructions.length, missing ? 0 : 1);
    assert.equal(h.preparedOwners[1].moved, !missing);
    assert.equal(h.preparedOwners[1].frees, missing ? 1 : 0,
      "only preparation not passed to a consuming constructor remains a JS-owned resource");
    assert.equal(h.of("play-error").length, 1);
    assert.match(h.of("play-error")[0].message, missing ? /finite section ownership/ : /section constructor refused/);
    assert.equal(h.libraries[0].preparations.length, 2, "no second gameplay preparation or unlimited fallback");
  }
});

test("prepared ownership, original-rate PCM transfers and setup batches retain their exact identities", async () => {
  const first = batch(9007199254740993n);
  const second = batch(9007199254740994n);
  const h = await started({ batches: [first, second] });
  const game = h.games[0];
  const metadata = h.of("play-reply")[0].result;
  assert.deepEqual(structuredClone(metadata), { kind: "prepared", samples: 2, opponentCount: 0, startNs: 0n,
    title: "Actual prepared metadata", artist: "Fixture", notes: 23, lanes: [0x11, 0x12] });
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

test("each playback owner retains its command batch limit across setup pulls, active pushes and exact acknowledgements", async () => {
  for (const [mode, requested, limit] of [["live", undefined, 256], ["live", 1, 1], ["live", 256, 256], ["replay", 2, 2]]) {
    const request = mode === "replay" ? replayRequest(replayFile().file, { commandBatchLimit: requested })
      : startRequest(requested === undefined ? {} : { commandBatchLimit: requested });
    const first = { sequence: 9007199254740993n, commands: Array.from({ length: Math.min(limit, 3) }, (_, index) => command(BigInt(index + 1))) };
    const next = { sequence: first.sequence + 1n, commands: [command(4n)] };
    const last = { sequence: first.sequence + 2n, commands: [command(5n)] };
    const h = await started({ startRequest: request, batches: [first, null, next, last, null] });
    const game = h.replays[0] ?? h.games[0];
    request.commandBatchLimit = 17;
    assert.deepEqual((await h.rpc("play-commands")).result, first);
    assert.deepEqual(game.calls.filter(row => row[0] === "commands"), [["commands", limit]]);
    await h.rpc("play-ack", { sequence: first.sequence, admitted: first.commands.length, success: true });
    assert.equal(game.calls.filter(row => row[0] === "commands").length, 1);
    assert.equal((await h.rpc("play-commands")).result, null);
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    for (const id of [1, 2]) {
      await h.send(mode === "replay"
        ? { kind: "play-render", playId: 7, renderId: id, report: renderReport({ available: false }), presentedNs: null }
        : step({ tickId: id }));
    }
    assert.deepEqual(h.of("play-commands").map(value => value.batch), [next]);
    assert.equal(game.calls.filter(row => row[0] === "commands").length, 3, "the held batch blocks another pull while real steps continue");
    await h.send({ kind: "play-ack", playId: 7, sequence: next.sequence, admitted: 1, success: true });
    assert.deepEqual(h.of("play-commands").map(value => value.batch), [next, last]);
    await h.send({ kind: "play-ack", playId: 7, sequence: last.sequence, admitted: 1, success: true });
    assert.deepEqual(game.calls.filter(row => row[0] === "commands"), Array.from({ length: 5 }, () => ["commands", limit]));
    assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [
      ["ack", first.sequence, first.commands.length, true], ["ack", next.sequence, 1, true], ["ack", last.sequence, 1, true],
    ]);
    await h.send({ kind: "play-stop", playId: 7 });
    assertReleased(h);
  }
});

test("batch limits reject before readiness and oversized binding results or rejected prefixes never split or retry", async () => {
  for (const value of [null, 0, -1, 257, 1.5, "1", 1n, NaN, Infinity]) {
    const gate = deferred();
    const h = await workerHarness({ initGate: gate });
    await h.send({ kind: "init", canvas: {} });
    const file = replayFile();
    await h.send(replayRequest(file.file, { commandBatchLimit: value }));
    assert.equal(h.of("ready").length, 0);
    assert.match(h.of("play-reply")[0].error, /batch limit/i, "invalid transport limits cannot wait on WASM readiness");
    assert.equal(h.of("play-error").length, 1);
    assert.equal(file.reads, 0);
    assert.equal(h.preparedOwners.length, 0);
    gate.resolve(); await flushJobs();
    assert.equal(h.games.length + h.replays.length, 0);
    assert.equal(h.preparedOwners.length, 0);
  }
  for (const setup of [true, false]) {
    const tooLarge = { sequence: 91n, commands: [command(1n), command(2n), command(3n)] };
    const h = await started({ startRequest: startRequest({ commandBatchLimit: 2 }), batches: [tooLarge] });
    const game = h.games[0];
    if (setup) assert.match((await h.rpc("play-commands")).error, /command batch/i);
    else {
      await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
      await h.send(step());
    }
    assert.match(h.of("play-error")[0].message, /command batch/i);
    assert.equal(h.of("play-commands").length, 0, "no accepted-looking truncated prefix is published");
    assert.deepEqual(game.calls.filter(row => row[0] === "commands"), [["commands", 2]]);
    assert.equal(game.calls.filter(row => row[0] === "ack").length, 0);
    assert.equal(tooLarge.commands.length, 3);
    await h.send(step({ tickId: 2 }));
    assert.equal(game.calls.filter(row => row[0] === "commands").length, 1);
    assertReleased(h);
  }
  const rejectedBatch = batch(9007199254741993n);
  const rejected = await active({ startRequest: startRequest({ commandBatchLimit: 2 }), batches: [rejectedBatch, batch(92n)],
    ack() { throw new Error("actual owner retained rejected prefix"); } });
  await rejected.send(step());
  await rejected.send({ kind: "play-ack", playId: 7, sequence: rejectedBatch.sequence, admitted: 1, success: false });
  const game = rejected.games[0];
  assert.deepEqual(game.calls.filter(row => row[0] === "ack"), [["ack", rejectedBatch.sequence, 1, false]]);
  assert.deepEqual(game.calls.filter(row => row[0] === "commands"), [["commands", 2]]);
  assert.deepEqual(rejected.of("play-commands")[0].batch, rejectedBatch);
  assert.equal(game.batches.length, 1, "no later batch or rejected remainder is consumed");
  assert.match(rejected.of("play-error")[0].message, /retained rejected prefix/);
  assertReleased(rejected);
});

test("remapped physical IDs reach the existing constructor and input path while invalid or incomplete bindings fail before consumption", async () => {
  const remapped = new Uint32Array([0x11, 100, 0x12, 101]);
  const h = await active({ startRequest: startRequest({ keyPairs: remapped }) });
  const game = h.games[0];
  remapped[1] = 2;
  assert.deepEqual(Array.from(game.args[5]), [0x11, 100, 0x12, 101]);
  await h.send(step({ events: [
    { hostNs: ORIGIN, key: 100, down: true, sequence: 1n },
    { hostNs: ORIGIN + 1n, key: 100, down: false, sequence: 2n },
    { hostNs: ORIGIN + 2n, key: 101, down: true, sequence: 3n },
  ], watermark: ORIGIN + 2n }));
  assert.deepEqual(game.calls.filter(call => call[0] === "input"), [
    ["input", ORIGIN, 100, true, 1n, 100000000n],
    ["input", ORIGIN + 1n, 100, false, 2n, 100000000n],
    ["input", ORIGIN + 2n, 101, true, 3n, 100000000n],
  ]);
  await h.send(step({ tickId: 2, events: [
    { hostNs: ORIGIN + 3n, key: 2, down: true, sequence: 4n },
  ], watermark: ORIGIN + 3n }));
  assert.equal(game.calls.filter(call => call[0] === "input").length, 3, "old default key cannot enter the remapped owner");
  assertReleased(h);
  for (const keyPairs of [new Uint32Array([0x11, 100, 0x12, 100]), new Uint32Array([0x11, 0]),
    new Uint32Array([0x11, 65536]), new Uint32Array([0x11]), new Uint32Array([0x10, 100]), new Uint32Array(38)]) {
    const invalid = await catalogWorker();
    await invalid.send(startRequest({ keyPairs }));
    assert.equal(invalid.games.length, 0);
    assert.equal(invalid.libraries[0].preparations.length, 1, "invalid pairs fail before a second chart preparation");
    assert.equal(invalid.of("play-error").length, 1);
  }
  const uncovered = await catalogWorker();
  await uncovered.send(startRequest({ keyPairs: new Uint32Array([0x11, 100]) }));
  assert.equal(uncovered.games.length, 0);
  assert.equal(uncovered.preparedOwners[1].moved, false);
  assert.equal(uncovered.preparedOwners[1].frees, 1);
  const empty = await active({ lanes: [], startRequest: startRequest({ keyPairs: new Uint32Array() }) });
  assert.deepEqual(Array.from(empty.games[0].args[5]), []);
  await empty.send({ kind: "play-stop", playId: 7 });
  assertReleased(empty);
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
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: unavailable, presentedNs: null, presentedHostNs: null });
  assert.deepEqual(game.calls.find(value => value[0] === "output"), ["output", unavailable.words, null]);
  assert.deepEqual(h.of("play-render-done")[0], { kind: "play-render-done", playId: 7, renderId: 1, completed: false });
  const actual = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 2, report: actual,
    presentedNs: 9007199254742999n, presentedHostNs: ORIGIN });
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
  await render.send({ kind: "play-render", playId: 7, renderId: 2, report: renderReport(), presentedNs: null, presentedHostNs: null });
  await render.send({ kind: "play-render", playId: 7, renderId: 2, report: renderReport(), presentedNs: null, presentedHostNs: null });
  assert.equal(render.games[0].calls.filter(value => value[0] === "output").length, 1);
  assert.equal(render.of("play-render-done").length, 1);
  assertReleased(render);
  const unknownSample = renderReport();
  unknownSample.words[36] = 1;
  const terminal = renderReport();
  terminal.words[54] = 1;
  for (const faulty of [renderReport({ start: START + 1n }), unknownSample, terminal]) {
    const h = await active();
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: faulty, presentedNs: null, presentedHostNs: null });
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
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report,
    presentedNs: 9223372036854775807n, presentedHostNs: ORIGIN });
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
    await broken.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
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
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs, presentedHostNs: ORIGIN });
    assert.equal(h.games[0].calls.filter(value => value[0] === "output").length, 0);
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  for (const result of ["true", 1]) {
    const h = await active({ observeOutput() { return result; } });
    await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
    assert.equal(h.of("play-render-done").length, 0);
    assertReleased(h);
  }
  const outstanding = await active({ batches: [batch(81n)], observeOutput() { return true; } });
  await outstanding.send(step());
  await outstanding.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
  assert.equal(outstanding.of("play-render-done").length, 0);
  assertReleased(outstanding);

  const retained = { song_ns: 888n, hits: 21n, misses: 5n, combo: 2n };
  const failed = await active({ observeOutput(game) {
    game.score = { ...retained };
    throw new Error("actual completion rejected the output domain");
  } });
  await failed.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n, presentedHostNs: ORIGIN });
  assert.equal(failed.of("play-render-done").length, 0);
  assert.match(failed.of("play-error")[0].message, /output domain/);
  assert.equal(failed.of("play-error")[0].released, true);
  assert.equal(failed.games[0].calls.filter(value => value[0] === "presentation").length, 0);
  assertReleased(failed, retained);
});

test("paired observations are preflighted together and preserve order before a correlated receipt", async () => {
  const h = await active();
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport({ available: false }),
    presentedNs: null, presentedHostNs: null });
  assert.equal(h.games[0].calls.filter(value => value[0] === "presentation").length, 0);
  const actual = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 2, report: actual,
    presentedNs: 0n, presentedHostNs: ORIGIN - 1n });
  assert.deepEqual(h.games[0].calls.filter(value => ["output", "presentation"].includes(value[0])).slice(-2), [
    ["output", actual.words, 0n], ["presentation", 0n, ORIGIN - 1n],
  ]);
  assert.equal(h.of("play-render-done").at(-1).renderId, 2);
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h);
  for (const [presentedNs, presentedHostNs] of [[null, 1n], [1n, null], [null, undefined],
    [1n, -1n], [1n, 9223372036854775808n], [1n, 1], [1n, "1"]]) {
    const invalid = await active();
    await invalid.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(),
      presentedNs, presentedHostNs });
    assert.equal(invalid.games[0].calls.filter(value => ["output", "presentation"].includes(value[0])).length, 0);
    assert.equal(invalid.of("play-render-done").length, 0);
    assertReleased(invalid);
  }
  const fault = await active({ observePresentation() { throw new Error("actual clock phase bound exceeded"); } });
  await fault.send({ kind: "play-render", playId: 7, renderId: 1, report: actual,
    presentedNs: 1n, presentedHostNs: ORIGIN });
  assert.deepEqual(fault.games[0].calls.filter(value => ["output", "presentation"].includes(value[0])).map(value => value[0]),
    ["output", "presentation"]);
  assert.equal(fault.of("play-render-done").length, 0);
  assert.match(fault.of("play-error")[0].message, /phase bound/);
  assertReleased(fault);
});

test("recording is opt-in before preparation and transfers one stopped-owner prefix without copying its identity", async () => {
  for (const recordReplay of [undefined, false, true]) {
    const h = await active({ recordReplay });
    const game = h.games[0];
    assert.deepEqual(game.calls.filter(row => row[0] === "capture"),
      recordReplay ? [["capture", 64 * 1024 * 1024, 1000000]] : []);
    assert.equal(game.prepared.path, "song/chart.bms");
    assert.equal(h.libraries[0].preparations[1].args[2], 18446744073709551615n);
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-stopped")[0];
    assert.equal(receipt.replayComplete, false);
    assert.equal(receipt.replayError, null);
    assert.equal(game.replayTakes, recordReplay ? 1 : 0);
    assert.deepEqual(game.disposals, recordReplay ? ["stop", "take", "free"] : ["stop", "free"]);
    const transfer = h.transfers[h.messages.indexOf(receipt)];
    if (recordReplay) {
      assert.deepEqual(Array.from(receipt.replay), [66, 75, 82, 255, 0, 1]);
      assert.equal(receipt.replay.byteOffset, 0);
      assert.equal(receipt.replay.byteLength, receipt.replay.buffer.byteLength);
      assert.equal(transfer.length, 1);
      assert.equal(transfer[0], game.replayBytes.buffer);
      assert.equal(game.replayBytes.buffer.byteLength, 0, "owned bytes leave the Worker exactly once");
    } else {
      assert.equal(receipt.replay, null);
      assert.equal(transfer.length, 0);
    }
    await h.send({ kind: "play-stop", playId: 7 });
    assert.equal(h.of("play-stopped").length, 1);
    assertReleased(h);
  }
  for (const recordReplay of [null, 1, "true"]) {
    const h = await catalogWorker();
    await h.send(startRequest({ recordReplay }));
    assert.equal(h.libraries[0].preparations.length, 1, "invalid choice never prepares gameplay");
    assert.equal(h.games.length, 0);
    assert.match(h.of("play-error")[0].message, /recording choice/);
    assert.equal(h.of("play-error")[0].replay, null);
  }
});

test("complete capture requires current actual completion, while stale proof and operation failures retain only prefixes", async () => {
  const natural = await active({ recordReplay: true, observeOutput: () => true });
  await natural.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(),
    presentedNs: 123n, presentedHostNs: ORIGIN });
  await natural.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(natural.of("play-stopped")[0].replayComplete, true);
  assert.equal(natural.of("play-stopped")[0].replayError, null);
  assertReleased(natural);

  for (const invalidation of ["no-proof", "input", "batch", "malformed-choice"]) {
    const h = await active({ recordReplay: true, observeOutput: () => true });
    if (invalidation !== "no-proof") {
      await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(),
        presentedNs: 1n, presentedHostNs: ORIGIN });
    }
    if (invalidation === "batch") h.games[0].batches.push(batch(10n));
    if (invalidation === "input" || invalidation === "batch") {
      await h.send(step({ events: invalidation === "input"
        ? [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }] : [] }));
    }
    await h.send({ kind: "play-stop", playId: 7, completed: invalidation === "malformed-choice" ? 1 : true });
    assert.equal(h.of("play-stopped").length, 0);
    const receipt = h.of("play-error")[0];
    assert.equal(receipt.replayComplete, false);
    assert.equal(receipt.released, true);
    assert.ok(receipt.replay instanceof Uint8Array);
    assertReleased(h);
  }
  const partial = await active({ recordReplay: true, input(game) {
    game.score.hits = 18n;
    throw new Error("committed input capture failed");
  } });
  await partial.send(step({ events: [{ hostNs: ORIGIN, key: 2, down: true, sequence: 1n }] }));
  const receipt = partial.of("play-error")[0];
  assert.match(receipt.message, /committed input capture failed/);
  assert.equal(receipt.replayComplete, false);
  assert.equal(receipt.replayError, null);
  assertReleased(partial, { ...SCORE, hits: 18n });
});

test("serialization and transferable-layout failures stay separate from stop/free ownership failures", async () => {
  for (const options of [
    { replayError: "actual codec byte limit" },
    { replayBytes: () => null },
    { replayBytes: () => new Uint8Array(0) },
    { replayBytes: () => new Uint8Array([1, 2, 3]).subarray(1) },
    { replayBytes: () => new Uint8Array(64 * 1024 * 1024 + 1) },
  ]) {
    const h = await active({ ...options, recordReplay: true });
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-stopped")[0];
    assert.equal(receipt.replay, null);
    assert.equal(receipt.replayComplete, false);
    assert.match(receipt.replayError, options.replayError ? /actual codec byte limit/ : /transferable layout/);
    assert.equal(h.of("play-error").length, 0, "export failure does not invent a cleanup leak");
    assert.equal(h.transfers[h.messages.indexOf(receipt)].length, 0);
    assert.deepEqual(h.games[0].disposals, ["stop", "take", "free"]);
    assertReleased(h);
  }
  for (const fault of ["stopError", "freeError"]) {
    const h = await active({ recordReplay: true, [fault]: "actual owner cleanup failure" });
    await h.send({ kind: "play-stop", playId: 7 });
    const receipt = h.of("play-error")[0];
    assert.equal(receipt.released, false);
    assert.equal(receipt.replayComplete, false);
    assert.equal(receipt.replayError, null);
    assert.equal(h.games[0].replayTakes, fault === "stopError" ? 0 : 1);
    assert.equal(receipt.replay === null, fault === "stopError");
    assertReleased(h);
  }
  const setup = await catalogWorker({ captureError: "actual capture setup limit" });
  await setup.send(startRequest({ recordReplay: true }));
  assert.match(setup.of("play-error")[0].message, /actual capture setup limit/);
  assert.equal(setup.of("play-error")[0].replayComplete, false);
  assert.equal(setup.of("play-error")[0].replayError, null);
  assert.equal(setup.games[0].replayTakes, 0, "a refused capture was never admitted for export");
  assertReleased(setup);
});

test("replay reads once through canonical preparation and shares original PCM, ACK and output owners without live calls", async () => {
  const file = replayFile();
  const first = batch(101n);
  const next = batch(102n);
  const h = await started({ startRequest: replayRequest(file.file, { seed: "not a live seed", keyPairs: null }),
    batches: [first, null, next], observeOutput(game, words, presented) {
      if (presented !== null) game.score = { song_ns: 604800000000001n, hits: 23n, misses: 4n, combo: 11n };
      return false;
    } });
  assert.equal(file.reads, 1);
  assert.equal(h.games.length, 0);
  const replay = h.replays[0];
  const metadata = h.of("play-reply")[0].result;
  assert.equal(metadata.mode, "replay");
  assert.equal(metadata.recordedUntilNs, SCORE.song_ns);
  assert.deepEqual(replay.args, [100000000n]);
  assert.equal(h.libraries[0].preparations.length, 1, "only the accepted preview uses live-seed preparation");
  const preparation = h.libraries[0].replayPreparations[0];
  assert.deepEqual(preparation.bytes, file.bytes);
  assert.deepEqual(preparation.args, [48000, 2, 64 * 1024 * 1024, 256 * 1024 * 1024, 1296]);
  for (const rate of [44100, 96000]) {
    const sample = (await h.rpc("play-sample")).result;
    assert.equal(sample.rate, rate);
    assert.ok(sample.pcm instanceof Float32Array);
  }
  assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
  assert.deepEqual((await h.rpc("play-commands")).result, first);
  await h.rpc("play-ack", { sequence: first.sequence, admitted: 2, success: true });
  assert.equal((await h.rpc("play-commands")).result, null);
  await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
  await h.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport({ available: false }), presentedNs: null });
  assert.deepEqual(h.of("play-commands")[0].batch, next);
  const report = renderReport();
  await h.send({ kind: "play-render", playId: 7, renderId: 2, report, presentedNs: 9007199254742999n,
    presentedHostNs: "not used by replay" });
  assert.deepEqual(replay.calls.filter(row => row[0] === "output").at(-1), ["output", report.words, 9007199254742999n]);
  const progress = h.of("play-render-done").at(-1);
  assert.equal(progress.songNs, 604800000000001n);
  assert.equal(progress.hits, 23n);
  assert.equal(progress.preOriginInputs, 0);
  await h.tick();
  assert.equal(h.views[0].replayDraws.at(-1), replay);
  assert.equal(h.views[0].gameDraws.length, 0);
  await h.send({ kind: "play-ack", playId: 7, sequence: next.sequence, admitted: 2, success: true });
  await h.send({ kind: "play-stop", playId: 7 });
  assertReleased(h, replay.score);
  assert.deepEqual(replay.disposals, ["stop", "free"]);
  const stopped = h.of("play-stopped")[0];
  assert.equal(stopped.replay, null);
  assert.equal(stopped.replayComplete, false);
  assert.equal(stopped.replayError, null);
  assert.equal(file.reads, 1);
});

test("actual replay endpoint getters are read once and finite metadata crosses setup without changing the PCM or replay owner", async () => {
  for (const finite of [false, true]) {
    const selected = replayFile();
    const startNs = 604800000000001n, endNs = startNs + 1n;
    const h = await started({ replayStart: startNs,
      startRequest: replayRequest(selected.file, { rate: 44100 }),
      replayEndGetter(owner) { assert.equal(owner.endpointReads.end, 1); return finite ? endNs : undefined; },
      replayFrameGetter(owner) { assert.equal(owner.endpointReads.frame, 1); return finite ? 4411n : undefined; },
    });
    const owner = h.replays[0], result = h.of("play-reply")[0].result;
    assert.equal(result.startNs, startNs);
    assert.equal(Object.hasOwn(result, "endNs"), finite);
    assert.equal(Object.hasOwn(result, "endFrame"), finite);
    if (finite) {
      assert.equal(result.endNs, endNs);
      assert.equal(result.endFrame, 4411n);
    }
    assert.equal(h.libraries[0].replayPreparations[0].args[0], 44100);
    assert.equal((await h.rpc("play-sample")).result.rate, 44100);
    assert.equal((await h.rpc("play-sample")).result.rate, 96000);
    assert.equal((await h.rpc("play-sample")).result.kind, "samples-end");
    assert.equal((await h.rpc("play-commands")).result, null);
    await h.rpc("play-activate", { hostNs: ORIGIN, startFrame: START });
    await h.send({ kind: "play-stop", playId: 7 });
    assertReleased(h);
    assert.deepEqual(owner.endpointReads, { end: 1, frame: 1 });
    assert.deepEqual(owner.disposals, ["stop", "free"]);
    assert.equal(selected.reads, 1);
  }
});

test("malformed or throwing replay endpoint getters fail the consumed owner before samples and never fall back to unlimited setup", async () => {
  for (const options of [
    { replayEnd: 1n }, { replayEndFrame: 4801n }, { replayEnd: null, replayEndFrame: null },
    { replayEnd: 0n, replayEndFrame: 4800n }, { replayEnd: 1n, replayEndFrame: 4800n },
    { replayEnd: 1n, replayEndFrame: 4801 },
    { replayEndGetter() { throw new Error("actual end getter failed"); } },
    { replayEnd: 1n, replayFrameGetter() { throw new Error("actual frame getter failed"); } },
  ]) {
    const h = await catalogWorker(options);
    await h.send(replayRequest(replayFile().file));
    assert.equal(h.replays.length, 1);
    const owner = h.replays[0];
    assert.equal(owner.calls.filter(row => ["sample", "commands", "output"].includes(row[0])).length, 0);
    assert.equal(h.of("play-reply").filter(reply => reply.result?.kind === "prepared").length, 0);
    assert.equal(h.of("play-error").length, 1);
    assert.equal(h.of("play-error")[0].released, true);
    assertReleased(h);
    assert.equal(h.preparedOwners[1].moved, true);
    assert.equal(h.preparedOwners[1].frees, 0, "the consuming replay constructor already owns preparation");
    assert.ok(owner.endpointReads.end <= 1 && owner.endpointReads.frame <= 1);
    await h.send({ kind: "play-stop", playId: 7 });
    assert.deepEqual(owner.disposals, ["stop", "free"]);
  }
});

test("replay metadata is bounded before acquisition and invalid or changed reads never reach WASM preparation", async () => {
  for (const size of [0, -1, 1.5, 64 * 1024 * 1024 + 1, Number.MAX_SAFE_INTEGER + 1]) {
    const file = replayFile(null, size);
    const h = await catalogWorker();
    await h.send(replayRequest(file.file));
    assert.equal(file.reads, 0);
    assert.equal(h.libraries[0].replayPreparations.length, 0);
    assert.equal(h.replays.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const fields of [{ replayFile: { size: 6, arrayBuffer() { assert.fail("unbranded file read"); } } },
    { recordReplay: true }, { mode: "unknown" }]) {
    const file = replayFile();
    const h = await catalogWorker();
    await h.send(replayRequest(file.file, fields));
    assert.equal(file.reads, 0);
    assert.equal(h.replays.length, 0);
    assert.equal(h.of("play-error").length, 1);
  }
  for (const acquire of [() => Promise.resolve(new ArrayBuffer(5)),
    () => Promise.resolve(new Uint8Array(6)), () => Promise.reject(new Error("actual file acquisition failed"))]) {
    const file = replayFile(acquire);
    const h = await catalogWorker();
    await h.send(replayRequest(file.file));
    assert.equal(file.reads, 1);
    assert.equal(h.libraries[0].replayPreparations.length, 0);
    assert.equal(h.replays.length, 0);
    assert.equal(h.of("play-error")[0].released, true);
  }
  const file = replayFile();
  const incompatible = await catalogWorker({ prepareReplayError: "canonical chart identity mismatch" });
  await incompatible.send(replayRequest(file.file));
  assert.equal(file.reads, 1);
  assert.equal(incompatible.libraries[0].replayPreparations.length, 1);
  assert.equal(incompatible.replays.length, 0);
  assert.match(incompatible.of("play-error")[0].message, /canonical chart identity mismatch/);
});

test("a cancelled replay read cannot construct a late owner or replace a newer live session", async () => {
  const gate = deferred();
  const file = replayFile(() => gate.promise);
  const h = await catalogWorker();
  await h.send(replayRequest(file.file));
  assert.equal(file.reads, 1);
  assert.equal(h.replays.length, 0);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(h.of("play-stopped")[0].songNs, null);
  assert.equal(h.of("play-stopped")[0].replay, null);
  await h.send(startRequest({ playId: 8 }));
  assert.equal(h.games.length, 1);
  gate.resolve(file.bytes.slice().buffer);
  await flushJobs();
  assert.equal(h.replays.length, 0);
  assert.equal(h.libraries[0].replayPreparations.length, 0);
  assert.equal(h.games[0].frees, 0);
  assert.equal(h.of("play-error").length, 0);
  await h.send({ kind: "play-stop", playId: 8 });
  assertReleased(h);
  assert.equal(file.reads, 1);
});

test("replay rejects live steps and preserves output/ACK failures without inventing a completed capture", async () => {
  const start = () => replayRequest(replayFile().file);
  const natural = await active({ startRequest: start(), observeOutput: () => true, recordedUntil: null });
  assert.equal(natural.of("play-reply")[0].result.recordedUntilNs, null);
  await natural.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
  assert.equal(natural.of("play-render-done")[0].completed, true);
  await natural.send({ kind: "play-stop", playId: 7, completed: true });
  assert.equal(natural.of("play-stopped")[0].replayComplete, false);
  assertReleased(natural);
  for (const request of [step(), { kind: "play-render", playId: 7, renderId: 1,
    report: renderReport(), presentedNs: -1n }]) {
    const h = await active({ startRequest: start() });
    await h.send(request);
    assert.equal(h.replays[0].calls.filter(row => ["input", "advance", "output"].includes(row[0])).length, 0);
    assert.equal(h.of("play-error")[0].replay, null);
    assertReleased(h);
  }
  const malformed = await active({ startRequest: start(), observeOutput: () => "true" });
  await malformed.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: 1n });
  assert.equal(malformed.of("play-render-done").length, 0);
  assertReleased(malformed);
  const rejected = await active({ startRequest: start(), batches: [batch(83n)], ack() {
    throw new Error("actual replay batch rejected after one command");
  } });
  await rejected.send({ kind: "play-render", playId: 7, renderId: 1, report: renderReport(), presentedNs: null });
  await rejected.send({ kind: "play-ack", playId: 7, sequence: 83n, admitted: 1, success: false });
  assert.deepEqual(rejected.replays[0].calls.filter(row => row[0] === "ack"), [["ack", 83n, 1, false]]);
  assert.equal(rejected.replays[0].calls.filter(row => row[0] === "commands").length, 1);
  assert.match(rejected.of("play-error")[0].message, /after one command/);
  assert.equal(rejected.of("play-error")[0].replay, null);
  assertReleased(rejected);
});

const multiplayer = () => ({ url: "https://example.test:4433/competition", host: true, windowOriginNs: 9000000000n });

async function preparedNetwork(options = {}) {
  const h = await started({ ...options, allowNetworkClock: true,
    startRequest: startRequest({ multiplayer: multiplayer() }) });
  while ((await h.rpc("play-sample")).result.kind !== "samples-end") {}
  for (;;) {
    const value = (await h.rpc("play-commands")).result;
    if (value === null) break;
    await h.rpc("play-ack", { sequence: value.sequence, admitted: value.commands.length, success: true });
  }
  return h;
}

async function requestNetwork(h) {
  const rpcId = ++h.rpcId;
  await h.send({ kind: "play-network-ready", playId: 7, rpcId });
  return rpcId;
}

async function activeNetwork(options = {}) {
  const h = await preparedNetwork(options);
  const rpcId = await requestNetwork(h);
  const owner = h.networks[0];
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 4n });
  await flushJobs();
  const reply = h.of("play-reply").find(value => value.rpcId === rpcId);
  assert.equal(reply.result.targetHostNs, 2500000000n);
  h.networkOrigin = 2500000100n;
  const activated = await h.rpc("play-activate", { hostNs: h.networkOrigin,
    startFrame: 123456n, targetHostNs: reply.result.targetHostNs });
  assert.equal(activated.result, null);
  return h;
}

test("multiplayer identity and readiness follow sample exhaustion and actual initial command acknowledgements", async () => {
  const early = await started({ allowNetworkClock: true, startRequest: startRequest({ multiplayer: multiplayer() }) });
  assert.equal(early.networks.length, 0);
  assert.equal(early.games[0].calls.filter(call => call[0] === "identity").length, 0);
  const refused = await early.rpc("play-network-ready");
  assert.match(refused.error, /preparation|sample|readiness/i);
  assert.equal(early.networkSessions.length, 0);
  assertReleased(early);

  const opening = deferred();
  const h = await preparedNetwork({ batches: [batch(31n)], networkOpenGate: opening });
  assert.equal(h.networks.length, 0);
  const game = h.games[0];
  const rpcId = await requestNetwork(h);
  const owner = h.networks[0];
  assert.deepEqual(h.networkSessions[0].identity, [66, 75, 82, 0, 255]);
  assert.equal(h.networkSessions[0].host, true);
  assert.equal(h.networkSessions[0].preroll, 100000000n);
  assert.equal(owner.origin, 11000000000n, "Worker timeOrigin and performance.now retain their independent epoch");
  assert.equal(owner.readyCalls, 0, "pending transport open is not local readiness");
  assert.equal(h.of("play-reply").filter(value => value.rpcId === rpcId).length, 0);
  assert.ok(game.calls.findIndex(call => call[0] === "identity") > game.calls.findIndex(call => call[0] === "ack"));
  opening.resolve(); await flushJobs();
  assert.equal(owner.readyCalls, 1);
  owner.emit({ kind: "connected" }); owner.emit({ kind: "ready" });
  assert.equal(h.of("play-reply").filter(value => value.rpcId === rpcId).length, 0);
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 7n });
  await flushJobs();
  assert.deepEqual(h.of("play-reply").find(value => value.rpcId === rpcId).result,
    { kind: "multiplayer-start", targetHostNs: 2500000000n, songTargetHostNs: 2600000000n, uncertaintyNs: 7n });
  const badActivation = await h.rpc("play-activate", { hostNs: 2500000000n,
    startFrame: 123456n, targetHostNs: 2500000001n });
  assert.ok(badActivation.error);
  assert.equal(game.calls.filter(call => call[0] === "activate").length, 0);
  assert.equal(owner.closes, 1);
  assert.equal(h.networkSessions[0].frees, 1);
  assertReleased(h);
});

test("actual score cadence stays bounded and remote or disconnected state never replaces local gameplay", async () => {
  const h = await activeNetwork();
  const game = h.games[0]; const owner = h.networks[0];
  h.setNetworkNow(1600);
  await h.send(step({ watermark: 2600000000n }));
  assert.equal(owner.submissions.length, 1);
  assert.deepEqual(owner.submissions[0].value, { songNs: SCORE.song_ns, hits: SCORE.hits,
    misses: SCORE.misses, combo: SCORE.combo, maxCombo: SCORE.max_combo });
  assert.equal(owner.submissions[0].final, false);
  h.setNetworkNow(1850);
  await h.send(step({ tickId: 2, watermark: 2850000000n }));
  assert.equal(owner.submissions.length, 1, "one pending write prevents an application queue from growing");
  owner.submissions[0].gate.resolve(); await flushJobs();
  game.score.hits = 18n; game.score.combo = 10n;
  h.setNetworkNow(1851);
  await h.send(step({ tickId: 3, watermark: 2851000000n }));
  assert.equal(owner.submissions.length, 2);
  assert.equal(owner.submissions[1].value.hits, 18n);
  owner.submissions[1].gate.resolve(); await flushJobs();
  h.setNetworkNow(2100);
  await h.send(step({ tickId: 4, watermark: 3100000000n }));
  assert.equal(owner.submissions.length, 2, "249 ms does not satisfy the score cadence");
  const peer = { songNs: -1n, hits: 18446744073709551615n, misses: 0n, combo: 0n, maxCombo: 1n };
  owner.emit({ kind: "progress", ...peer });
  owner.emit({ kind: "progress", ...peer, songNs: 0n });
  assert.equal(h.of("play-multiplayer").filter(value => value.event.kind === "progress").length, 1);
  owner.emit({ kind: "final-progress", ...peer, songNs: 1n });
  assert.equal(h.of("play-multiplayer").at(-1).event.kind, "final-progress");
  assert.equal(game.score.hits, 18n);
  owner.disconnect(new Error("actual transport lost"));
  assert.match(h.of("play-multiplayer").at(-1).event.error, /transport lost/);
  assert.equal(game.stops, 0);
  await h.send(step({ tickId: 5, watermark: 3100000001n }));
  assert.equal(h.of("play-step-done").at(-1).tickId, 5);
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(h.of("play-stopped").at(-1).multiplayer.finalWritten, false);
  assertReleased(h, game.score);
});

test("stop frees gameplay immediately but reports final write and peer ACK as separate bounded receipts", async () => {
  const h = await activeNetwork(); const owner = h.networks[0];
  h.setNetworkNow(1600);
  await h.send(step({ watermark: 2600000000n }));
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(h.games[0].stops, 1); assert.equal(h.games[0].frees, 1);
  assert.equal(owner.submissions.length, 1);
  assert.equal(owner.config.signal.aborted, false, "network final drain owns a separate lifetime after local game release");
  assert.equal(h.of("play-stopped").length, 0);
  owner.submissions[0].gate.resolve(); await flushJobs();
  assert.equal(owner.submissions.length, 2);
  assert.equal(owner.submissions[1].final, true);
  assert.equal(owner.submissions[1].value.maxCombo, SCORE.max_combo);
  owner.submissions[1].gate.resolve(); await flushJobs();
  assert.equal(owner.ackCalls, 1);
  assert.equal(h.of("play-stopped").length, 0, "local write alone cannot claim final application ACK");
  owner.emit({ kind: "final-acknowledged" }); owner.ack.resolve(); await flushJobs();
  assert.deepEqual(h.of("play-stopped")[0].multiplayer, { finalWritten: true, finalAcknowledged: true, error: null });
  assert.equal(owner.config.signal.aborted, true);
  assert.equal(owner.closes, 1); assert.equal(h.networkSessions[0].frees, 1);
  assertReleased(h);

  for (const wrote of [false, true]) {
    const stalled = await activeNetwork(); const old = stalled.networks[0];
    await stalled.send({ kind: "play-stop", playId: 7 });
    if (wrote) { old.submissions[0].gate.resolve(); await flushJobs(); }
    await stalled.expireNetwork();
    const receipt = stalled.of("play-stopped")[0];
    assert.equal(receipt.multiplayer.finalWritten, wrote);
    assert.equal(receipt.multiplayer.finalAcknowledged, false);
    assert.match(receipt.multiplayer.error, /2 seconds|timed out/i);
    await stalled.send(startRequest({ playId: 8 }));
    const newer = stalled.games[1];
    const messageCount = stalled.messages.length;
    old.submissions[0].gate.resolve(); old.ack.resolve();
    old.emit({ kind: "final-acknowledged" }); await flushJobs();
    assert.equal(stalled.messages.length, messageCount, "late final evidence cannot publish a second old-session receipt");
    assert.equal(newer.stops, 0);
    assert.equal(old.closes, 1); assert.equal(stalled.networkSessions[0].frees, 1);
    await stalled.send({ kind: "play-stop", playId: 8 });
  }
});

test("pre-activation failure and cancelled late network opens release identity and gameplay exactly once", async () => {
  for (const options of [{ identityError: "identity refused" }, { networkOpenError: "server refused" }]) {
    const h = await preparedNetwork(options);
    const rpcId = await requestNetwork(h);
    assert.ok(h.of("play-reply").find(value => value.rpcId === rpcId).error);
    assert.equal(h.of("play-error").length, 1);
    assertReleased(h);
    if (h.networkSessions.length) assert.equal(h.networkSessions[0].frees, 1);
  }
  const opening = deferred();
  const h = await preparedNetwork({ networkOpenGate: opening });
  const rpcId = await requestNetwork(h); const owner = h.networks[0];
  await h.send({ kind: "play-stop", playId: 7 });
  assert.equal(owner.config.signal.aborted, true);
  assert.ok(h.of("play-reply").find(value => value.rpcId === rpcId).error);
  assertReleased(h);
  await h.send(startRequest({ playId: 8 }));
  opening.resolve(); await flushJobs();
  assert.equal(owner.readyCalls, 0);
  assert.equal(owner.closes, 1);
  assert.equal(h.networkSessions[0].closes, 1);
  assert.equal(h.networkSessions[0].frees, 1);
  const messages = h.messages.length;
  owner.emit({ kind: "start", targetNs: 500000000n, songTargetNs: 600000000n, uncertaintyNs: 4n });
  assert.equal(h.messages.length, messages);
  assert.equal(h.games[1].stops, 0);
  await h.send({ kind: "play-stop", playId: 8 });
});

import init, { BrowserGame, BrowserLibrary, BrowserView } from "./pkg/beatkernel_bms_runtime.js";
import { LIMITS, preflight, previewNanos } from "./host_model.mjs";
import { bindingsFor, renderedCursor } from "./play-model.mjs";
let ready = null;
let view = null;
let library = null;
let stagedLibrary = null;
let libraryId = 0;
let importGeneration = 0;
let importing = false;
let pendingImport = null;
let importPumpRunning = false;
let selectedId = 0;
let redraw = null;
let retries = 0;
let failed = false;
let extent = [0, 0];
let play = null;
let lastPlayId = 0;
const I64_MAX = 9223372036854775807n;
const U64_MAX = 18446744073709551615n;

function report(kind, fields = {}, transfer = []) { self.postMessage({ kind, ...fields }, transfer); }
function message(error) { return String(error?.message ?? error).slice(0, 4096); }
function stopRedraw() {
  if (redraw === null) return;
  if (redraw.animation) self.cancelAnimationFrame(redraw.id);
  else clearTimeout(redraw.id);
  redraw = null;
}
function fatal(error) {
  if (failed) return;
  failed = true;
  if (play) failPlay(play, error);
  stopRedraw();
  report("fatal", { message: message(error) });
  // Main terminates this owner. Do not try to reuse potentially failed GPU state.
}

function scheduleDraw(reset = true) {
  if (failed || !view || (!selectedId && !play?.game) || extent.includes(0)) return;
  if (reset) retries = 0;
  if (redraw !== null) return;
  const draw = () => {
    redraw = null;
    try {
      if (play?.game) view.draw_game(play.game);
      else view.draw();
      if (view.needs_redraw()) {
        if (++retries <= 3) scheduleDraw(false);
        else report("render-wait", { selectedId, ...(play ? { playId: play.id } : {}) });
      } else report("drawn", { selectedId, ...(play ? { playId: play.id } : {}) });
    } catch (error) { fatal(error); }
  };
  // Only schedules presentation. Its timestamp is never a song/audio clock.
  if (typeof self.requestAnimationFrame === "function") {
    try {
      redraw = { animation: true, id: self.requestAnimationFrame(draw) };
      return;
    } catch (error) {
      if (error?.name !== "NotSupportedError") return fatal(error);
    }
  }
  redraw = { animation: false, id: setTimeout(draw, 16) };
}

async function importFiles(request, generation) {
  let candidate = null;
  try {
    // Metadata is admitted before any selected-file arrayBuffer acquisition.
    const entries = preflight(request.files);
    await ready;
    if (generation !== importGeneration || failed) return;
    candidate = new BrowserLibrary(LIMITS.files, LIMITS.file, LIMITS.total, LIMITS.path);
    for (let index = 0; index < entries.length; index++) {
      const { file, path } = entries[index];
      const bytes = await file.arrayBuffer();
      if (generation !== importGeneration || failed) return;
      if (bytes.byteLength !== file.size) throw new Error("Selected-file size changed during import.");
      candidate.add_file(path, new Uint8Array(bytes));
      if (index % 16 === 0) report("import-progress", { id: request.id, read: index + 1, total: entries.length });
    }
    const charts = candidate.chart_paths();
    if (!charts.length) throw new Error("No BMS, BME, BML or PMS chart was selected.");
    stagedLibrary = { library: candidate, id: request.id, generation };
    candidate = null;
    // Main may already have requested a newer import before receiving this.
    // Keep the accepted library until the current catalog is acknowledged.
    report("catalog", { id: request.id, charts });
  } catch (error) {
    if (generation === importGeneration && !failed) report("import-error", { id: request.id, message: message(error) });
  } finally {
    candidate?.free();
  }
}

function queueImport(request) {
  stagedLibrary?.library.free();
  stagedLibrary = null;
  pendingImport = { request, generation: ++importGeneration };
  importing = true;
  if (!importPumpRunning) void drainImports().catch(fatal);
}

function acceptLibrary(request) {
  if (!stagedLibrary || request.id !== stagedLibrary.id || stagedLibrary.generation !== importGeneration) return;
  const previous = library;
  library = stagedLibrary.library;
  libraryId = stagedLibrary.id;
  stagedLibrary = null;
  previous?.free();
}

async function drainImports() {
  importPumpRunning = true;
  try {
    while (pendingImport && !failed) {
      const { request, generation } = pendingImport;
      pendingImport = null;
      await importFiles(request, generation);
    }
  } finally {
    // At most one unabortable read/candidate and one latest metadata request.
    importPumpRunning = false;
    importing = false;
    if (failed) pendingImport = null;
  }
}

async function selectChart(request) {
  const generation = importGeneration;
  await ready;
  if (failed || generation !== importGeneration) return;
  let prepared = null;
  try {
    if (play) throw new Error("Stop gameplay before changing the preview chart.");
    if (importing || !library || request.libraryId !== libraryId) throw new Error("Wait for the selected library to finish loading.");
    if (!Number.isInteger(request.rate) || request.rate < 1 || request.rate > 0xffffffff) throw new Error("Sample rate must be a positive 32-bit integer.");
    if (typeof request.seed !== "string" || !/^\d{1,20}$/.test(request.seed) || BigInt(request.seed) > 0xffffffffffffffffn) throw new Error("Chart seed must fit an unsigned 64-bit integer.");
    prepared = library.prepare_chart(request.path, request.rate, 2, BigInt(request.seed), 64 * 1024 * 1024, 256 * 1024 * 1024, 1296);
    const metadata = {
      title: prepared.title, artist: prepared.artist, duration: prepared.duration_ns.toString(),
      notes: prepared.note_count, samples: prepared.sample_count, images: prepared.image_count,
    };
    view.set_chart(prepared);
    prepared = null; // Ownership moved into Rust; calling free would double-release.
    selectedId = request.id;
    report("selected", { id: request.id, libraryId, path: request.path, ...metadata });
    scheduleDraw();
  } catch (error) {
    report("selection-error", { id: request.id, message: message(error) });
  } finally { prepared?.free(); }
}

async function seek(request) {
  await ready;
  if (failed || request.selectedId !== selectedId) return;
  try {
    if (play) throw new Error("Stop gameplay before seeking the preview.");
    const ns = previewNanos(request.ns);
    view.seek(ns);
    report("position", { id: request.id, selectedId, ns: ns.toString() });
    scheduleDraw();
  } catch (error) { report("seek-error", { id: request.id, selectedId, message: message(error) }); }
}

async function resize(request) {
  if (![request.width, request.height].every(value => Number.isInteger(value) && value >= 0 && value <= 0xffffffff)) throw new Error("Invalid canvas extent.");
  extent = [request.width, request.height];
  await ready;
  if (failed) return;
  view.resize(...extent);
  if (extent.includes(0)) stopRedraw();
  else scheduleDraw();
}

function integer(value, minimum, maximum) {
  return Number.isSafeInteger(value) && value >= minimum && value <= maximum;
}
function identity(value) { return integer(value, 1, Number.MAX_SAFE_INTEGER); }
function unsigned(value) { return typeof value === "bigint" && value >= 0n && value <= U64_MAX; }
function hostTime(value) { return typeof value === "bigint" && value >= 0n && value <= I64_MAX; }
function signed(value) { return typeof value === "bigint" && value >= -I64_MAX - 1n && value <= I64_MAX; }

function statistics(state) {
  const result = { songNs: null, hits: null, misses: null, combo: null, preOriginInputs: state.preOriginInputs };
  if (state.game) {
    for (const [field, getter] of [["songNs", "song_ns"], ["hits", "hits"], ["misses", "misses"], ["combo", "combo"]]) {
      // Preserve every readable actual field even if a terminal binding fault
      // makes another getter unavailable. Null never pretends to be a zero score.
      try {
        const value = state.game[getter];
        if (typeof value === "bigint") result[field] = value;
      } catch {}
    }
  }
  return result;
}

function disposeGame(state) {
  const game = state.game;
  state.game = null;
  if (!game) return null;
  let error = null;
  try { game.stop(); } catch (cause) { error = cause; }
  try { game.free(); } catch (cause) { error ??= cause; }
  return error;
}

function failPlay(state, error, request = null) {
  if (play !== state) return;
  const score = statistics(state);
  play = null; // Invalidates a still-awaiting preparation before releasing owners.
  stopRedraw();
  const cleanupError = disposeGame(state);
  const text = message(cleanupError ? `${message(error)}; cleanup: ${message(cleanupError)}` : error);
  const rpcId = request?.rpcId;
  if (identity(rpcId)) report("play-reply", { playId: state.id, rpcId, error: text });
  if (identity(state.startRpcId) && state.startRpcId !== rpcId) {
    report("play-reply", { playId: state.id, rpcId: state.startRpcId, error: text });
  }
  report("play-error", { playId: state.id, message: text, released: cleanupError === null, ...score });
  scheduleDraw();
}

function stopPlay(state) {
  const score = statistics(state);
  play = null;
  stopRedraw();
  const error = disposeGame(state);
  if (state.startRpcId !== null) {
    report("play-reply", { playId: state.id, rpcId: state.startRpcId, error: "Gameplay setup was stopped." });
  }
  if (error) report("play-error", { playId: state.id, message: message(error), released: false, ...score });
  else report("play-stopped", { playId: state.id, ...score });
  scheduleDraw();
}

function rpc(state, request, required) {
  const id = request.rpcId;
  if (id === undefined && !required) return;
  if (!identity(id) || id <= state.lastRpc) throw new Error("Gameplay RPC identity must increase without retry.");
  state.lastRpc = id;
}

function reply(state, request, result, transfer = []) {
  report("play-reply", { playId: state.id, rpcId: request.rpcId, result }, transfer);
}

async function preparePlay(state, request) {
  let prepared = null;
  try {
    rpc(state, request, true);
    await ready;
    if (failed || play !== state) return;
    if (importing || importPumpRunning || pendingImport || stagedLibrary
      || !library || request.libraryId !== libraryId) throw new Error("Wait for the accepted library before starting gameplay.");
    if (typeof request.path !== "string" || !request.path.length
      || !integer(request.rate, 1, 0xffffffff)
      || typeof request.seed !== "string" || !/^\d{1,20}$/.test(request.seed) || BigInt(request.seed) > U64_MAX) {
      throw new Error("Invalid gameplay chart, sample rate or seed.");
    }
    if (!(request.keyPairs instanceof Uint32Array) || request.keyPairs.length > 36 || request.keyPairs.length % 2 !== 0) {
      throw new Error("Invalid bounded gameplay key bindings.");
    }
    const pairs = request.keyPairs.slice();
    const lanes = [];
    const keys = new Set();
    for (let index = 0; index < pairs.length; index += 2) {
      lanes.push(pairs[index]);
      if (!integer(pairs[index + 1], 1, 65535) || keys.has(pairs[index + 1])) throw new Error("Gameplay keys must be valid and unique.");
      keys.add(pairs[index + 1]);
    }
    bindingsFor(lanes);
    prepared = library.prepare_chart(request.path, request.rate, 2, BigInt(request.seed), 64 * 1024 * 1024, 256 * 1024 * 1024, 1296);
    const chartLanes = Array.from(prepared.lanes);
    bindingsFor(chartLanes);
    if (chartLanes.some(lane => !lanes.includes(lane))) throw new Error("A prepared lane has no supplied key binding.");
    const metadata = { title: prepared.title, artist: prepared.artist, notes: prepared.note_count, lanes: chartLanes };
    const moved = prepared;
    prepared = null; // A consuming Rust constructor also owns the argument on Err.
    state.game = new BrowserGame(moved, 0n, 100000000n, 50000000n, 50000000n, 0n, pairs);
    state.keys = keys;
    reply(state, request, { kind: "prepared", samples: state.game.sample_count(), ...metadata });
    state.startRpcId = null;
    scheduleDraw();
  } catch (error) {
    if (play === state) failPlay(state, error, request);
  } finally { prepared?.free(); }
}

function samplePlay(state, request) {
  if (state.active) throw new Error("Samples are setup-only resources.");
  const sample = state.game.next_sample();
  if (sample == null) { reply(state, request, { kind: "samples-end" }); return; }
  let result = null;
  let failure = null;
  try {
    const id = sample.id;
    const rate = sample.rate;
    const channels = sample.channels;
    const pcm = sample.take_pcm();
    if (!unsigned(id) || !integer(rate, 1, 0xffffffff) || channels !== 2
      || !(pcm instanceof Float32Array) || !(pcm.buffer instanceof ArrayBuffer)
      || pcm.byteOffset !== 0 || pcm.byteLength !== pcm.buffer.byteLength || pcm.length % channels !== 0) {
      throw new Error("Prepared sample has an invalid transferable layout.");
    }
    result = { kind: "sample", id, rate, channels, pcm };
  } catch (error) { failure = error; }
  try { sample.free(); } catch (error) { failure ??= error; }
  if (failure) throw failure;
  reply(state, request, result, [result.pcm.buffer]);
}

function commandBatch(state) {
  const batch = state.game.commands(256);
  if (batch === null) return null;
  if (!batch || !unsigned(batch.sequence) || batch.sequence === 0n || !Array.isArray(batch.commands)
    || !integer(batch.commands.length, 1, 256)) throw new Error("Invalid actual gameplay command batch.");
  for (const command of batch.commands) {
    if (!command || !integer(command.kind, 0, 3) || !unsigned(command.voice) || !unsigned(command.sample)
      || !signed(command.at) || typeof command.gain !== "number" || !Number.isFinite(command.gain)
      || !Number.isFinite(Math.fround(command.gain)) || !signed(command.value) || !unsigned(command.denominator)) {
      throw new Error("Invalid actual gameplay command fields.");
    }
  }
  state.batch = { sequence: batch.sequence, count: batch.commands.length };
  return batch;
}

function pumpCommands(state) {
  if (!state.active || state.batch !== null) return;
  const batch = commandBatch(state);
  if (batch !== null) report("play-commands", { playId: state.id, batch });
}

function stepPlay(state, request) {
  if (!state.active || !identity(request.tickId) || request.tickId <= state.lastTick
    || !Array.isArray(request.events) || request.events.length > 256 || !hostTime(request.audioNs)
    || !(request.watermark === null || hostTime(request.watermark))) throw new Error("Invalid active gameplay step.");
  let host = state.lastHost;
  let sequence = state.lastSequence;
  let ignored = 0;
  // Validate the complete bounded batch before the first actual Runtime call.
  for (const event of request.events) {
    if (!event || !hostTime(event.hostNs) || !integer(event.key, 1, 65535) || !state.keys.has(event.key)
      || typeof event.down !== "boolean" || !unsigned(event.sequence)
      || (host !== null && event.hostNs < host) || (sequence !== null && event.sequence < sequence)) {
      throw new Error("Invalid gameplay input or source chronology.");
    }
    host = event.hostNs;
    sequence = event.sequence;
    if (host < state.origin) ignored++;
  }
  if (request.watermark !== null && host !== null && request.watermark < host) throw new Error("Gameplay watermark precedes its input prefix.");
  if (!Number.isSafeInteger(state.preOriginInputs + ignored)) throw new Error("Pre-origin input count overflow.");
  state.lastTick = request.tickId;
  for (const event of request.events) {
    if (event.hostNs < state.origin) state.preOriginInputs++;
    else state.game.input(event.hostNs, event.key, event.down, event.sequence, request.audioNs);
    state.lastHost = event.hostNs;
    state.lastSequence = event.sequence;
  }
  if (request.watermark !== null) {
    if (request.watermark >= state.origin) state.game.advance(request.watermark, request.audioNs);
    state.lastHost = request.watermark;
  }
  report("play-step-done", { playId: state.id, tickId: request.tickId, ...statistics(state) });
  scheduleDraw();
  pumpCommands(state);
}

function handlePlay(request) {
  if (!identity(request.playId)) {
    if (play) failPlay(play, new Error("Invalid gameplay identity."), request);
    return;
  }
  if (request.kind === "play-start" && play === null) {
    if (request.playId <= lastPlayId) return;
    const state = {
      id: request.playId, startRpcId: identity(request.rpcId) ? request.rpcId : null,
      game: null, keys: null, active: false, origin: null, startFrame: null,
      batch: null, lastRpc: 0, lastTick: 0, lastRender: 0,
      lastHost: null, lastSequence: null, preOriginInputs: 0,
    };
    play = state; // Reserve before the ready await so stop cannot race a late owner.
    lastPlayId = state.id;
    void preparePlay(state, request).catch(fatal);
    return;
  }
  const state = play;
  if (!state || request.playId !== state.id) return;
  try {
    if (request.kind === "play-stop") { stopPlay(state); return; }
    if (request.kind === "play-start") throw new Error("Gameplay setup is already owned by this identity.");
    const requiresRpc = ["play-sample", "play-commands", "play-activate"].includes(request.kind);
    if (request.rpcId !== undefined && !requiresRpc && request.kind !== "play-ack") throw new Error("Unexpected gameplay RPC identity.");
    rpc(state, request, requiresRpc);
    if (!state.game) throw new Error("Wait for actual gameplay preparation.");
    if (request.kind === "play-sample") samplePlay(state, request);
    else if (request.kind === "play-commands") {
      if (state.batch !== null) throw new Error("An actual audio batch is still awaiting acknowledgement.");
      reply(state, request, commandBatch(state));
    } else if (request.kind === "play-ack") {
      if (!unsigned(request.sequence) || !integer(request.admitted, 0, 0xffffffff) || typeof request.success !== "boolean") throw new Error("Invalid audio acknowledgement fields.");
      // The actual core validates correlation/full success/rejected prefix and
      // retains its original batch on error. Never substitute or replay a prefix.
      state.game.acknowledge(request.sequence, request.admitted, request.success);
      state.batch = null;
      if (request.rpcId !== undefined) reply(state, request, null);
      pumpCommands(state);
    } else if (request.kind === "play-activate") {
      if (state.active || !hostTime(request.hostNs) || !unsigned(request.startFrame)) throw new Error("Invalid or repeated gameplay activation.");
      state.game.activate(request.hostNs);
      state.origin = request.hostNs;
      state.startFrame = request.startFrame;
      state.active = true;
      reply(state, request, null);
    } else if (request.kind === "play-step") stepPlay(state, request);
    else if (request.kind === "play-render") {
      if (!state.active || !identity(request.renderId) || request.renderId <= state.lastRender) throw new Error("Invalid rendered-report identity or state.");
      if (!(request.presentedNs === null || hostTime(request.presentedNs))) throw new Error("Invalid output presentation point.");
      renderedCursor(request.report, state.startFrame);
      const completed = state.game.observe_output(request.report.words, request.presentedNs);
      if (typeof completed !== "boolean" || (completed && state.batch !== null)) throw new Error("Invalid completion with outstanding gameplay commands.");
      state.lastRender = request.renderId;
      report("play-render-done", { playId: state.id, renderId: request.renderId, completed });
      pumpCommands(state);
    } else throw new Error("Unknown gameplay request.");
  } catch (error) { failPlay(state, error, request); }
}

self.addEventListener("message", event => {
  if (failed) return;
  const request = event.data;
  if (!request || typeof request !== "object" || typeof request.kind !== "string") {
    if (play) failPlay(play, new Error("Malformed Worker request."));
    else fatal(new Error("Malformed Worker request."));
    return;
  }
  if (request.kind === "init") {
    if (ready) return fatal(new Error("Graphics owner is already initialized."));
    ready = (async () => {
      if (!self.isSecureContext || !self.navigator.gpu) throw new Error("This browser needs WebGPU in a secure context (HTTPS or localhost).");
      await init();
      view = await BrowserView.create(request.canvas);
      view.resize(...extent);
      report("ready");
    })();
    ready.catch(fatal);
    return;
  }
  if (!ready) return fatal(new Error("Initialize graphics before sending commands."));
  if (request.kind.startsWith("play-")) { handlePlay(request); return; }
  if (request.kind === "import" || request.kind === "accept-library") {
    if (play) report("import-error", { id: request.id, message: "Stop gameplay before changing the selected library." });
    else if (request.kind === "import") queueImport(request);
    else acceptLibrary(request);
  }
  else if (request.kind === "select") {
    if (play) report("selection-error", { id: request.id, message: "Stop gameplay before changing the preview chart." });
    else void selectChart(request).catch(fatal);
  }
  else if (request.kind === "seek") {
    if (play) report("seek-error", { id: request.id, selectedId, message: "Stop gameplay before seeking the preview." });
    else void seek(request).catch(fatal);
  }
  else if (request.kind === "resize") void resize(request).catch(fatal);
  else if (play) failPlay(play, new Error("Unknown Worker request."));
});

import init, * as runtime from "./pkg/beatkernel_bms_runtime.js";
import { LIMITS, preflight, previewNanos } from "./host_model.mjs";
import { PLAY_PCM_SAMPLES, bindingsFor, validateTiming, validateStart, replayOutputFromMetadata, millisecondsToNanos, renderedCursor } from "./play-model.mjs";
import { BrowserMultiplayerOwner } from "./multiplayer-owner.mjs";
import { validateSelections, validateOpponentSnapshot } from "./saved-opponents.mjs";
const { BrowserGame, BrowserLibrary, BrowserMultiplayer, BrowserReplay, BrowserView } = runtime;
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
const PROGRESS_INTERVAL_NS = 250000000n;
const NO_OPPONENTS = Object.freeze([]);

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
      if (play?.game && play.mode === "replay") view.draw_replay(play.game);
      else if (play?.game) view.draw_game(play.game);
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
  const result = { songNs: null, hits: null, misses: null, combo: null, maxCombo: null, preOriginInputs: state.preOriginInputs };
  if (state.game) {
    for (const [field, getter] of [["songNs", "song_ns"], ["hits", "hits"], ["misses", "misses"], ["combo", "combo"], ["maxCombo", "max_combo"]]) {
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

function networkNow() {
  return millisecondsToNanos(self.performance.timeOrigin) + millisecondsToNanos(self.performance.now());
}

function multiplayerConfiguration(value, mode) {
  if (value === undefined) return null;
  if (mode !== "live" || !value || typeof value !== "object" || typeof value.host !== "boolean"
    || typeof value.url !== "string" || value.url.length === 0 || value.url.length > 4096
    || !hostTime(value.windowOriginNs)) throw new Error("Invalid live multiplayer configuration.");
  const url = new URL(value.url);
  if (url.protocol !== "https:" || url.username || url.password || url.hash || url.href.length > 4096) throw new Error("Multiplayer requires a bounded HTTPS WebTransport URL without credentials or a fragment.");
  return { url: url.href, host: value.host, windowOriginNs: value.windowOriginNs,
    owner: null, controller: null, requested: false, rpcId: null, start: null,
    disposed: false, stopping: false, failure: null, pending: null, lastProgress: null,
    remote: null, remoteTimer: null, lastRemote: null, finalWritten: false, finalAcknowledged: false };
}

function clearRemoteProgress(network) {
  clearTimeout(network.remoteTimer);
  network.remoteTimer = null;
  network.remote = null;
}

function closeNetwork(network) {
  if (!network || network.disposed) return;
  network.disposed = true;
  clearRemoteProgress(network);
  const owner = network.owner;
  network.owner = null;
  try { network.controller?.abort(); } catch {}
  try { owner?.close(); } catch {}
}

function networkFailure(state, error) {
  const network = state.network;
  if (!network || network.disposed) return;
  network.failure ??= error;
  closeNetwork(network);
  if (play !== state) return;
  if (!state.active) failPlay(state, error);
  else report("play-multiplayer", { playId: state.id, event: { kind: "disconnected", error: message(error) } });
}

function progressSnapshot(score) {
  if (!signed(score.songNs) || ![score.hits, score.misses, score.combo, score.maxCombo].every(unsigned)) {
    throw new Error("Actual multiplayer score is unavailable.");
  }
  return { songNs: score.songNs, hits: score.hits, misses: score.misses, combo: score.combo, maxCombo: score.maxCombo };
}

function publishRemoteProgress(state) {
  const network = state.network;
  if (network.disposed || network.stopping || play !== state || network.remote === null) return;
  const now = networkNow();
  const remaining = network.lastRemote === null ? 0n : PROGRESS_INTERVAL_NS - (now - network.lastRemote);
  if (remaining > 0n) {
    if (network.remoteTimer === null) network.remoteTimer = setTimeout(() => {
      network.remoteTimer = null;
      try { publishRemoteProgress(state); } catch (error) { networkFailure(state, error); }
    }, Number((remaining + 999999n) / 1000000n));
    return;
  }
  const event = network.remote;
  network.remote = null;
  network.lastRemote = now;
  report("play-multiplayer", { playId: state.id, event });
}

function networkEvent(state, event) {
  const network = state.network;
  if (network.disposed || (play !== state && !network.stopping)) return;
  if (!event || typeof event.kind !== "string") throw new Error("Invalid actual multiplayer event.");
  let forwarded;
  if (event.kind === "progress" || event.kind === "final-progress") {
    forwarded = { kind: event.kind, ...progressSnapshot(event) };
    if (event.kind === "progress") {
      if (!network.stopping) { network.remote = forwarded; publishRemoteProgress(state); }
      return;
    }
    clearRemoteProgress(network);
  } else if (event.kind === "start") {
    if (network.stopping) return;
    if (!network.owner || network.start !== null || !hostTime(event.targetNs)
      || !hostTime(event.songTargetNs) || !hostTime(event.uncertaintyNs)) throw new Error("Invalid committed multiplayer start.");
    const offset = network.owner.origin - network.windowOriginNs;
    const targetHostNs = offset + event.targetNs;
    const songTargetHostNs = offset + event.songTargetNs;
    if (!hostTime(targetHostNs) || !hostTime(songTargetHostNs) || songTargetHostNs - targetHostNs !== 100000000n) {
      throw new Error("Committed multiplayer start cannot map to the Window clock.");
    }
    network.start = { kind: "multiplayer-start", targetHostNs, songTargetHostNs, uncertaintyNs: event.uncertaintyNs };
    const rpcId = network.rpcId;
    network.rpcId = null;
    if (!identity(rpcId)) throw new Error("Multiplayer start has no pending readiness request.");
    report("play-reply", { playId: state.id, rpcId, result: network.start });
    return;
  } else if (event.kind === "final-acknowledged") {
    network.finalAcknowledged = true;
    forwarded = { kind: event.kind };
  } else if (event.kind === "connected" || event.kind === "ready") forwarded = { kind: event.kind };
  else if (event.kind === "clock") {
    const fields = ["lowerNs", "upperNs", "midpointNs", "roundTripNs", "observedLocalNs"];
    if (!fields.every(field => signed(event[field]))) throw new Error("Invalid actual multiplayer clock estimate.");
    forwarded = { kind: event.kind };
    for (const field of fields) forwarded[field] = event[field];
  } else if (event.kind === "disconnected") {
    networkFailure(state, new Error(message(event.error)));
    return;
  } else throw new Error("Unknown actual multiplayer event.");
  report("play-multiplayer", { playId: state.id, event: forwarded });
}

async function networkReady(state, request) {
  const network = state.network;
  // A readiness request owns its RPC until committed start, failure or stop.
  network.requested = true;
  network.rpcId = request.rpcId;
  let session = null;
  try {
    network.controller = new AbortController();
    session = new BrowserMultiplayer(state.game.competition_identity(), network.host, 100000000n);
    const opening = BrowserMultiplayerOwner.open(network.url, { session, now: networkNow,
      signal: network.controller.signal,
      onEvent: event => networkEvent(state, event),
      onClose: error => networkFailure(state, error) });
    // Owner.open consumes a valid session/configuration, including setup errors.
    session = null;
    const owner = await opening;
    if (network.disposed || play !== state || failed) { owner.close(); return; }
    network.owner = owner;
    if (owner.closed) throw new Error("Multiplayer closed before readiness.");
    owner.request_ready();
  } catch (error) {
    // Only a failure before transferring into Owner.open leaves a local session.
    try { session?.close(); } catch {}
    try { session?.free(); } catch {}
    networkFailure(state, error);
  }
}

function sendProgress(state, score) {
  const network = state.network;
  if (!network || network.disposed || !network.owner || network.pending !== null) return;
  try {
    const now = networkNow();
    if (network.lastProgress !== null && now - network.lastProgress < PROGRESS_INTERVAL_NS) return;
    const pending = Promise.resolve(network.owner.submit(progressSnapshot(score), false));
    network.pending = pending;
    network.lastProgress = now;
    pending.then(() => { if (network.pending === pending) network.pending = null; }, error => {
      if (network.pending === pending) network.pending = null;
      networkFailure(state, error);
    });
  } catch (error) { networkFailure(state, error); }
}

async function drainNetwork(state, score) {
  const network = state.network;
  let timer = null;
  try {
    if (network.failure) throw network.failure;
    if (!state.active || !network.start || !network.owner || network.disposed) throw new Error("Multiplayer stopped before activation.");
    const owner = network.owner;
    const ensureOpen = () => {
      if (network.disposed || owner.closed) throw network.failure ?? new Error("Multiplayer closed during final drain.");
    };
    const drain = (async () => {
      if (network.pending !== null) await network.pending;
      ensureOpen();
      await owner.submit(progressSnapshot(score), true);
      network.finalWritten = true; // Local full-write completion, separately from peer ACK.
      ensureOpen();
      await owner.wait_final_ack();
      network.finalAcknowledged = true;
    })();
    await Promise.race([drain, new Promise((_, reject) => {
      timer = setTimeout(() => {
        const error = new Error("Multiplayer final drain timed out after 2 seconds.");
        network.failure ??= error;
        closeNetwork(network);
        reject(error);
      }, 2000);
    })]);
  } catch (error) { network.failure ??= error; }
  finally { clearTimeout(timer); closeNetwork(network); }
  return { finalWritten: network.finalWritten, finalAcknowledged: network.finalAcknowledged,
    error: network.failure === null ? null : message(network.failure) };
}

function disposeGame(state) {
  const game = state.game;
  state.game = null;
  const result = { cleanupError: null, replay: null, replayError: null };
  if (!game) return result;
  let stopped = false;
  try { game.stop(); stopped = true; } catch (cause) { result.cleanupError = cause; }
  if (state.mode === "live" && state.recordReplay && stopped) {
    try {
      const bytes = game.take_replay();
      if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
        || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
        || bytes.length === 0 || bytes.length > 64 * 1024 * 1024) throw new Error("Recorded replay has an invalid bounded transferable layout.");
      result.replay = bytes;
    } catch (cause) { result.replayError = message(cause); }
  }
  try { game.free(); } catch (cause) { result.cleanupError ??= cause; }
  return result;
}

function failPlay(state, error, request = null) {
  if (play !== state) return;
  const score = statistics(state);
  play = null; // Invalidates a still-awaiting preparation before releasing owners.
  stopRedraw();
  closeNetwork(state.network);
  const { cleanupError, replay, replayError } = disposeGame(state);
  const text = message(cleanupError ? `${message(error)}; cleanup: ${message(cleanupError)}` : error);
  const pending = new Set([request?.rpcId, state.startRpcId, state.network?.rpcId]);
  state.startRpcId = null;
  if (state.network) state.network.rpcId = null;
  for (const rpcId of pending) if (identity(rpcId)) report("play-reply", { playId: state.id, rpcId, error: text });
  report("play-error", { playId: state.id, message: text, released: cleanupError === null,
    replay, replayComplete: false, replayError, ...score }, replay ? [replay.buffer] : []);
  scheduleDraw();
}

function stopPlay(state, request) {
  if (request.completed !== undefined && typeof request.completed !== "boolean") throw new Error("Invalid stopped-play completion choice.");
  const completed = request.completed === true;
  if (completed && (!state.completed || state.batch !== null)) throw new Error("Natural stop has no current completion evidence.");
  const score = statistics(state);
  play = null;
  stopRedraw();
  if (state.network) {
    state.network.stopping = true;
    clearRemoteProgress(state.network);
  }
  const { cleanupError, replay, replayError } = disposeGame(state);
  const pending = new Set([state.startRpcId, state.network?.rpcId]);
  state.startRpcId = null;
  if (state.network) state.network.rpcId = null;
  for (const rpcId of pending) if (identity(rpcId)) {
    report("play-reply", { playId: state.id, rpcId, error: "Gameplay setup was stopped." });
  }
  const stopped = multiplayer => {
    const result = { replay, replayError, ...score, ...(multiplayer ? { multiplayer } : {}) };
    if (cleanupError) report("play-error", { playId: state.id, message: message(cleanupError), released: false,
      ...result, replayComplete: false }, replay ? [replay.buffer] : []);
    else report("play-stopped", { playId: state.id, ...result,
      replayComplete: completed && replay !== null }, replay ? [replay.buffer] : []);
  };
  scheduleDraw();
  // The game and samples are already released. Network disposal cannot delay
  // local ownership release or turn its failure into an incomplete replay.
  if (state.network) void drainNetwork(state, score).then(stopped).catch(fatal);
  else stopped(null);
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
    const commandBatchLimit = request.commandBatchLimit === undefined ? 256 : request.commandBatchLimit;
    if (!integer(commandBatchLimit, 1, 256)) throw new Error("Gameplay command batch limit must be an integer from 1 to 256.");
    state.commandBatchLimit = commandBatchLimit;
    if (request.mode !== undefined && request.mode !== "live" && request.mode !== "replay") throw new Error("Invalid playback mode.");
    state.mode = request.mode ?? "live";
    const timing = state.mode === "live" ? validateTiming(request.timing) : null;
    const requestedStart = state.mode === "live" ? validateStart(request.startNs) : null;
    state.network = multiplayerConfiguration(request.multiplayer, state.mode);
    const opponents = state.mode === "live" && request.opponents !== undefined
      ? validateSelections(request.opponents) : NO_OPPONENTS;
    let replayFile = null;
    let replaySize = 0;
    if (state.mode === "replay") {
      if (!(request.replayFile instanceof File)) throw new Error("Select an actual replay file.");
      replayFile = request.replayFile;
      replaySize = replayFile.size;
      if (!integer(replaySize, 1, 64 * 1024 * 1024)) throw new Error("Select a nonempty replay file no larger than 64 MiB.");
    }
    await ready;
    if (failed || play !== state) return;
    if (importing || importPumpRunning || pendingImport || stagedLibrary
      || !library || request.libraryId !== libraryId) throw new Error("Wait for the accepted library before starting gameplay.");
    if (typeof request.path !== "string" || !request.path.length || !integer(request.rate, 1, 0xffffffff)) {
      throw new Error("Invalid gameplay chart or sample rate.");
    }
    state.rate = request.rate;
    if (request.recordReplay !== undefined && typeof request.recordReplay !== "boolean") throw new Error("Invalid replay recording choice.");
    let pairs = null;
    const lanes = [];
    const keys = new Set();
    if (state.mode === "replay") {
      if (request.recordReplay === true) throw new Error("Replay playback cannot record live input.");
      const bytes = await replayFile.arrayBuffer();
      if (failed || play !== state) return;
      if (!(bytes instanceof ArrayBuffer) || bytes.byteLength !== replaySize) throw new Error("Replay file size changed or returned an invalid buffer.");
      prepared = library.prepare_replay_chart(request.path, new Uint8Array(bytes), request.rate, 2,
        64 * 1024 * 1024, 256 * 1024 * 1024, 1296);
    } else {
      if (typeof request.seed !== "string" || !/^\d{1,20}$/.test(request.seed) || BigInt(request.seed) > U64_MAX) throw new Error("Invalid gameplay chart seed.");
      if (!(request.keyPairs instanceof Uint32Array) || request.keyPairs.length > 36 || request.keyPairs.length % 2 !== 0) throw new Error("Invalid bounded gameplay key bindings.");
      pairs = request.keyPairs.slice();
      for (let index = 0; index < pairs.length; index += 2) {
        lanes.push(pairs[index]);
        if (!integer(pairs[index + 1], 1, 65535) || keys.has(pairs[index + 1])) throw new Error("Gameplay keys must be valid and unique.");
        keys.add(pairs[index + 1]);
      }
      bindingsFor(lanes);
      prepared = requestedStart === 0n
        ? library.prepare_chart(request.path, request.rate, 2, BigInt(request.seed), 64 * 1024 * 1024, 256 * 1024 * 1024, 1296)
        : library.prepare_chart_at(request.path, request.rate, 2, BigInt(request.seed), requestedStart,
          64 * 1024 * 1024, 256 * 1024 * 1024, 1296);
    }
    const actualStart = prepared.start_ns;
    const startNs = actualStart === undefined && requestedStart === 0n ? 0n : actualStart;
    if (typeof startNs !== "bigint") throw new Error("Prepared chart omitted its actual song start.");
    validateStart(startNs);
    if (state.mode === "live" && startNs !== requestedStart) throw new Error("Prepared live section start differs from its request.");
    const chartLanes = Array.from(prepared.lanes);
    bindingsFor(chartLanes);
    if (state.mode === "live" && chartLanes.some(lane => !lanes.includes(lane))) throw new Error("A prepared lane has no supplied key binding.");
    const metadata = { title: prepared.title, artist: prepared.artist, notes: prepared.note_count, lanes: chartLanes, startNs };
    const moved = prepared;
    prepared = null; // A consuming Rust constructor also owns the argument on Err.
    state.game = state.mode === "replay"
      ? new BrowserReplay(moved, 100000000n)
      : new BrowserGame(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, pairs);
    if (state.mode === "replay") {
      const output = replayOutputFromMetadata(startNs, state.game.end_ns, state.game.playback_end_frame, request.rate);
      if (output.endFrame !== undefined) {
        metadata.endNs = output.endNs;
        metadata.endFrame = output.endFrame;
      }
    }
    for (const opponent of opponents) {
      const bytes = await opponent.file.arrayBuffer();
      // A stopped owner may have freed its game during this unabortable read.
      if (failed || play !== state) return;
      if (!(bytes instanceof ArrayBuffer) || bytes.byteLength !== opponent.file.size) throw new Error("Opponent recording returned an invalid buffer or changed size.");
      const index = state.game.add_saved_opponent(new Uint8Array(bytes), opponent.own, opponent.label);
      if (index !== state.opponentCount) throw new Error("Actual saved opponent admission count changed.");
      state.opponentCount++;
    }
    if (state.mode === "replay") {
      const recordedUntilNs = state.game.recorded_until_ns ?? null;
      if (recordedUntilNs !== null && !signed(recordedUntilNs)) throw new Error("Invalid actual replay prefix extent.");
      metadata.mode = "replay";
      metadata.recordedUntilNs = recordedUntilNs;
    } else if (request.recordReplay === true) {
      state.game.configure_capture(64 * 1024 * 1024, 1000000);
      state.recordReplay = true;
    }
    state.keys = keys;
    state.prepared = true;
    const samples = state.game.sample_count();
    if (!integer(samples, 0, PLAY_PCM_SAMPLES)) throw new Error("Prepared PCM sample count exceeds the bounded section capacity.");
    reply(state, request, { kind: "prepared", samples, opponentCount: state.opponentCount, ...metadata });
    state.startRpcId = null;
    scheduleDraw();
  } catch (error) {
    if (play === state) failPlay(state, error, request);
  } finally { prepared?.free(); }
}

function samplePlay(state, request) {
  if (state.active) throw new Error("Samples are setup-only resources.");
  const sample = state.game.next_sample();
  if (sample == null) { state.samplesEnded = true; reply(state, request, { kind: "samples-end" }); return; }
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
  const batch = state.game.commands(state.commandBatchLimit);
  state.commandsDrained = batch === null;
  if (batch === null) return null;
  if (!batch || !unsigned(batch.sequence) || batch.sequence === 0n || !Array.isArray(batch.commands)
    || !integer(batch.commands.length, 1, state.commandBatchLimit)) throw new Error("Invalid actual gameplay command batch.");
  for (const command of batch.commands) {
    if (!command || !integer(command.kind, 0, 3) || !unsigned(command.voice) || !unsigned(command.sample)
      || !signed(command.at) || typeof command.gain !== "number" || !Number.isFinite(command.gain)
      || !Number.isFinite(Math.fround(command.gain)) || !signed(command.value) || !unsigned(command.denominator)) {
      throw new Error("Invalid actual gameplay command fields.");
    }
  }
  state.batch = { sequence: batch.sequence, count: batch.commands.length };
  state.completed = false;
  return batch;
}

function pumpCommands(state) {
  if (!state.active || state.batch !== null) return;
  const batch = commandBatch(state);
  if (batch !== null) report("play-commands", { playId: state.id, batch });
}

function publishOpponents(state) {
  if (state.opponentCount === 0 || state.opponentsFailed) return;
  try {
    // Display cadence only; the Rust owner advances from actual gameplay time.
    const now = millisecondsToNanos(self.performance.now());
    if (state.lastOpponents !== null && now < state.lastOpponents) throw new Error("Saved comparison display clock regressed.");
    if (state.lastOpponents !== null && now - state.lastOpponents < PROGRESS_INTERVAL_NS) return;
    state.lastOpponents = now;
    const opponents = validateOpponentSnapshot(state.game.saved_opponents(), state.opponentCount);
    report("play-opponents", { playId: state.id, opponents, error: null });
  } catch (error) {
    state.opponentsFailed = true;
    report("play-opponents", { playId: state.id, opponents: null, error: message(error) });
  }
}

function stepPlay(state, request) {
  if (state.mode !== "live") throw new Error("Replay playback cannot accept live gameplay steps.");
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
  if (request.events.length !== 0) state.completed = false;
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
  const score = statistics(state);
  report("play-step-done", { playId: state.id, tickId: request.tickId, ...score });
  scheduleDraw();
  pumpCommands(state);
  sendProgress(state, score);
  publishOpponents(state);
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
      recordReplay: false, completed: false,
      mode: "live", rate: null, network: null, samplesEnded: false, commandsDrained: false,
      prepared: false, opponentCount: 0, opponentsFailed: false, lastOpponents: null,
    };
    play = state; // Reserve before the ready await so stop cannot race a late owner.
    lastPlayId = state.id;
    void preparePlay(state, request).catch(fatal);
    return;
  }
  const state = play;
  if (!state || request.playId !== state.id) return;
  try {
    if (request.kind === "play-stop") { stopPlay(state, request); return; }
    if (request.kind === "play-start") throw new Error("Gameplay setup is already owned by this identity.");
    const requiresRpc = ["play-sample", "play-commands", "play-activate", "play-network-ready"].includes(request.kind);
    if (request.rpcId !== undefined && !requiresRpc && request.kind !== "play-ack") throw new Error("Unexpected gameplay RPC identity.");
    rpc(state, request, requiresRpc);
    if (!state.game || !state.prepared) throw new Error("Wait for actual gameplay preparation.");
    if (request.kind === "play-sample") samplePlay(state, request);
    else if (request.kind === "play-network-ready") {
      if (!state.network || state.network.requested || state.active || !state.samplesEnded
        || !state.commandsDrained || state.batch !== null) throw new Error("Multiplayer readiness requires completed sample and command preparation.");
      void networkReady(state, request);
    } else if (request.kind === "play-commands") {
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
      if (state.network) {
        const network = state.network;
        const target = network.start?.targetHostNs;
        const now = networkNow() - network.windowOriginNs;
        const rounding = (1000000000n + BigInt(state.rate) - 1n) / BigInt(state.rate) + 1n;
        if (network.disposed || !network.owner || network.owner.closed || !hostTime(target)
          || request.targetHostNs !== target || request.hostNs < target || request.hostNs - target > rounding
          || !hostTime(now) || now >= request.hostNs) throw new Error("Multiplayer activation has no live, future committed start within one output frame.");
      }
      if (state.mode === "live") state.game.activate(request.hostNs);
      state.origin = request.hostNs;
      state.startFrame = request.startFrame;
      state.active = true;
      reply(state, request, null);
    } else if (request.kind === "play-step") stepPlay(state, request);
    else if (request.kind === "play-render") {
      if (!state.active || !identity(request.renderId) || request.renderId <= state.lastRender) throw new Error("Invalid rendered-report identity or state.");
      if (state.mode === "replay") {
        if (!(request.presentedNs === null || hostTime(request.presentedNs))) throw new Error("Invalid replay output presentation point.");
      } else if (!(request.presentedNs === null && request.presentedHostNs === null)
        && !(hostTime(request.presentedNs) && hostTime(request.presentedHostNs))) throw new Error("Invalid output presentation pair.");
      renderedCursor(request.report, state.startFrame);
      const completed = state.game.observe_output(request.report.words, request.presentedNs);
      if (typeof completed !== "boolean" || (completed && state.batch !== null)) throw new Error("Invalid completion with outstanding gameplay commands.");
      if (state.mode === "live" && request.presentedNs !== null) state.game.observe_presentation(request.presentedNs, request.presentedHostNs);
      state.completed = completed;
      state.lastRender = request.renderId;
      report("play-render-done", { playId: state.id, renderId: request.renderId, completed,
        ...(state.mode === "replay" ? statistics(state) : {}) });
      if (state.mode === "replay") scheduleDraw();
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

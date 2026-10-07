import { RenderClient, validateRenderLimits, unsignedIdentity } from "./render-protocol.mjs";
import { validateCompletedResults, resultRequest } from "./completed-results-model.mjs";
import init, * as runtime from "./pkg/beatkernel_bms_runtime.js";
import { LIMITS, preflight, previewNanos, validateHistoricalGradeSnapshot } from "./host_model.mjs";
import { ORIGINAL_PCM_SAMPLES, PLAY_PCM_SAMPLES, bindingsFor, validateTiming, validateStart, validateEnd, replayOutputFromMetadata, millisecondsToNanos, audioScheduleFromFrame, presentationPair, presentationAvailability, audioClockExpired, renderedCursor, reportWord } from "./play-model.mjs";
import { BrowserMultiplayerOwner } from "./multiplayer-owner.mjs";
import { BrowserRoomOwner, ROOM_SESSION_METHODS } from "./room-owner.mjs";
import { validateSelections, validateOpponentSnapshot, validateOpponentTargets, validateLocalOpponentSnapshot } from "./saved-opponents.mjs";
import { keyboardBindingWords, encodeKeyboardEvent, touchBindingWords, encodeTouchEvent, encodeRawHidEvent, encodePointerEvent, encodePointerButtonEvent } from "./physical-input.mjs";
import { snapshotHidDevices, hidSetupFromProfile } from "./hid-profile.mjs";
import { AudioCommandClient } from "./audio-command-client.mjs";
import { AudioSampleClient } from "./audio-sample-client.mjs";
import { snapshotGamepadSetup, snapshotGamepadDevices, automaticGamepadSetup, gamepadSetupFromProfile, GamepadAdapter } from "./gamepad-profile.mjs";
import { snapshotLocalPlan, localBindingWords } from "./local-play-model.mjs";
import { encodeBrowserSettings, decodeBrowserSettings } from "./settings-profile.mjs";
import { snapshotPointerSetup } from "./pointer-profile.mjs";
const { BrowserGame, BrowserLocalGame, BrowserLibrary, BrowserMultiplayer, BrowserReplay, BrowserRoomClient, BrowserRoomResults, BrowserHistoricalRecord } = runtime;
let ready = null;
let cpuReady = false;
let preview = null;
let previewSongNs = 0n;
let renderPort = null;
let renderLimits = null;
let visual = null;
let visualGeneration = 0n;
let visualPump = null;
let surface = null;
let geometryVersion = 0n;
let surfaceSentVersion = 0n;
let disposed = false;
let capturesPending = 0;
let deferredFatal = null;
let library = null;
let stagedLibrary = null;
let libraryId = 0;
let importGeneration = 0;
let importing = false;
let pendingImport = null;
let importPumpRunning = false;
let selectedId = 0;
let failed = false;
let extent = [0, 0];
let play = null;
let roomFinalization = null;
let roomResults = null;
let completedResults = null;
let historicalRecord = null;
let historicalEpoch = {};
let lastHistoricalId = 0;
let roomResultsEpoch = {};
let lastPlayId = 0;
let settingsOperation = null;
let lastSettingsId = 0;
const I64_MAX = 9223372036854775807n;
const U64_MAX = 18446744073709551615n;
const PROGRESS_INTERVAL_NS = 250000000n;
const NO_OPPONENTS = Object.freeze([]);

function report(kind, fields = {}, transfer = []) { self.postMessage({ kind, ...fields }, transfer); }
function message(error) { return String(error?.message ?? error).slice(0, 4096); }
async function settingsProfile(request) {
  const id = request.id;
  if (!identity(id)) {
    report("settings-profile-error", { id, message: "Settings request identity must be a positive safe integer." });
    return;
  }
  if (id <= lastSettingsId) return;
  lastSettingsId = id;
  if (!cpuReady || settingsOperation || play || roomFinalization || importing || stagedLibrary) {
    report("settings-profile-error", { id, message: "Wait for an initialized idle Worker before processing settings." });
    return;
  }
  const operation = { id };
  settingsOperation = operation;
  try {
    if (request.kind === "settings-profile-save") {
      const bytes = encodeBrowserSettings(request.settings);
      if (!failed && settingsOperation === operation) report("settings-profile-saved", { id, bytes }, [bytes.buffer]);
    } else {
      const settings = await decodeBrowserSettings(request.file);
      if (!failed && settingsOperation === operation) report("settings-profile-loaded", { id, settings });
    }
  } catch (error) {
    if (!failed && settingsOperation === operation) report("settings-profile-error", { id, message: message(error) });
  } finally {
    // An obsolete Window deadline never releases an unabortable File read.
    if (settingsOperation === operation) settingsOperation = null;
  }
}
function completedResultsMetadata(results) {
  if (!results) return null;
  return validateCompletedResults(results);
}
function discardCompletedResults() {
  const previous = completedResults;
  completedResults = null;
  if (previous?.binding) { try { previous.binding.free(); } catch {} previous.binding = null; }
}
function captureCompletedResults(state) {
  if (state.mode !== "live" || !state.game || !state.completed || state.completedResults) return;
  const binding = state.game.completed_results();
  if (binding == null) return;
  try {
    const metadata = validateCompletedResults({ proof: true, players: Array.from(binding.players),
      page: binding.page, pages: binding.pages, comparisons: binding.comparisons,
      hasComparisons: binding.has_comparisons, failed: binding.failed,
      detailPages: binding.detail_pages, comparisonPages: binding.comparison_pages });
    state.completedResults = { ...metadata, id: state.id, lastRpc: state.lastRpc, ready: false, shown: false,
      epoch: state.resultsEpoch, binding, displayError: binding.failed ? message(binding.error ?? "Completed Results projection unavailable.") : null };
  } catch (error) { try { binding.free(); } catch {} throw error; }
}
function retainCompletedResults(state, error = null) {
  const results = state.completedResults;
  if (!results) return;
  if (failed || results.epoch !== roomResultsEpoch || play !== null) {
    try { results.binding?.free(); } catch {} results.binding = null; return;
  }
  results.ready = true;
  results.lastRpc = state.lastRpc;
  results.technicalError = error === null ? null : message(error);
  completedResults = results;
}
function completedResultsFailure(results, error) {
  if (completedResults !== results) return;
  // A rendering failure does not erase the Rust-owned historical completion table.
  results.failed = true;
  results.page = results.pages = results.detailPages = results.comparisonPages = 0;
  results.comparisons = results.hasComparisons = false;
  fenceVisual();
  report("play-completed-results", { playId: results.id, completedResults: completedResultsMetadata(results), error: message(error) });
}

function discardHistoricalRecord() {
  historicalEpoch = {};
  const previous = historicalRecord;
  historicalRecord = null;
  if (visual?.owner === previous) fenceVisual();
  try { previous?.binding.free(); } catch { /* Historical display disposal has no gameplay outcome. */ }
}
function historicalReply(id, available, error = null, grades = null) {
  report("historical-record-result", { id, available, gradePage: grades?.page ?? null, gradePages: grades?.pages ?? 0, error: error === null ? null : (message(error) || "Historical presentation failed.") });
}
function historicalRequestId(request) {
  if (!identity(request.id)) return false;
  if (request.id <= lastHistoricalId) { historicalReply(request.id, false, "Historical operation ID must increase."); return false; }
  lastHistoricalId = request.id;
  return true;
}
async function presentHistoricalRecord(request) {
  if (!historicalRequestId(request)) return;
  if (failed || !cpuReady || play || roomFinalization || importing || stagedLibrary || settingsOperation
    || completedResults?.shown || roomResults) {
    historicalReply(request.id, false, "Historical display requires an initialized idle preview.");
    return;
  }
  discardHistoricalRecord();
  const epoch = historicalEpoch;
  const current = () => epoch === historicalEpoch && !failed && !play && !roomFinalization && !importing
    && !stagedLibrary && !settingsOperation && !completedResults?.shown && !roomResults;
  let binding = null;
  try {
    const file = request.replayFile;
    const replaySize = file?.size;
    if (!file || !Number.isSafeInteger(replaySize) || replaySize < 1 || replaySize > 64 * 1024 * 1024
      || typeof file.arrayBuffer !== "function") throw new Error("Historical replay file exceeds its bound.");
    const archive = request.completedArchive;
    const player = request.archivePlayer;
    if (archive != null) {
      if (!(archive instanceof Uint8Array) || !(archive.buffer instanceof ArrayBuffer)
        || archive.buffer.resizable === true || archive.byteOffset !== 0 || archive.byteLength !== archive.buffer.byteLength
        || archive.byteLength < 1 || archive.byteLength > 5 * 1024 * 1024
        || (player != null && (!Number.isInteger(player) || player < 1 || player > 0xffffffff))) {
        throw new Error("Historical archive envelope is invalid.");
      }
    } else if (player != null) throw new Error("Historical player association requires archive bytes.");
    await ready;
    if (!current()) return;
    const bytes = await file.arrayBuffer();
    if (!current()) return;
    if (!(bytes instanceof ArrayBuffer) || bytes.resizable === true || bytes.byteLength !== replaySize) {
      throw new Error("Historical replay acquisition changed its bounded extent.");
    }
    binding = new BrowserHistoricalRecord(new Uint8Array(bytes), archive ?? undefined, player ?? undefined);
    if (!current()) { const previous = binding; binding = null; previous.free(); return; }
    const available = binding.available;
    const error = binding.error;
    if (typeof available !== "boolean" || !(error == null || (typeof error === "string" && error.length >= 1 && error.length <= 4096))
      || (available && error != null)) throw new Error("Historical presentation returned invalid metadata.");
    const grades = available ? validateHistoricalGradeSnapshot({ page: binding.grade_page, pages: binding.grade_pages }) : null;
    if (available) { historicalRecord = { id: request.id, binding, grades, lastRpc: 0 }; binding = null; }
    else { const previous = binding; binding = null; previous.free(); }
    historicalReply(request.id, available, error ?? null, grades);
    publishVisual();
  } catch (error) {
    try { binding?.free(); } catch {}
    if (current()) { historicalReply(request.id, false, error); publishVisual(); }
  }
}
function historicalPageReply(request, grades, error = null) {
  report("historical-record-page-result", { id: request.id, rpcId: request.rpcId,
    gradePage: grades?.page ?? null, gradePages: grades?.pages ?? 0, error: grades === null ? (message(error ?? "Historical grade request failed.") || "Historical grade request failed.") : null });
}
function pageHistoricalRecord(request) {
  if (!identity(request.id) || !identity(request.rpcId)) return;
  const current = historicalRecord;
  if (failed || !cpuReady || play || roomFinalization || importing || stagedLibrary || settingsOperation
    || completedResults?.shown || roomResults || !current || current.id !== request.id
    || request.rpcId <= current.lastRpc) {
    historicalPageReply(request, null, "Historical grade request requires the current idle selection.");
    return;
  }
  let requested;
  try { requested = validateHistoricalGradeSnapshot({ page: request.page, pages: current.grades.pages }); }
  catch (error) { historicalPageReply(request, null, error); return; }
  let before;
  try {
    before = validateHistoricalGradeSnapshot({ page: current.binding.grade_page, pages: current.binding.grade_pages });
    if (before.page !== current.grades.page || before.pages !== current.grades.pages) throw new Error("Historical grade binding changed unexpectedly.");
  } catch (error) {
    discardHistoricalRecord(); historicalReply(current.id, false, error); historicalPageReply(request, null, error); publishVisual(); return;
  }
  if (requested.page === before.page) {
    current.lastRpc = request.rpcId;
    historicalPageReply(request, before);
    return;
  }
  let refusal = null;
  let refused = false;
  try { current.binding.set_grade_page(requested.page); }
  catch (error) { refused = true; refusal = error; }
  try {
    const after = validateHistoricalGradeSnapshot({ page: current.binding.grade_page, pages: current.binding.grade_pages });
    if (after.pages !== before.pages || after.page !== (refused ? before.page : requested.page)) throw new Error("Historical grade mutation returned unexpected metadata.");
    if (refused) { historicalPageReply(request, null, refusal ?? "Historical grade request refused."); return; }
    current.grades = after;
    current.lastRpc = request.rpcId;
    historicalPageReply(request, after);
    queueVisualControl("page", { page: after.page, comparisons: false, geometryVersion: nextGeometry(request.geometryVersion) });
  } catch (error) {
    discardHistoricalRecord(); historicalReply(current.id, false, error); historicalPageReply(request, null, error); publishVisual();
  }
}

function clearHistoricalRecord(request) {
  if (!historicalRequestId(request)) return;
  discardHistoricalRecord();
  historicalReply(request.id, false);
  publishVisual();
}

function discardRoomResults() {
  const previousCompleted = completedResults;
  discardHistoricalRecord();
  if (visual?.owner === previousCompleted) fenceVisual();
  discardCompletedResults();
  roomResultsEpoch = {};
  const previous = roomResults;
  roomResults = null;
  if (visual?.owner === previous) fenceVisual();
  if (previous?.binding) {
    const binding = previous.binding;
    previous.binding = null;
    try { binding.free(); } catch { /* Presentation disposal cannot change a joined gameplay outcome. */ }
  }
}
function roomResultsMetadata(results) {
  return results ? { page: results.page, pages: results.pages, failed: results.failed } : null;
}
function roomResultsFailure(results, error) {
  if (roomResults !== results) return;
  results.failed = true;
  const binding = results.binding;
  results.binding = null;
  try { binding?.free(); } catch {}
  fenceVisual();
  report("play-room-results", { playId: results.id, ...roomResultsMetadata(results), error: message(error) });
}
function fenceVisual() {
  // Fence producer callbacks before gameplay or prepared owners are freed.
  if (visual) visual.retired = true;
}
function fatal(error) {
  if (failed) return;
  if (capturesPending) {
    deferredFatal ??= error;
    // Cancel transport immediately; its retained reads/writes still join before
    // the capture outcome and the deferred global failure are delivered.
    cancelRoomFinalization();
    return;
  }
  if (play) {
    // failPlay joins room cleanup and delivers genuine captures first.
    play.fatalAfterCapture = error;
    failPlay(play, error);
    return;
  }
  failed = true;
  discardRoomResults();
  cancelRoomFinalization();
  fenceVisual();
  report("fatal", { message: message(error) });
}
function captureDelivered() {
  --capturesPending;
  if (!capturesPending && deferredFatal) {
    const error = deferredFatal;
    deferredFatal = null;
    fatal(error);
  }
}
function visualSelection() {
  if (play?.game) return { owner: play, source: play.game, mode: play.mode === "replay" ? "replay" : play.localPlan ? "local" : "live" };
  if (completedResults?.shown && completedResults.binding && !completedResults.failed) return { owner: completedResults, source: completedResults.binding, mode: "results", room: roomResults?.binding && !roomResults.failed ? roomResults.binding : null };
  if (roomResults?.binding && !roomResults.failed) return { owner: roomResults, source: roomResults.binding, mode: "room" };
  if (historicalRecord?.binding) return { owner: historicalRecord, source: historicalRecord.binding, mode: "history" };
  if (preview) return { owner: preview, source: preview, mode: "preview" };
  return null;
}
function currentVisual(context) {
  const selected = visualSelection();
  return !failed && !disposed && visual === context && !context.retired && selected?.owner === context.owner
    && selected.source === context.source && selected.mode === context.mode && selected.room === context.room;
}
function visualFailure(context, error) {
  if (!currentVisual(context)) return;
  context.retired = true;
  if (context.mode === "live" || context.mode === "local" || context.mode === "replay") failPlay(context.owner, error);
  else if (context.mode === "results") completedResultsFailure(context.owner, error);
  else if (context.mode === "room") roomResultsFailure(context.owner, error);
  else if (context.mode === "history") { discardHistoricalRecord(); historicalReply(context.owner.id, false, error); }
  else report("selection-error", { id: selectedId, message: message(error) });
}
function scopedRenderPort(context) {
  const port = renderPort;
  return {
    postMessage: (...args) => port.postMessage(...args),
    start: () => port.start(),
    close: () => port.close(),
    set onmessage(listener) {
      port.onmessage = listener === null ? null : event => {
        const evidence = event.data;
        if (currentVisual(context) && evidence?.generation === context.generation && evidence.content === context.content
          && ["drawn", "render-wait"].includes(evidence.kind)) {
          report(evidence.kind, { selectedId, generation: context.generation, content: context.content,
            ...(play === context.owner ? { playId: play.id } : {}) });
        }
        listener(event);
      };
    },
    set onmessageerror(listener) { port.onmessageerror = listener; },
  };
}
function nextGeometry(requested) {
  if (requested !== undefined && (!unsignedIdentity(requested) || requested <= geometryVersion)) throw new Error("Geometry version must increase.");
  const next = requested ?? geometryVersion + 1n;
  if (!unsignedIdentity(next)) throw new Error("Geometry identity exhausted.");
  geometryVersion = next;
  return next;
}
function publishVisual() {
  if (failed || disposed || !cpuReady || !renderPort) return;
  if (visualPump) { visualPump.dirty = true; return; }
  const pump = { dirty: true };
  visualPump = pump;
  void (async () => {
    while (pump.dirty && !failed && !disposed) {
      pump.dirty = false;
      const selected = visualSelection();
      if (!selected) continue;
      let context = visual;
      if (!context || !currentVisual(context)) {
        if (context) {
          context.retired = true;
          try { if (context.client.state === "ready") await context.client.retire(); }
          catch { context.client.close(); }
          if (failed || disposed) return;
        }
        // Navigation during retirement selects the latest owner, never the old candidate.
        const latest = visualSelection();
        if (!latest) { visual = null; continue; }
        if (visualGeneration === U64_MAX) throw new Error("Visual generation exhausted.");
        context = { ...latest, generation: ++visualGeneration, content: visualGeneration, sequence: 0n, retired: false, controls: [] };
        visual = context;
        context.client = new RenderClient({ port: scopedRenderPort(context), ...renderLimits, generation: context.generation, content: context.content,
          onError: error => visualFailure(context, error),
          onGeometry: evidence => { if (currentVisual(context)) report("render-geometry", { ...evidence, mode: context.mode, selectedId, ...(play === context.owner ? { playId: play.id } : {}) }); } });
        const bytes = ["preview", "live", "local", "replay"].includes(context.mode)
          ? context.source.visual_registration(context.generation, context.content, renderLimits.maxPacketBytes, renderLimits.maxDiagnosticBytes)
          : context.source.visual_snapshot(context.generation, context.content, renderLimits.maxPacketBytes, renderLimits.maxDiagnosticBytes);
        // A new content owner needs its own geometry submission evidence even
        // when the renderer keeps the same surface. Preserve an unsent resize's
        // reserved version; its control supplies the new owner's evidence.
        const registrationGeometry = surface && surfaceSentVersion >= surface.geometryVersion ? nextGeometry() : undefined;
        await context.client.packet(bytes, { mode: context.mode,
          ...(registrationGeometry === undefined ? {} : { geometryVersion: registrationGeometry }) });
        if (!currentVisual(context)) { pump.dirty = true; continue; }
        if (context.room) {
          await context.client.packet(context.room.visual_snapshot(context.generation, context.content, renderLimits.maxPacketBytes, renderLimits.maxDiagnosticBytes));
          if (!currentVisual(context)) { pump.dirty = true; continue; }
        }
        if (surface && surface.geometryVersion > surfaceSentVersion && !context.controls.some(control => control.operation === "resize")) context.controls.unshift({ operation: "resize", fields: { ...surface } });
        // Frozen Results transport omits selection. Admit the latest owner
        // selection explicitly before the receiver can submit its first draw.
        if (context.mode === "results" && !context.controls.some(control => control.operation === "page")) {
          context.controls.push({ operation: "page", fields: { page: context.owner.page,
            comparisons: context.owner.comparisons, geometryVersion: nextGeometry() } });
        }
      }
      if (!currentVisual(context)) { pump.dirty = true; continue; }
      // Controls wait only for the visual channel; input/audio remain independent.
      while (context.controls.length) {
        if (context.client.pending) break;
        // A committed local page frame owns its reserved version. Submit it
        // before newer controls, while retaining older surface controls first.
        if (context.frameGeometry !== undefined && context.controls[0].fields.geometryVersion > context.frameGeometry) break;
        const control = context.controls.shift();
        await context.client.control(control.operation, control.fields);
        if (control.operation === "resize") surfaceSentVersion = control.fields.geometryVersion;
        if (!currentVisual(context)) { pump.dirty = true; break; }
      }
      if (!currentVisual(context)) continue;
      if (context.controls.length && (context.frameGeometry === undefined
        || context.controls[0].fields.geometryVersion < context.frameGeometry)) continue;
      if (["preview", "live", "local", "replay"].includes(context.mode)) {
        context.client.publish(() => {
          if (!currentVisual(context)) throw new Error("Visual producer retired.");
          if (context.sequence === U64_MAX) throw new Error("Visual sequence exhausted.");
          const sequence = ++context.sequence;
          const bytes = context.mode === "preview" ? context.source.preview_state(sequence, previewSongNs)
            : context.source.visual_frame(sequence, context.owner.localPage ?? 0);
          context.frameGeometry = undefined;
          return bytes;
        }, header => {
          if (!currentVisual(context)) return false;
          const accepted = context.source.acknowledge_visual(header.generation, header.content, header.sequence);
          if (accepted && context.controls.length) setTimeout(() => { if (currentVisual(context)) publishVisual(); }, 0);
          return accepted;
        }, context.frameGeometry === undefined ? {} : { geometryVersion: context.frameGeometry });
      }
    }
  })().catch(error => {
    if (visual && currentVisual(visual)) visualFailure(visual, error);
    else if (!failed && !disposed) pump.dirty = true;
  })
    .finally(() => { if (visualPump === pump) { visualPump = null; if (pump.dirty) publishVisual(); } });
}
function queueVisualControl(operation, fields) {
  const context = visual;
  if (context && currentVisual(context)) {
    context.controls.push({ operation, fields });
    // Bound repeated resize/page requests to the latest control of each kind.
    context.controls = context.controls.filter((control, index, all) => all.findLastIndex(candidate => candidate.operation === control.operation) === index);
  }
  publishVisual();
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
  if (failed || disposed || generation !== importGeneration) return;
  let prepared = null;
  try {
    if (play) throw new Error("Stop gameplay before changing the preview chart.");
    if (importing || !library || request.libraryId !== libraryId) throw new Error("Wait for the selected library to finish loading.");
    if (!Number.isInteger(request.rate) || request.rate < 1 || request.rate > 0xffffffff) throw new Error("Sample rate must be a positive 32-bit integer.");
    if (typeof request.seed !== "string" || !/^\d{1,20}$/.test(request.seed) || BigInt(request.seed) > 0xffffffffffffffffn) throw new Error("Chart seed must fit an unsigned 64-bit integer.");
    prepared = library.prepare_chart(request.path, request.rate, 2, BigInt(request.seed), 64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES);
    const metadata = {
      title: prepared.title, artist: prepared.artist, duration: prepared.duration_ns.toString(),
      notes: prepared.note_count, samples: prepared.sample_count, images: prepared.image_count,
    };
    fenceVisual();
    const previous = preview;
    preview = prepared;
    prepared = null;
    previewSongNs = 0n;
    previous?.free();
    selectedId = request.id;
    report("selected", { id: request.id, libraryId, path: request.path, ...metadata });
    publishVisual();
  } catch (error) {
    report("selection-error", { id: request.id, message: message(error) });
  } finally { prepared?.free(); }
}

async function seek(request) {
  await ready;
  if (failed || disposed || request.selectedId !== selectedId) return;
  try {
    if (play) throw new Error("Stop gameplay before seeking the preview.");
    const ns = previewNanos(request.ns);
    if (!preview) throw new Error("Select a prepared preview before seeking.");
    previewSongNs = ns;
    report("position", { id: request.id, selectedId, ns: ns.toString() });
    publishVisual();
  } catch (error) { report("seek-error", { id: request.id, selectedId, message: message(error) }); }
}

async function resize(request) {
  if (![request.width, request.height].every(value => Number.isInteger(value) && value >= 0 && value <= 0xffffffff)) throw new Error("Invalid canvas extent.");
  extent = [request.width, request.height];
  await ready;
  if (failed || disposed) return;
  surface = { width: request.width, height: request.height, geometryVersion: nextGeometry(request.geometryVersion) };
  queueVisualControl("resize", surface);
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
  if (state.localPlan) {
    result.primaryPlayer = state.localPlan.members[0].player;
    result.localScores = state.localPlan.members.map(({ player }) => {
      const row = { player, songNs: null, hits: null, misses: null, combo: null, maxCombo: null };
      if (state.game) for (const [field, method] of [["songNs", "member_song_ns"], ["hits", "hits"], ["misses", "misses"], ["combo", "combo"], ["maxCombo", "max_combo"]]) {
        try {
          const value = state.game[method](player);
          if (field === "songNs" ? signed(value) : unsigned(value)) row[field] = value;
        } catch {}
      }
      return row;
    });
    const first = result.localScores[0];
    for (const name of ["songNs", "hits", "misses", "combo", "maxCombo"]) result[name] = first[name];
    return result;
  }
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

function networkWords(value, minimum, maximum, stride, name) {
  if (!(value instanceof Uint32Array) || !(value.buffer instanceof ArrayBuffer)
    || value.buffer.resizable === true || !integer(value.length, minimum, maximum)
    || value.length % stride !== 0 || value.byteLength !== value.length * 4) {
    throw new Error(`Invalid bounded ${name}.`);
  }
  new Uint32Array(value.buffer, value.byteOffset, value.length); // Reject detached storage, including empty mappings.
  const snapshot = new Uint32Array(value.length);
  snapshot.set(value);
  return snapshot;
}

function multiplayerConfiguration(value, mode, localPlan) {
  if (value === undefined) return null;
  if (mode !== "live" || !value || typeof value !== "object" || typeof value.host !== "boolean"
    || typeof value.url !== "string" || value.url.length === 0 || value.url.length > 4096
    || !hostTime(value.windowOriginNs)) throw new Error("Invalid live multiplayer configuration.");
  const url = new URL(value.url);
  if (url.protocol !== "https:" || url.username || url.password || url.hash || url.href.length > 4096) throw new Error("Multiplayer requires a bounded HTTPS WebTransport URL without credentials or a fragment.");
  const localPlayers = localPlan ? Uint32Array.from(localPlan.members, member => member.player) : null;
  let peerTargets = null;
  if (value.peerTargets !== undefined) {
    if (localPlayers === null) throw new Error("Peer targets require local group gameplay.");
    peerTargets = networkWords(value.peerTargets, 0, 128, 2, "local peer targets");
    const selected = new Set();
    for (let index = 0; index < peerTargets.length; index += 2) {
      if (!localPlayers.includes(peerTargets[index]) || selected.has(peerTargets[index]) || peerTargets[index + 1] === 0) {
        throw new Error("Peer targets require distinct admitted local players and positive remote players.");
      }
      selected.add(peerTargets[index]);
    }
  }
  return { url: url.href, host: value.host, windowOriginNs: value.windowOriginNs,
    localPlayers, peerTargets, remoteRoster: null, finalGroup: null,
    peers: localPlayers === null ? null : Array.from(localPlayers, player => ({ player, remotePlayer: null,
      peerStatus: 0, peerPrefix: null, peerFinal: false, peerHudError: null })),
    owner: null, controller: null, requested: false, rpcId: null, start: null,
    disposed: false, stopping: false, failure: null, pending: null, lastProgress: null,
    remote: null, remoteTimer: null, lastRemote: null, finalWritten: false, finalAcknowledged: false,
    peerStatus: 0, peerPrefix: null, peerFinal: false, peerHudError: null };
}

function peerOutcome(network) {
  return { status: ["waiting", "connected", "disconnected", "stopped"][network.peerStatus],
    progress: network.peerPrefix, final: network.peerFinal, error: network.peerHudError };
}

function multiplayerOutcome(network) {
  return { finalWritten: network.finalWritten, finalAcknowledged: network.finalAcknowledged,
    error: network.failure === null ? null : message(network.failure),
    ...(network.peers === null ? { peer: peerOutcome(network) }
      : { peers: network.peers.map(peer => ({ player: peer.player, remotePlayer: peer.remotePlayer, ...peerOutcome(peer) })) }) };
}

function updatePeerHud(state, progress = null, member = null) {
  const network = state.network;
  const peer = member ?? network;
  if (!state.game || !network || network.disposed || peer.peerHudError !== null || play !== state) return;
  try {
    const words = new Uint32Array(progress === null ? 0 : 10);
    if (progress !== null) {
      let index = 0;
      for (const value of [progress.songNs, progress.hits, progress.misses, progress.combo, progress.maxCombo]) {
        const bits = BigInt.asUintN(64, value);
        words[index++] = Number(bits & 0xffffffffn);
        words[index++] = Number(bits >> 32n);
      }
    }
    if (member === null) state.game.update_peer_hud(peer.peerStatus, words);
    else state.game.update_peer_hud(member.player, peer.peerStatus, words);
    publishVisual();
  } catch (error) {
    peer.peerHudError = message(error) || "Peer presentation failed.";
    try {
      if (member === null) state.game.disable_peer_hud();
      else state.game.disable_peer_hud(member.player);
    } catch (cause) { peer.peerHudError = message(`${peer.peerHudError}; disable peer HUD: ${message(cause)}`); }
    report("play-multiplayer", { playId: state.id, event: { kind: "peer-display-unavailable", error: peer.peerHudError,
      ...(member === null ? {} : { player: member.player }) } });
    publishVisual();
  }
}

function updateNetworkHud(state) {
  const network = state.network;
  if (network.peers === null) updatePeerHud(state, network.peerPrefix);
  else for (const peer of network.peers) updatePeerHud(state, peer.peerPrefix, peer);
}

function clearRemoteProgress(network) {
  clearTimeout(network.remoteTimer);
  network.remoteTimer = null;
  network.remote = null;
}

function closeNetwork(network) {
  if (!network || network.disposed) return;
  network.disposed = true;
  if (network.peerStatus !== 2) network.peerStatus = 3;
  if (network.peers !== null) for (const peer of network.peers) if (peer.peerStatus !== 2) peer.peerStatus = 3;
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
  network.peerStatus = 2;
  if (network.peers !== null) for (const peer of network.peers) peer.peerStatus = 2;
  updateNetworkHud(state);
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

function groupWords(value, players, name) {
  const words = networkWords(value, 11, 704, 11, name);
  if (words.length !== players.length * 11) throw new Error(`${name} differs from the admitted roster.`);
  for (let index = 0; index < players.length; index++) {
    if (words[index * 11] !== players[index]) throw new Error(`${name} changed the admitted player order.`);
  }
  return words;
}

function groupProgressSnapshot(state) {
  if (!state.game || state.network.localPlayers === null) throw new Error("Actual local group progress is unavailable.");
  return groupWords(state.game.progress_words(), state.network.localPlayers, "local group progress");
}

function retainFinalGroup(state) {
  if (!state.network || state.network.localPlayers === null) return;
  try { state.network.finalGroup = groupProgressSnapshot(state); }
  catch (error) { state.network.failure ??= error; }
}

function localCompetitionIdentity(state, players) {
  let identity = null;
  for (const player of players) {
    const bytes = state.game.competition_identity(player);
    if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
      || bytes.buffer.resizable === true || !integer(bytes.byteLength, 1, 65536)) {
      throw new Error("Local competition identity must be nonempty bounded ordinary bytes.");
    }
    new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    if (identity === null) {
      identity = new Uint8Array(bytes.byteLength);
      identity.set(bytes);
    } else if (bytes.byteLength !== identity.byteLength || bytes.some((value, index) => value !== identity[index])) {
      throw new Error("Local members require the same canonical competition identity.");
    }
  }
  return identity;
}

function acceptGroupRoster(network, value) {
  if (network.peers === null || network.remoteRoster !== null) throw new Error("Unexpected or repeated remote group roster.");
  const players = networkWords(value, 1, 64, 1, "remote group roster");
  const seen = new Set();
  for (const player of players) {
    if (player === 0 || seen.has(player)) throw new Error("Remote group roster requires positive distinct players.");
    seen.add(player);
  }
  const targets = new Map();
  if (network.peerTargets === null) {
    for (let index = 0; index < Math.min(network.peers.length, players.length); index++) {
      targets.set(network.peers[index].player, players[index]);
    }
  } else for (let index = 0; index < network.peerTargets.length; index += 2) {
    if (!seen.has(network.peerTargets[index + 1])) throw new Error("An explicit peer target is absent from the remote roster.");
    targets.set(network.peerTargets[index], network.peerTargets[index + 1]);
  }
  network.remoteRoster = players;
  for (const peer of network.peers) peer.remotePlayer = targets.get(peer.player) ?? null;
}

function acceptGroupProgress(state, event) {
  const network = state.network;
  if (network.peers === null || network.remoteRoster === null || !unsigned(event.sequence)) {
    throw new Error("Group progress requires its admitted roster and exact sequence.");
  }
  const words = groupWords(event.words, network.remoteRoster, "remote group progress");
  if (network.peerStatus >= 2 || network.peerFinal) return;
  const prefixes = new Map();
  for (let offset = 0; offset < words.length; offset += 11) {
    const value = at => BigInt(words[offset + at]) | (BigInt(words[offset + at + 1]) << 32n);
    prefixes.set(words[offset], { songNs: BigInt.asIntN(64, value(1)), hits: value(3),
      misses: value(5), combo: value(7), maxCombo: value(9) });
  }
  const final = event.kind === "group-final-progress";
  for (const peer of network.peers) if (peer.remotePlayer !== null) {
    peer.peerPrefix = prefixes.get(peer.remotePlayer);
    peer.peerFinal = final;
  }
  if (!final) {
    if (!network.stopping) { network.remote = true; publishRemoteProgress(state); }
    return;
  }
  network.peerFinal = true;
  clearRemoteProgress(network);
  for (const peer of network.peers) if (peer.remotePlayer !== null) updatePeerHud(state, peer.peerPrefix, peer);
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
  if (network.peers === null) updatePeerHud(state, event);
  else for (const peer of network.peers) if (peer.remotePlayer !== null) updatePeerHud(state, peer.peerPrefix, peer);
}

function networkEvent(state, event) {
  const network = state.network;
  if (network.disposed || (play !== state && !network.stopping)) return;
  if (!event || typeof event.kind !== "string") throw new Error("Invalid actual multiplayer event.");
  let forwarded;
  if (event.kind === "roster") {
    acceptGroupRoster(network, event.players);
    return;
  } else if (event.kind === "group-progress" || event.kind === "group-final-progress") {
    acceptGroupProgress(state, event);
    return;
  } else if (event.kind === "progress" || event.kind === "final-progress") {
    if (network.peers !== null) throw new Error("Scalar progress cannot describe a local group.");
    if (network.peerStatus >= 2 || network.peerFinal) return;
    const progress = progressSnapshot(event);
    network.peerPrefix = progress;
    if (event.kind === "progress") {
      if (!network.stopping) { network.remote = progress; publishRemoteProgress(state); }
      return;
    }
    network.peerFinal = true;
    clearRemoteProgress(network);
    updatePeerHud(state, progress);
    return;
  } else if (event.kind === "start") {
    if (network.stopping) return;
    if (!network.owner || network.start !== null || !hostTime(event.targetNs)
      || !hostTime(event.songTargetNs) || !hostTime(event.uncertaintyNs)
      || (network.localPlayers !== null && network.remoteRoster === null)) throw new Error("Invalid committed multiplayer start.");
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
  } else if (event.kind === "connected") {
    if (network.peerStatus >= 2) return;
    network.peerStatus = 1;
    if (network.peers !== null) for (const peer of network.peers) peer.peerStatus = 1;
    updateNetworkHud(state);
    forwarded = { kind: event.kind };
  } else if (event.kind === "ready") forwarded = { kind: event.kind };
  else if (event.kind === "clock") {
    const fields = ["lowerNs", "upperNs", "midpointNs", "roundTripNs", "observedLocalNs"];
    if (!fields.every(field => signed(event[field]))) throw new Error("Invalid actual multiplayer clock estimate.");
    return; // Clock/start ownership remains on the common session, with no Window HUD copy.
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
    session = network.localPlayers === null
      ? new BrowserMultiplayer(state.game.competition_identity(), network.host, 100000000n)
      : BrowserMultiplayer.new_group(localCompetitionIdentity(state, network.localPlayers), network.localPlayers, network.host, 100000000n);
    const opening = BrowserMultiplayerOwner.open(network.url, { session, now: networkNow,
      ...(network.localPlayers === null ? {} : { group: true }),
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
    const pending = Promise.resolve(network.localPlayers === null
      ? network.owner.submit(progressSnapshot(score), false)
      : network.owner.submit_group(groupProgressSnapshot(state), false));
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
      if (network.localPlayers === null) await owner.submit(progressSnapshot(score), true);
      else {
        if (network.finalGroup === null) throw new Error("Actual final local group progress is unavailable.");
        await owner.submit_group(network.finalGroup, true);
      }
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
  return multiplayerOutcome(network);
}

function roomAudioReady(state) {
  return state.prepared && state.game && state.mode === "live" && state.localPlan !== null
    && !state.active && !state.network && state.samplesEnded && state.commandsDrained
    && !commandsPending(state) && !state.audioPumping && state.audioRpcId === null
    && state.commandClient !== null && state.commandClient.state === "ready";
}

function roomProgressWords(value, players, name) {
  if (!(value instanceof Uint32Array) || !(value.buffer instanceof ArrayBuffer)
    || value.buffer.byteLength > 2816) throw new Error(`Invalid bounded ${name}.`);
  return groupWords(value, players, name);
}

function retainRoomReceipts(room, receipts) {
  if (!receipts || typeof receipts.localFinalWritten !== "boolean"
    || typeof receipts.localFinalAcknowledged !== "boolean" || typeof receipts.complete !== "boolean"
    || typeof receipts.drainComplete !== "boolean" || (receipts.drainComplete && !receipts.complete)
    || (receipts.localFinalAcknowledged && !receipts.localFinalWritten)
    || (receipts.complete && !receipts.localFinalAcknowledged)) throw new Error("Invalid actual room receipts.");
  room.receipts = Object.freeze({ localFinalWritten: receipts.localFinalWritten,
    localFinalAcknowledged: receipts.localFinalAcknowledged, complete: receipts.complete,
    drainComplete: receipts.drainComplete });
}

function retainRoomSnapshot(room, snapshot, participant) {
  if (!unsigned(participant) || participant === 0n || (room.participant !== null && room.participant !== participant)) {
    throw new Error("Room snapshot has no stable admitted participant.");
  }
  if (!snapshot || !integer(snapshot.phase, 0, 2)) throw new Error("Invalid actual room snapshot.");
  if (snapshot.phase === 2 && room.peerPlayers === null) {
    if (!Array.isArray(snapshot.members) || !integer(snapshot.members.length, 2, 64)) throw new Error("Invalid Prepared room roster.");
    const roster = new Map();
    for (const member of snapshot.members) {
      if (!member || !unsigned(member.participant) || member.participant === 0n || roster.has(member.participant)
        || member.prepared !== true || !(member.players instanceof Uint32Array)
        || !(member.players.buffer instanceof ArrayBuffer) || member.players.buffer.byteLength > 256) {
        throw new Error("Invalid Prepared room member.");
      }
      const players = networkWords(member.players, 1, 64, 1, "Prepared room players");
      if (players.some(player => player === 0) || new Set(players).size !== players.length) throw new Error("Invalid Prepared player identities.");
      roster.set(member.participant, Object.freeze(Array.from(players)));
    }
    const own = roster.get(participant);
    if (!own || own.length !== room.localPlayers.length || own.some((player, index) => player !== room.localPlayers[index])) {
      throw new Error("Prepared room changed the actual local roster.");
    }
    room.peerPlayers = roster;
  }
  room.participant = participant;
}

function roomOutcome(room) {
  const peers = [];
  if (room.peerPlayers !== null) for (const participant of room.peerPlayers.keys()) {
    const prefix = room.peers.get(participant);
    if (prefix) peers.push(prefix);
  }
  const errors = [room.failure, room.cleanupError].filter(value => value !== null)
    .map(value => message(value) || "Room failure.");
  return { participant: room.participant, finalQueued: room.finalQueued,
    finalWritten: room.receipts.localFinalWritten, finalAcknowledged: room.receipts.localFinalAcknowledged,
    localComplete: room.receipts.complete, finalDrain: room.finalDrain,
    error: errors.length ? message(errors.join("; cleanup: ")) : null, peers };
}

function roomHudFailure(state, room, error) {
  if (room.hudFailed) return;
  room.hudFailed = true;
  if (play === state && state.game) {
    try { state.game.disable_room_hud(); } catch {}
    report("play-room", { playId: state.id, event: { kind: "display-unavailable", error: message(error) || "Room score display unavailable." } });
    publishVisual();
  }
}

function updateRoomHud(state, room, action) {
  if (play !== state || !state.game || !room.hudConfigured || room.hudFailed) return;
  try { action(state.game); publishVisual(); }
  catch (error) { roomHudFailure(state, room, error); }
}

function configureRoomHud(state, room) {
  if (room.peerPlayers === null || room.hudConfigured || room.hudFailed || play !== state || !state.game) return false;
  try {
    const game = state.game;
    if (["configure_room_hud", "update_room_hud", "set_room_hud_status", "set_room_hud_page", "room_hud_pages", "disable_room_hud"]
      .some(name => typeof game[name] !== "function")) throw new Error("Room score display binding unavailable.");
    const { words, remote } = roomRosterWords(room);
    game.configure_room_hud(room.participant, words);
    const pages = game.room_hud_pages();
    if (!integer(pages, 1, 1008) || pages !== Math.ceil(remote / 4)) throw new Error("Room display returned an incorrect page count.");
    room.hudConfigured = true; room.hudPages = pages;
    publishVisual();
    return true;
  } catch (error) { roomHudFailure(state, room, error); return false; }
}

function roomRosterWords(room) {
  if (!room.peerPlayers || !unsigned(room.participant) || room.participant === 0n) throw new Error("Prepared room roster unavailable.");
  let length = 0, remote = 0;
  for (const [participant, players] of room.peerPlayers) {
    length += 3 + players.length;
    if (participant !== room.participant) remote += players.length;
  }
  if (!integer(length, 8, 4288) || !integer(remote, 1, 4032)) throw new Error("Invalid room display roster extent.");
  const words = new Uint32Array(length);
  let offset = 0;
  for (const [participant, players] of room.peerPlayers) {
    words[offset++] = Number(participant & 0xffffffffn);
    words[offset++] = Number(participant >> 32n);
    words[offset++] = players.length;
    words.set(players, offset); offset += players.length;
  }
  return { words, remote };
}

function retainRoomResults(state, natural) {
  const room = state.room;
  if (failed || state.resultsEpoch !== roomResultsEpoch || play !== null || !room.peerPlayers) return;
  let binding = null;
  const results = { id: state.id, lastRpc: state.lastRpc, binding: null,
    page: room.hudPage, pages: 0, failed: room.hudFailed };
  try {
    const { words, remote } = roomRosterWords(room);
    results.pages = Math.ceil(remote / 4);
    if (!integer(results.page, 0, results.pages - 1)) throw new Error("Invalid retained room page.");
    if (typeof BrowserRoomResults !== "function" || typeof BrowserRoomResults?.prototype?.visual_snapshot !== "function") {
      throw new Error("Retained room Results binding unavailable.");
    }
    binding = new BrowserRoomResults(room.participant, words);
    for (const participant of room.peerPlayers.keys()) {
      const prefix = room.peers.get(participant);
      if (prefix) binding.update(participant, prefix.sequence, prefix.finalPrefix,
        roomProgressWords(prefix.words, room.peerPlayers.get(participant), "retained room progress"));
    }
    const causes = [room.failure, room.cleanupError].filter(cause => cause !== null);
    // Bound Unicode text by at most 4096 UTF-8 bytes without changing the
    // operational diagnostics retained by the actual terminal room outcome.
    const diagnostic = causes.length === 0 ? null : Array.from(causes.map(message).join("; ")
      .replace(/[\u0000-\u001f\u007f-\u009f]/g, " ")).slice(0, 1024).join("");
    binding.freeze(results.page, !natural || room.finalDrain === "cancelled", diagnostic, results.failed);
    if (binding.page !== results.page || binding.pages !== results.pages || binding.failed !== results.failed) {
      throw new Error("Retained room Results changed their actual page metadata.");
    }
    results.binding = binding;
    binding = null;
  } catch {
    results.failed = true;
  } finally {
    try { binding?.free(); } catch {}
  }
  roomResults = results;
  state.roomResults = results;
  publishVisual();
}

function sendRoomProgress(state, final = false) {
  const room = state.room;
  if (play !== state || !state.active || !state.game || !room || !room.start || room.disposed
    || room.leaving || room.failure !== null || room.finalQueued || !room.owner || room.owner.closed) return;
  try {
    if (!room.owner.progressDue(final)) return;
    const words = roomProgressWords(state.game.progress_words(), room.localPlayers, "actual room progress");
    const accepted = room.owner.publishProgress(words, final);
    if (accepted && final) room.finalQueued = true;
  } catch (error) { roomFailure(state, room, error); }
}

function closeRoomOwner(room) {
  if (room.owner === null) return null;
  if (room.ownerClosing === null) {
    try { room.ownerClosing = Promise.resolve(room.owner.close()); }
    catch (error) { room.ownerClosing = Promise.reject(error); }
    room.ownerClosing.catch(() => {});
  }
  return room.ownerClosing;
}

function closeRoom(room) {
  if (!room) return Promise.resolve(null);
  if (room.closing !== null) return room.closing;
  if (room.owner !== null) {
    try {
      const participant = room.owner.participant;
      if (participant !== 0n) {
        if (!unsigned(participant) || (room.participant !== null && room.participant !== participant)) throw new Error("Invalid retained room participant.");
        room.participant = participant;
      }
      retainRoomReceipts(room, room.owner.receipts);
    } catch (error) { room.failure ??= error; }
  }
  room.disposed = true; // Fence callbacks before abort can synchronously notify.
  try { room.controller.abort(); } catch {}
  closeRoomOwner(room);
  room.closing = (async () => {
    // A rejected open joins its own continuations. A late successful open is
    // assigned below and closed here without touching the released game.
    try { await room.opening; } catch {}
    try { await closeRoomOwner(room); return room.cleanupError; }
    catch (error) { room.cleanupError ??= error; return room.cleanupError; }
  })();
  return room.closing;
}

function roomCallbacksCurrent(state, room) {
  return state.room === room && !room.disposed && !room.leaving
    && (play === state || roomFinalization?.state === state);
}

function cancelRoomFinalization() {
  const pending = roomFinalization;
  if (!pending) return null;
  pending.room.draining = false;
  pending.room.finalDrain = "cancelled";
  void closeRoom(pending.room);
  return pending.promise;
}

function finishRoom(state, natural) {
  const room = state.room;
  const pending = { state, room, promise: null };
  room.draining = natural;
  roomFinalization = pending;
  // Reserve the exact old owner before drain can invoke callbacks. No game
  // access occurs here; the caller releases it before this continuation runs.
  pending.promise = Promise.resolve().then(async () => {
    if (room.draining) {
      try {
        if (room.failure !== null || room.disposed || room.leaving || !room.finalQueued
          || !room.owner || room.owner.closed) throw room.failure ?? new Error("Room final drain is unavailable.");
        const receipts = await room.owner.drain();
        if (room.draining) {
          retainRoomReceipts(room, receipts);
          if (!room.receipts.drainComplete) throw new Error("Room drain returned without actual completion.");
          room.finalDrain = "complete";
        }
      } catch (error) {
        if (room.draining) {
          room.failure ??= error;
          room.finalDrain = "failed";
        }
      }
    }
    const cleanupError = await closeRoom(room);
    if ((cleanupError || room.failure !== null) && room.finalDrain === "complete") room.finalDrain = "failed";
    room.draining = false;
    retainRoomResults(state, natural);
    return cleanupError;
  }).finally(() => { if (roomFinalization === pending) roomFinalization = null; });
  return pending.promise;
}

function roomClosed(state, room, error) {
  if (play !== state || state.room !== room || room.closedReported) return;
  room.closedReported = true;
  report("play-room", { playId: state.id, event: { kind: "closed", error: message(error) } });
}

function roomFailure(state, room, error) {
  if (!roomCallbacksCurrent(state, room)) return;
  room.failure ??= error;
  updateRoomHud(state, room, game => game.set_room_hud_status(2));
  if (roomFinalization?.state === state && room.draining) {
    room.finalDrain = "failed";
    void closeRoom(room);
    return;
  }
  roomClosed(state, room, error);
  if (!state.active) failPlay(state, error);
  else {
    const rpcId = room.rpcId;
    room.rpcId = null;
    if (identity(rpcId)) report("play-reply", { playId: state.id, rpcId, error: message(error) });
    void closeRoom(room);
  }
}

function openRoom(state, request) {
  if (!roomAudioReady(state) || state.room !== null) {
    throw new Error("Room admission requires one pristine local game with completed audio preparation and no existing network.");
  }
  const windowOriginNs = request.windowOriginNs;
  if (!hostTime(windowOriginNs) || windowOriginNs !== state.windowOriginNs) throw new Error("Room start requires the actual live Window clock origin.");
  const url = request.url;
  if (typeof url !== "string" || url.length === 0 || url.length > 4096) {
    throw new Error("Room admission requires a canonical HTTPS room URL.");
  }
  let address;
  try { address = new URL(url); } catch {}
  if (!address || address.href !== url
    || address.protocol !== "https:" || !address.hostname || address.port === "0"
    || address.username || address.password || address.search || address.hash
    || !/^\/rooms\/[A-Za-z0-9_-]{1,1024}$/.test(address.pathname)) {
    throw new Error("Room admission requires a canonical HTTPS room URL.");
  }
  const methods = ROOM_SESSION_METHODS;
  if (typeof BrowserRoomClient !== "function" || typeof AbortController !== "function"
    || typeof BrowserRoomClient.new_with_start !== "function"
    || methods.some(name => typeof BrowserRoomClient.prototype?.[name] !== "function")
    || typeof state.game.competition_identity !== "function" || typeof state.game.progress_words !== "function") {
    throw new Error("The gameplay binding does not provide actual room start/progress ownership.");
  }
  const players = networkWords(state.game.players, 1, 64, 1, "actual room roster");
  if (players.length !== state.localPlan.members.length
    || players.some((player, index) => player !== state.localPlan.members[index].player)) {
    throw new Error("Actual room players differ from the frozen local plan.");
  }
  const identity = localCompetitionIdentity(state, players);
  const controller = new AbortController();
  let session = BrowserRoomClient.new_with_start(identity, players, 100000000n);
  // Configuration validation in Owner.open precedes its ownership transfer.
  // Validate the real instance too so a refused configuration stays local.
  try {
    if (methods.some(name => typeof session?.[name] !== "function")) throw new Error("Malformed room client binding.");
  } catch (error) {
    const refused = session;
    session = null;
    try { refused?.close(); } catch {}
    try { refused?.free(); } catch {}
    throw error;
  }
  const room = { owner: null, ownerClosing: null, controller, opening: null, closing: null,
    rpcId: request.rpcId, disposed: false, leaving: false, closedReported: false, cleanupError: null,
    windowOriginNs, originNs: null, start: null, failure: null, participant: null,
    localPlayers: Object.freeze(Array.from(players)), peerPlayers: null, peers: new Map(),
    finalQueued: false, draining: false, finalDrain: "cancelled",
    hudConfigured: false, hudFailed: false, hudPage: 0, hudPages: 0,
    receipts: Object.freeze({ localFinalWritten: false, localFinalAcknowledged: false, complete: false, drainComplete: false }) };
  const client = session;
  state.room = room; // One attempt per play; this slot is never reset or reused.
  // Install the joining promise before callbacks can fail reentrantly in open.
  room.opening = Promise.resolve().then(async () => {
    if (room.disposed || play !== state) {
      const abandoned = session;
      session = null;
      try { abandoned.close(); } catch (error) { room.cleanupError ??= error; }
      try { abandoned.free(); } catch (error) { room.cleanupError ??= error; }
      return;
    }
    const opening = BrowserRoomOwner.open(url, { session, now: networkNow, signal: controller.signal,
      onSnapshot: snapshot => {
        if (play !== state || state.room !== room || room.disposed || room.leaving) return;
        // Accepted callbacks can run before open returns the owner. The actual
        // session provides the participant; no roster position is substituted.
        const participant = room.owner?.participant ?? client.participant_id();
        retainRoomSnapshot(room, snapshot, participant);
        const configured = configureRoomHud(state, room);
        report("play-room", { playId: state.id, event: { kind: "snapshot", participant, snapshot } });
        if (configured) report("play-room", { playId: state.id, event: { kind: "score-pages", page: 0, pages: room.hudPages } });
      },
      onProgress: prefix => {
        if (!roomCallbacksCurrent(state, room)) return;
        if (!prefix || !unsigned(prefix.participant) || prefix.participant === 0n || prefix.participant === room.participant
          || !unsigned(prefix.sequence) || prefix.sequence === 0n || typeof prefix.finalPrefix !== "boolean"
          || !room.peerPlayers?.has(prefix.participant)) throw new Error("Invalid accepted room peer prefix.");
        const words = roomProgressWords(prefix.words, room.peerPlayers.get(prefix.participant), "accepted room peer progress");
        room.peers.set(prefix.participant, Object.freeze({ participant: prefix.participant,
          sequence: prefix.sequence, finalPrefix: prefix.finalPrefix, words }));
        updateRoomHud(state, room, game => game.update_room_hud(prefix.participant, prefix.sequence, prefix.finalPrefix, words));
      },
      onReceipts: receipts => {
        if (!roomCallbacksCurrent(state, room)) return;
        retainRoomReceipts(room, receipts);
      },
      onStart: (schedule, originNs) => {
        if (play !== state || state.room !== room || room.disposed || room.leaving) return;
        if (state.active || room.start !== null || !hostTime(originNs)
          || (room.originNs !== null && room.originNs !== originNs)
          || !schedule || !hostTime(schedule.targetNs) || !hostTime(schedule.songTargetNs)
          || !hostTime(schedule.uncertaintyNs)) throw new Error("Invalid committed room start.");
        // The callback supplies the same original owner origin even when it
        // precedes open's return. Never substitute a later clock observation.
        const offset = originNs - room.windowOriginNs;
        const targetHostNs = offset + schedule.targetNs;
        const songTargetHostNs = offset + schedule.songTargetNs;
        if (!hostTime(targetHostNs) || !hostTime(songTargetHostNs)
          || songTargetHostNs - targetHostNs !== 100000000n) {
          throw new Error("Committed room start cannot map to the Window clock.");
        }
        room.originNs = originNs;
        room.start = Object.freeze({ targetHostNs, songTargetHostNs, uncertaintyNs: schedule.uncertaintyNs });
        updateRoomHud(state, room, game => game.set_room_hud_status(1));
        report("play-room", { playId: state.id, event: { kind: "start", ...room.start } });
      },
      onClose: error => {
        if (!room.leaving) roomFailure(state, room, error);
      } });
    session = null; // Accepted Owner.open owns and frees its actual client once.
    let owner;
    try { owner = await opening; }
    catch (error) {
      // Only configuration validation precedes Owner.open's ownership transfer.
      // Core/transport failures already close and free the consumed client.
      if (error?.code === "validation") session = client;
      throw error;
    }
    room.owner = owner;
    if (room.disposed || play !== state) { await closeRoomOwner(room); return; }
    if (owner.closed) throw new Error("Room closed before acquisition completed.");
    const originNs = owner.origin;
    if (!hostTime(originNs) || (room.originNs !== null && room.originNs !== originNs)) {
      throw new Error("Room owner clock origin changed during acquisition.");
    }
    room.originNs = originNs;
    const rpcId = room.rpcId;
    room.rpcId = null;
    report("play-reply", { playId: state.id, rpcId, result: { kind: "room-opened" } });
  }).catch(error => {
    if (error?.cleanupError) room.cleanupError ??= error.cleanupError;
    if (session !== null) {
      const local = session;
      session = null;
      try { local.close(); } catch (failure) { room.cleanupError ??= failure; }
      try { local.free(); } catch (failure) { room.cleanupError ??= failure; }
    }
    roomFailure(state, room, error);
  });
}

function roomRequest(state, request) {
  try {
    if (request.kind === "play-room-open") { openRoom(state, request); return; }
    const room = state.room;
    if (request.kind === "play-room-page") {
      if (!room || !state.game || !state.prepared || room.hudFailed || !room.hudConfigured || room.rpcId !== null) {
        throw new Error("Room score display is unavailable.");
      }
      if (!integer(request.page, 0, room.hudPages - 1)) throw new Error("Room score page is out of range.");
      try { state.game.set_room_hud_page(request.page); }
      catch (error) { roomHudFailure(state, room, error); throw error; }
      room.hudPage = request.page;
      publishVisual();
      reply(state, request, { kind: "room-page", page: room.hudPage, pages: room.hudPages });
      return;
    }
    if (!room || room.disposed || room.leaving || room.rpcId !== null || !room.owner || room.owner.closed) {
      throw new Error("Wait for the current room owner before requesting a room action.");
    }
    if (request.kind === "play-room-leave") {
      room.leaving = true; // Its expected onClose is not a gameplay failure.
      room.rpcId = request.rpcId;
      void (async () => {
        try {
          await room.owner.leave();
          const cleanupError = await closeRoom(room);
          if (cleanupError) throw cleanupError;
          if (play !== state || room.rpcId !== request.rpcId) return;
          room.rpcId = null;
          reply(state, request, { kind: "room-left", leaveWritten: true });
          roomClosed(state, room, "Room leave was written.");
        } catch (error) {
          if (play !== state || room.rpcId !== request.rpcId) return;
          if (error?.code === "state" && !room.owner.closed) {
            room.leaving = false;
            room.rpcId = null;
            report("play-reply", { playId: state.id, rpcId: request.rpcId, error: message(error) });
          } else {
            if (room.disposed) {
              // A joined Leave cleanup can fail after its room was fenced.
              // Settle that RPC without reviving or disposing active gameplay.
              room.failure ??= error;
              room.rpcId = null;
              report("play-reply", { playId: state.id, rpcId: request.rpcId, error: message(error) });
              roomClosed(state, room, error);
              if (!state.active) failPlay(state, error);
            } else roomFailure(state, room, error);
          }
        }
      })();
      return;
    }
    if (!roomAudioReady(state)) throw new Error("Room requests require completed local audio preparation.");
    const operation = request.kind === "play-room-seal" ? "seal" : "ready";
    if (operation === "seal") room.owner.requestSeal();
    else room.owner.requestReady();
    reply(state, request, { kind: "room-requested", operation });
  } catch (error) {
    if (play === state) report("play-reply", { playId: state.id, rpcId: request.rpcId, error: message(error) });
  }
}

function disposeGame(state) {
  const sampleClient = state.sampleClient;
  state.sampleClient = null;
  sampleClient?.close();
  const client = state.commandClient;
  state.commandClient = null;
  state.commandPumping = false;
  state.audioPumping = false;
  state.renderObservation = null;
  state.lastPresentation = null;
  state.gamepadAdapter = null;
  state.pointerSources?.clear();
  state.pointerSources = null;
  state.sourceOrder.clear();
  client?.close();
  let captureError = null;
  try { captureCompletedResults(state); } catch (error) { captureError = error; }
  const game = state.game;
  state.game = null;
  const result = { cleanupError: captureError, replay: null, replayError: null,
    completedArchive: null, archivePlayers: null, archiveError: null,
    ...(state.localPlan ? { replays: state.localPlan.members.map(({ player }) => ({ player, replay: null, replayError: null, replayComplete: false })) } : {}) };
  if (!game) return result;
  // Rust alone admits genuine whole-roster completion; export before consuming captures.
  if (state.mode === "live" && state.recordReplay) {
    try {
      const bytes = game.completed_archive();
      if (bytes != null) {
        if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
          || bytes.buffer.resizable === true || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
          || bytes.byteLength === 0 || bytes.byteLength > 5 * 1024 * 1024) {
          throw new Error("Completed archive has an invalid bounded transferable layout.");
        }
        result.completedArchive = bytes;
        result.archivePlayers = state.localPlan ? state.localPlan.members.map(({ player }) => player) : [1];
      }
    } catch (cause) { result.archiveError = message(cause); }
  }
  let stopped = false;
  try { game.stop(); stopped = true; } catch (cause) { result.cleanupError ??= cause; }
  if (state.localPlan && state.recordReplay && stopped) {
    let total = 0;
    const buffers = new Set();
    for (const row of result.replays) {
      try {
        const bytes = game.take_replay(row.player);
        if (bytes == null) continue; // A setup failure may precede this member's capture.
        if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
          || bytes.buffer.resizable === true || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
          || bytes.length === 0 || bytes.length > state.recordLimits.bytes
          || total + bytes.length > 64 * 1024 * 1024 || buffers.has(bytes.buffer)) {
          throw new Error("Local recorded replay has an invalid bounded transferable layout.");
        }
        total += bytes.length;
        buffers.add(bytes.buffer);
        row.replay = bytes;
      } catch (cause) { row.replayError = message(cause); }
    }
  } else if (state.mode === "live" && state.recordReplay && stopped) {
    try {
      const bytes = game.take_replay();
      if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
        || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
        || bytes.length === 0 || bytes.length > 64 * 1024 * 1024) throw new Error("Recorded replay has an invalid bounded transferable layout.");
      result.replay = bytes;
    } catch (cause) { result.replayError = message(cause); }
  }
  if (result.completedArchive && (result.replay?.buffer === result.completedArchive.buffer
    || result.replays?.some(row => row.replay?.buffer === result.completedArchive.buffer))) {
    result.completedArchive = null;
    result.archivePlayers = null;
    result.archiveError = "Completed archive must own a separate transferable buffer.";
  }
  try { game.free(); } catch (cause) { result.cleanupError ??= cause; }
  return result;
}

function replayTransfers(replay, replays, completedArchive) {
  const buffers = replays ? replays.filter(row => row.replay !== null).map(row => row.replay.buffer)
    : replay ? [replay.buffer] : [];
  if (completedArchive) buffers.push(completedArchive.buffer);
  return buffers;
}

function failPlay(state, error, request = null) {
  if (play !== state) return;
  ++capturesPending;
  const score = statistics(state);
  const savedOpponents = finalOpponents(state);
  retainFinalGroup(state);
  sendRoomProgress(state, true);
  play = null; // Invalidates a still-awaiting preparation before releasing owners.
  fenceVisual();
  closeNetwork(state.network);
  const roomClosing = state.room ? finishRoom(state, false) : null;
  const { cleanupError, replay, replayError, replays, completedArchive, archivePlayers, archiveError } = disposeGame(state);
  const text = message(cleanupError ? `${message(error)}; cleanup: ${message(cleanupError)}` : error);
  const pending = new Set([request?.rpcId, state.startRpcId, state.sampleRpcId, state.audioRpcId, state.network?.rpcId, state.room?.rpcId]);
  state.startRpcId = null;
  state.sampleRpcId = null;
  state.audioRpcId = null;
  if (state.network) state.network.rpcId = null;
  if (state.room) state.room.rpcId = null;
  for (const rpcId of pending) if (identity(rpcId)) report("play-reply", { playId: state.id, rpcId, error: text });
  const finished = roomError => {
    retainCompletedResults(state, cleanupError ?? roomError ?? error);
    report("play-error", { playId: state.id,
    completedResults: completedResultsMetadata(state.completedResults), completedResultsError: state.completedResults?.displayError ?? null,
    message: roomError ? message(`${text}; room cleanup: ${message(roomError)}`) : text,
    released: cleanupError === null && roomError === null,
    completedArchive, archivePlayers, archiveError, replay, replayComplete: false, replayError, ...score, ...(replays ? { replays } : {}),
    ...(state.network ? { multiplayer: multiplayerOutcome(state.network) } : {}),
    ...(state.room ? { room: roomOutcome(state.room), roomResults: roomResultsMetadata(state.roomResults) } : {}),
    ...(savedOpponents ? { savedOpponents } : {}) }, replayTransfers(replay, replays, completedArchive));
    captureDelivered();
    if (state.fatalAfterCapture) fatal(state.fatalAfterCapture);
  };
  if (roomClosing) void roomClosing.then(finished, cause => finished(cause));
  else finished(null);
  publishVisual();
}

function stopPlay(state, request) {
  if (request.completed !== undefined && typeof request.completed !== "boolean") throw new Error("Invalid stopped-play completion choice.");
  const completed = request.completed === true;
  if (completed && (!state.completed || commandsPending(state) || state.audioPumping
    || state.renderObservation !== null)) throw new Error("Natural stop has no current completion evidence.");
  ++capturesPending;
  const score = statistics(state);
  const savedOpponents = finalOpponents(state);
  retainFinalGroup(state);
  sendRoomProgress(state, true);
  play = null;
  fenceVisual();
  if (state.network) {
    state.network.stopping = true;
    clearRemoteProgress(state.network);
  }
  const roomClosing = state.room ? finishRoom(state, completed) : null;
  const { cleanupError, replay, replayError, replays, completedArchive, archivePlayers, archiveError } = disposeGame(state);
  const pending = new Set([state.startRpcId, state.sampleRpcId, state.audioRpcId, state.network?.rpcId, state.room?.rpcId]);
  state.startRpcId = null;
  state.sampleRpcId = null;
  state.audioRpcId = null;
  if (state.network) state.network.rpcId = null;
  if (state.room) state.room.rpcId = null;
  for (const rpcId of pending) if (identity(rpcId)) {
    report("play-reply", { playId: state.id, rpcId, error: "Gameplay setup was stopped." });
  }
  const stopped = (multiplayer, roomError = null) => {
    const cleanupFailure = cleanupError ?? roomError;
    retainCompletedResults(state, cleanupFailure);
    if (replays) for (const row of replays) row.replayComplete = !cleanupFailure && completed && row.replay !== null && row.replayError === null;
    const result = { completedArchive, archivePlayers, archiveError, replay, replayError, ...score, completedResults: completedResultsMetadata(state.completedResults), completedResultsError: state.completedResults?.displayError ?? null, ...(multiplayer ? { multiplayer } : {}),
      ...(state.room ? { room: roomOutcome(state.room), roomResults: roomResultsMetadata(state.roomResults) } : {}),
      ...(savedOpponents ? { savedOpponents } : {}), ...(replays ? { replays } : {}) };
    if (cleanupFailure) report("play-error", { playId: state.id, message: message(cleanupFailure), released: false,
      ...result, replayComplete: false }, replayTransfers(replay, replays, completedArchive));
    else report("play-stopped", { playId: state.id, ...result,
      replayComplete: completed && replay !== null }, replayTransfers(replay, replays, completedArchive));
    captureDelivered();
  };
  publishVisual();
  // The game and samples are already released. Network disposal cannot delay
  // local ownership release or turn its failure into an incomplete replay.
  if (roomClosing) void roomClosing.then(error => stopped(null, error), cause => stopped(null, cause));
  else if (state.network) void drainNetwork(state, score).then(stopped, cause => stopped(null, cause));
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

function hidConfiguration(value, mode, physicalInput, keyboardWords) {
  if (value === undefined) return null;
  if (mode !== "live" || !physicalInput || !value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("HID setup requires live canonical physical input ownership.");
  }
  const { bindingWords, deviceWords, fieldWords, axisParams } = value;
  if (!(bindingWords instanceof Uint32Array) || bindingWords.length > 256 * 7 || bindingWords.length % 7 !== 0
    || !(deviceWords instanceof Uint32Array) || deviceWords.length < 6 || deviceWords.length > 16 * 6 || deviceWords.length % 6 !== 0
    || !(fieldWords instanceof Uint32Array) || fieldWords.length > 16 * 512 * 13 || fieldWords.length % 13 !== 0
    || !(axisParams instanceof Float32Array) || axisParams.length !== fieldWords.length / 13 * 2
    || keyboardWords.length + bindingWords.length > 256 * 7) {
    throw new Error("HID setup requires bounded complete numeric device, field, parameter and binding rows.");
  }
  // Snapshot all bounded arrays before setup can await readiness or file reads.
  // The actual Rust owner validates field syntax and physical binding membership.
  const snapshot = { bindingWords: bindingWords.slice(), deviceWords: deviceWords.slice(),
    fieldWords: fieldWords.slice(), axisParams: axisParams.slice(), sources: new Set(), lanes: new Set() };
  for (let index = 0; index < snapshot.deviceWords.length; index += 6) {
    const source = BigInt(snapshot.deviceWords[index]) | (BigInt(snapshot.deviceWords[index + 1]) << 32n);
    if (source < 3n || snapshot.sources.has(source)) throw new Error("HID sources must be distinct full-width identities at least three.");
    snapshot.sources.add(source);
  }
  for (let index = 0; index < snapshot.bindingWords.length; index += 7) {
    const lane = snapshot.bindingWords[index];
    if (!integer(lane, 0x11, 0x19) && !integer(lane, 0x21, 0x29)) throw new Error("HID constructor binding has an invalid BMS lane.");
    snapshot.lanes.add(lane);
  }
  return snapshot;
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
    if (state.mode === "live") {
      if (!hostTime(request.windowOriginNs)) throw new Error("Live gameplay requires the actual Window clock origin.");
      state.windowOriginNs = request.windowOriginNs;
      state.latencyHint = request.latencyHint;
      // Validate the selected startup allowance without reading any clock.
      audioClockExpired(0n, 0n, null, state.latencyHint);
    }
    if (request.inputMode !== undefined && (!["physical", "physical-contact"].includes(request.inputMode) || state.mode !== "live")) {
      throw new Error("Invalid live gameplay input mode.");
    }
    state.touchInput = request.inputMode === "physical-contact";
    state.physicalInput = request.inputMode === "physical" || state.touchInput;
    if (request.localPlanWords !== undefined) {
      if (state.mode !== "live" || !state.physicalInput) throw new Error("Local players require live canonical physical input.");
      state.localPlan = snapshotLocalPlan(request.localPlanWords);
      state.localPage = request.localPage === undefined ? 0 : request.localPage;
      if (!integer(state.localPage, 0, Math.ceil(state.localPlan.members.length / 4) - 1)) throw new Error("Invalid initial local player page.");
    } else if (request.localPage !== undefined) throw new Error("A local page requires a local source plan.");
    let pairs = null;
    let bindingWords = null;
    const lanes = [];
    const keys = new Set();
    if (state.mode === "live") {
      if (!(request.keyPairs instanceof Uint32Array) || request.keyPairs.length > 36 || request.keyPairs.length % 2 !== 0) throw new Error("Invalid bounded gameplay key bindings.");
      pairs = request.keyPairs.slice();
      for (let index = 0; index < pairs.length; index += 2) {
        lanes.push(pairs[index]);
        if (!integer(pairs[index + 1], 1, 65535) || keys.has(pairs[index + 1])) throw new Error("Gameplay keys must be valid and unique.");
        keys.add(pairs[index + 1]);
      }
      bindingsFor(lanes);
      if (state.physicalInput) bindingWords = keyboardBindingWords(pairs);
    }
    let pointer = null;
    if (request.pointerSetup !== undefined) {
      if (state.mode !== "live" || !state.physicalInput) throw new Error("Pointer setup requires live canonical physical input ownership.");
      pointer = snapshotPointerSetup(request.pointerSetup);
    }
    let gamepad = null;
    let gamepadAdapter = null;
    let gamepadDevices = null;
    let gamepadProfileFile = null;
    let gamepadProfileSize = 0;
    if (request.gamepadSetup !== undefined || request.gamepadDevices !== undefined || request.gamepadProfileFile !== undefined) {
      if (state.mode !== "live" || !state.physicalInput) throw new Error("Gamepad setup requires live canonical physical input ownership.");
      if (request.gamepadSetup !== undefined && (request.gamepadDevices !== undefined || request.gamepadProfileFile !== undefined)) {
        throw new Error("Choose Gamepad devices or explicit numeric bindings, not both.");
      }
      if (request.gamepadProfileFile !== undefined) {
        if (!(request.gamepadProfileFile instanceof File) || !integer(request.gamepadProfileFile.size, 1, 1024 * 1024)) {
          throw new Error("Gamepad profiles require a nonempty file no larger than 1 MiB.");
        }
        gamepadProfileFile = request.gamepadProfileFile;
        gamepadProfileSize = gamepadProfileFile.size;
        gamepadDevices = snapshotGamepadDevices(request.gamepadDevices);
      } else {
        gamepad = request.gamepadDevices === undefined ? snapshotGamepadSetup(request.gamepadSetup)
          : automaticGamepadSetup(request.gamepadDevices);
        if (bindingWords.length + gamepad.physicalWords.length > 256 * 7) throw new Error("Combined physical binding capacity exceeded.");
        gamepadAdapter = new GamepadAdapter(gamepad);
      }
    }
    let hid = hidConfiguration(request.hidSetup, state.mode, state.physicalInput, bindingWords);
    let hidProfileFile = null;
    let hidProfileSize = 0;
    let hidDevices = null;
    if (request.hidProfileFile !== undefined || request.hidDevices !== undefined) {
      if (request.hidSetup !== undefined || state.mode !== "live" || !state.physicalInput
        || !(request.hidProfileFile instanceof File)
        || !integer(request.hidProfileFile.size, 1, 1024 * 1024)) {
        throw new Error("HID profile files require exclusive live physical setup and a nonempty file no larger than 1 MiB.");
      }
      hidProfileFile = request.hidProfileFile;
      hidProfileSize = hidProfileFile.size;
      hidDevices = snapshotHidDevices(request.hidDevices);
    }
    const timing = state.mode === "live" ? validateTiming(request.timing) : null;
    const requestedStart = state.mode === "live" ? validateStart(request.startNs) : null;
    const requestedEnd = state.mode === "live" ? validateEnd(requestedStart, request.endNs) : undefined;
    state.network = multiplayerConfiguration(request.multiplayer, state.mode, state.localPlan);
    if (state.network && state.network.windowOriginNs !== state.windowOriginNs) {
      throw new Error("Multiplayer and live input Window clock origins differ.");
    }
    const opponents = state.mode === "live" && request.opponents !== undefined
      ? validateSelections(request.opponents) : NO_OPPONENTS;
    validateOpponentTargets(opponents, state.localPlan ? state.localPlan.members.map(member => member.player) : null);
    state.opponentSelections = opponents;
    state.localOpponentErrors = new Map();
    state.localOpponentNotified = new Set();
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
    if (hidProfileFile !== null) {
      const bytes = await hidProfileFile.arrayBuffer();
      if (failed || play !== state) return;
      if (!(bytes instanceof ArrayBuffer) || bytes.byteLength !== hidProfileSize) throw new Error("HID profile file size changed or returned an invalid buffer.");
      hid = hidConfiguration(hidSetupFromProfile(new Uint8Array(bytes), hidDevices), state.mode, state.physicalInput, bindingWords);
    }
    if (gamepadProfileFile !== null) {
      const bytes = await gamepadProfileFile.arrayBuffer();
      if (failed || play !== state) return;
      if (!(bytes instanceof ArrayBuffer) || bytes.resizable === true || bytes.byteLength !== gamepadProfileSize) throw new Error("Gamepad profile file size changed or returned an invalid buffer.");
      gamepad = gamepadSetupFromProfile(new Uint8Array(bytes), gamepadDevices);
      gamepadAdapter = new GamepadAdapter(gamepad);
    }
    if (gamepad !== null && hid !== null && gamepad.sources.some(source => hid.sources.has(source))) {
      throw new Error("Gamepad and HID source identities must not overlap.");
    }
    if (pointer !== null && pointer.sources.some(source => hid?.sources.has(source) || gamepad?.sources.includes(source))) {
      throw new Error("Pointer, Gamepad and HID source identities must not overlap.");
    }
    if (state.localPlan && hid !== null && !state.localPlan.members.some(member => member.source === null)
      && [...hid.sources].some(source => !state.localPlan.members.some(member => member.source === source))) {
      throw new Error("Every configured HID source must belong to a local player.");
    }
    if (bindingWords !== null && bindingWords.length + (hid?.bindingWords.length ?? 0)
      + (gamepad?.physicalWords.length ?? 0) + (pointer?.physicalWords.length ?? 0) > 256 * 7) throw new Error("Combined physical binding capacity exceeded.");
    if (hid !== null) {
      const combined = new Uint32Array(bindingWords.length + hid.bindingWords.length);
      combined.set(bindingWords);
      combined.set(hid.bindingWords, bindingWords.length);
      bindingWords = combined;
      for (const lane of hid.lanes) if (!lanes.includes(lane)) lanes.push(lane);
    }
    if (gamepad !== null) {
      const combined = new Uint32Array(bindingWords.length + gamepad.physicalWords.length);
      combined.set(bindingWords);
      combined.set(gamepad.physicalWords, bindingWords.length);
      bindingWords = combined;
      for (const lane of gamepad.lanes) if (!lanes.includes(lane)) lanes.push(lane);
    }
    let pressBindingWords = null;
    if (pointer !== null) {
      if (state.localPlan) {
        let buttonRows = 0;
        for (let index = 6; index < pointer.physicalWords.length; index += 7) {
          if (pointer.physicalWords[index] !== 0) buttonRows++;
        }
        pressBindingWords = new Uint32Array(bindingWords.length + buttonRows * 7);
        pressBindingWords.set(bindingWords);
        let offset = bindingWords.length;
        for (let index = 0; index < pointer.physicalWords.length; index += 7) {
          if (pointer.physicalWords[index + 6] === 0) continue;
          pressBindingWords.set(pointer.physicalWords.subarray(index, index + 7), offset);
          offset += 7;
        }
      }
      const combined = new Uint32Array(bindingWords.length + pointer.physicalWords.length);
      combined.set(bindingWords);
      combined.set(pointer.physicalWords, bindingWords.length);
      bindingWords = combined;
      for (const lane of pointer.lanes) if (!lanes.includes(lane)) lanes.push(lane);
    }
    if (state.mode === "replay") {
      if (request.recordReplay === true) throw new Error("Replay playback cannot record live input.");
      const bytes = await replayFile.arrayBuffer();
      if (failed || play !== state) return;
      if (!(bytes instanceof ArrayBuffer) || bytes.byteLength !== replaySize) throw new Error("Replay file size changed or returned an invalid buffer.");
      prepared = library.prepare_replay_chart(request.path, new Uint8Array(bytes), request.rate, 2,
        64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES);
    } else {
      if (typeof request.seed !== "string" || !/^\d{1,20}$/.test(request.seed) || BigInt(request.seed) > U64_MAX) throw new Error("Invalid gameplay chart seed.");
      prepared = requestedStart === 0n
        ? library.prepare_chart(request.path, request.rate, 2, BigInt(request.seed), 64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES)
        : library.prepare_chart_at(request.path, request.rate, 2, BigInt(request.seed), requestedStart,
          64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES);
    }
    const actualStart = prepared.start_ns;
    const startNs = actualStart === undefined && requestedStart === 0n ? 0n : actualStart;
    if (typeof startNs !== "bigint") throw new Error("Prepared chart omitted its actual song start.");
    validateStart(startNs);
    if (state.mode === "live" && startNs !== requestedStart) throw new Error("Prepared live section start differs from its request.");
    const chartLanes = Array.from(prepared.lanes);
    bindingsFor(chartLanes);
    if (state.mode === "live" && !state.localPlan && chartLanes.some(lane => !lanes.includes(lane))) {
      throw new Error(hid === null && gamepad === null && pointer === null ? "A prepared lane has no supplied key binding." : "A prepared lane has no supplied press-capable physical binding.");
    }
    const metadata = { title: prepared.title, artist: prepared.artist, notes: prepared.note_count, lanes: chartLanes, startNs };
    let localBindings = null;
    if (state.localPlan) {
      // Position/displacement controls cannot establish local press coverage.
      if (pressBindingWords !== null) localBindingWords(state.localPlan, pressBindingWords, chartLanes, state.touchInput);
      localBindings = localBindingWords(state.localPlan, bindingWords, chartLanes, state.touchInput);
      state.touchPlayer = localBindings.touchPlayer;
      if (state.touchPlayer !== null && Math.floor(state.localPlan.members.findIndex(member => member.player === state.touchPlayer) / 4) !== state.localPage) {
        throw new Error("The touch player must be visible on the initial local page.");
      }
      if (typeof BrowserLocalGame?.new_physical !== "function" || typeof BrowserLocalGame?.prototype?.visual_registration !== "function") {
        throw new Error("The gameplay binding does not provide local player ownership and rendering.");
      }
    }
    const Game = state.localPlan ? BrowserLocalGame : BrowserGame;
    const physicalConstructor = state.localPlan || !state.touchInput ? Game?.new_physical : Game?.new_physical_contact;
    if (state.physicalInput && (typeof physicalConstructor !== "function"
      || typeof Game?.prototype?.queue_input_blob !== "function")) {
      throw new Error("The gameplay binding does not provide canonical physical input ownership.");
    }
    if (state.touchInput && (typeof Game?.prototype?.configure_touch_regions !== "function"
      || typeof Game?.prototype?.[state.localPlan ? "queue_input_blob_on_surface_on_page" : "queue_input_blob_on_surface"] !== "function"
      || typeof Game?.prototype?.preflight_touch_surface !== "function"
      || (state.localPlan && (typeof Game?.prototype?.touch_bounds !== "function"
        || typeof Game?.prototype?.set_touch_page !== "function")))) {
      throw new Error("The gameplay binding does not provide contact routing ownership.");
    }
    if (hid !== null && (typeof Game?.prototype?.configure_hid_devices !== "function"
      || typeof Game?.prototype?.queue_hid_blob !== "function")) {
      throw new Error("The gameplay binding does not provide HID profile ownership.");
    }
    if (state.mode === "live" && !["queue_input", "close_input_prefix", "service_audio", "pending_inputs",
      "admit_output", "observe_presentation", "evaluate_completion"].every(name => typeof Game?.prototype?.[name] === "function")) {
      throw new Error("The gameplay binding does not provide joined audio and acquired-input ownership.");
    }
    if (opponents.length && (typeof Game?.prototype?.add_saved_opponent !== "function"
      || typeof Game?.prototype?.saved_opponents !== "function"
      || typeof Game?.prototype?.disable_saved_opponent_hud !== "function")) {
      throw new Error("The gameplay binding does not provide retained saved comparison presentation.");
    }
    if (state.network && (typeof Game?.prototype?.update_peer_hud !== "function"
      || typeof Game?.prototype?.disable_peer_hud !== "function"
      || typeof Game?.prototype?.competition_identity !== "function"
      || (state.localPlan && (typeof Game?.prototype?.configure_peer_hud !== "function"
        || typeof Game?.prototype?.progress_words !== "function"
        || typeof BrowserMultiplayer?.new_group !== "function")))) {
      throw new Error("The gameplay binding does not provide retained peer presentation.");
    }
    if (state.mode === "live" && !state.physicalInput && requestedEnd !== undefined && typeof BrowserGame.new_section !== "function") {
      throw new Error("The gameplay binding does not provide finite section ownership.");
    }
    const moved = prepared;
    prepared = null; // A consuming Rust constructor also owns the argument on Err.
    state.game = state.mode === "replay"
      ? new BrowserReplay(moved, 100000000n)
      : state.localPlan
        ? BrowserLocalGame.new_physical(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs,
          state.localPlan.words, localBindings.words, requestedEnd, state.touchInput, 4096, 1024)
      : state.touchInput
        ? BrowserGame.new_physical_contact(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, bindingWords, requestedEnd, 4096, 1024)
        : state.physicalInput
          ? BrowserGame.new_physical(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, bindingWords, requestedEnd, 4096, 1024)
          : requestedEnd === undefined
            ? new BrowserGame(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, pairs)
            : BrowserGame.new_section(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, pairs, requestedEnd);
    if (state.localPlan) {
      const actual = state.game.players;
      if (!(actual instanceof Uint32Array) || !(actual.buffer instanceof ArrayBuffer)
        || actual.buffer.resizable === true || actual.length !== state.localPlan.members.length
        || actual.some((player, index) => player !== state.localPlan.members[index].player)) {
        throw new Error("Actual local player identities differ from the prepared source plan.");
      }
      metadata.localPlayers = Array.from(actual);
      metadata.localPage = state.localPage;
      if (state.network) state.network.localPlayers = networkWords(actual, 1, 64, 1, "actual local network roster");
    }
    if (state.network && !state.localPlan) updatePeerHud(state);
    if (state.physicalInput) metadata.inputMode = request.inputMode;
    if (hid !== null) {
      state.game.configure_hid_devices(hid.deviceWords, hid.fieldWords, hid.axisParams);
      state.hidSources = hid.sources;
      metadata.hidSourceCount = hid.sources.size;
      metadata.hidSources = [...hid.sources];
    }
    if (gamepad !== null) {
      state.gamepadAdapter = gamepadAdapter;
      metadata.gamepadSources = state.localPlan && !state.localPlan.members.some(member => member.source === null)
        ? gamepad.sources.filter(source => state.localPlan.members.some(member => member.source === source)) : gamepad.sources;
    }
    if (pointer !== null) {
      const devices = state.localPlan && !state.localPlan.members.some(member => member.source === null)
        ? pointer.devices.filter(device => state.localPlan.members.some(member => member.source === device.source)) : pointer.devices;
      state.pointerSources = new Map(devices.map(device => [device.source, { pointerType: device.pointerType, controls: new Set() }]));
      for (let index = 0; index < pointer.bindingWords.length; index += 4) {
        const source = BigInt(pointer.bindingWords[index + 1]) | (BigInt(pointer.bindingWords[index + 2]) << 32n);
        state.pointerSources.get(source)?.controls.add(pointer.bindingWords[index + 3]);
      }
      metadata.pointerDevices = devices;
    }
    const output = replayOutputFromMetadata(startNs, state.game.end_ns, state.game.playback_end_frame, request.rate);
    if (state.mode === "live" && output.endNs !== requestedEnd) throw new Error("Actual live section end differs from its request.");
    if (output.endFrame !== undefined) {
      metadata.endNs = output.endNs;
      metadata.endFrame = output.endFrame;
    }
    for (const opponent of opponents) {
      const bytes = await opponent.file.arrayBuffer();
      // A stopped owner may have freed its game during this unabortable read.
      if (failed || play !== state) return;
      if (!(bytes instanceof ArrayBuffer) || bytes.byteLength !== opponent.file.size) throw new Error("Opponent recording returned an invalid buffer or changed size.");
      const expected = state.localPlan ? state.opponentSelections.slice(0, state.opponentCount).filter(entry => entry.player === opponent.player).length : state.opponentCount;
      const index = state.localPlan
        ? state.game.add_saved_opponent(opponent.player, new Uint8Array(bytes), opponent.own, opponent.label)
        : state.game.add_saved_opponent(new Uint8Array(bytes), opponent.own, opponent.label);
      if (index !== expected) throw new Error("Actual saved opponent admission count changed.");
      state.opponentCount++;
    }
    if (state.network && state.localPlan) {
      for (const player of state.network.localPlayers) state.game.configure_peer_hud(player);
      updateNetworkHud(state);
    }
    if (state.touchInput) {
      const width = state.game.touch_width;
      const height = state.game.touch_height;
      if (!integer(width, 1, 0xffffffff) || !integer(height, 1, 0xffffffff)) {
        throw new Error("Actual touch layout dimensions or lane bounds are invalid.");
      }
      if (!state.localPlan || state.touchPlayer !== null) {
        const bounds = state.localPlan ? state.game.touch_bounds(state.touchPlayer, state.localPage) : state.game.touch_bounds;
        if (!(bounds instanceof Float32Array) || bounds.length !== chartLanes.length * 4) throw new Error("Actual touch layout lane bounds are invalid.");
        for (let index = 0; index < bounds.length; index += 4) {
          if (!Number.isFinite(bounds[index]) || !Number.isFinite(bounds[index + 1])
            || !Number.isFinite(bounds[index + 2]) || !Number.isFinite(bounds[index + 3])
            || bounds[index] >= bounds[index + 2] || bounds[index + 1] >= bounds[index + 3]) {
            throw new Error("Actual touch layout has invalid region bounds.");
          }
        }
        const touchWords = touchBindingWords(chartLanes);
        if (state.localPlan) {
          const member = state.localPlan.members.find(member => member.player === state.touchPlayer);
          if (member.source !== null) for (let index = 0; index < touchWords.length; index += 7) {
            touchWords[index + 1] = 1; touchWords[index + 2] = 2;
          }
          state.game.configure_touch_regions(state.touchPlayer, touchWords, bounds, 256);
        } else state.game.configure_touch_regions(touchWords, bounds, 256);
      }
      state.touchWidth = width;
      state.touchHeight = height;
    }
    if (state.mode === "replay") {
      const recordedUntilNs = state.game.recorded_until_ns ?? null;
      if (recordedUntilNs !== null && !signed(recordedUntilNs)) throw new Error("Invalid actual replay prefix extent.");
      metadata.mode = "replay";
      metadata.recordedUntilNs = recordedUntilNs;
    } else if (request.recordReplay === true) {
      if (state.localPlan) {
        state.recordLimits = { bytes: Math.floor(64 * 1024 * 1024 / state.localPlan.members.length),
          records: Math.floor(1000000 / state.localPlan.members.length) };
        metadata.recordLimits = { ...state.recordLimits };
        state.recordReplay = true; // Preserve earlier configured prefixes if a later setup refuses.
        for (const { player } of state.localPlan.members) state.game.configure_capture(player, state.recordLimits.bytes, state.recordLimits.records);
      } else {
        state.game.configure_capture(64 * 1024 * 1024, 1000000);
        state.recordReplay = true;
      }
    }
    state.keys = keys;
    state.prepared = true;
    const samples = state.game.sample_count();
    if (!integer(samples, 0, PLAY_PCM_SAMPLES)) throw new Error("Prepared PCM sample count exceeds the bounded section capacity.");
    state.sampleCount = samples;
    reply(state, request, { kind: "prepared", samples, opponentCount: state.opponentCount, ...metadata });
    state.startRpcId = null;
    publishVisual();
  } catch (error) {
    if (play === state) failPlay(state, error, request);
  } finally { prepared?.free(); }
}

function nextSample(game) {
  const sample = game.next_sample();
  if (sample == null) return null;
  let result = null;
  let failure = null;
  try {
    const id = sample.id;
    const rate = sample.rate;
    const channels = sample.channels;
    const pcm = sample.take_pcm();
    if (!unsigned(id) || !integer(rate, 1, 0xffffffff) || channels !== 2
      || !(pcm instanceof Float32Array) || !(pcm.buffer instanceof ArrayBuffer)
      || pcm.buffer.resizable === true || pcm.byteOffset !== 0
      || pcm.byteLength !== pcm.buffer.byteLength || pcm.length % channels !== 0) {
      throw new Error("Prepared sample has an invalid transferable layout.");
    }
    result = { kind: "sample", id, rate, channels, pcm };
  } catch (error) { failure = { cause: error }; }
  try { sample.free(); } catch (error) { failure ??= { cause: error }; }
  if (failure !== null) throw failure.cause;
  return result;
}

function samplePlay(state, request) {
  if (state.active || state.commandClient !== null || state.sampleMode === "direct") {
    throw new Error("Samples belong to one setup producer before command handoff.");
  }
  state.sampleMode = "legacy";
  const game = state.game;
  const result = nextSample(game);
  if (failed || play !== state || state.game !== game) return;
  if (result === null) { state.samplesEnded = true; reply(state, request, { kind: "samples-end" }); return; }
  reply(state, request, result, [result.pcm.buffer]);
}

function commandBatch(state) {
  state.commandStarted = true;
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

function commandsPending(state) {
  return state.batch !== null || state.commandPumping
    || (state.commandClient !== null && !state.commandsDrained);
}

function closeAudioHandoff(request) {
  if (request?.kind === "play-audio" || request?.kind === "play-samples-upload") {
    try { request.port?.close(); } catch {}
  }
}

function uploadSamples(state, request) {
  let adopted = false;
  try {
    const port = request.port;
    const generation = request.generation;
    const channels = request.channels;
    const timeoutMs = request.timeoutMs;
    const suppliedLimits = request.pcmLimits;
    const limits = { maxAssetBytes: suppliedLimits?.maxAssetBytes,
      maxTotalBytes: suppliedLimits?.maxTotalBytes, maxSamples: suppliedLimits?.maxSamples };
    if (state.active || state.sampleMode !== null || state.samplesEnded || state.sampleClient !== null
      || state.sampleRpcId !== null || state.commandStarted || state.commandClient !== null
      || state.audioRpcId !== null || state.batch !== null || state.audioPumping
      || state.commandPumping || state.commandsDrained || !integer(state.sampleCount, 0, PLAY_PCM_SAMPLES)
      || generation !== state.id || channels !== 2
      || limits.maxAssetBytes !== 64 * 1024 * 1024 || limits.maxTotalBytes !== 256 * 1024 * 1024
      || limits.maxSamples !== PLAY_PCM_SAMPLES || !integer(timeoutMs, 1, 60000)
      || !port || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")) {
      throw new Error("Direct samples require one matching bounded endpoint in pristine gameplay setup.");
    }
    if (failed || play !== state || state.game === null) { closeAudioHandoff(request); return; }
    const game = state.game;
    const expected = state.sampleCount;
    state.sampleMode = "direct";
    state.sampleRpcId = request.rpcId;
    // Valid constructor admission owns the endpoint even when port.start fails.
    adopted = true;
    const client = new AudioSampleClient({ port, generation, channels, pcmLimits: limits, timeoutMs });
    if (failed || play !== state || state.game !== game) { client.close(); return; }
    state.sampleClient = client;
    const current = () => !failed && play === state && state.game === game
      && state.sampleClient === client && state.sampleRpcId === request.rpcId;
    const reportAdmission = () => {
      if (current() && (client.state === "ready" || client.state === "ended")) {
        report("play-samples-admitted", { playId: state.id, rpcId: request.rpcId, count: expected });
      }
    };
    void (async () => {
      let count = 0;
      let bytes = 0;
      while (count < expected) {
        if (!current()) return;
        const sample = nextSample(game);
        if (!current()) return;
        if (sample === null) throw new Error("Prepared PCM bank ended before its declared sample count.");
        const sampleBytes = sample.pcm.byteLength;
        if (count === 0) {
          // First-request validation must finish before announcing admission.
          // A detached empty buffer otherwise looks like a valid zero-byte asset.
          if (!integer(sampleBytes, 0, limits.maxAssetBytes)) throw new Error("Prepared PCM asset exceeds its byte limit.");
          new Float32Array(sample.pcm.buffer, 0, 0);
        }
        if (!current()) return;
        const pending = client.sample(sample);
        if (count === 0) {
          pending.catch(() => {}); // A throwing notification still owns rejection cleanup.
          reportAdmission();
        }
        await pending;
        if (!current()) return;
        count += 1;
        bytes += sampleBytes;
      }
      if (!current()) return;
      const extra = game.next_sample();
      if (extra != null) {
        extra.free();
        if (!current()) return;
        throw new Error("Prepared PCM bank exceeds its declared sample count.");
      }
      if (!current()) return;
      const ending = client.end();
      if (expected === 0) {
        ending.catch(() => {});
        reportAdmission();
      }
      const ended = await ending;
      if (!current()) return;
      if (ended.count !== expected || ended.count !== count || ended.bytes !== bytes
        || !integer(bytes, 0, limits.maxTotalBytes)) {
        throw new Error("Direct PCM acknowledgement did not preserve the prepared bank.");
      }
      state.samplesEnded = true;
      state.sampleRpcId = null;
      reply(state, request, { kind: "samples-uploaded", count, bytes });
    })().catch(error => {
      if (play === state && state.game === game) failPlay(state, error, request);
    });
  } catch (error) {
    if (!adopted) closeAudioHandoff(request);
    throw error;
  }
}

function attachAudio(state, request) {
  let adopted = false;
  try {
    if (state.active || !state.samplesEnded || state.commandClient !== null || state.audioRpcId !== null
      || state.batch !== null || state.audioPumping || state.commandsDrained
      || request.generation !== state.id || !integer(request.queueCapacity, 1, 65536)
      || Math.min(256, request.queueCapacity) !== state.commandBatchLimit
      || !integer(request.timeoutMs, 1, 60000) || !request.port
      || !["postMessage", "start", "close"].every(name => typeof request.port[name] === "function")) {
      throw new Error("Direct audio requires one matching bounded endpoint after sample preparation.");
    }
    // Valid constructor admission owns cleanup even when starting the port fails.
    adopted = true;
    state.commandClient = new AudioCommandClient({ port: request.port, generation: request.generation,
      queueCapacity: request.queueCapacity, timeoutMs: request.timeoutMs });
    state.audioRpcId = request.rpcId;
    pumpAudio(state);
  } catch (error) {
    if (!adopted) closeAudioHandoff(request);
    throw error;
  }
}

function currentWindowHost(state) {
  const now = networkNow() - state.windowOriginNs;
  if (!hostTime(now)) throw new Error("Current Window-equivalent HOST time is invalid.");
  return now;
}

function pendingInputs(state) {
  const pending = state.mode === "live" ? state.game.pending_inputs() : 0;
  if (!integer(pending, 0, 65536)) throw new Error("Invalid retained input count.");
  return pending;
}

function presentationUnavailable(state, reason) {
  if (state.presentationReason === reason) return;
  state.presentationReason = reason;
  report("play-presentation-unavailable", { playId: state.id, reason });
}

function serviceLiveAudio(state, now, audioNs) {
  const processed = state.game.service_audio(now, audioNs);
  if (!integer(processed, 0, 65536)) throw new Error("Invalid processed input count.");
  if (audioClockExpired(now, state.origin, state.lastPresentation?.hostNs ?? null, state.latencyHint)) {
    throw new Error("Audio presentation clock unavailable: no progressing accepted association within the declared timeout.");
  }
  return processed;
}

function observeOutput(state, observation, output) {
  renderedCursor(output, state.startFrame);
  let completed;
  if (state.mode === "replay") {
    completed = state.game.observe_output(output.words, observation.presentedNs);
  } else {
    // An asynchronous Worklet poll may outlive the original request timestamp.
    // Refresh Window-equivalent time while retaining the original association.
    const now = currentWindowHost(state);
    let pair = Object.hasOwn(observation, "timestamp")
      ? observation.timestamp === null ? null
        : presentationPair(observation.timestamp, state.startFrame, state.rate, Number(now) / 1000000)
      : observation.presentedNs === null ? null
        : { outputNs: observation.presentedNs, hostNs: observation.presentedHostNs };
    if (pair !== null && (pair.hostNs > now || now - pair.hostNs > 1000000000n)) pair = null;
    const availability = presentationAvailability(state.lastPresentation, pair);
    pair = availability.pair;
    state.game.admit_output(output.words, pair?.outputNs ?? null);
    if (pair !== null) {
      state.game.observe_presentation(pair.outputNs, pair.hostNs);
      state.lastPresentation = pair;
      if (state.presentationReason !== null) presentationUnavailable(state, null);
    } else if (availability.reason !== null) presentationUnavailable(state, availability.reason);
    if (reportWord(output.words, 23) === 1n) {
      const audioNs = audioScheduleFromFrame(reportWord(output.words, 24), state.startFrame, state.rate);
      if (audioNs > state.audioNs) state.audioNs = audioNs;
    }
    serviceLiveAudio(state, now, state.audioNs);
    completed = state.game.evaluate_completion();
  }
  if (typeof completed !== "boolean" || (completed && (state.batch !== null || state.commandPumping))) {
    throw new Error("Invalid completion with outstanding gameplay commands.");
  }
  state.completed = completed;
  if (completed && state.mode === "live") captureCompletedResults(state);
  state.commandsDrained = false; // Actual output may admit more BGM work.
  state.completed = completed;
  state.lastRender = observation.renderId;
  return completed;
}

function snapshotPresentation(state, request, direct) {
  const raw = Object.hasOwn(request, "timestamp");
  if (raw) {
    if (Object.hasOwn(request, "presentedNs") || Object.hasOwn(request, "presentedHostNs")) {
      throw new Error("Raw and projected presentation observations cannot be mixed.");
    }
    const input = request.timestamp;
    const observedNowMs = request.observedNowMs;
    if (typeof observedNowMs !== "number" || !Number.isFinite(observedNowMs) || observedNowMs < 0
      || (input !== null && (typeof input !== "object" || Array.isArray(input)))) {
      throw new Error("Invalid raw presentation observation.");
    }
    const timestamp = input === null ? null : Object.freeze({
      contextTime: input.contextTime, performanceTime: input.performanceTime,
    });
    const nowMs = state.mode === "live" ? Number(currentWindowHost(state)) / 1000000 : observedNowMs;
    const pair = timestamp === null ? null : presentationPair(timestamp, state.startFrame, state.rate, nowMs);
    return Object.freeze({ renderId: request.renderId,
      timestamp, presentedNs: pair?.outputNs ?? null, presentedHostNs: pair?.hostNs ?? null });
  }
  if (Object.hasOwn(request, "observedNowMs")) throw new Error("Raw presentation time requires its timestamp snapshot.");
  if (!direct && state.mode === "replay") {
    if (!(request.presentedNs === null || hostTime(request.presentedNs))) throw new Error("Invalid replay output presentation point.");
  } else if (!(request.presentedNs === null && request.presentedHostNs === null)
    && !(hostTime(request.presentedNs) && hostTime(request.presentedHostNs))) throw new Error("Invalid output presentation pair.");
  return Object.freeze({ renderId: request.renderId,
    presentedNs: request.presentedNs, presentedHostNs: request.presentedHostNs });
}

function publishRender(state, observation, completed) {
  if (completed && commandsPending(state)) throw new Error("Completion produced outstanding gameplay commands.");
  sendRoomProgress(state, completed);
  report("play-render-done", { playId: state.id, renderId: observation.renderId, completed,
    commandsPending: commandsPending(state), pendingInputs: pendingInputs(state), observedTick: state.lastTick,
    ...statistics(state) });
  if (state.mode === "live") {
    sendProgress(state, statistics(state));
    publishOpponents(state);
  }
  publishVisual();
}

async function drainAudio(state) {
  const game = state.game;
  const client = state.commandClient;
  const current = () => play === state && state.game === game && state.commandClient === client;
  try {
    while (current()) {
      if (client.state !== "ready") throw client.failure ?? new Error("Direct audio command owner is unavailable.");
      let batch;
      if (state.active && state.renderObservation !== null) {
        const observation = state.renderObservation;
        const output = await client.poll();
        if (!current()) return;
        // Input can advance during the read. Apply the original presentation
        // pair to the actual current game, retaining its latest input frontier.
        const completed = observeOutput(state, observation, output);
        state.renderObservation = null;
        batch = commandBatch(state);
        publishRender(state, observation, completed);
      } else batch = commandBatch(state);
      if (batch === null) break;
      state.commandPumping = true;
      let ack;
      try { ack = await client.commands(batch.commands); }
      catch (error) {
        if (!current()) return;
        if (integer(error?.admitted, 0, batch.commands.length)) {
          try { game.acknowledge(batch.sequence, error.admitted, false); }
          catch { /* Retain the original processor rejection and its admitted prefix. */ }
        }
        throw error;
      }
      if (!current()) return;
      // Transport sequence is independent; acknowledge the genuine core batch.
      game.acknowledge(batch.sequence, ack.admitted, true);
      state.batch = null;
      state.commandPumping = false;
      // The next iteration services a waiting report before extracting another
      // batch; report-driven BGM credits cannot be starved by repeated commands.
    }
    if (!current()) return;
    state.audioPumping = false;
    if (state.audioRpcId !== null) {
      const rpcId = state.audioRpcId;
      state.audioRpcId = null;
      report("play-reply", { playId: state.id, rpcId, result: { kind: "audio-ready", commandsPending: false } });
    }
  } catch (error) {
    if (current()) failPlay(state, error);
  } finally {
    if (current()) { state.commandPumping = false; state.audioPumping = false; }
  }
}

function pumpAudio(state) {
  if (state.commandClient !== null) {
    if (state.audioPumping || (!state.active && state.audioRpcId === null)) return;
    state.audioPumping = true;
    void drainAudio(state);
    return;
  }
  if (!state.active || state.batch !== null) return;
  const batch = commandBatch(state);
  if (batch !== null) report("play-commands", { playId: state.id, batch });
}

function disableOpponents(state, error) {
  if (state.opponentsFailed) return state.opponentError;
  state.opponentsFailed = true;
  state.opponentError = message(error) || "Saved comparison failed.";
  try {
    if (state.localPlan) for (const member of state.localPlan.members) state.game.disable_saved_opponent_hud(member.player);
    else state.game.disable_saved_opponent_hud();
  }
  catch (cause) { state.opponentError = message(`${state.opponentError}; disable saved HUD: ${message(cause)}`); }
  return state.opponentError;
}

function readLocalOpponents(state) {
  const groups = validateLocalOpponentSnapshot(state.game.saved_opponents(), state.localPlan.members.map(member => member.player), state.opponentSelections);
  return groups.map(group => {
    const prior = state.localOpponentErrors.get(group.player);
    if (prior) return { player: group.player, opponents: null, error: prior };
    if (group.error !== null) {
      state.localOpponentErrors.set(group.player, group.error);
      try { state.game.disable_saved_opponent_hud(group.player); }
      catch (error) { state.localOpponentErrors.set(group.player, message(error)); }
      return { player: group.player, opponents: null, error: state.localOpponentErrors.get(group.player) };
    }
    return group;
  });
}

function finalOpponents(state) {
  if (state.opponentCount === 0) return null;
  if (state.opponentsFailed) return { opponents: null, error: state.opponentError };
  try {
    // Read once before stop/free. The binding uses only the actual local frontier.
    if (state.localPlan) return { localOpponents: readLocalOpponents(state), opponents: null, error: null };
    return { opponents: validateOpponentSnapshot(state.game.saved_opponents(), state.opponentCount), error: null };
  } catch (error) {
    return { opponents: null, error: disableOpponents(state, error) };
  }
}

function publishOpponents(state) {
  if (state.opponentCount === 0 || state.opponentsFailed) return;
  try {
    // Display cadence only; the Rust owner advances from actual gameplay time.
    const now = millisecondsToNanos(self.performance.now());
    if (state.lastOpponents !== null && now < state.lastOpponents) throw new Error("Saved comparison display clock regressed.");
    if (state.lastOpponents !== null && now - state.lastOpponents < PROGRESS_INTERVAL_NS) return;
    state.lastOpponents = now;
    // The getter refreshes the retained Rust HUD. Normal counters stay here.
    if (state.localPlan) {
      const groups = readLocalOpponents(state);
      for (const group of groups) if (group.error !== null && !state.localOpponentNotified.has(group.player)) {
        state.localOpponentNotified.add(group.player);
        report("play-opponents", { playId: state.id, player: group.player, opponents: null, error: group.error });
      }
    } else validateOpponentSnapshot(state.game.saved_opponents(), state.opponentCount);
  } catch (error) {
    report("play-opponents", { playId: state.id, opponents: null, error: disableOpponents(state, error) });
  }
}

function stepPlay(state, request) {
  if (state.mode !== "live") throw new Error("Replay playback cannot accept live gameplay steps.");
  if (!state.active || !identity(request.tickId) || request.tickId <= state.lastTick
    || !Array.isArray(request.events) || request.events.length > 256
    || !hostTime(request.nowNs)
    || !(request.watermark === null || hostTime(request.watermark))) throw new Error("Invalid active gameplay step.");
  const rawFrame = Object.hasOwn(request, "contextFrame");
  if (rawFrame === Object.hasOwn(request, "audioNs")) throw new Error("Choose exactly one raw or projected audio schedule.");
  const audioNs = rawFrame ? audioScheduleFromFrame(request.contextFrame, state.startFrame, state.rate) : request.audioNs;
  if (!hostTime(audioNs)) throw new Error("Invalid projected audio schedule.");
  let ignored = 0;
  let fanout = 0;
  let gamepadDraft = null;
  const sourceOrder = new Map(state.sourceOrder);
  const entries = [];
  // Validate the complete bounded batch before the first actual Runtime call.
  for (let index = 0; index < request.events.length; index++) {
    let event = request.events[index];
    if (!event || typeof event !== "object" || Array.isArray(event)) {
      throw new Error("Invalid gameplay input or source chronology.");
    }
    const kind = event.kind;
    if (kind === "pointer" || kind === "pointer-button") {
      const { pointerType, hostNs, source, sequence, code, control } = event;
      event = kind === "pointer"
        ? { kind, pointerType, hostNs, source, sequence, code, control, mode: event.mode, x: event.x, y: event.y }
        : { kind, pointerType, hostNs, source, sequence, code, control, state: event.state };
    }
    if (!hostTime(event.hostNs) || event.hostNs > request.nowNs || !unsigned(event.sequence)) {
      throw new Error("Invalid gameplay input or source chronology.");
    }
    let encoded = null;
    let source;
    if (event.kind === "gamepad") {
      if (state.gamepadAdapter === null) throw new Error("Gamepad input requires an admitted physical profile.");
      gamepadDraft ??= state.gamepadAdapter.fork();
      const bytes = gamepadDraft.decode(event);
      encoded = { kind: "gamepad", bytes };
      fanout += bytes.length;
      if (fanout > 256) throw new Error("Canonical input fanout capacity exceeded.");
      if (event.hostNs < state.origin) ignored++;
      // A browser may expose an unchanged state with its old acquisition time.
      // Keep source order in the draft without rewinding the global frontier.
      if (bytes.length === 0) continue;
    } else if (event.kind === "hid") {
      if (state.hidSources === null || !state.hidSources.has(event.source)) throw new Error("HID input requires an admitted source profile.");
      encoded = { kind: "hid", bytes: encodeRawHidEvent(event) };
      source = event.source;
    } else if (event.kind === "pointer" || event.kind === "pointer-button") {
      const device = state.pointerSources?.get(event.source);
      if (!device || device.pointerType !== event.pointerType
        || (event.kind === "pointer" ? event.control !== 0
          : !integer(event.control, 1, 32) || !device.controls.has(event.control))) {
        throw new Error("Pointer input requires an admitted source, matching type and configured button control.");
      }
      encoded = { kind: event.kind, bytes: event.kind === "pointer" ? encodePointerEvent(event) : encodePointerButtonEvent(event) };
      source = event.source;
    } else if (event.kind === "touch") {
      if (!state.touchInput) throw new Error("Touch input requires the prepared contact mode.");
      if (!integer(event.surfaceWidth, 1, 0xffffffff) || !integer(event.surfaceHeight, 1, 0xffffffff)) {
        throw new Error("Touch input requires a positive original backing extent.");
      }
      if (state.localPlan && !integer(event.page, 0, Math.ceil(state.localPlan.members.length / 4) - 1)) {
        throw new Error("Local touch input requires its original admitted acquisition page.");
      }
      const bytes = encodeTouchEvent(event); // Includes original CSS/sample validation.
      state.game.preflight_touch_surface(Math.fround(event.x), Math.fround(event.y),
        event.width, event.height, event.surfaceWidth, event.surfaceHeight);
      encoded = { kind: "touch", bytes, width: event.width, height: event.height,
        surfaceWidth: event.surfaceWidth, surfaceHeight: event.surfaceHeight, ...(state.localPlan ? { page: event.page } : {}) };
      source = 2n;
    } else {
      if (event.kind !== undefined || !integer(event.key, 1, 65535) || !state.keys.has(event.key)
        || typeof event.down !== "boolean") throw new Error("Invalid gameplay keyboard input.");
      if (state.physicalInput) encoded = { kind: "keyboard", bytes: encodeKeyboardEvent(event) };
      source = 1n;
    }
    if (event.kind !== "gamepad") {
      fanout++;
      if (fanout > 256) throw new Error("Canonical input fanout capacity exceeded.");
      if (event.hostNs < state.origin) ignored++;
      const previous = sourceOrder.get(source);
      if (previous && (event.hostNs < previous.hostNs || event.sequence < previous.sequence)) {
        throw new Error("Gameplay input regressed within its original source chronology.");
      }
      sourceOrder.set(source, { hostNs: event.hostNs, sequence: event.sequence });
    }
    entries.push({ event, encoded, index });
  }
  // Core sequences belong to each DeviceId. Different sources may be sampled
  // in another order; preserve all original metadata while ordering timestamps.
  entries.sort((a, b) => a.event.hostNs < b.event.hostNs ? -1 : a.event.hostNs > b.event.hostNs ? 1 : a.index - b.index);
  const firstLive = entries.find(entry => entry.event.hostNs >= state.origin);
  if (firstLive && state.acquiredPrefix !== null && firstLive.event.hostNs < state.acquiredPrefix) {
    throw new Error("Changed gameplay input precedes the closed acquired prefix.");
  }
  if (request.watermark !== null && (request.watermark > request.nowNs
    || (state.acquiredPrefix !== null && request.watermark < state.acquiredPrefix))) throw new Error("Gameplay watermark regressed or exceeds acquisition time.");
  if (!Number.isSafeInteger(state.preOriginInputs + ignored)) throw new Error("Pre-origin input count overflow.");
  // The envelope samples the same Window clock after its original events.
  // Reconstructed Worker time need not preserve that cross-global causal order;
  // fresh service below holds input until its own current sample covers it.
  const received = request.nowNs;
  if (gamepadDraft !== null) state.gamepadAdapter = gamepadDraft;
  state.sourceOrder = sourceOrder;
  state.preOriginInputs += ignored;
  state.lastTick = request.tickId;
  state.completed = false;
  state.commandsDrained = false;
  for (const { event, encoded: entry } of entries) {
    if (event.hostNs >= state.origin && entry !== null) {
      if (entry.kind === "hid") state.game.queue_hid_blob(entry.bytes, received);
      else if (entry.kind === "touch" && state.localPlan) state.game.queue_input_blob_on_surface_on_page(entry.bytes,
        entry.width, entry.height, entry.surfaceWidth, entry.surfaceHeight, entry.page, received);
      else if (entry.kind === "touch") state.game.queue_input_blob_on_surface(entry.bytes,
        entry.width, entry.height, entry.surfaceWidth, entry.surfaceHeight, received);
      else if (entry.kind === "gamepad") for (const bytes of entry.bytes) state.game.queue_input_blob(bytes, received);
      else state.game.queue_input_blob(entry.bytes, received);
    } else if (event.hostNs >= state.origin) state.game.queue_input(event.hostNs, event.key, event.down, event.sequence, received);
    if (state.lastHost === null || event.hostNs > state.lastHost) state.lastHost = event.hostNs;
    state.lastSequence = event.sequence;
  }
  if (request.watermark !== null) {
    // Freshly acquired events may lie beyond this lagged prefix. Their original
    // entries stay pending until a later complete prefix covers them.
    if (request.watermark >= state.origin) state.game.close_input_prefix(request.watermark);
    state.acquiredPrefix = request.watermark;
  }
  if (audioNs > state.audioNs) state.audioNs = audioNs;
  serviceLiveAudio(state, currentWindowHost(state), state.audioNs);
  const score = statistics(state);
  pumpAudio(state);
  if (play !== state) return;
  report("play-step-done", { playId: state.id, tickId: request.tickId, commandsPending: commandsPending(state),
    pendingInputs: pendingInputs(state), ...score });
  publishVisual();
  sendProgress(state, score);
  sendRoomProgress(state);
  publishOpponents(state);
}

function handlePlay(request) {
  if (!identity(request.playId)) {
    closeAudioHandoff(request);
    if (play) failPlay(play, new Error("Invalid gameplay identity."), request);
    return;
  }
  if (request.kind === "play-start" && play === null) {
    if (request.playId <= lastPlayId) return;
    discardRoomResults();
    const priorRoom = cancelRoomFinalization();
    const state = {
      resultsEpoch: roomResultsEpoch, roomResults: null, completedResults: null,
      id: request.playId, startRpcId: identity(request.rpcId) ? request.rpcId : null,
      game: null, keys: null, active: false, origin: null, startFrame: null,
      batch: null, commandClient: null, commandPumping: false, audioPumping: false,
      sampleMode: null, sampleClient: null, sampleRpcId: null, sampleCount: null, commandStarted: false,
      audioRpcId: null, renderObservation: null, lastPresentation: null,
      windowOriginNs: null, latencyHint: undefined, audioNs: 0n, presentationReason: null,
      lastRpc: 0, lastTick: 0, lastRender: 0,
      lastHost: null, acquiredPrefix: null, lastSequence: null, sourceOrder: new Map(), preOriginInputs: 0,
      recordReplay: false, completed: false,
      localPlan: null, localPage: 0, touchPlayer: null, recordLimits: null,
      mode: "live", physicalInput: false, touchInput: false, touchWidth: null, touchHeight: null,
      hidSources: null, gamepadAdapter: null, pointerSources: null,
      rate: null, network: null, room: null, samplesEnded: false, commandsDrained: false,
      prepared: false, opponentCount: 0, opponentsFailed: false, opponentError: null, lastOpponents: null,
    };
    play = state; // Reserve before the ready await so stop cannot race a late owner.
    lastPlayId = state.id;
    void (async () => {
      if (priorRoom) {
        const cleanupError = await priorRoom;
        if (play !== state) return;
        if (cleanupError) throw new Error("Previous room cleanup failed; reload before playing again.", { cause: cleanupError });
      }
      if (play === state) await preparePlay(state, request);
    })().catch(fatal);
    return;
  }
  if (completedResults?.id === request.playId && play === null
    && ["play-results-present", "play-results-page"].includes(request.kind)) {
    const results = completedResults;
    try {
      const room = roomResults?.id === results.id ? roomResults : null;
      results.lastRpc = Math.max(results.lastRpc, room?.lastRpc ?? 0);
      const prior = { ...results };
      rpc(results, request, true);
      if (room) room.lastRpc = results.lastRpc;
      const next = resultRequest(prior, request);
      if (request.kind === "play-results-page") {
        results.binding.set_presentation(next.page, next.comparisons);
        if (results.binding.page !== next.page || results.binding.pages !== next.pages
          || results.binding.comparisons !== next.comparisons) throw new Error("Results binding changed admitted page metadata.");
      }
      Object.assign(results, next);
      reply(results, request, { kind: "completed-results", completedResults: completedResultsMetadata(results) });
      if (request.kind === "play-results-page") queueVisualControl("page", { page: results.page, comparisons: results.comparisons, geometryVersion: nextGeometry(request.geometryVersion) });
      else publishVisual();
    } catch (error) {
      if (identity(request.rpcId)) report("play-reply", { playId: results.id, rpcId: request.rpcId, error: message(error) });
    }
    return;
  }
  if (roomResults?.id === request.playId && play === null) {
    const results = roomResults;
    try {
      const local = completedResults?.id === results.id ? completedResults : null;
      results.lastRpc = Math.max(results.lastRpc, local?.lastRpc ?? 0);
      rpc(results, request, true);
      if (local) local.lastRpc = results.lastRpc;
      if (request.kind !== "play-room-page") throw new Error("Joined room Results accept only page requests.");
      if (results.failed || !results.binding) throw new Error("Room Results display unavailable.");
      if (!integer(request.page, 0, results.pages - 1)) throw new Error("Invalid room Results page.");
      try {
        results.binding.set_page(request.page);
        if (results.binding.page !== request.page || results.binding.pages !== results.pages
          || results.binding.failed !== false) throw new Error("Room Results page changed its admitted metadata.");
        results.page = request.page;
      } catch (error) { roomResultsFailure(results, error); throw error; }
      reply(results, request, { kind: "room-page", page: results.page, pages: results.pages });
      queueVisualControl(local?.shown ? "room-page" : "page", { page: results.page, comparisons: false, geometryVersion: nextGeometry(request.geometryVersion) });
    } catch (error) {
      closeAudioHandoff(request);
      if (identity(request.rpcId)) report("play-reply", { playId: results.id, rpcId: request.rpcId, error: message(error) });
    }
    return;
  }
  if (request.kind === "play-stop" && roomFinalization?.state.id === request.playId) {
    if (request.completed !== undefined && typeof request.completed !== "boolean") return;
    if (request.completed !== true) cancelRoomFinalization();
    return;
  }
  if (request.kind === "play-room-leave" && roomFinalization?.state.id === request.playId) {
    const previous = roomFinalization.state;
    try {
      rpc(previous, request, true);
      cancelRoomFinalization();
      report("play-reply", { playId: previous.id, rpcId: request.rpcId,
        error: "Room drain cancelled; gameplay already stopped." });
    } catch (error) {
      report("play-reply", { playId: previous.id, rpcId: request.rpcId, error: message(error) });
    }
    return;
  }
  const state = play;
  if (!state || request.playId !== state.id) { closeAudioHandoff(request); return; }
  let audioHandled = false;
  try {
    if (request.kind === "play-stop") { stopPlay(state, request); return; }
    if (request.kind === "play-start") throw new Error("Gameplay setup is already owned by this identity.");
    const roomRpc = ["play-room-open", "play-room-seal", "play-room-ready", "play-room-leave", "play-room-page"].includes(request.kind);
    const requiresRpc = roomRpc || ["play-sample", "play-samples-upload", "play-audio", "play-commands", "play-activate", "play-network-ready", "play-page"].includes(request.kind);
    if (request.rpcId !== undefined && !requiresRpc && request.kind !== "play-ack") throw new Error("Unexpected gameplay RPC identity.");
    rpc(state, request, requiresRpc);
    if (roomRpc) { roomRequest(state, request); return; }
    if (request.kind === "play-page") {
      let reason = null;
      if (!state.localPlan || !state.game || !state.prepared) reason = "Wait for actual local player preparation before paging.";
      else if (!integer(request.page, 0, Math.ceil(state.localPlan.members.length / 4) - 1)) reason = "Invalid local player page.";
      else if (state.active && pendingInputs(state) !== 0) reason = "Wait for acquired input to be processed before changing the local page.";
      let touchVisible;
      if (reason === null && state.touchPlayer !== null && state.touchPlayer !== undefined) {
        try { touchVisible = state.game.set_touch_page(state.touchPlayer, request.page); }
        catch (error) { reason = message(error); }
        const slot = state.localPlan.members.findIndex(member => member.player === state.touchPlayer);
        if (reason === null && touchVisible !== (Math.floor(slot / 4) === request.page)) throw new Error("Local touch page returned invalid visibility.");
      }
      if (reason !== null) report("play-reply", { playId: state.id, rpcId: request.rpcId, error: reason });
      else {
        const version = nextGeometry(request.geometryVersion);
        state.localPage = request.page;
        if (visual && currentVisual(visual)) visual.frameGeometry = version;
        reply(state, request, { kind: "local-page", page: state.localPage, ...(touchVisible === undefined ? {} : { touchVisible }) });
        publishVisual();
      }
      return;
    }
    if (!state.game || !state.prepared) throw new Error("Wait for actual gameplay preparation.");
    if (request.kind === "play-sample") samplePlay(state, request);
    else if (request.kind === "play-samples-upload") { audioHandled = true; uploadSamples(state, request); }
    else if (request.kind === "play-audio") { audioHandled = true; attachAudio(state, request); }
    else if (request.kind === "play-network-ready") {
      if (state.room !== null || !state.network || state.network.requested || state.active || !state.samplesEnded
        || !state.commandsDrained || commandsPending(state)) throw new Error("Multiplayer readiness requires completed sample and command preparation.");
      void networkReady(state, request);
    } else if (request.kind === "play-commands") {
      if (state.sampleMode === "direct" && (!state.samplesEnded || state.sampleRpcId !== null)) {
        throw new Error("Direct samples require their genuine end acknowledgement before commands.");
      }
      if (state.commandClient !== null) throw new Error("Commands belong to the direct audio endpoint.");
      if (state.batch !== null) throw new Error("An actual audio batch is still awaiting acknowledgement.");
      reply(state, request, commandBatch(state));
    } else if (request.kind === "play-ack") {
      if (state.commandClient !== null) throw new Error("Audio acknowledgements belong to the direct endpoint.");
      if (!unsigned(request.sequence) || !integer(request.admitted, 0, 0xffffffff) || typeof request.success !== "boolean") throw new Error("Invalid audio acknowledgement fields.");
      // The actual core validates correlation/full success/rejected prefix and
      // retains its original batch on error. Never substitute or replay a prefix.
      state.game.acknowledge(request.sequence, request.admitted, request.success);
      state.batch = null;
      if (request.rpcId !== undefined) reply(state, request, null);
      pumpAudio(state);
    } else if (request.kind === "play-activate") {
      if (state.active || !hostTime(request.hostNs) || !unsigned(request.startFrame)) throw new Error("Invalid or repeated gameplay activation.");
      if (state.sampleMode === "direct" && (!state.samplesEnded || state.sampleRpcId !== null)) {
        throw new Error("Direct samples require their genuine end acknowledgement before activation.");
      }
      if (state.commandClient !== null && (!state.samplesEnded || !state.commandsDrained || commandsPending(state)
        || state.audioRpcId !== null || state.commandClient.state !== "ready")) throw new Error("Direct audio preparation is not fully acknowledged.");
      const synchronized = state.room ?? state.network;
      if (synchronized) {
        const target = synchronized.start?.targetHostNs;
        const now = networkNow() - synchronized.windowOriginNs;
        const rounding = (1000000000n + BigInt(state.rate) - 1n) / BigInt(state.rate) + 1n;
        if (synchronized.disposed || synchronized.leaving || !synchronized.owner || synchronized.owner.closed || !hostTime(target)
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
      if (!state.active || state.renderObservation !== null || !identity(request.renderId)
        || request.renderId <= state.lastRender) throw new Error("Invalid rendered-report identity or state.");
      const direct = state.commandClient !== null;
      if (direct && Object.hasOwn(request, "report")) throw new Error("Direct audio reports cannot be supplied by the caller.");
      const observation = snapshotPresentation(state, request, direct);
      if (direct) {
        state.renderObservation = observation;
        state.completed = false;
        pumpAudio(state);
        return;
      }
      const completed = observeOutput(state, observation, request.report);
      pumpAudio(state);
      if (play !== state) return;
      publishRender(state, observation, completed);
    } else throw new Error("Unknown gameplay request.");
  } catch (error) {
    if (!audioHandled) closeAudioHandoff(request);
    failPlay(state, error, request);
  }
}

self.addEventListener("message", event => {
  const request = event.data;
  if (request?.kind === "dispose") {
    if (play || roomFinalization || capturesPending) { report("dispose-error", { message: "Join gameplay capture and room cleanup before disposing the Worker." }); return; }
    if (disposed) return;
    disposed = true;
    fenceVisual();
    visual?.client.close();
    visual = null;
    try { renderPort?.close(); } catch {}
    renderPort = null;
    preview?.free(); preview = null;
    discardRoomResults();
    stagedLibrary?.library.free(); stagedLibrary = null;
    library?.free(); library = null;
    ++importGeneration; pendingImport = null;
    settingsOperation = null;
    report("disposed");
    return;
  }
  if (failed || disposed) { closeAudioHandoff(request); return; }
  if (!request || typeof request !== "object" || typeof request.kind !== "string") {
    if (play) failPlay(play, new Error("Malformed Worker request."));
    else fatal(new Error("Malformed Worker request."));
    return;
  }
  if (request.kind === "init") {
    if (ready) return fatal(new Error("Gameplay owner is already initialized."));
    ready = (async () => {
      validateRenderLimits(request.maxPacketBytes, request.maxDiagnosticBytes);
      const port = request.renderPort;
      if (!port || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")
        || !Number.isInteger(request.renderTimeoutMs) || request.renderTimeoutMs < 1 || request.renderTimeoutMs > 60000) throw new Error("Gameplay initialization requires a transferred render port and trusted timeout.");
      renderPort = port;
      renderLimits = { maxPacketBytes: request.maxPacketBytes, maxDiagnosticBytes: request.maxDiagnosticBytes, timeoutMs: request.renderTimeoutMs };
      await init();
      if (disposed || failed) return;
      cpuReady = true;
      report("ready");
    })();
    ready.catch(fatal);
    return;
  }
  if (request.kind === "historical-record-page") { pageHistoricalRecord(request); return; }
  if (request.kind === "historical-record-clear") { clearHistoricalRecord(request); return; }
  if (request.kind === "historical-record-present") { void presentHistoricalRecord(request); return; }
  if (request.kind === "settings-profile-save" || request.kind === "settings-profile-load") {
    void settingsProfile(request).catch(() => { /* An unavailable response port leaves Window's bounded deadline authoritative. */ });
    return;
  }
  if (!ready) { closeAudioHandoff(request); return fatal(new Error("Initialize gameplay before sending commands.")); }
  if (settingsOperation && request.kind !== "resize") {
    closeAudioHandoff(request);
    if (request.kind === "play-start") {
      report("play-error", { playId: request.playId, rpcId: request.rpcId, released: true, message: "Wait for settings processing before starting gameplay." });
    } else if (request.kind.startsWith("play-") && identity(request.playId) && identity(request.rpcId)) {
      report("play-reply", { playId: request.playId, rpcId: request.rpcId, error: "Wait for settings processing before sending gameplay requests." });
    } else if (request.kind === "import" || request.kind === "accept-library") {
      report("import-error", { id: request.id, message: "Wait for settings processing before importing files." });
    } else if (request.kind === "select") {
      report("selection-error", { id: request.id, message: "Wait for settings processing before preparing a chart." });
    } else if (request.kind === "seek") {
      report("seek-error", { id: request.id, selectedId, message: "Wait for settings processing before changing the preview." });
    }
    return;
  }
  if (request.kind.startsWith("play-")) { handlePlay(request); return; }
  if (request.kind === "import" || request.kind === "accept-library") {
    if (play) report("import-error", { id: request.id, message: "Stop gameplay before changing the selected library." });
    else if (request.kind === "import") { discardRoomResults(); queueImport(request); }
    else acceptLibrary(request);
  }
  else if (request.kind === "select") {
    if (play) report("selection-error", { id: request.id, message: "Stop gameplay before changing the preview chart." });
    else { discardRoomResults(); void selectChart(request).catch(fatal); }
  }
  else if (request.kind === "seek") {
    if (play) report("seek-error", { id: request.id, selectedId, message: "Stop gameplay before seeking the preview." });
    else { discardRoomResults(); void seek(request).catch(fatal); }
  }
  else if (request.kind === "resize") void resize(request).catch(fatal);
  else if (play) failPlay(play, new Error("Unknown Worker request."));
});

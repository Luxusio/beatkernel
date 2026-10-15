import { RenderClient, validateRenderLimits, unsignedIdentity, snapshotMenuMotion } from "./render-protocol.mjs";
import { validateCompletedResults, resultRequest } from "./completed-results-model.mjs";
import init, * as runtime from "./pkg/beatkernel_bms_runtime.js";
import { LIMITS, preflight, previewNanos, validateHistoricalGradeSnapshot, normalizedPath } from "./host_model.mjs";
import { ORIGINAL_PCM_SAMPLES, PLAY_PCM_SAMPLES, bindingsFor, validateTiming, validateTimingPolicy, validateStart, validateEnd, validateLiveColdPhase, replaySpeedFromFraction, playbackSpeedFromFraction, playbackOutputFromMetadata, sectionFromSeconds, millisecondsToNanos, audioScheduleFromFrame, presentationPair, presentationAvailability, presentationOutputAvailability, audioClockExpired, renderedCursor, reportWord } from "./play-model.mjs";
import { BrowserMultiplayerOwner } from "./multiplayer-owner.mjs";
import { BrowserRoomOwner, ROOM_SESSION_METHODS } from "./room-owner.mjs";
import { SavedOpponentSelection, opponentLabel, validateSelections, validateOpponentSnapshot, validateOpponentTargets, validateLocalOpponentSnapshot } from "./saved-opponents.mjs";
import { keyboardBindingWords, encodeKeyboardEvent, touchBindingWords, encodeTouchEvent, encodeRawHidEvent, encodePointerEvent, encodePointerButtonEvent } from "./physical-input.mjs";
import { snapshotHidDevices, hidSetupFromProfile } from "./hid-profile.mjs";
import { AudioCommandClient } from "./audio-command-client.mjs";
import { AudioSampleClient } from "./audio-sample-client.mjs";
import { snapshotGamepadSetup, snapshotGamepadDevices, automaticGamepadSetup, gamepadSetupFromProfile, GamepadAdapter } from "./gamepad-profile.mjs";
import { snapshotLocalPlan, localBindingWords } from "./local-play-model.mjs";
import { LocalRoster } from "./local-play-host.mjs";
import { encodeBrowserSettings, decodeBrowserSettings, settingsMenuProfileFields, settingsMenuProfileSnapshot, settingsMenuSnapshot } from "./settings-profile.mjs";
import { validateBrowserDisplayFields } from "./display-settings.mjs";
import { snapshotPointerSetup } from "./pointer-profile.mjs";
import { RecordsStore } from "./record-store.mjs";
const { BrowserGame, BrowserLocalGame, BrowserLibrary, BrowserMultiplayer, BrowserReplay, BrowserRoomClient, BrowserRoomResults, BrowserHistoricalRecord } = runtime;
let ready = null;
let cpuReady = false;
let audioSetup = null;
let audioModule = null;
let audioSetupRetired = false;
let lastAudioSetupId = 0;
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
let retiredCaptureOwner = null;
let deferredFatal = null;
let libraryContext = null;
let preparationOwner = null;
let stagedLibrary = null;
let libraryId = 0;
let importGeneration = 0;
let importing = false;
let pendingImport = null;
let importPumpRunning = false;
let selectedId = 0;
let selectionEpoch = 0;
let failed = false;
let extent = [0, 0];
let play = null;
let roomFinalization = null;
let roomResults = null;
let completedResults = null;
let historicalRecord = null;
let historicalCandidate = null;
let historicalAcquiring = false;
let menuOwner = null;
let menuGeneration = 0n;
let menuVisible = false;
let lastMenuAction = 0n;
let menuRoster = null;
let menuAssignPlayer = null;
let menuOpponents = null;
let menuRecordFile = null;
let menuRecordAcquiring = false;
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
let recordsStore = null;
let recordsOpening = null;
let recordsOperation = null;
let recordsClosing = null;
let recordsEpoch = 0;
let lastRecordsId = 0;
let recordsRetired = false;
const RECORD_BYTES = 64 * 1024 * 1024;
const ARCHIVE_BYTES = 5 * 1024 * 1024;
function recordId(id) { return Number.isSafeInteger(id) && id > 0; }
function recordError(error) { return message(error) || "Recording operation failed."; }
function recordHandleSize(handle, limit, file = false) {
  if (!(handle instanceof Blob) || file && !(handle instanceof File)) throw new Error("Recording requires an immutable binary handle.");
  if (file) Object.getOwnPropertyDescriptor(File.prototype, "name").get.call(handle);
  const size = Object.getOwnPropertyDescriptor(Blob.prototype, "size").get.call(handle);
  if (size < 1 || size > limit) throw new Error("Recording handle exceeds its bounded extent.");
  return size;
}
function recordReply(request, result, error = null) {
  report("records-reply", { id: request.id, action: request.action, result, error: error === null ? null : recordError(error) });
}
function recordCurrent(operation) {
  return recordsOperation === operation && operation.epoch === recordsEpoch && !operation.retired && !recordsRetired && !failed && !disposed;
}
function recordFence() {
  ++recordsEpoch;
  if (recordsOperation) recordsOperation.retired = true;
  recordsOpening?.close();
  recordsStore?.close();
}
function cleanupDeadline(promise) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Recording cleanup timed out; commit status may be unknown.")), 10000);
    promise.then(value => { clearTimeout(timer); resolve(value); }, error => { clearTimeout(timer); reject(error); });
  });
}
function joinRecords() {
  const opening = recordsOpening, store = recordsStore, operation = recordsOperation;
  return cleanupDeadline(Promise.all([opening?.terminal, store?.terminal, operation?.physical])).then(() => {
    if (recordsOpening === opening) recordsOpening = null;
    if (recordsStore === store) recordsStore = null;
  });
}
function closeRecords(request) {
  if (!recordId(request.id)) return;
  if (recordsClosing?.id === request.id) {
    void recordsClosing.done.then(error => report("records-closed", { id: request.id, error }));
    return;
  }
  if (request.id <= lastRecordsId) return;
  if (recordsClosing && !recordsClosing.finished) {
    report("records-closed", { id: request.id, error: "Recording cleanup is already in progress." }); return;
  }
  lastRecordsId = request.id;
  recordFence();
  const closing = { id: request.id, finished: false, done: null };
  recordsClosing = closing;
  closing.done = joinRecords().then(() => null, recordError).then(error => {
    closing.finished = true;
    report("records-closed", { id: closing.id, error });
    return error;
  });
}
function saveRecordValue(value) {
  if (!value || typeof value !== "object") throw new Error("Recording save value is required.");
  const { replayFile, archiveBlob, archivePlayer, player } = value;
  if (!Array.isArray(value.players) || value.players.length < 1 || value.players.length > 64)
    throw new Error("Recording original player roster is invalid.");
  const players = Array.from(value.players);
  recordHandleSize(replayFile, RECORD_BYTES, true);
  if (archiveBlob !== null) recordHandleSize(archiveBlob, ARCHIVE_BYTES);
  if (players.some(id => !Number.isInteger(id) || id < 1 || id > 0xffffffff)
    || new Set(players).size !== players.length || !players.includes(player)
    || (archiveBlob === null ? archivePlayer !== null : !(archiveBlob instanceof Blob)
      || archiveBlob.size < 1 || archiveBlob.size > ARCHIVE_BYTES || archivePlayer !== player))
    throw new Error("Recording handle or original player association is invalid.");
  const source = value.metadata;
  if (!source || typeof source.name !== "string" || !source.name.length || source.name.length > 512
    || Array.from(source.name).length > 256 || typeof source.complete !== "boolean"
    || typeof source.chartPath !== "string" || source.chartPath.length > 4096)
    throw new Error("Recording metadata is invalid.");
  const metadata = { name: source.name, chartPath: normalizedPath(source.chartPath, 4096), complete: source.complete };
  for (const key of ["hits", "misses", "combo"]) {
    const score = source[key];
    if (score !== null && (typeof score !== "bigint" || score < 0n || score > U64_MAX)) throw new Error("Recording score is invalid.");
    metadata[key] = score;
  }
  return { metadata, replayFile, archiveBlob, archivePlayer };
}
async function recordBody(handle, limit) {
  const size = recordHandleSize(handle, limit);
  const buffer = await handle.arrayBuffer();
  if (!(buffer instanceof ArrayBuffer) || buffer.resizable === true || buffer.byteLength !== size || size < 1 || size > limit)
    throw new Error("Recording acquisition changed its bounded extent.");
  return new Uint8Array(buffer);
}
function requestRecords(request) {
  if (!recordId(request.id)) return;
  if (recordsOperation?.id === request.id) return;
  if (request.id <= lastRecordsId) { recordReply(request, null, "Recording operation ID must increase."); return; }
  if (!cpuReady || play || capturesPending || roomFinalization || importing || stagedLibrary || preparationOwner || settingsOperation
    || recordsRetired || recordsOperation || recordsClosing && !recordsClosing.finished) {
    recordReply(request, null, "Recording operation requires an initialized idle owner with no pending cleanup."); return;
  }
  let value;
  try {
    if (!["list", "load", "save", "remove"].includes(request.action)) throw new Error("Unknown recording action.");
    if (["load", "remove"].includes(request.action) && !recordId(request.recordId)) throw new Error("Recording database key is invalid.");
    if (request.action === "save") value = saveRecordValue(request.value);
  } catch (error) { recordReply(request, null, error); return; }
  lastRecordsId = request.id;
  const operation = { id: request.id, epoch: recordsEpoch, retired: false, physical: null };
  recordsOperation = operation;
  const timer = setTimeout(() => {
    if (!recordCurrent(operation)) return;
    recordReply(request, null, "Recording operation timed out; commit status may be unknown.");
    recordFence();
  }, 10000);
  operation.physical = (async () => {
    let successful = false;
    try {
      let bytes, completedArchive;
      if (value) {
        bytes = await recordBody(value.replayFile, RECORD_BYTES);
        if (!recordCurrent(operation)) return;
        if (value.archiveBlob !== null) {
          completedArchive = await recordBody(value.archiveBlob, ARCHIVE_BYTES);
          if (!recordCurrent(operation)) return;
        }
      }
      if (!recordsStore) {
        const lease = RecordsStore.openTracked(); recordsOpening = lease;
        const store = await lease.ready;
        recordsStore = store;
        if (!recordCurrent(operation)) { store.close(); return; }
        await lease.terminal;
        if (!recordCurrent(operation)) return;
        if (recordsOpening === lease) recordsOpening = null;
      }
      if (!recordCurrent(operation)) return;
      const store = recordsStore;
      let result;
      if (request.action === "list") result = { entries: await store.list() };
      else if (request.action === "save") result = { metadata: await store.save({ ...value.metadata, bytes,
        completedArchive, archivePlayer: value.archivePlayer }) };
      else if (request.action === "remove") result = { removed: await store.remove(request.recordId) };
      else {
        const loaded = await store.load(request.recordId);
        if (!recordCurrent(operation)) return;
        result = { metadata: loaded.metadata,
          replayFile: new File([loaded.bytes], loaded.metadata.name, { type: "application/octet-stream" }),
          archiveBlob: loaded.completedArchive ? new Blob([loaded.completedArchive], { type: "application/octet-stream" }) : null,
          archivePlayer: loaded.archivePlayer ?? null };
      }
      if (!recordCurrent(operation)) return;
      successful = true;
      recordReply(request, result);
    } catch (error) {
      if (recordCurrent(operation)) recordReply(request, null, error);
    } finally {
      clearTimeout(timer);
      // Failed/canceled caller promises cannot release native transaction capacity.
      if (!successful || operation.retired) {
        recordsOpening?.close(); recordsStore?.close();
        await Promise.all([recordsOpening?.terminal, recordsStore?.terminal]);
        recordsOpening = null; recordsStore = null;
      }
      if (recordsOperation === operation) recordsOperation = null;
    }
  })();
  void operation.physical.catch(() => {});
}
function currentAudioSetup(operation) {
  return audioSetup === operation && !operation.retired && !audioSetupRetired && !failed && !disposed;
}
function cancelAudioReader(operation) {
  if (!operation.reader || operation.readerDone || operation.cancelRequested) return;
  operation.cancelRequested = true;
  try { Promise.resolve(operation.reader.cancel()).catch(() => {}); } catch {}
}
function retireAudioSetup() {
  audioSetupRetired = true;
  if (!audioSetup) return;
  audioSetup.retired = true;
  clearTimeout(audioSetup.timer);
  audioSetup.controller.abort();
  cancelAudioReader(audioSetup);
}
function audioSetupError(id, error) {
  // A cloning/transport failure remains optional; do not make preview fatal.
  try { report("audio-module-error", { id, message: message(error) }); } catch {}
}
async function prepareAudioModule(operation) {
  try {
    const response = await fetch(new URL("./audio-pkg/beatkernel_bms_runtime_bg.wasm", import.meta.url),
      { signal: operation.controller.signal });
    if (!currentAudioSetup(operation)) return;
    if (!response.ok || !response.body) throw new Error("Audio WASM requires a successful response with a body.");
    operation.reader = response.body.getReader();
    const limit = 64 * 1024 * 1024;
    let storage = new Uint8Array(64 * 1024);
    let length = 0;
    for (;;) {
      const chunk = await operation.reader.read();
      if (!currentAudioSetup(operation)) return;
      if (!chunk || typeof chunk.done !== "boolean") throw new Error("Invalid audio WASM body read.");
      if (chunk.done) { operation.readerDone = true; break; }
      if (!(chunk.value instanceof Uint8Array)) throw new Error("Audio WASM body must contain byte chunks.");
      if (chunk.value.byteLength === 0) continue;
      if (chunk.value.byteLength > limit - length) throw new Error("Audio WASM exceeds the 64 MiB setup limit.");
      const nextLength = length + chunk.value.byteLength;
      if (nextLength > storage.byteLength) {
        let capacity = storage.byteLength;
        while (capacity < nextLength) capacity = Math.min(limit, capacity * 2);
        const grown = new Uint8Array(capacity);
        grown.set(storage.subarray(0, length));
        storage = grown;
      }
      storage.set(chunk.value, length);
      length = nextLength;
    }
    const module = await WebAssembly.compile(storage.subarray(0, length));
    if (!currentAudioSetup(operation)) return;
    clearTimeout(operation.timer);
    report("audio-module-ready", { id: operation.id, module });
    audioModule = module;
  } catch (error) {
    if (currentAudioSetup(operation)) audioSetupError(operation.id, error);
  } finally {
    clearTimeout(operation.timer);
    cancelAudioReader(operation);
    try { operation.reader?.releaseLock(); } catch {}
    // Retired fetch/compile retains physical single-flight capacity until here.
    if (audioSetup === operation) audioSetup = null;
  }
}
function loadAudioModule(request) {
  if (!Number.isSafeInteger(request.id) || request.id < 1 || audioSetupRetired) return;
  if (!cpuReady) { audioSetupError(request.id, new Error("Initialize gameplay before preparing audio.")); return; }
  if (audioSetup) {
    if (audioSetup.id !== request.id || audioSetup.retired)
      audioSetupError(request.id, new Error("Audio setup is already pending."));
    return;
  }
  if (audioModule) {
    try { report("audio-module-ready", { id: request.id, module: audioModule }); }
    catch (error) { audioSetupError(request.id, error); }
    return;
  }
  if (request.id <= lastAudioSetupId) {
    audioSetupError(request.id, new Error("Retry audio setup with a fresh request id."));
    return;
  }
  const operation = { id: request.id, controller: new AbortController(), reader: null,
    readerDone: false, cancelRequested: false, retired: false, timer: null };
  lastAudioSetupId = request.id;
  audioSetup = operation;
  operation.timer = setTimeout(() => {
    if (!currentAudioSetup(operation)) return;
    operation.retired = true;
    clearTimeout(operation.timer);
    operation.controller.abort();
    cancelAudioReader(operation);
    audioSetupError(operation.id, new Error("Audio WASM setup exceeded the ten-second deadline."));
  }, 10000);
  void prepareAudioModule(operation);
}
function menuToken() {
  return { menuGeneration, screen: menuOwner.screen, revision: menuOwner.revision };
}
function sameMenuToken(left, right) {
  return left != null && right != null
    && unsignedIdentity(left.menuGeneration) && unsignedIdentity(left.screen) && unsignedIdentity(left.revision)
    && left.menuGeneration === right.menuGeneration && left.screen === right.screen && left.revision === right.revision;
}
function currentSettingsOperation(operation) {
  return settingsOperation === operation && !failed && !disposed
    && (!operation.owner || menuVisible && menuOwner === operation.owner && sameMenuToken(operation.menuToken, menuToken()));
}
function finishSettingsPending(operation, error = null) {
  const previousToken = operation.menuToken;
  operation.owner.set_operation_status(previousToken.screen, previousToken.revision, false, error);
  operation.menuToken = menuToken();
  reportMenu({ settingsOperationId: operation.id, previousToken });
  publishVisual();
}
function settingsProfileDecision(request) {
  const operation = settingsOperation;
  if (!operation || request.id !== operation.id || !operation.owner || !currentSettingsOperation(operation)) return;
  if (request.kind === "settings-profile-cancel") {
    if (operation.phase === "admitted" || operation.retired
      || !sameMenuToken(request.menuToken, operation.menuToken) && !sameMenuToken(request.menuToken, operation.originalToken)) return;
    operation.retired = true;
    operation.candidate = null;
    finishSettingsPending(operation);
    // The single unabortable read keeps its capacity until its finally settles.
    if (operation.phase !== "reading") settingsOperation = null;
    return;
  }
  if (operation.phase !== "candidate" || operation.retired || !sameMenuToken(request.menuToken, operation.menuToken)) return;
  const admittedToken = operation.menuToken;
  operation.phase = "admitted";
  try {
    const fields = settingsMenuProfileFields(operation.candidate);
    operation.owner.replace_settings_fields(admittedToken.screen, admittedToken.revision, fields);
    operation.menuToken = menuToken();
    const settings = operation.candidate;
    reportMenu({ settingsOperationId: operation.id, previousToken: admittedToken });
    report("settings-profile-loaded", { id: operation.id, admittedToken, resultToken: operation.menuToken, settings });
    publishVisual();
  } catch (error) {
    if (currentSettingsOperation(operation)) {
      finishSettingsPending(operation, message(error));
      report("settings-profile-error", { id: operation.id, resultToken: operation.menuToken, message: message(error) });
    }
  } finally {
    operation.candidate = null;
    if (settingsOperation === operation) settingsOperation = null;
  }
}
async function settingsProfile(request) {
  const id = request.id;
  if (!identity(id)) {
    report("settings-profile-error", { id, message: "Settings request identity must be a positive safe integer." });
    return;
  }
  if (id <= lastSettingsId) return;
  lastSettingsId = id;
  if (!cpuReady || settingsOperation || recordsOperation && !recordsOperation.retired || play || roomFinalization || importing || stagedLibrary) {
    report("settings-profile-error", { id, message: "Wait for an initialized idle Worker before processing settings." });
    return;
  }
  let savedBytes = null, owner = null, originalToken = null;
  try {
    if (request.menuToken != null) {
      if (!menuVisible || !menuOwner || !menuOwner.has_settings_parent || !sameMenuToken(request.menuToken, menuToken()))
        throw new Error("Settings processing requires the exact retained Settings owner.");
      owner = menuOwner;
      originalToken = menuToken();
    } else if (menuVisible && menuOwner?.has_settings_parent) {
      throw new Error("Retained Settings processing requires its current menu token.");
    }
    if (request.kind === "settings-profile-save") savedBytes = encodeBrowserSettings(request.settings);
    else {
      if (typeof File !== "function" || !(request.file instanceof File)) throw new Error("Choose an actual settings file.");
      if (!Number.isSafeInteger(request.file.size) || request.file.size < 1 || request.file.size > 16384)
        throw new Error("Choose one nonempty settings file no larger than 16 KiB.");
    }
  } catch (error) { report("settings-profile-error", { id, message: message(error) }); return; }
  const operation = { id, owner, originalToken, menuToken: originalToken, phase: "reading", retired: false, candidate: null };
  discardHistoricalCandidate();
  ++selectionEpoch;
  releasePreparation();
  settingsOperation = operation;
  try {
    if (owner) {
      owner.set_operation_status(originalToken.screen, originalToken.revision, true, undefined);
      operation.menuToken = menuToken();
      reportMenu({ settingsOperationId: id, previousToken: originalToken });
      publishVisual();
    }
    if (request.kind === "settings-profile-save") {
      if (currentSettingsOperation(operation)) {
        if (owner) finishSettingsPending(operation);
        report("settings-profile-saved", { id, bytes: savedBytes, ...(owner ? { resultToken: operation.menuToken } : {}) }, [savedBytes.buffer]);
      }
    } else {
      const settings = await decodeBrowserSettings(request.file);
      if (!operation.retired && currentSettingsOperation(operation)) {
        if (owner) {
          operation.candidate = settings;
          operation.phase = "candidate";
          report("settings-profile-candidate", { id, menuToken: operation.menuToken, settings });
        } else report("settings-profile-loaded", { id, settings });
      }
    }
  } catch (error) {
    if (!operation.retired && currentSettingsOperation(operation)) {
      if (owner && owner.pending) finishSettingsPending(operation, message(error));
      report("settings-profile-error", { id, ...(owner ? { resultToken: operation.menuToken } : {}), message: message(error) });
    }
  } finally {
    // Candidate/admitted operations retain their bounded DTO; held reads join even after cancellation.
    if (settingsOperation === operation && operation.phase !== "candidate" && operation.phase !== "admitted") settingsOperation = null;
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

function discardHistoricalCandidate() {
  historicalEpoch = {};
  const previous = historicalCandidate;
  historicalCandidate = null;
  try { previous?.binding.free(); } catch { /* An unaccepted candidate owns no presentation. */ }
}
function discardHistoricalRecord() {
  discardHistoricalCandidate();
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
  discardHistoricalCandidate();
  if (failed || !cpuReady || play || roomFinalization || importing || stagedLibrary || settingsOperation
    || roomResults) {
    historicalReply(request.id, false, "Historical display requires an initialized idle preview.");
    return;
  }
  if (historicalAcquiring) {
    historicalReply(request.id, false, "Wait for the current historical recording acquisition to finish.");
    return;
  }
  historicalAcquiring = true;
  const epoch = historicalEpoch = {};
  const current = () => request.id === lastHistoricalId && epoch === historicalEpoch && !failed && !disposed && !play && !roomFinalization && !importing
    && !stagedLibrary && !settingsOperation && !roomResults;
  let binding = null;
  try {
    const file = request.replayFile;
    const replaySize = file?.size;
    if (!(file instanceof File) || !Number.isSafeInteger(replaySize) || replaySize < 1 || replaySize > 64 * 1024 * 1024
      || typeof file.arrayBuffer !== "function") throw new Error("Historical replay file exceeds its bound.");
    const archiveHandle = request.completedArchive;
    const player = request.archivePlayer;
    if (archiveHandle != null) {
      if (!(archiveHandle instanceof Blob) || archiveHandle.size < 1 || archiveHandle.size > ARCHIVE_BYTES
        || (player != null && (!Number.isInteger(player) || player < 1 || player > 0xffffffff)))
        throw new Error("Historical archive envelope is invalid.");
    } else if (player != null) throw new Error("Historical player association requires an archive.");
    await ready;
    if (!current()) return;
    const bytes = await file.arrayBuffer();
    if (!current()) return;
    if (!(bytes instanceof ArrayBuffer) || bytes.resizable === true || bytes.byteLength !== replaySize) {
      throw new Error("Historical replay acquisition changed its bounded extent.");
    }
    const archive = archiveHandle == null ? undefined : await recordBody(archiveHandle, ARCHIVE_BYTES);
    if (!current()) return;
    binding = new BrowserHistoricalRecord(new Uint8Array(bytes), archive, player ?? undefined);
    if (!current()) { const previous = binding; binding = null; previous.free(); return; }
    const available = binding.available;
    const error = binding.error;
    if (typeof available !== "boolean" || !(error == null || (typeof error === "string" && error.length >= 1 && error.length <= 4096))
      || (available && error != null)) throw new Error("Historical presentation returned invalid metadata.");
    const grades = available ? validateHistoricalGradeSnapshot({ page: binding.grade_page, pages: binding.grade_pages }) : null;
    if (!current()) { const previous = binding; binding = null; previous.free(); return; }
    if (available) {
      // Keep the previous owner until Window accepts this correlated response.
      historicalCandidate = { id: request.id, binding, grades, lastRpc: 0, epoch };
      binding = null;
    }
    else { const previous = binding; binding = null; previous.free(); }
    historicalReply(request.id, available, error ?? null, grades);
    publishVisual();
  } catch (error) {
    try { binding?.free(); } catch {}
    if (current()) { historicalReply(request.id, false, error); publishVisual(); }
  } finally {
    // Retiring a candidate cannot cancel its outstanding File/Blob body read.
    historicalAcquiring = false;
  }
}
function acceptHistoricalRecord(request) {
  const candidate = historicalCandidate;
  if (!identity(request.id) || !candidate || candidate.id !== request.id) return;
  if (request.id !== lastHistoricalId || candidate.epoch !== historicalEpoch || failed || disposed || !cpuReady
    || play || roomFinalization || importing || stagedLibrary || settingsOperation || roomResults) {
    discardHistoricalCandidate();
    historicalReply(request.id, false, "Historical acceptance requires the current idle selection.");
    return;
  }
  historicalCandidate = null;
  // Detach the candidate before retirement so only the previous owners are freed.
  discardRoomResults();
  historicalRecord = candidate;
  publishVisual();
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
  if (request.cancelId != null) {
    if (!identity(request.cancelId) || request.cancelId !== lastHistoricalId) return;
    if (!historicalRequestId(request)) return;
    discardHistoricalCandidate();
    historicalReply(request.id, false);
    publishVisual();
    return;
  }
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
const MENU_MOTION_TIMEOUT_MS = 10000;
function settleMenuMotion(context, entry, admitted, error, admittedGeometryVersion) {
  if (entry.settled) return;
  entry.settled = true; clearTimeout(entry.timer);
  const index = context?.motionRequests?.indexOf(entry) ?? -1;
  if (index >= 0) context.motionRequests.splice(index, 1);
  report("menu-motion-reply", { requestId: entry.requestId, hostOwner: entry.hostOwner,
    menuGeneration: entry.menuGeneration, screen: entry.screen, revision: entry.revision,
    generation: entry.generation, content: entry.content, geometryVersion: entry.geometryVersion, admitted,
    ...(admitted ? { admittedGeometryVersion } : { message: message(error).slice(0, 4096) }) });
}
function retireMenuMotions(context, reason = "Menu motion owner retired.") {
  if (!context) return;
  for (const entry of [...(context.motionRequests ?? []), ...(context.motionActive ? [context.motionActive] : [])])
    settleMenuMotion(context, entry, false, new Error(reason));
}
function validateMenuMotionTicket(context, entry) {
  const geometry = context?.geometry;
  if (!context || context.mode !== "menu" || !currentVisual(context) || !menuVisible || menuOwner !== context.owner
    || !cpuReady || failed || disposed || play || settingsOperation || importing
    || entry.menuGeneration !== menuGeneration || entry.screen !== menuOwner.screen || entry.revision !== menuOwner.revision
    || context.menuRevision !== entry.revision || entry.generation !== context.generation || entry.content !== context.content
    || !geometry || geometry.geometryVersion !== entry.geometryVersion || geometry.generation !== entry.generation
    || geometry.content !== entry.content || geometry.menuGeneration !== entry.menuGeneration || geometry.screen !== entry.screen
    || geometry.revision !== entry.revision || geometry.width === 0 || geometry.height === 0)
    throw new Error("Menu motion requires the exact current submitted menu ticket.");
}
function enqueueMenuMotion(request) {
  const context = visual;
  const entry = { requestId: request.requestId, hostOwner: request.hostOwner, menuGeneration: request.menuGeneration,
    screen: request.screen, revision: request.revision, generation: request.generation, content: request.content,
    geometryVersion: request.geometryVersion, settled: false, timer: null };
  try {
    if (!unsignedIdentity(entry.requestId) || !Number.isSafeInteger(entry.hostOwner) || entry.hostOwner < 0
      || ![entry.menuGeneration, entry.screen, entry.revision, entry.generation, entry.content, entry.geometryVersion].every(unsignedIdentity))
      throw new Error("Invalid menu motion request identity.");
    validateMenuMotionTicket(context, entry);
    if (entry.requestId <= (context.motionRequestId ?? 0n)) throw new Error("Duplicate menu motion request identity.");
    if (context.motionRequests.length + (context.motionActive ? 1 : 0) >= 64) throw new Error("Menu motion request capacity exhausted.");
    entry.motion = snapshotMenuMotion(request.control, request.transforms, request.durationMs, request.easing);
    context.motionRequestId = entry.requestId;
    context.motionRequests.push(entry);
    entry.timer = setTimeout(() => settleMenuMotion(context, entry, false, new Error("Menu motion acknowledgement timed out.")), MENU_MOTION_TIMEOUT_MS);
    publishVisual();
  } catch (error) { settleMenuMotion(context, entry, false, error); }
}
async function dispatchMenuMotions(context) {
  while (context.motionRequests.length && !context.client.pending) {
    const entry = context.motionRequests.shift();
    if (entry.settled) continue;
    context.motionActive = entry;
    try {
      validateMenuMotionTicket(context, entry);
      const issued = nextGeometry();
      await context.client.control("menu-motion", { ...entry.motion, menuGeneration: entry.menuGeneration,
        screen: entry.screen, revision: entry.revision, geometryVersion: issued });
      if (!entry.settled) {
        validateMenuMotionTicket(context, entry);
        settleMenuMotion(context, entry, true, null, issued);
      }
    } catch (error) {
      settleMenuMotion(context, entry, false, error);
      if (context.client.state !== "ready") {
        retireMenuMotions(context, "Menu renderer transport failed.");
        if (currentVisual(context)) visualFailure(context, error);
        break;
      }
    } finally { if (context.motionActive === entry) context.motionActive = null; }
    if (!currentVisual(context)) { retireMenuMotions(context); break; }
  }
}
function fenceVisual() {
  // Fence producer callbacks before gameplay or prepared owners are freed.
  if (visual) { retireMenuMotions(visual); visual.retired = true; }
}
function fatal(error) {
  recordsRetired = true;
  recordFence();
  retireAudioSetup();
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
  if (settingsOperation) {
    settingsOperation.retired = true;
    settingsOperation.candidate = null;
    if (settingsOperation.phase !== "reading") settingsOperation = null;
  }
  discardRoomResults();
  cancelRoomFinalization();
  fenceVisual();
  void joinRecords().then(() => report("fatal", { message: message(error) }), cause =>
    report("fatal", { message: message(`${message(error)}; ${recordError(cause)}`) }));
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
  if (menuVisible && menuOwner) return { owner: menuOwner, source: menuOwner, mode: "menu" };
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
  retireMenuMotions(context, "Menu renderer failed.");
  context.retired = true;
  if (context.mode === "live" || context.mode === "local" || context.mode === "replay") failPlay(context.owner, error);
  else if (context.mode === "results") completedResultsFailure(context.owner, error);
  else if (context.mode === "room") roomResultsFailure(context.owner, error);
  else if (context.mode === "history") {
    // A failing old visual does not invalidate a newer staged acquisition.
    historicalRecord = null;
    try { context.owner.binding.free(); } catch { /* Historical display failure has no gameplay outcome. */ }
    historicalReply(context.owner.id, false, error);
  }
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
          && ["drawn", "render-wait"].includes(evidence.kind)
          && (context.mode !== "menu" || evidence.menuGeneration === menuGeneration
            && evidence.screen === menuOwner.screen && evidence.revision === menuOwner.revision)) {
          report(evidence.kind, { selectedId, generation: context.generation, content: context.content,
            mode: context.mode, ...(context.mode === "menu" ? { menuGeneration: evidence.menuGeneration,
              screen: evidence.screen, revision: evidence.revision } : {}),
            ...(play === context.owner ? { playId: play.id } : {}) });
        }
        if (currentVisual(context) && evidence?.kind === "video-unavailable"
          && evidence.generation === context.generation && evidence.content === context.content) report("video-unavailable", { selectedId, message: evidence.message, ...(play === context.owner ? { playId: play.id } : {}) });
        if (currentVisual(context) && context.mode === "menu" && evidence?.kind === "menu-action"
          && evidence.generation === context.generation && evidence.content === context.content) handleMenu(evidence);
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
          retireMenuMotions(context);
          context.retired = true;
          try { if (context.client.state === "ready") await context.client.retire(); }
          catch { context.client.close(); }
          if (failed || disposed) return;
        }
        // Navigation during retirement selects the latest owner, never the old candidate.
        const latest = visualSelection();
        if (!latest) { visual = null; continue; }
        if (visualGeneration === U64_MAX) throw new Error("Visual generation exhausted.");
        context = { ...latest, generation: ++visualGeneration, content: visualGeneration, sequence: 0n, retired: false, controls: [], motionRequests: [], motionActive: null, motionRequestId: 0n };
        visual = context;
        context.client = new RenderClient({ port: scopedRenderPort(context), ...renderLimits, generation: context.generation, content: context.content,
          onError: error => visualFailure(context, error),
          onGeometry: evidence => { if (currentVisual(context)) { context.geometry = evidence; report("render-geometry", { ...evidence, mode: context.mode, selectedId, ...(play === context.owner ? { playId: play.id } : {}) }); } } });
        const bytes = context.mode === "menu" ? context.source.snapshot() : ["preview", "live", "local", "replay"].includes(context.mode)
          ? context.source.visual_registration(context.generation, context.content, renderLimits.maxPacketBytes, renderLimits.maxDiagnosticBytes)
          : context.source.visual_snapshot(context.generation, context.content, renderLimits.maxPacketBytes, renderLimits.maxDiagnosticBytes);
        // A new content owner needs its own geometry submission evidence even
        // when the renderer keeps the same surface. Preserve an unsent resize's
        // reserved version; its control supplies the new owner's evidence.
        const registrationGeometry = surface && surfaceSentVersion >= surface.geometryVersion ? nextGeometry() : undefined;
        if (context.mode === "menu") {
          const revision = context.source.revision;
          await context.client.menu(bytes, { recordPreview: context.source.record_snapshot(), opponents: menuOpponentState(),
            ...(registrationGeometry === undefined ? {} : { geometryVersion: registrationGeometry }) });
          context.menuRevision = revision;
        } else await context.client.packet(bytes, { mode: context.mode,
          ...(registrationGeometry === undefined ? {} : { geometryVersion: registrationGeometry }) });
        if (!currentVisual(context)) { pump.dirty = true; continue; }
        if (["preview", "live", "local", "replay"].includes(context.mode)) {
          const movies = context.mode === "preview" ? context.source.video_registration() : context.owner.movies;
          if (movies && movies.images.length) renderPort.postMessage({ kind: "video-registration",
            generation: context.generation, content: context.content, registration: movies });
        }
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
      if (context.mode === "menu") await dispatchMenuMotions(context);
      if (!currentVisual(context)) continue;
      if (context.mode === "menu" && context.source.revision > context.menuRevision) {
        const revision = context.source.revision;
        await context.client.menu(context.source.snapshot(), { geometryVersion: nextGeometry(), recordPreview: context.source.record_snapshot(), opponents: menuOpponentState() });
        context.menuRevision = revision;
        if (currentVisual(context) && context.source.revision > revision) pump.dirty = true;
      }
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
function reportMenu(receipt = {}) {
  if (visual?.mode === "menu") {
    for (const entry of [...visual.motionRequests, ...(visual.motionActive ? [visual.motionActive] : [])]) {
      try { validateMenuMotionTicket(visual, entry); }
      catch (error) { settleMenuMotion(visual, entry, false, error); }
    }
  }
  if (!menuOwner || !menuVisible) return;
  report("menu-state", { menuGeneration, screen: menuOwner.screen, revision: menuOwner.revision,
    route: menuOwner.route, selected: menuOwner.selected, fields: menuOwner.fields(),
    hasSettingsParent: menuOwner.has_settings_parent === true, pending: menuOwner.pending === true, ...receipt,
    ...(menuRoster ? { roster: menuRoster.exportState() } : {}), ...(menuOpponents ? { opponents: menuOpponents.snapshot() } : {}) });
}
function menuOpponentState() {
  if (!menuOwner || menuOwner.route !== 4) return { count: menuOpponents?.size ?? 0, own: 0, other: 0 };
  const key = `record:${menuOwner.selected_value()}`;
  const selected = menuOpponents?.snapshot().filter(entry => entry.sourceKey === key) ?? [];
  return { count: menuOpponents?.size ?? 0, own: selected.filter(entry => entry.own).length, other: selected.filter(entry => !entry.own).length };
}
function menuRosterFields(fields, roster = menuRoster, route = menuOwner.route) {
  if (!roster || ![5, 6, 9].includes(route)) return fields;
  const oldCount = Number(fields[2]);
  if (!Number.isInteger(oldCount) || oldCount < 0 || oldCount > 64 || fields.length < 4 + oldCount * 2) throw new Error("Invalid browser roster metadata.");
  const sourceTable = fields.slice(3 + oldCount * 2);
  const values = [fields[0], fields[1], String(roster.players.length)];
  for (const player of roster.players) values.push(String(player), roster.selected(player)?.toString() ?? "");
  return [...values, ...sourceTable];
}
function commitMenuRoster(change) {
  const candidate = new LocalRoster(); candidate.importState(menuRoster.exportState());
  change(candidate);
  if ([5, 6, 9].includes(menuOwner.route)) menuOwner.set_fields(menuOwner.screen, menuOwner.revision, menuRosterFields(menuOwner.fields(), candidate));
  menuRoster = candidate;
}
function commitAcquiredMenuFields(fields) {
  const admitted = menuFields(fields);
  if (!menuRoster || ![5, 9].includes(menuOwner.route)) {
    menuOwner.set_fields(menuOwner.screen, menuOwner.revision, menuRosterFields(admitted));
    return;
  }
  const count = Number(admitted[2]), at = 3 + count * 2;
  const sources = Number(admitted[at]);
  if (!Number.isInteger(count) || count < 0 || count > 64 || !Number.isInteger(sources)
    || sources < 0 || sources > 1024 || admitted.length !== at + 1 + sources * 5) {
    throw new Error("Invalid acquired browser source inventory.");
  }
  const inventory = new Set(Array.from({ length: sources }, (_, index) => admitted[at + 1 + index * 5]));
  const candidate = new LocalRoster(); candidate.importState(menuRoster.exportState());
  for (const player of candidate.players) {
    const source = candidate.selected(player);
    if (source !== null && !inventory.has(source.toString())) candidate.assign(player, null);
  }
  const values = menuRosterFields(admitted, candidate);
  // Preserve set_fields' pending-operation refusal without changing a value or
  // revision. Same-route admission then performs the existing domain validation
  // before either the field model or canonical roster is published.
  menuOwner.set_fields(menuOwner.screen, menuOwner.revision, menuOwner.fields());
  menuOwner.navigate_with_fields(menuOwner.screen, menuOwner.revision, menuOwner.route, values);
  menuRoster = candidate;
}
function menuFields(fields) {
  if (!Array.isArray(fields) || fields.length > 8192) throw new Error("Menu field count exceeds limit.");
  let total = 48;
  const encoder = new TextEncoder();
  for (const value of fields) {
    if (typeof value !== "string" || value.length > 4096) throw new Error("Invalid menu field.");
    const length = encoder.encode(value).length;
    if (length > 4096 || /[\u0000-\u001f\u007f-\u009f]/.test(value)) throw new Error("Menu field exceeds text limit.");
    total += 4 + length;
    if (total > 4 * 1024 * 1024) throw new Error("Menu exceeds byte limit.");
  }
  return fields;
}
function handleMenu(request) {
  if (request.kind === "menu-motion") { enqueueMenuMotion(request); return; }
  try {
    if (!cpuReady || failed || disposed || play || settingsOperation || importing) throw new Error("Menus require the initialized idle gameplay owner.");
    if (request.kind === "menu-open") {
      if (menuGeneration === U64_MAX) throw new Error("Menu generation exhausted.");
      const next = new runtime.BrowserMenuOwner(++menuGeneration);
      const roster = new LocalRoster();
      const opponents = new SavedOpponentSelection();
      try {
        for (const entry of validateSelections(request.opponents ?? [])) opponents.add(entry);
        if (request.roster) roster.importState(request.roster);
        next.set_fields(next.screen, next.revision, menuFields(request.fields ?? []));
      }
      catch (error) { next.free(); throw error; }
      retireMenuMotions(visual, "Menu motion owner replaced.");
      if (menuOwner) { menuOwner.dispose(); menuOwner.free(); }
      menuOwner = next; menuRoster = roster; menuOpponents = opponents; menuRecordFile = null;
      menuVisible = true; lastMenuAction = 0n; reportMenu(); publishVisual(); return;
    }
    if (!menuVisible || !menuOwner || request.menuGeneration !== menuGeneration || request.screen !== menuOwner.screen || request.revision !== menuOwner.revision) throw new Error("Stale menu owner or action.");
    if (request.kind === "menu-input") {
      if (!visual || visual.mode !== "menu" || !currentVisual(visual)) return;
      renderPort.postMessage({ ...request, generation: visual.generation, content: visual.content }); return;
    }
    if (request.kind === "menu-navigate") {
      if (!Number.isInteger(request.route) || ![1, 2, 3, 4, 5, 6, 7, 9].includes(request.route)) throw new Error("Menu navigation has no gameplay completion authority.");
      const fields = request.fields ? menuFields(request.fields) : null;
      if (fields) {
        const admitted = menuRosterFields(fields, menuRoster, request.route);
        menuOwner.navigate_with_fields(request.screen, request.revision, request.route, admitted);
      } else menuOwner.navigate(request.screen, request.revision, request.route);
    } else if (request.kind === "menu-fields") commitAcquiredMenuFields(request.fields);
    else if (request.kind === "menu-roster-count") {
      commitMenuRoster(roster => roster.setCount(request.count));
    } else if (request.kind === "menu-roster-assign") {
      const fields = menuOwner.fields(), count = Number(fields[2]), at = 3 + count * 2;
      const sources = Number(fields[at]);
      if (!Number.isInteger(sources) || sources < 0 || sources > 1024 || fields.length !== at + 1 + sources * 5) throw new Error("Invalid acquired browser source inventory.");
      if (request.source !== null && (fields[0] !== "1" || menuRoster.players.length === 1)) throw new Error("Browser assignment is unavailable or automatic solo.");
      if (request.source !== null && !Array.from({ length: sources }, (_, index) => fields[at + 1 + index * 5]).includes(request.source?.toString())) throw new Error("Source is not in the acquired browser inventory.");
      commitMenuRoster(roster => roster.assign(request.player, request.source));
    } else if (request.kind === "menu-record-preview") {
      if (menuOwner.route !== 4 || !libraryContext || typeof request.chartPath !== "string" || typeof request.key !== "string"
        || !(request.replayFile instanceof File) || request.replayFile.size < 1 || request.replayFile.size > RECORD_BYTES
        || request.archiveBlob != null && (!(request.archiveBlob instanceof Blob) || request.archiveBlob.size < 1 || request.archiveBlob.size > ARCHIVE_BYTES)) throw new Error("Invalid actual record preview request.");
      void acquireMenuRecord(request);
      return;
    }
    else if (request.kind.startsWith("menu-opponent-")) {
      const candidate = new SavedOpponentSelection(); for (const entry of menuOpponents.snapshot()) candidate.add(entry);
      if (request.kind === "menu-opponent-add") candidate.add(request.entry);
      else if (request.kind === "menu-opponent-remove") { if (!candidate.remove(request.sourceKey)) throw new Error("Saved opponent is unavailable."); }
      else if (request.kind === "menu-opponent-clear") candidate.clear();
      else if (request.kind === "menu-opponent-target") {
        if (request.player !== null && !menuRoster.players.includes(request.player)) throw new Error("Choose a current local player.");
        candidate.setPlayer(request.sourceKey, request.player);
      } else throw new Error("Unknown saved opponent request.");
      menuOwner.touch(menuOwner.screen, menuOwner.revision); menuOpponents = candidate;
    }
    else if (request.kind === "menu-select") {
      if (menuOwner.route !== 1 || !unsignedIdentity(request.actionId) || request.actionId <= lastMenuAction
        || !Number.isInteger(request.index) || request.index < 0 || request.index >= menuOwner.fields().length)
        throw new Error("Invalid catalog selection or action identity.");
      menuOwner.select(request.screen, request.revision, request.index);
      lastMenuAction = request.actionId;
    }
    else if (request.kind === "menu-edit") {
      if (![2, 3, 7].includes(menuOwner.route)) throw new Error("This menu does not support text editing.");
      menuOwner.edit(request.screen, request.revision, request.index, request.value);
    } else if (request.kind === "menu-action") {
      if (!unsignedIdentity(request.actionId) || request.actionId <= lastMenuAction || !unsignedIdentity(request.control)) throw new Error("Invalid menu action identity.");
      const route = menuOwner.route;
      const control = request.control;
      const index = route === 2 && control >= 1000n ? control - 1000n : route === 7 && control >= 40000n ? control - 40000n
        : route === 3 && control === 70n ? 0n : route === 3 && control === 75n ? 1n
        : route === 4 && control >= 50000n ? control - 50000n : route === 5 && control >= 20000n ? control - 20000n
        : [6, 9].includes(route) && control >= 10000n ? control - 10000n : null;
      if (index !== null) {
        if (index > 0xffffffffn) throw new Error("Menu field index exceeds u32.");
        menuOwner.select(request.screen, request.revision, Number(index));
        if ([2, 3, 7].includes(route)) report("menu-focus", { menuGeneration, screen: menuOwner.screen, revision: menuOwner.revision, index: Number(index), value: menuOwner.fields()[Number(index)] });
      } else {
        if (route === 4 && [52n, 53n, 54n, 60n, 61n].includes(control)) {
          const key = `record:${menuOwner.fields()[menuOwner.selected]}`;
          const candidate = new SavedOpponentSelection();
          for (const entry of menuOpponents.snapshot()) candidate.add(entry);
          if (control === 54n) candidate.clear();
          else if ([60n, 61n].includes(control)) {
            const selected = candidate.snapshot().find(entry => entry.sourceKey === key && entry.own === (control === 60n));
            if (!selected || !candidate.remove(key)) throw new Error("Selected saved opponent is unavailable.");
          } else {
            if (!menuRecordFile || !menuOwner.record_snapshot()) throw new Error("Preview this actual recorded prefix before adding it.");
            candidate.add({ file: menuRecordFile, sourceKey: key, own: control === 52n, label: opponentLabel(menuRecordFile.name) });
          }
          menuOwner.touch(menuOwner.screen, menuOwner.revision); menuOpponents = candidate;
          lastMenuAction = request.actionId; reportMenu(); publishVisual(); return;
        }
        if (route === 4 && [66n, 67n, 68n].includes(control)) {
          const geometry = visual?.geometry;
          if (!geometry || geometry.menuGeneration !== menuGeneration || geometry.screen !== menuOwner.screen || geometry.revision !== menuOwner.revision) throw new Error("Wait for submitted record geometry.");
          const details = control === 66n ? geometry.details !== true : true;
          const page = !details ? 0 : control === 67n ? Math.max(0, geometry.page - 1) : control === 68n ? geometry.page + 1 : 0;
          queueVisualControl("menu-details", { details, page, geometryVersion: nextGeometry() });
          lastMenuAction = request.actionId; return;
        }
        if (route === 5 && control === 34n) menuAssignPlayer = menuRoster.players[menuOwner.selected];
        if (route === 9 && control === 20n) {
          const fields = menuOwner.fields(), offset = 4 + Number(fields[2]) * 2 + menuOwner.selected * 5;
          if (fields[0] !== "1" || fields[offset + 4] !== "1") throw new Error("Browser source assignment is unavailable.");
          const metadata = fields;
          commitMenuRoster(roster => roster.assign(menuAssignPlayer, BigInt(fields[offset])));
          menuOwner.back(menuOwner.screen, menuOwner.revision);
          menuOwner.set_fields(menuOwner.screen, menuOwner.revision, menuRosterFields(metadata));
          lastMenuAction = request.actionId; reportMenu(); publishVisual(); return;
        }
        if (route === 4 && [56n, 57n].includes(control) || route === 5 && [36n, 37n].includes(control)
          || [6, 9].includes(route) && [23n, 24n].includes(control)) {
          const fields = menuOwner.fields();
          const count = route === 4 ? fields.length : route === 5 ? menuRoster.players.length : Number(fields[3 + Number(fields[2]) * 2]);
          const direction = [56n, 36n, 23n].includes(control) ? -1 : 1;
          const index = Math.max(0, Math.min(count - 1, menuOwner.selected + direction * 10));
          menuOwner.select(menuOwner.screen, menuOwner.revision, index);
          lastMenuAction = request.actionId; reportMenu(); publishVisual(); return;
        }
        if (route === 5 && [32n, 33n, 35n].includes(control)) {
          commitMenuRoster(roster => {
            if (control === 35n) roster.assign(roster.players[menuOwner.selected], null);
            else roster.setCount(roster.players.length + (control === 33n ? 1 : -1));
          });
          lastMenuAction = request.actionId; reportMenu(); publishVisual(); return;
        }
        if (route === 2 && [12n, 15n].includes(control)) throw new Error("This Settings control is unavailable in the browser.");
        if (route === 2 && control === 10n) {
          const fields = menuOwner.fields();
          if (fields.length === 34) settingsMenuProfileSnapshot(fields);
          else settingsMenuSnapshot(fields);
        }
        if (route === 7 && control === 40n) {
          validateBrowserDisplayFields(menuOwner.fields());
        }
        const effect = menuOwner.action(request.screen, request.revision, request.actionId, control);
        reportMenu();
        if (effect !== 0n) report("menu-effect", { menuGeneration, screen: menuOwner.screen, revision: menuOwner.revision,
          route, effect, control, selected: menuOwner.selected, fields: menuOwner.fields() });
      }
      lastMenuAction = request.actionId;
    } else throw new Error("Unknown menu request.");
    reportMenu();
    if (menuOwner.fields().length || menuOwner.route === 1 || request.kind === "menu-fields") publishVisual();
  } catch (error) { report("menu-error", { message: message(error), menuGeneration: request.menuGeneration,
    screen: request.screen, revision: request.revision }); }
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

async function acquireMenuRecord(request) {
  if (menuRecordAcquiring) {
    report("menu-error", { message: "Wait for the current recorded prefix acquisition to finish.",
      menuGeneration: request.menuGeneration, screen: request.screen, revision: request.revision });
    return;
  }
  menuRecordAcquiring = true;
  const context = libraryContext, generation = importGeneration, epoch = selectionEpoch;
  const current = () => !failed && !disposed && epoch === selectionEpoch && menuOwner !== null && menuVisible && !play && !settingsOperation && !importing
    && generation === importGeneration && request.menuGeneration === menuGeneration
    && request.screen === menuOwner.screen && request.revision === menuOwner.revision;
  try {
    if (!current()) return;
    const replay = await recordBody(request.replayFile, RECORD_BYTES);
    if (!current()) return;
    const archive = request.archiveBlob == null ? undefined : await recordBody(request.archiveBlob, ARCHIVE_BYTES);
    if (!current()) return;
    if (!await acquireLibraryFile(context, request.chartPath, current)) return;
    if (!currentLibrary(context) || !current()) return;
    menuOwner.preview_record(request.screen, request.revision, context.library, request.chartPath, request.key,
      replay, archive, request.player);
    menuRecordFile = request.replayFile;
    reportMenu(); publishVisual();
  } catch (error) {
    if (currentLibrary(context) && current()) report("menu-error", { message: message(error),
      menuGeneration: request.menuGeneration, screen: request.screen, revision: request.revision });
  } finally {
    // Stale publication does not cancel an in-flight File/Blob body read.
    menuRecordAcquiring = false;
  }
}
function retireLibrary(context) {
  if (preparationOwner?.context === context) releasePreparation();
  if (!context || context.retired) return;
  context.retired = true;
  const owned = context.library;
  context.library = null;
  for (const reading of context.pending.values()) {
    for (const finish of Array.from(reading.waiters)) finish({ cancelled: true });
  }
  context.files.clear(); context.loaded.clear(); context.pending.clear();
  owned?.free();
}
function currentLibrary(context) {
  return !failed && !disposed && context !== null && context === libraryContext && !context.retired && context.library !== null;
}
async function acquireLibraryFile(context, path, current, preparation = null) {
  if (!currentLibrary(context) || !current()) return false;
  if (context.loaded.has(path)) return true;
  const entry = context.files.get(path);
  if (!entry) throw new Error(`Selected file not found: ${path}`);
  const { file, size } = entry;
  let reading = context.pending.get(path);
  if (!reading) {
    // One subscription per underlying read; cancelled callers remove their waiters.
    reading = { waiters: new Set() };
    context.pending.set(path, reading);
    const settle = result => {
      if (context.pending.get(path) === reading) context.pending.delete(path);
      for (const finish of Array.from(reading.waiters)) finish(result);
    };
    try {
      Promise.resolve(file.arrayBuffer()).then(bytes => settle({ bytes }), error => settle({ error }));
    } catch (error) {
      context.pending.delete(path);
      throw error;
    }
  }
  const result = await new Promise(resolve => {
    const cancel = () => finish({ cancelled: true });
    const finish = result => {
      reading.waiters.delete(finish);
      if (preparation?.wake === cancel) preparation.wake = null;
      resolve(result);
    };
    reading.waiters.add(finish);
    if (preparation) preparation.wake = cancel;
  });
  if (result.cancelled || !currentLibrary(context) || !current()) return false;
  if (Object.hasOwn(result, "error")) throw result.error;
  const bytes = result.bytes;
  if (!(bytes instanceof ArrayBuffer) || bytes.resizable === true || bytes.byteLength !== size)
    throw new Error("Selected-file size changed or returned an invalid buffer.");
  if (!context.loaded.has(path)) {
    context.library.add_file(path, new Uint8Array(bytes));
    context.loaded.add(path);
  }
  return true;
}
function releasePreparation(record = preparationOwner) {
  if (!record || record.cancelled) return;
  record.cancelled = true;
  if (preparationOwner === record) preparationOwner = null;
  const job = record.job; record.job = null;
  record.queued = false;
  const channel = record.channel; record.channel = null;
  if (channel) {
    channel.port1.onmessage = null;
    channel.port1.close(); channel.port2.close();
  }
  const wake = record.wake; record.wake = null; wake?.();
  if (job) { try { job.cancel(); } finally { job.free(); } }
}
function preparationCurrent(record) {
  return preparationOwner === record && !record.cancelled && currentLibrary(record.context) && record.current();
}
function preparationProgress(record, stage = record.job?.stage ?? "file-read") {
  if (!preparationCurrent(record)) return;
  const job = record.job;
  report(record.kind === "preview" ? "selection-progress" : "play-preparation-progress", {
    ...(record.kind === "preview" ? { id: record.id } : { playId: record.id }),
    libraryId: record.context.id, path: record.path, stage,
    completedAssets: job?.completed_assets ?? 0, totalAssets: job?.total_assets ?? 0,
    parses: job?.parse_count ?? 0, compiles: job?.compile_count ?? 0,
    reads: job?.read_count ?? 0, decodes: job?.decode_count ?? 0,
    sharedAssets: job?.shared_asset_count ?? 0, beginMs: record.beginMs, maxStepMs: record.maxStepMs,
  });
}
async function prepareChartJob(context, path, current, kind, id, begin, configure = null) {
  releasePreparation();
  const record = { context, path, current, kind, id, job: null, cancelled: false,
    channel: null, queued: false, wake: null, beginMs: 0, maxStepMs: 0 };
  preparationOwner = record;
  const alive = () => preparationCurrent(record);
  try {
    preparationProgress(record);
    if (!await acquireLibraryFile(context, path, alive, record) || !alive()) return null;
    const started = performance.now();
    record.job = begin();
    configure?.(record.job);
    record.beginMs = performance.now() - started;
    while (alive()) {
      const pending = record.job.pending_asset_path();
      if (pending !== undefined && pending !== null) {
        preparationProgress(record, "file-read");
        if (!await acquireLibraryFile(context, pending, alive, record) || !alive()) return null;
      }
      if (!alive()) return null;
      const step = performance.now();
      const complete = record.job.advance(context.library);
      record.maxStepMs = Math.max(record.maxStepMs, performance.now() - step);
      if (!alive()) return null;
      preparationProgress(record);
      if (record.channel === null) {
        record.channel = new MessageChannel();
        record.channel.port1.onmessage = () => {
          if (!record.queued) return;
          record.queued = false;
          const wake = record.wake; record.wake = null; wake?.();
        };
      }
      await new Promise(resolve => {
        record.wake = resolve;
        record.queued = true;
        record.channel.port2.postMessage(0);
      });
      if (!alive()) return null;
      if (complete) return record.job.take_prepared(context.library);
    }
    return null;
  } finally { releasePreparation(record); }
}
function cancelSelection(request) {
  const record = preparationOwner;
  if (!record || record.kind !== "preview" || record.id !== request.id
    || record.context.id !== request.libraryId || !preparationCurrent(record)) return;
  ++selectionEpoch;
  releasePreparation(record);
  report("selection-cancelled", { id: request.id, libraryId: request.libraryId });
}
async function importFiles(request, generation) {
  let candidate = null;
  try {
    const entries = preflight(request.files);
    await ready;
    if (generation !== importGeneration || failed || disposed) return;
    candidate = { library: new BrowserLibrary(LIMITS.files, LIMITS.file, LIMITS.total, LIMITS.path),
      id: request.id, generation, files: new Map(), pending: new Map(), loaded: new Set(), retired: false };
    for (const { file, path } of entries) {
      const size = file.size;
      candidate.library.declare_file(path, size);
      candidate.files.set(path, { file, size });
    }
    const charts = candidate.library.chart_paths();
    if (!charts.length) throw new Error("No BMS, BME, BML or PMS chart was selected.");
    stagedLibrary = candidate;
    candidate = null;
    // Catalog admission needs metadata only; preparation remains demand-loaded.
    report("catalog", { id: request.id, charts });
  } catch (error) {
    if (generation === importGeneration && !failed && !disposed) report("import-error", { id: request.id, message: message(error) });
  } finally { retireLibrary(candidate); }
}
function queueImport(request) {
  ++selectionEpoch;
  releasePreparation();
  retireLibrary(stagedLibrary); stagedLibrary = null;
  pendingImport = { request, generation: ++importGeneration };
  importing = true;
  if (!importPumpRunning) void drainImports().catch(fatal);
}
function acceptLibrary(request) {
  if (!stagedLibrary || request.id !== stagedLibrary.id || stagedLibrary.generation !== importGeneration) return;
  const previous = libraryContext;
  libraryContext = stagedLibrary;
  libraryId = stagedLibrary.id;
  stagedLibrary = null;
  retireLibrary(previous);
}
async function drainImports() {
  importPumpRunning = true;
  try {
    while (pendingImport && !failed && !disposed) {
      const { request, generation } = pendingImport;
      pendingImport = null;
      await importFiles(request, generation);
    }
  } finally {
    importPumpRunning = false;
    importing = false;
    if (failed || disposed) pendingImport = null;
  }
}
async function selectChart(request) {
  const generation = importGeneration, epoch = ++selectionEpoch;
  releasePreparation();
  const context = libraryContext;
  const playingAtEntry = play !== null;
  const currentIdentity = () => epoch === selectionEpoch && generation === importGeneration;
  const current = () => currentIdentity() && !play && !settingsOperation;
  await ready;
  if (failed || disposed || !currentIdentity()) return;
  if (playingAtEntry) {
    report("selection-error", { id: request.id, message: "Stop gameplay before changing the preview chart." });
    return;
  }
  // A play/settings owner acquired after this request fences its continuation.
  if (!current()) return;
  let prepared = null;
  try {
    if (importing || !currentLibrary(context) || request.libraryId !== libraryId) throw new Error("Wait for the selected library to finish loading.");
    if (!Number.isInteger(request.rate) || request.rate < 1 || request.rate > 0xffffffff) throw new Error("Sample rate must be a positive 32-bit integer.");
    if (typeof request.seed !== "string" || !/^\d{1,20}$/.test(request.seed) || BigInt(request.seed) > 0xffffffffffffffffn) throw new Error("Chart seed must fit an unsigned 64-bit integer.");
    prepared = await prepareChartJob(context, request.path, current, "preview", request.id,
      () => context.library.begin_prepare_chart(request.path, request.rate, 2, BigInt(request.seed),
        64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES));
    if (!prepared) return;
    if (!currentLibrary(context) || !current()) return;
    const metadata = { title: prepared.title, artist: prepared.artist, duration: prepared.duration_ns.toString(),
      notes: prepared.note_count, samples: prepared.sample_count, images: prepared.image_count };
    fenceVisual();
    const previous = preview;
    preview = prepared; prepared = null; previewSongNs = 0n; previous?.free();
    selectedId = request.id;
    report("selected", { id: request.id, libraryId, path: request.path, ...metadata });
    publishVisual();
  } catch (error) {
    if (!failed && !disposed && current() && (context === null || currentLibrary(context)))
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
  let preOriginInputs = state.preOriginInputs;
  if (state.mode === "live") {
    preOriginInputs = null;
    try {
      const actual = state.game?.pre_origin_inputs;
      if (unsigned(actual)) {
        const combined = BigInt(state.preOriginInputs) + actual;
        if (combined <= BigInt(Number.MAX_SAFE_INTEGER)) preOriginInputs = Number(combined);
      }
    } catch {}
  }
  const result = { songNs: null, hits: null, misses: null, combo: null, maxCombo: null, preOriginInputs };
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

function disposeRetainedGame(state) {
  state.sampleClient?.close(); state.sampleClient = null;
  state.commandClient?.close(); state.commandClient = null;
  state.audioPumping = state.commandPumping = false;
  state.renderObservation = null;
  const game = state.game;
  const result = { cleanupError: null, replay: null, replayError: null, completedArchive: null,
    archivePlayers: null, archiveError: null,
    ...(state.localPlan ? { replays: retainedPlayers(state).map(player => ({ player, replay: null, replayError: null, replayComplete: false })) } : {}) };
  if (!game) return result;
  try { captureCompletedResults(state); } catch (error) { result.cleanupError = error; }
  const cache = state.currentCaptures ??= { rows: [], archive: null, archiveAttempted: false };
  if (state.recordReplay && !cache.archiveAttempted) {
    try {
      const bytes = game.completed_archive();
      if (bytes != null) {
        if (!(bytes instanceof Uint8Array) || bytes.byteLength === 0 || bytes.byteLength > 5 * 1024 * 1024) throw new Error("Invalid bounded completed archive.");
        cache.archive = new Blob([bytes], { type: "application/octet-stream" });
      }
      cache.archiveAttempted = true;
    } catch (error) { result.archiveError = message(error); }
  }
  result.completedArchive = cache.archive;
  if (cache.archive) result.archivePlayers = retainedPlayers(state);
  try { game.stop(); } catch (error) { result.cleanupError ??= error; }
  if (state.recordReplay) for (const player of retainedPlayers(state)) {
    let row = cache.rows.find(row => row.player === player);
    if (!row) { row = { player, bytes: null, file: null, exported: false }; cache.rows.push(row); }
    try {
      if (!row.exported) {
        row.bytes = state.localPlan ? game.take_replay(player) : game.take_replay();
        row.exported = true;
      }
      if (row.bytes != null && row.file === null) {
        if (!(row.bytes instanceof Uint8Array) || !(row.bytes.buffer instanceof ArrayBuffer) || row.bytes.buffer.resizable === true
          || row.bytes.byteLength === 0 || row.bytes.byteLength > (state.recordLimits?.bytes ?? 64 * 1024 * 1024)) throw new Error("Invalid retained current recording bytes.");
        row.file = new File([row.bytes], "recording.bkr", { type: "application/octet-stream" });
      }
      if (state.localPlan) result.replays.find(value => value.player === player).replay = row.file;
      else result.replay = row.file;
    } catch (error) {
      result.cleanupError ??= error;
      if (state.localPlan) result.replays.find(value => value.player === player).replayError = message(error);
      else result.replayError = message(error);
    }
  }
  if (state.retiredCohort) result.cleanupError ??= new Error("Retired recording custody is still owned by this Worker.");
  if (result.cleanupError === null) {
    try { game.free(); state.game = null; } catch (error) { result.cleanupError = error; }
  }
  return result;
}
function disposeGame(state) {
  if (state.retained) return disposeRetainedGame(state);
  if (preparationOwner?.kind === "play" && preparationOwner.id === state.id) releasePreparation();
  const sampleClient = state.sampleClient;
  state.sampleClient = null;
  sampleClient?.close();
  const client = state.commandClient;
  state.commandClient = null;
  state.commandPumping = false;
  state.audioPumping = false;
  state.renderObservation = null;
  state.lastPresentation = null;
  state.lastReplayPresentedNs = null;
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
        || bytes.buffer.resizable === true || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
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
  if (result.completedArchive) {
    try { result.completedArchive = new Blob([result.completedArchive], { type: "application/octet-stream" }); }
    catch (cause) { result.completedArchive = null; result.archivePlayers = null; result.archiveError = recordError(cause); }
  }
  const captureFile = bytes => new File([bytes], "recording.bkr", { type: "application/octet-stream" });
  if (result.replay) {
    try { result.replay = captureFile(result.replay); }
    catch (cause) { result.replay = null; result.replayError = recordError(cause); }
  }
  for (const row of result.replays ?? []) if (row.replay) {
    try { row.replay = captureFile(row.replay); }
    catch (cause) { row.replay = null; row.replayError = recordError(cause); }
  }
  try { game.free(); } catch (cause) { result.cleanupError ??= cause; }
  return result;
}


function retainedCustodyFailure(state, error) {
  state.retiredClosing = false;
  if (state.retiredCohort) {
    clearTimeout(state.retiredCohort.timer);
    state.retiredCohort.promise = null;
  }
  const score = statistics(state);
  const { cleanupError, ...captures } = disposeRetainedGame(state);
  state.active = false;
  fenceVisual();
  report("play-error", { playId: state.id, released: false, message: message(error),
    ...captures, replayComplete: false, ...score });
}
function failPlay(state, error, request = null) {
  if (play !== state) return;
  if (state.retained && state.retiredCohort && !state.retiredClosing) {
    state.retiredClosing = true;
    void drainRetiredRecordings(state, state.retiredCohort.boundarySerial).then(() => {
      state.retiredClosing = false; failPlay(state, error, request);
    }, cause => retainedCustodyFailure(state, cause));
    return;
  }
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
  if (state.retained && cleanupError && state.game) { play = state; state.active = false; }
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
    ...(savedOpponents ? { savedOpponents } : {}) });
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
  if (state.retained && state.retiredCohort) {
    if (state.retiredClosing) return;
    state.retiredClosing = true;
    void drainRetiredRecordings(state, state.retiredCohort.boundarySerial).then(() => {
      state.retiredClosing = false; stopPlay(state, request);
    }, cause => retainedCustodyFailure(state, cause));
    return;
  }
  if (completed && (!state.completed || commandsPending(state) || state.audioPumping
    || state.renderObservation !== null)) throw new Error("Natural stop has no current completion evidence.");
  ++capturesPending;
  const score = statistics(state);
  const savedOpponents = finalOpponents(state);
  retainFinalGroup(state);
  sendRoomProgress(state, true);
  if (preparationOwner?.kind === "play" && preparationOwner.id === state.id) releasePreparation();
  play = null;
  fenceVisual();
  if (state.network) {
    state.network.stopping = true;
    clearRemoteProgress(state.network);
  }
  const roomClosing = state.room ? finishRoom(state, completed) : null;
  const { cleanupError, replay, replayError, replays, completedArchive, archivePlayers, archiveError } = disposeGame(state);
  if (state.retained && cleanupError && state.game) { play = state; state.active = false; }
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
      ...result, replayComplete: false });
    else report("play-stopped", { playId: state.id, ...result,
      replayComplete: completed && replay !== null });
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
    const selectedSpeed = state.mode === "live" ? state.liveSpeed : state.replaySpeed;
    const normalSpeed = selectedSpeed.numerator === selectedSpeed.denominator;
    if (state.mode === "live" && state.replaySpeed.numerator !== state.replaySpeed.denominator) throw new Error("Replay speed cannot change live gameplay.");
    if (state.mode === "replay" && !normalSpeed && typeof BrowserReplay?.new_with_speed !== "function") {
      throw new Error("The replay binding does not provide selected speed ownership.");
    }
    if (state.mode === "live" || request.windowOriginNs !== undefined) {
      if (!hostTime(request.windowOriginNs)) throw new Error("Gameplay requires the actual Window clock origin.");
      state.windowOriginNs = request.windowOriginNs;
    }
    if (state.mode === "live") {
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
    const timingPolicy = state.mode === "live" ? validateTimingPolicy(request.timingPolicy) : null;
    const requestedStart = state.mode === "live" ? state.requestedStartNs : null;
    const requestedEnd = state.mode === "live" ? validateEnd(requestedStart, request.endNs) : undefined;
    const speedConstructor = state.localPlan || state.physicalInput && !state.touchInput ? "new_physical_and_speed"
      : state.touchInput ? "new_physical_contact_and_speed"
      : requestedEnd !== undefined ? "new_section_and_speed" : "new_and_speed";
    if (state.mode === "live" && !normalSpeed
      && typeof (state.localPlan ? BrowserLocalGame : BrowserGame)?.[speedConstructor] !== "function") {
      throw new Error("The gameplay binding does not provide selected live speed ownership.");
    }
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
      || !libraryContext || request.libraryId !== libraryId) throw new Error("Wait for the accepted library before starting gameplay.");
    if (typeof request.path !== "string" || !request.path.length || !integer(request.rate, 1, 0xffffffff)) {
      throw new Error("Invalid gameplay chart or sample rate.");
    }
    const context = libraryContext;
    const generation = importGeneration;
    const current = () => play === state && generation === importGeneration && !failed && !disposed;
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
      const recording = new Uint8Array(bytes);
      prepared = await prepareChartJob(context, request.path, current, "play", state.id,
        () => context.library.begin_prepare_replay_chart(request.path, recording, request.rate, 2,
          64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES));
    } else {
      if (typeof request.seed !== "string" || !/^\d{1,20}$/.test(request.seed) || BigInt(request.seed) > U64_MAX) throw new Error("Invalid gameplay chart seed.");
      prepared = await prepareChartJob(context, request.path, current, "play", state.id, () => timingPolicy !== null
        ? context.library.begin_prepare_chart_with_policy_at(request.path, request.rate, 2, BigInt(request.seed), requestedStart,
          64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES,
          timingPolicy.presetId, timingPolicy.rankPrecedence, timingPolicy.gauge, timing.offsetNs)
        : requestedStart === 0n
        ? context.library.begin_prepare_chart(request.path, request.rate, 2, BigInt(request.seed), 64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES)
        : context.library.begin_prepare_chart_at(request.path, request.rate, 2, BigInt(request.seed), requestedStart,
          64 * 1024 * 1024, 256 * 1024 * 1024, ORIGINAL_PCM_SAMPLES),
        state.retained ? job => job.enable_retained_practice() : null);
    }
    if (!prepared || !currentLibrary(context) || !current()) return;
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
    state.movies = prepared.video_registration();
    const moved = prepared;
    prepared = null; // A consuming Rust constructor also owns the argument on Err.
    state.game = state.mode === "live" && !normalSpeed
      ? state.localPlan
        ? BrowserLocalGame.new_physical_and_speed(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs,
          state.localPlan.words, localBindings.words, requestedEnd, state.touchInput, 4096, 1024, selectedSpeed.fraction)
        : state.touchInput
          ? BrowserGame.new_physical_contact_and_speed(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, bindingWords, requestedEnd, 4096, 1024, selectedSpeed.fraction)
          : state.physicalInput
            ? BrowserGame.new_physical_and_speed(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, bindingWords, requestedEnd, 4096, 1024, selectedSpeed.fraction)
            : requestedEnd === undefined
              ? BrowserGame.new_and_speed(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, pairs, selectedSpeed.fraction)
              : BrowserGame.new_section_and_speed(moved, 0n, 100000000n, timing.earlyNs, timing.lateNs, timing.offsetNs, pairs, requestedEnd, selectedSpeed.fraction)
      : state.mode === "replay"
      ? typeof BrowserReplay?.new_with_speed === "function"
        ? BrowserReplay.new_with_speed(moved, 100000000n, state.replaySpeed.fraction)
        : new BrowserReplay(moved, 100000000n)
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
    if (state.retained) {
      const limits = request.audioLimits;
      if (!limits || !integer(limits.maxVoices, 1, 4096) || !integer(limits.maxCommandsPerRender, 1, 65536)) {
        throw new Error("Retained playback requires the actual negotiated audio limits.");
      }
      state.game.configure_retained_limits(limits.maxVoices, limits.maxCommandsPerRender, 64, 64, 4096);
      state.retainedSetup = retainedWords(state.game.retained_setup_words(), 32, "setup");
      state.programId = retainedWord(state.retainedSetup, 1);
      if (retainedWord(state.retainedSetup, 0) !== 1n || state.programId === 0n) throw new Error("Invalid retained Program identity.");
      metadata.retained = true;
      metadata.programId = state.programId;
      metadata.capability = retainedCapability(state);
      if (request.recordReplay === true) {
        const budget = request.captureBudget;
        if (!budget || !integer(budget.remainingRecords, 0, 128) || !integer(budget.remainingBytes, 0, 256 * 1024 * 1024)) {
          throw new Error("Retained recording requires the actual remaining File custody budget.");
        }
        state.captureBudget = { ...budget };
        requireCaptureBudget(state, false);
      }
    }
    {
      const numerator = state.game.speed_numerator;
      const denominator = state.game.speed_denominator;
      if (!(normalSpeed && numerator === undefined && denominator === undefined)
        && (numerator !== selectedSpeed.numerator || denominator !== selectedSpeed.denominator)) {
        throw new Error(`Actual ${state.mode === "live" ? "live" : "replay"} speed differs from the prepared request.`);
      }
      if (state.mode === "live") {
        metadata.liveSpeed = selectedSpeed.fraction;
        metadata.liveSpeedNumerator = selectedSpeed.numerator;
        metadata.liveSpeedDenominator = selectedSpeed.denominator;
      } else {
        metadata.replaySpeed = selectedSpeed.fraction;
        metadata.replaySpeedNumerator = selectedSpeed.numerator;
        metadata.replaySpeedDenominator = selectedSpeed.denominator;
      }
    }
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
    const output = state.retained
      ? { endNs: validateEnd(startNs, state.game.end_ns), endFrame: undefined }
      : playbackOutputFromMetadata(startNs, state.game.end_ns, state.game.playback_end_frame, request.rate, selectedSpeed);
    if (state.retained && state.game.playback_end_frame !== undefined) throw new Error("Retained playback cannot have an immutable output end frame.");
    if (state.mode === "live" && output.endNs !== requestedEnd) throw new Error("Actual live section end differs from its request.");
    if (state.retained && output.endNs !== undefined) metadata.endNs = output.endNs;
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
    const pcm = sample.take_pcm_array();
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
  if (state.retained) throw new Error("Retained original samples belong to the direct sample producer port.");
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
      if (state.retained) {
        await client.retainedSetup(state.retainedSetup.slice());
        if (!current()) return;
      }
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
      if (state.retained) {
        const cues = game.retained_cue_count();
        if (!integer(cues, 0, 1000000) || BigInt(cues) !== retainedWord(state.retainedSetup, 9)) throw new Error("Retained cue count differs from original setup.");
        for (let index = 0; index < cues; index++) {
          await client.retainedCue(index, retainedWords(game.retained_cue_words(index), 9, "cue"));
          if (!current()) return;
        }
      }
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
      queueCapacity: request.queueCapacity, timeoutMs: request.timeoutMs,
      ...(state.retained ? { programId: state.programId } : {}) });
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

function retainedWords(words, length, label) {
  if (!(words instanceof Uint32Array) || !(words.buffer instanceof ArrayBuffer) || words.buffer.resizable === true
    || words.length !== length || words.byteOffset !== 0 || words.byteLength !== words.buffer.byteLength) {
    throw new Error(`Invalid retained ${label} words.`);
  }
  return words;
}
function retainedWord(words, index) {
  return BigInt(words[index * 2]) | BigInt(words[index * 2 + 1]) << 32n;
}
function retainedCapability(state, required = false) {
  const words = state.game.retained_capability_words();
  if (words?.length === 0) {
    retainedWords(words, 0, "capability");
    if (required) throw new Error("Retained control capability is unavailable before actual playback starts or after it ends.");
    return null;
  }
  retainedWords(words, 16, "capability");
  if (retainedWord(words, 0) !== 1n || retainedWord(words, 1) !== state.programId
    || retainedWord(words, 6) === 0n || retainedWord(words, 7) === 0n) throw new Error("Retained capability changed Program ownership.");
  return words;
}
function retainedFresh(event) {
  const tag = retainedWord(event, 1), kind = retainedWord(event, 25);
  return tag === 1n && (kind === 1n || kind === 2n)
    || tag === 3n && retainedWord(event, 17) === 0n && kind === 1n;
}
function retainedPlayers(state) { return state.localPlan ? state.localPlan.members.map(({ player }) => player) : [1]; }
function requireCaptureBudget(state, fresh) {
  if (!state.recordReplay && state.captureBudget === null) return;
  const cohorts = fresh ? 2 : 1;
  if (!state.captureBudget || state.captureBudget.remainingRecords < retainedPlayers(state).length * cohorts
    || state.captureBudget.remainingBytes < cohorts * 64 * 1024 * 1024) {
    throw new Error("Retained recording capacity exhausted; save captured recordings before starting another attempt.");
  }
}
function prepareRetained(state, request) {
  const fields = { rate: ["fraction"], region: ["startSeconds", "endSeconds", "repeat"],
    "disable-loop": [], "restart-original": [], pause: ["paused"] };
  const extra = fields[request.action];
  if (!state.retained || !state.active || !state.commandClient || request.version !== 1 || !extra
    || Reflect.ownKeys(request).length !== 5 + extra.length
    || Reflect.ownKeys(request).some(key => !["kind", "version", "playId", "rpcId", "action", ...extra].includes(key))) {
    throw new Error("Retained controls require the active retained Program and an exact version 1 draft.");
  }
  if (state.retainedSend || state.retainedPending.has(BigInt(request.rpcId))) throw new Error("A retained control admission is already pending.");
  const pause = request.action === "pause";
  const pending = [...state.retainedPending.values()];
  if (pending.length && !(pause && request.paused === false && pending.length === 1 && pending[0].action === "rate")) {
    throw new Error("Wait for the actual pending retained control result.");
  }
  if (request.action === "rate" && (state.opponentCount !== 0 || state.network || state.room)) {
    throw new Error("Runtime rate changes are unavailable with an admitted fixed-condition saved or network opponent; start a separate practice session.");
  }
  const capability = retainedCapability(state, true);
  const id = BigInt(request.rpcId), attempt = retainedWord(capability, 2), revision = retainedWord(capability, 5);
  const entry = { request, id, action: request.action, attempt, revision, pause, preparation: null };
  switch (request.action) {
    case "rate": {
      const speed = playbackSpeedFromFraction(request.fraction);
      entry.preparation = retainedWords(state.game.retained_prepare_rate(id, speed.numerator, speed.denominator), 20, "rate preparation");
      break;
    }
    case "region": {
      if (typeof request.repeat !== "boolean") throw new Error("Choose whether this retained region repeats.");
      const section = sectionFromSeconds(request.startSeconds, request.endSeconds);
      entry.start = section.startNs;
      entry.end = section.endNs ?? BigInt.asIntN(64, retainedWord(state.retainedSetup, 3));
      entry.repeat = request.repeat;
      requireCaptureBudget(state, true);
      state.game.retained_prepare_region(id, entry.start, entry.end, entry.repeat);
      break;
    }
    case "disable-loop": state.game.retained_prepare_disable_loop(id); break;
    case "restart-original":
      requireCaptureBudget(state, true);
      entry.preparation = retainedWords(state.game.retained_prepare_original_restart(id), 20, "restart preparation");
      break;
    case "pause":
      if (typeof request.paused !== "boolean") throw new Error("Invalid retained pause draft.");
      entry.paused = request.paused;
      state.game.retained_prepare_pause(id, entry.paused);
      break;
  }
  state.retainedSend = entry;
  state.retainedPending.set(id, entry);
  pumpAudio(state);
}
async function sendRetained(state, entry, client) {
  const request = { id: entry.id, attempt: entry.attempt, revision: entry.revision };
  try {
    switch (entry.action) {
      case "rate": await client.retainedRate({ ...request, preparation: entry.preparation }); break;
      case "region": await client.retainedRegion({ ...request, start: entry.start, end: entry.end, repeat: entry.repeat }); break;
      case "disable-loop": await client.retainedDisableLoop(request); break;
      case "restart-original": await client.retainedRestartOriginal({ ...request, preparation: entry.preparation }); break;
      case "pause": await client.retainedPause({ id: entry.id, attempt: entry.attempt, paused: entry.paused }); break;
    }
  } catch (error) {
    if (play !== state || state.commandClient !== client) return;
    if (error?.terminal !== false) throw error;
    state.game.retained_admission(entry.pause ? 1 : 0, entry.id, false, message(error));
    if (!entry.pause) { try { state.game.retained_response_words(); } catch { /* Consumes the correlated Rust admission refusal. */ } }
    state.retainedPending.delete(entry.id);
    state.retainedSend = null;
    report("play-reply", { playId: state.id, rpcId: entry.request.rpcId, error: message(error) });
    return;
  }
  if (play !== state || state.commandClient !== client) return;
  state.game.retained_admission(entry.pause ? 1 : 0, entry.id, true, "");
  state.retainedSend = null;
  reply(state, entry.request, { kind: "retained-pending", version: 1, id: entry.id,
    action: entry.action, capability: retainedCapability(state, true) });
}

async function drainRetiredRecordings(state, boundarySerial) {
  if (!state.retained || !state.recordReplay) return;
  let cohort = state.retiredCohort;
  if (cohort && cohort.boundarySerial !== boundarySerial) throw new Error("An earlier retired cohort still owns its File custody.");
  if (!cohort) {
    cohort = state.retiredCohort = { state, boundarySerial, players: retainedPlayers(state), rows: [], bytes: 0, accepted: false, promise: null };
    retiredCaptureOwner = cohort;
  }
  for (const player of cohort.players) {
    if (cohort.rows.some(row => row.player === player)) continue;
    const bytes = state.localPlan ? state.game.take_retired_replay(player) : state.game.take_retired_replay();
    if (bytes === undefined || bytes === null) {
      cohort.rows.push({ player, replay: null, replayComplete: false, replayError: null });
      continue;
    }
    const row = { player, bytes, replay: null, replayComplete: false, replayError: null };
    cohort.rows.push(row);
    if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer) || bytes.buffer.resizable === true
      || bytes.byteLength === 0 || bytes.byteLength > (state.recordLimits?.bytes ?? 64 * 1024 * 1024)
      || cohort.bytes + bytes.byteLength > 64 * 1024 * 1024) throw new Error("Retired cohort exceeds its recording bounds.");
    // Keep the actual exported bytes until File construction and Window acceptance succeed.
    cohort.bytes += bytes.byteLength;
  }
  let total = 0;
  for (const row of cohort.rows) if (row.bytes !== undefined) {
    if (!(row.bytes instanceof Uint8Array) || !(row.bytes.buffer instanceof ArrayBuffer) || row.bytes.buffer.resizable === true
      || row.bytes.byteLength === 0 || row.bytes.byteLength > (state.recordLimits?.bytes ?? 64 * 1024 * 1024)
      || total + row.bytes.byteLength > 64 * 1024 * 1024) throw new Error("Retired cohort exceeds its recording bounds.");
    total += row.bytes.byteLength;
    if (row.replay === null) row.replay = new File([row.bytes], "recording.bkr", { type: "application/octet-stream" });
  }
  cohort.bytes = total;
  if (!cohort.accepted && !cohort.promise) cohort.promise = new Promise((resolve, reject) => {
    cohort.resolve = resolve; cohort.reject = reject;
    cohort.timer = setTimeout(() => reject(new Error("Retired recording custody acknowledgement timed out.")), 10000);
    try {
      report("play-retired-recordings", { version: 1, playId: state.id, programId: state.programId,
        boundarySerial, players: cohort.players,
        replays: cohort.rows.map(({ player, replay, replayComplete, replayError }) => ({ player, replay, replayComplete, replayError })), bytes: cohort.bytes });
    } catch (error) { clearTimeout(cohort.timer); reject(error); }
  });
  if (!cohort.accepted) await cohort.promise;
  if (state.retiredCohort === cohort) state.retiredCohort = null;
  if (retiredCaptureOwner === cohort) retiredCaptureOwner = null;
}
function acceptRetiredRecordings(request) {
  const cohort = retiredCaptureOwner;
  if (!cohort || request.playId !== cohort.state.id || request.programId !== cohort.state.programId
    || request.boundarySerial !== cohort.boundarySerial) return;
  if (Reflect.ownKeys(request).length !== 9 || request.version !== 1 || typeof request.accepted !== "boolean"
    || !integer(request.remainingRecords, 0, 128) || !integer(request.remainingBytes, 0, 256 * 1024 * 1024)
    || !(request.error === null || typeof request.error === "string" && request.error.length <= 4096)) return;
  if (cohort.accepted) return;
  clearTimeout(cohort.timer);
  if (!request.accepted || request.error !== null) cohort.reject(new Error(request.error ?? "Window refused retired recording custody."));
  else {
    cohort.accepted = true;
    cohort.state.captureBudget = { remainingRecords: request.remainingRecords, remainingBytes: request.remainingBytes };
    cohort.resolve();
  }
}

async function observeRetainedOutput(state, observation, snapshot, client) {
  const game = state.game;
  let completed = false;
  if (snapshot !== null) {
    const { report: output, retained } = snapshot;
    if (!retained) throw new Error("Retained poll omitted its actual remote history.");
    retainedWords(retained.metadata, 12, "metadata");
    const history = retained.history;
    if (!(history instanceof Uint32Array) || history.length < 20 || history.length > 20 + 10 * 4096
      || history.length !== 20 + 10 * Number(retainedWord(history, 9))) throw new Error("Invalid bounded retained history.");
    game.retained_history(history);
    const event = retained.event;
    if (event !== null) {
      retainedWords(event, 80, "event");
      if (retainedWord(event, 2) !== state.programId) throw new Error("Retained event changed Program identity.");
      if (retainedFresh(event)) requireCaptureBudget(state, true);
      game.retained_event(event);
      state.retainedEvent = event;
    }
    // Service the held actual boundary before allowing the ordinary acquired-input drain.
    completed = observeOutput(state, observation, output, false);
  }
  const serial = game.retained_service(currentWindowHost(state), state.audioNs);
  if (serial !== undefined && serial !== null) {
    const event = state.retainedEvent;
    if (!event || retainedWord(event, 3) !== serial) throw new Error("Retained service returned an unowned commit serial.");
    const capability = retainedCapability(state);
    if (retainedFresh(event)) {
      fenceVisual();
      await drainRetiredRecordings(state, serial);
    }
    if (play !== state || state.game !== game || state.commandClient !== client) return false;
    const tag = retainedWord(event, 1), id = retainedWord(event, 4);
    const pending = state.retainedPending.get(id);
    if (pending) {
      const outcome = tag === 4n ? retainedWord(event, 36) === 1n ? "paused" : "resumed"
        : tag === 1n ? retainedWord(event, 25) === 5n ? "refused" : "applied"
          : ["applied", "noop", "refused"][Number(retainedWord(event, 17))];
      if (!outcome) throw new Error("Unknown retained result outcome.");
      let response = event, reason = outcome === "refused" ? "The audio owner refused this retained control." : null;
      if (tag !== 4n) {
        try { response = retainedWords(game.retained_response_words(), 80, "response"); }
        catch (error) { if (outcome !== "refused") throw error; reason = message(error); }
      }
      if (retainedWord(response, 3) !== serial) throw new Error("Retained result serial differs from actual committed event.");
      report("play-retained-result", { version: 1, playId: state.id, id, action: pending.action,
        event: response, capability, outcome, reason });
      state.retainedPending.delete(id);
    }
    await client.retainedEventAck(serial);
    if (play !== state || state.game !== game || state.commandClient !== client) return false;
    game.retained_event_ack(serial);
    state.retainedEvent = null;
    // Another actual event can share this cut; observe it before advancing inputs.
    return false;
  }
  serviceLiveAudio(state, currentWindowHost(state), state.audioNs);
  const pin = game.retained_history_pin();
  if (snapshot !== null && pin !== undefined && pin !== null && pin !== state.retainedPin) {
    try { await client.retainedHistoryRetire(pin); }
    catch (error) {
      if (error?.code !== "remote" || error.status !== 1 || error.terminal !== false
        || error.admitted !== 0 || client.state !== "ready") throw error;
      // A newly queued Worklet event still owns history; retry after a fresh poll.
      return false;
    }
    if (play !== state || state.game !== game || state.commandClient !== client) return false;
    state.retainedPin = pin;
  }
  state.completed = game.evaluate_completion();
  if (state.completed) captureCompletedResults(state);
  return state.completed || completed;
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

function observeOutput(state, observation, output, service = true) {
  renderedCursor(output, state.startFrame);
  let completed;
  let replayEvidence;
  if (state.mode === "replay") {
    let pair = null;
    let point = observation.presentedNs;
    let reason = null;
    if (Object.hasOwn(observation, "timestamp") || observation.paired) {
      const now = state.windowOriginNs === null ? null : currentWindowHost(state);
      pair = Object.hasOwn(observation, "timestamp")
        ? observation.timestamp === null ? null
          : presentationPair(observation.timestamp, state.startFrame, state.rate, Number(now) / 1000000)
        : observation.presentedNs === null ? null
          : { outputNs: observation.presentedNs, hostNs: observation.presentedHostNs };
      if (pair !== null && now !== null && (pair.hostNs > now || now - pair.hostNs > 1000000000n)) pair = null;
      const availability = presentationAvailability(state.lastPresentation, pair);
      pair = availability.pair;
      point = pair?.outputNs ?? null;
      reason = availability.reason;
    }
    const available = presentationOutputAvailability(state.lastReplayPresentedNs, point);
    replayEvidence = { point: available.point, pair, reason: reason ?? available.reason };
    completed = state.game.observe_output(output.words, available.point);
  } else {
    // An asynchronous Worklet poll may outlive the original request timestamp.
    // Refresh Window-equivalent time while retaining the original association.
    const now = currentWindowHost(state);
    let pair = Object.hasOwn(observation, "timestamp")
      ? observation.timestamp === null ? null
        : presentationPair(observation.timestamp, state.startFrame, state.rate, Number(now) / 1000000, 1000, state.retained)
      : observation.presentedNs === null ? null
        : { outputNs: observation.presentedNs, hostNs: observation.presentedHostNs };
    if (pair !== null && (pair.hostNs > now || now - pair.hostNs > 1000000000n)) pair = null;
    const availability = presentationAvailability(state.lastPresentation, pair, state.retained);
    pair = availability.pair;
    state.game.admit_output(output.words, pair !== null && pair.outputNs >= 0n ? pair.outputNs : null);
    if (pair !== null) {
      if (state.retained) state.game.retained_observe_presentation(pair.outputNs, pair.hostNs);
      else state.game.observe_presentation(pair.outputNs, pair.hostNs);
      state.lastPresentation = pair;
      if (state.presentationReason !== null) presentationUnavailable(state, null);
    } else if (availability.reason !== null) presentationUnavailable(state, availability.reason);
    if (reportWord(output.words, 23) === 1n) {
      const audioNs = audioScheduleFromFrame(reportWord(output.words, 24), state.startFrame, state.rate);
      if (audioNs > state.audioNs) state.audioNs = audioNs;
    }
    if (service) serviceLiveAudio(state, now, state.audioNs);
    completed = service ? state.game.evaluate_completion() : false;
  }
  if (typeof completed !== "boolean" || (completed && (state.batch !== null || state.commandPumping))) {
    throw new Error("Invalid completion with outstanding gameplay commands.");
  }
  if (replayEvidence) {
    if (replayEvidence.point !== null) {
      state.lastReplayPresentedNs = replayEvidence.point;
      if (replayEvidence.pair !== null) state.lastPresentation = replayEvidence.pair;
      if (state.presentationReason !== null) presentationUnavailable(state, null);
    } else if (replayEvidence.reason !== null) presentationUnavailable(state, replayEvidence.reason);
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
    if (state.mode === "replay" && state.windowOriginNs === null) throw new Error("Raw replay presentation requires the actual Window clock origin.");
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
    const nowMs = Number(currentWindowHost(state)) / 1000000;
    const pair = timestamp === null ? null : presentationPair(timestamp, state.startFrame, state.rate, nowMs, 1000, state.retained);
    return Object.freeze({ renderId: request.renderId,
      timestamp, presentedNs: pair?.outputNs ?? null, presentedHostNs: pair?.hostNs ?? null });
  }
  if (Object.hasOwn(request, "observedNowMs")) throw new Error("Raw presentation time requires its timestamp snapshot.");
  if (!direct && state.mode === "replay") {
    if (!(request.presentedNs === null || hostTime(request.presentedNs))) throw new Error("Invalid replay output presentation point.");
  } else if (!(request.presentedNs === null && request.presentedHostNs === null)
    && !(hostTime(request.presentedNs) && hostTime(request.presentedHostNs))) throw new Error("Invalid output presentation pair.");
  return Object.freeze({ renderId: request.renderId,
    presentedNs: request.presentedNs, presentedHostNs: request.presentedHostNs, paired: direct });
}

function publishRender(state, observation, completed) {
  if (completed && commandsPending(state)) throw new Error("Completion produced outstanding gameplay commands.");
  sendRoomProgress(state, completed);
  report("play-render-done", { playId: state.id, renderId: observation.renderId, completed,
    commandsPending: commandsPending(state), pendingInputs: pendingInputs(state), observedTick: state.lastTick,
    ...(state.retained ? { retainedCapability: retainedCapability(state) } : {}),
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
      if (state.retainedSend) {
        await sendRetained(state, state.retainedSend, client);
        if (!current()) return;
        continue;
      }
      let batch;
      if (state.active && state.renderObservation !== null) {
        const observation = state.renderObservation;
        const output = state.retained ? await client.retainedPoll() : await client.poll();
        if (!current()) return;
        // Input can advance during the read. Apply the original presentation
        // pair to the actual current game, retaining its latest input frontier.
        const completed = state.retained ? await observeRetainedOutput(state, observation, output, client)
          : observeOutput(state, observation, output);
        if (!current()) return;
        state.renderObservation = null;
        batch = commandBatch(state);
        publishRender(state, observation, completed);
      } else {
        if (state.active && state.retainedEvent) {
          await observeRetainedOutput(state, null, null, client);
          if (!current()) return;
        }
        batch = commandBatch(state);
      }
      if (batch === null) {
        if (state.retainedSend) continue;
        break;
      }
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
  if (!state.retained) serviceLiveAudio(state, currentWindowHost(state), state.audioNs);
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
    let mode;
    let replaySpeed;
    let liveSpeed;
    let requestedStartNs = null;
    try {
      if (request.mode !== undefined && request.mode !== "live" && request.mode !== "replay") throw new Error("Invalid playback mode.");
      mode = request.mode ?? "live";
      if (request.retained !== undefined && typeof request.retained !== "boolean") throw new Error("Invalid retained playback selection.");
      if (request.retained === true && mode !== "live") throw new Error("Recorded replay cannot opt into retained live preparation.");
      replaySpeed = replaySpeedFromFraction(request.replaySpeed);
      liveSpeed = playbackSpeedFromFraction(request.liveSpeed);
      if (mode === "replay" && liveSpeed.numerator !== liveSpeed.denominator) {
        throw new Error("Live speed cannot change replay playback.");
      }
      if (mode === "live") {
        requestedStartNs = validateStart(request.startNs);
        validateLiveColdPhase(requestedStartNs, liveSpeed);
      }
    } catch (error) {
      lastPlayId = request.playId;
      const text = message(error);
      if (identity(request.rpcId)) report("play-reply", { playId: request.playId, rpcId: request.rpcId, error: text });
      report("play-error", { playId: request.playId, message: text, released: true });
      return;
    }
    discardRoomResults();
    const priorRoom = cancelRoomFinalization();
    const state = {
      resultsEpoch: roomResultsEpoch, roomResults: null, completedResults: null,
      id: request.playId, startRpcId: identity(request.rpcId) ? request.rpcId : null,
      game: null, keys: null, active: false, origin: null, startFrame: null,
      batch: null, commandClient: null, commandPumping: false, audioPumping: false,
      sampleMode: null, sampleClient: null, sampleRpcId: null, sampleCount: null, commandStarted: false,
      audioRpcId: null, renderObservation: null, lastPresentation: null, lastReplayPresentedNs: null,
      windowOriginNs: null, latencyHint: undefined, audioNs: 0n, presentationReason: null,
      lastRpc: 0, lastTick: 0, lastRender: 0,
      lastHost: null, acquiredPrefix: null, lastSequence: null, sourceOrder: new Map(), preOriginInputs: 0,
      recordReplay: false, completed: false,
      localPlan: null, localPage: 0, touchPlayer: null, recordLimits: null,
      mode, replaySpeed, liveSpeed, requestedStartNs, physicalInput: false, touchInput: false, touchWidth: null, touchHeight: null,
      hidSources: null, gamepadAdapter: null, pointerSources: null,
      rate: null, network: null, room: null, samplesEnded: false, commandsDrained: false,
      prepared: false, opponentCount: 0, opponentsFailed: false, opponentError: null, lastOpponents: null,
      retained: request.retained === true, programId: null, retainedSetup: null,
      retainedPending: new Map(), retainedSend: null, retainedEvent: null, retainedPin: null,
      captureBudget: null, retiredCohort: null,
    };
    ++selectionEpoch;
    releasePreparation();
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
    const requiresRpc = roomRpc || ["play-retained", "play-sample", "play-samples-upload", "play-audio", "play-commands", "play-activate", "play-network-ready", "play-page"].includes(request.kind);
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
    if (request.kind === "play-retained") { prepareRetained(state, request); return; }
    if (request.kind === "play-sample") samplePlay(state, request);
    else if (request.kind === "play-samples-upload") { audioHandled = true; uploadSamples(state, request); }
    else if (request.kind === "play-audio") { audioHandled = true; attachAudio(state, request); }
    else if (request.kind === "play-network-ready") {
      if (state.room !== null || !state.network || state.network.requested || state.active || !state.samplesEnded
        || !state.commandsDrained || commandsPending(state)) throw new Error("Multiplayer readiness requires completed sample and command preparation.");
      void networkReady(state, request);
    } else if (request.kind === "play-commands") {
      if (state.retained) throw new Error("Retained commands belong to the direct serial audio endpoint.");
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
    if (request.kind === "play-retained") report("play-reply", { playId: state.id, rpcId: request.rpcId, error: message(error) });
    else failPlay(state, error, request);
  }
}

self.addEventListener("message", event => {
  const request = event.data;
  if (request?.kind === "play-retired-recordings-ack") { acceptRetiredRecordings(request); return; }
  if (request?.kind === "dispose") {
    recordsRetired = true;
    recordFence();
    retireAudioSetup();
    if (play || roomFinalization || capturesPending) { report("dispose-error", { message: "Join gameplay capture and room cleanup before disposing the Worker." }); return; }
    if (disposed) return;
    disposed = true;
    if (menuOwner) { menuOwner.dispose(); menuOwner.free(); menuOwner = null; }
    menuVisible = false;
    fenceVisual();
    visual?.client.close();
    visual = null;
    try { renderPort?.close(); } catch {}
    renderPort = null;
    preview?.free(); preview = null;
    discardRoomResults();
    ++selectionEpoch;
    releasePreparation();
    retireLibrary(stagedLibrary); stagedLibrary = null;
    retireLibrary(libraryContext); libraryContext = null;
    ++importGeneration; pendingImport = null;
    if (settingsOperation) { settingsOperation.retired = true; settingsOperation.candidate = null; }
    settingsOperation = null;
    void joinRecords().then(() => report("disposed"), error => report("dispose-error", { message: recordError(error) }));
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
  if (request.kind === "records-close") { closeRecords(request); return; }
  if (request.kind === "records-request") { requestRecords(request); return; }
  if (request.kind === "audio-load") { loadAudioModule(request); return; }
  if (request.kind === "historical-record-page") { pageHistoricalRecord(request); return; }
  if (request.kind.startsWith("menu-")) { handleMenu(request); return; }
  if (request.kind === "historical-record-clear") { clearHistoricalRecord(request); return; }
  if (request.kind === "historical-record-accept") { acceptHistoricalRecord(request); return; }
  if (request.kind === "select-cancel") { cancelSelection(request); return; }
  if (request.kind === "historical-record-present") { void presentHistoricalRecord(request); return; }
  if (request.kind === "settings-profile-admit" || request.kind === "settings-profile-cancel") {
    settingsProfileDecision(request); return;
  }
  if (request.kind === "settings-profile-save" || request.kind === "settings-profile-load") {
    void settingsProfile(request).catch(() => { /* An unavailable response port leaves Window's bounded deadline authoritative. */ });
    return;
  }
  if (!ready) { closeAudioHandoff(request); return fatal(new Error("Initialize gameplay before sending commands.")); }
  if ((settingsOperation || recordsOperation && !recordsOperation.retired) && request.kind !== "resize") {
    closeAudioHandoff(request);
    if (request.kind === "play-start") {
      report("play-error", { playId: request.playId, rpcId: request.rpcId, released: true, message: "Wait for settings or recording processing before starting gameplay." });
    } else if (request.kind.startsWith("play-") && identity(request.playId) && identity(request.rpcId)) {
      report("play-reply", { playId: request.playId, rpcId: request.rpcId, error: "Wait for settings or recording processing before sending gameplay requests." });
    } else if (request.kind === "import" || request.kind === "accept-library") {
      report("import-error", { id: request.id, message: "Wait for settings or recording processing before importing files." });
    } else if (request.kind === "select") {
      report("selection-error", { id: request.id, message: "Wait for settings or recording processing before preparing a chart." });
    } else if (request.kind === "seek") {
      report("seek-error", { id: request.id, selectedId, message: "Wait for settings or recording processing before changing the preview." });
    }
    return;
  }
  if (request.kind.startsWith("play-")) {
    if (request.kind === "play-start" && menuVisible) { retireMenuMotions(visual); menuVisible = false; menuOwner?.suspend(); }
    handlePlay(request); return;
  }
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

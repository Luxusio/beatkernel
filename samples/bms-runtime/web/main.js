import { snapshotFiles, nanoseconds, seconds } from "./host_model.mjs";
import { AudioHost } from "./audio-host.mjs";
import { RecordsStore } from "./record-store.mjs";
import { KEY_BINDINGS, bindingsFor, millisecondsToNanos, frameNanos, startProjection, presentationPair } from "./play-model.mjs";

const byId = id => document.getElementById(id);
const ui = Object.fromEntries(["folder", "files", "chart", "rate", "seed", "prepare", "position", "seek", "title", "details", "status", "viewport", "play", "stop", "keys", "record", "export", "replay-file", "replay-play", "replay-name", "records", "records-refresh", "records-save", "records-use", "records-delete"].map(id => [id, byId(id)]));
let canvas = byId("canvas");
let worker = null;
let observer = null;
let density = null;
let owner = 0;
let serial = 0;
let importId = 0;
let libraryId = 0;
let selectId = 0;
let selectedId = 0;
let seekId = 0;
let initialized = false;
let importing = false;
let preparing = false;
let hasPreview = false;
let audioModule = null;
let activePlay = null;
let lastReplay = null;
let replayURL = null;
let replayURLTimer = null;
let selectedReplay = null;
let recordsStore = null;
let recordsOperation = null;

function status(text, error = false) {
  ui.status.textContent = text;
  ui.status.dataset.error = String(error);
}
function controls() {
  const playing = activePlay !== null;
  const busy = recordsOperation !== null;
  ui.folder.disabled = !initialized || preparing || playing || busy || !("webkitdirectory" in ui.folder);
  ui.files.disabled = !initialized || preparing || playing || busy;
  for (const field of [ui.chart, ui.rate, ui.seed, ui.prepare]) field.disabled = !initialized || !libraryId || importing || preparing || playing || busy;
  ui.position.disabled = ui.seek.disabled = !initialized || !hasPreview || importing || preparing || playing || busy;
  ui.play.disabled = !initialized || !hasPreview || importing || preparing || playing || busy || !audioModule;
  ui.stop.disabled = !playing || activePlay.phase === "closing";
  ui.record.disabled = ui.play.disabled;
  ui.export.disabled = playing || busy || lastReplay === null;
  ui["replay-file"].disabled = !initialized || importing || preparing || playing || busy;
  ui["replay-play"].disabled = ui.play.disabled || selectedReplay === null;
  const recordsDisabled = !initialized || importing || preparing || playing || busy;
  ui.records.disabled = ui["records-refresh"].disabled = recordsDisabled;
  ui["records-save"].disabled = recordsDisabled || lastReplay === null;
  ui["records-use"].disabled = ui["records-delete"].disabled = recordsDisabled || !ui.records.value;
}
function stop() {
  revokeReplayURL();
  closeRecords();
  if (activePlay?.phase !== "closing") void stopPlay("Playback stopped with the page.");
  ++owner;
  worker?.terminate();
  if (activePlay) releasePlayWorker(activePlay);
  worker = null;
  observer?.disconnect();
  observer = null;
  density?.removeEventListener("change", densityChanged);
  density = null;
  initialized = false;
  controls();
}
function fatal(error) {
  stop();
  canvas.hidden = true;
  status(`${String(error?.message ?? error).slice(0, 4096)} Reload this page to initialize a new preview.`, true);
}

function resize() {
  if (!worker) return;
  const box = ui.viewport.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  // Do not assign canvas backing dimensions here; the renderer validates first.
  const dimensions = [box.width, box.height].map(value => Math.round(value * dpr));
  if (dimensions.some(value => !Number.isSafeInteger(value) || value < 0 || value > 0xffffffff)) return fatal(new Error("Canvas dimensions are outside the supported range."));
  worker.postMessage({ kind: "resize", width: dimensions[0], height: dimensions[1] });
}
function densityChanged() {
  density?.removeEventListener("change", densityChanged);
  density = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  density.addEventListener("change", densityChanged);
  resize();
}

function prepare() {
  if (!worker || !libraryId || importing || preparing || activePlay || recordsOperation) return;
  try {
    const rate = Number(ui.rate.value);
    const seed = ui.seed.value;
    if (!Number.isInteger(rate) || rate < 1 || rate > 0xffffffff) throw new Error("Sample rate must be a positive 32-bit integer.");
    if (!/^\d{1,20}$/.test(seed) || BigInt(seed) > 0xffffffffffffffffn) throw new Error("Chart seed must fit an unsigned 64-bit integer.");
    if (!ui.chart.value) throw new Error("Select a chart first.");
    selectId = ++serial;
    preparing = true;
    canvas.hidden = true;
    controls();
    status("Preparing chart, sounds and backgrounds…");
    worker.postMessage({ kind: "select", id: selectId, libraryId, path: ui.chart.value, rate, seed });
  } catch (error) { preparing = false; controls(); status(error.message, true); }
}

function received(data) {
  if (data.kind.startsWith("play-")) { receivePlay(data); return; }
  if (data.kind === "ready") {
    initialized = true;
    controls();
    status("Choose a song folder, or select a chart and its resources together.");
    void loadAudio(owner);
  } else if (data.kind === "fatal") fatal(new Error(data.message));
  else if (data.kind === "import-progress" && data.id === importId) status(`Reading files: ${data.read} / ${data.total}`);
  else if (data.kind === "catalog" && data.id === importId) {
    worker.postMessage({ kind: "accept-library", id: data.id });
    importing = false;
    libraryId = data.id;
    ui.chart.replaceChildren();
    const options = document.createDocumentFragment();
    for (const path of data.charts) {
      const option = document.createElement("option");
      option.value = path;
      option.textContent = path;
      options.append(option);
    }
    ui.chart.append(options);
    controls();
    status(`${data.charts.length} chart(s) loaded. Choose a sample rate and prepare a chart.`);
  } else if (data.kind === "import-error" && data.id === importId) {
    importing = false;
    controls();
    status(`${data.message} Previous library and preview are retained.`, true);
  } else if (data.kind === "selected" && data.id === selectId && data.libraryId === libraryId) {
    preparing = false;
    hasPreview = true;
    selectedId = data.id;
    seekId = 0;
    ui.title.textContent = data.title || data.path;
    ui.details.textContent = `${data.artist || "Unknown artist"} · ${data.notes} notes · ${data.samples} sounds · ${data.images} images · last note: ${seconds(data.duration)} s`;
    ui.position.value = "0";
    controls();
    status("Chart prepared at 0 seconds. This preview does not play audio or judge input.");
  } else if (data.kind === "drawn" && data.selectedId === selectedId && !preparing) canvas.hidden = false;
  else if (data.kind === "selection-error" && data.id === selectId) {
    preparing = false;
    canvas.hidden = !hasPreview;
    controls();
    status(`${data.message} ${hasPreview ? "The previous preview is retained." : "No chart has been prepared."}`, true);
  } else if (data.kind === "position" && data.id === seekId && data.selectedId === selectedId) {
    ui.position.value = seconds(data.ns);
    status(`Preview position: ${seconds(data.ns)} seconds.`);
  } else if (data.kind === "seek-error" && data.id === seekId && data.selectedId === selectedId) status(data.message, true);
  else if (data.kind === "render-wait" && data.selectedId === selectedId) status("The graphics surface is not ready. Resize the view or choose Show position to retry.", true);
}

function start() {
  stop();
  libraryId = importId = selectId = selectedId = seekId = 0;
  importing = preparing = hasPreview = false;
  audioModule = null;
  selectedReplay = null;
  ui["replay-file"].value = "";
  ui["replay-name"].textContent = "Choose a recording and prepare its matching chart. Replay uses the recorded seed and section.";
  ui.records.replaceChildren(new Option("Refresh to browse saved records", ""));
  ui.keys.textContent = "";
  ui.folder.value = ui.files.value = "";
  ui.chart.replaceChildren(new Option("Choose files first", ""));
  ui.title.textContent = "No chart prepared";
  ui.details.textContent = "Select a song folder to begin.";
  ui.position.value = "0";
  const fresh = document.createElement("canvas");
  fresh.id = "canvas";
  fresh.width = 960;
  fresh.height = 720;
  fresh.hidden = true;
  fresh.setAttribute("aria-label", "Chart lanes and background at the selected song time");
  canvas.replaceWith(fresh);
  canvas = fresh;
  controls();
  status("Initializing graphics…");
  try {
    if (!window.isSecureContext || !window.Worker || !window.OffscreenCanvas || !canvas.transferControlToOffscreen || !window.ResizeObserver) throw new Error("This preview needs a secure context, Workers and OffscreenCanvas support.");
    const generation = owner;
    worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    worker.addEventListener("message", event => { if (generation === owner) received(event.data); });
    worker.addEventListener("error", event => {
      if (generation !== owner) return;
      event.preventDefault();
      fatal(new Error(event.message || "Could not load the browser Worker and generated WASM package."));
    });
    worker.addEventListener("messageerror", () => { if (generation === owner) fatal(new Error("Could not receive a Worker response.")); });
    const surface = canvas.transferControlToOffscreen();
    worker.postMessage({ kind: "init", canvas: surface }, [surface]);
    observer = new ResizeObserver(resize);
    observer.observe(ui.viewport);
    densityChanged();
  } catch (error) { fatal(error); }
}

function choose(event) {
  if (!initialized || preparing || !worker || activePlay || recordsOperation) return;
  const files = event.target.files;
  if (!files?.length) return;
  if (files.length > 32768) return status("Select no more than 32,768 files.", true);
  importId = ++serial;
  importing = true;
  controls();
  status("Checking the selected files…");
  try { worker.postMessage({ kind: "import", id: importId, files: snapshotFiles(files) }); }
  catch (error) { importing = false; controls(); status(error.message, true); }
  // A subsequent selection of the same folder still triggers change.
  event.target.value = "";
}
ui.folder.addEventListener("change", choose);
ui.files.addEventListener("change", choose);
byId("prepare-form").addEventListener("submit", event => { event.preventDefault(); prepare(); });
byId("seek-form").addEventListener("submit", event => {
  event.preventDefault();
  if (!worker || !hasPreview || preparing || importing || activePlay || recordsOperation) return;
  try {
    const ns = nanoseconds(ui.position.value);
    seekId = ++serial;
    worker.postMessage({ kind: "seek", id: seekId, selectedId, ns });
  } catch (error) { status(error.message, true); }
});
window.addEventListener("pagehide", stop);
window.addEventListener("pageshow", event => { if (event.persisted) start(); });
window.addEventListener("resize", resize);
ui.play.addEventListener("click", () => { void play("live"); });
ui["replay-play"].addEventListener("click", () => { void play("replay"); });
ui["replay-file"].addEventListener("change", event => {
  if (!initialized || importing || preparing || activePlay || recordsOperation) return;
  try {
    const files = event.target.files;
    if (!files?.length) return;
    const file = files[0];
    if (files.length !== 1 || !(file instanceof File) || !Number.isSafeInteger(file.size)
      || file.size < 1 || file.size > 64 * 1024 * 1024) throw new Error("Choose one nonempty replay no larger than 64 MiB.");
    selectedReplay = file;
    ui["replay-name"].textContent = `${file.name.slice(0, 256)} · ${file.size} bytes · uses recorded seed and section`;
    controls();
    status("Replay selected. Prepare its matching chart, then choose Play replay.");
  } catch (error) { status(String(error.message).slice(0, 4096), true); }
  finally { event.target.value = ""; }
});
ui.stop.addEventListener("click", () => { void stopPlay("Playback stopped."); });
ui.export.addEventListener("click", downloadReplay);
ui.records.addEventListener("change", controls);
for (const [id, action] of [["records-refresh", "refresh"], ["records-save", "save"], ["records-use", "use"], ["records-delete", "delete"]]) {
  ui[id].addEventListener("click", () => { void recordAction(action); });
}
window.addEventListener("blur", () => { void stopPlay("Playback stopped after losing focus."); });
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    void stopPlay("Playback stopped while the page is hidden.");
    const pending = recordsOperation !== null;
    closeRecords();
    controls();
    if (pending) status("Record library operation stopped while the page is hidden.");
  }
});
window.addEventListener("keydown", event => key(event, true));
window.addEventListener("keyup", event => key(event, false));

async function loadAudio(generation) {
  try {
    const response = await fetch(new URL("./audio-pkg/beatkernel_bms_runtime_bg.wasm", import.meta.url));
    if (!response.ok || !response.body) throw new Error("Build the separate browser-audio package to enable Play.");
    const reader = response.body.getReader();
    const chunks = [];
    let bytes = 0;
    try {
      for (;;) {
        const next = await reader.read();
        if (next.done) break;
        bytes += next.value.byteLength;
        if (bytes > 64 * 1024 * 1024) throw new Error("Audio WASM exceeds the 64 MiB setup limit.");
        chunks.push(next.value);
      }
    } catch (error) { await reader.cancel().catch(() => {}); throw error; }
    finally { reader.releaseLock(); }
    const binary = new Uint8Array(bytes);
    let offset = 0;
    for (const chunk of chunks) { binary.set(chunk, offset); offset += chunk.byteLength; }
    const compiled = await WebAssembly.compile(binary);
    if (generation !== owner) return;
    audioModule = compiled;
    controls();
  } catch (error) {
    if (generation === owner) status(`Chart preview remains available. ${String(error.message).slice(0, 4096)}`, true);
  }
}

function playRpc(session, kind, fields = {}) {
  if (activePlay !== session || session.phase === "closing" || !worker) return Promise.reject(new Error("Playback owner is closed."));
  if (session.rpc) return Promise.reject(new Error("A playback setup operation is already pending."));
  const rpcId = ++serial;
  if (!Number.isSafeInteger(rpcId)) return Promise.reject(new Error("Playback request identity exhausted."));
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      if (session.rpc?.rpcId !== rpcId) return;
      session.rpc = null;
      reject(new Error("Playback Worker operation timed out."));
    }, 10000);
    session.rpc = { rpcId, timer, resolve, reject };
    try { worker.postMessage({ kind, playId: session.id, rpcId, ...fields }); }
    catch (error) { clearTimeout(timer); session.rpc = null; reject(error); }
  });
}

async function play(mode = "live") {
  if (!initialized || !hasPreview || !audioModule || importing || preparing || activePlay || recordsOperation) return;
  if (mode === "replay" && selectedReplay === null) return;
  const session = { id: ++serial, owner, mode, phase: "preparing", controller: new AbortController(), audio: null, opening: null,
    rpc: null, timer: null, events: [], pressed: new Set(), bindings: [], sequence: 0n,
    tickId: 0, tickPending: null, audioBusy: false, batch: null, startFrame: null,
    origin: null, lastHost: 0n, lastStatus: 0, stopping: null, renderId: 0, renderPending: null,
    workerStarted: false, workerReleased: false, workerStop: null, finalScore: null,
    completionReady: false, lastPresentation: null, cleanupError: null,
    recordReplay: mode === "live" && ui.record.checked === true, replay: null, replayError: null, naturalFinishRequested: false,
    replayFile: mode === "replay" ? selectedReplay : null,
    chartPath: ui.chart.value,
    preview: { title: ui.title.textContent, details: ui.details.textContent, position: ui.position.value } };
  activePlay = session;
  controls();
  status(mode === "replay" ? "Preparing recorded replay and audio…" : "Preparing playable chart and audio…");
  try {
    // open invokes resume synchronously here, inside the button's user gesture.
    const opening = AudioHost.open({ module: audioModule, generation: session.id, channels: 2,
      pcmLimits: { maxAssetBytes: 64 * 1024 * 1024, maxTotalBytes: 256 * 1024 * 1024, maxSamples: 1296 },
      audioLimits: { queueCapacity: 4096, maxVoices: 4096, pendingCapacity: 4096, maxFrames: 4096, maxCommandsPerRender: 4096 },
      timeoutMs: 10000, signal: session.controller.signal });
    session.opening = opening;
    session.audio = await opening;
    if (activePlay !== session || session.phase === "closing") { await session.audio.stop(); return; }
    session.workerStarted = true;
    const source = mode === "replay" ? { mode, replayFile: session.replayFile }
      : { mode, seed: ui.seed.value, recordReplay: session.recordReplay,
        keyPairs: Uint32Array.from(KEY_BINDINGS.flatMap(row => [row[0], row[2]])) };
    const prepared = await playRpc(session, "play-start", { libraryId, path: ui.chart.value,
      rate: session.audio.sampleRate, ...source });
    if (mode === "replay" ? prepared.mode !== "replay" : prepared.mode !== undefined && prepared.mode !== "live") throw new Error("Playback preparation mode changed.");
    ui.title.textContent = prepared.title || ui.chart.value;
    ui.details.textContent = `${prepared.artist || "Unknown artist"} · ${prepared.notes} notes · ${prepared.samples} sounds · ${session.audio.sampleRate} Hz output`;
    session.bindings = mode === "replay" ? [] : bindingsFor(prepared.lanes);
    ui.keys.textContent = mode === "replay" ? "Recorded input playback · Escape stops the replay."
      : session.bindings.map(row => `${row[0].toString(16).toUpperCase()}: ${row[1]}`).join(" · ");
    for (let index = 0; index < prepared.samples; index++) {
      const sample = await playRpc(session, "play-sample");
      if (sample?.kind !== "sample") throw new Error("Prepared audio asset count changed.");
      await session.audio.sample(sample);
    }
    const end = await playRpc(session, "play-sample");
    if (end?.kind !== "samples-end") throw new Error("Prepared audio assets exceeded their declared count.");
    await session.audio.finish();
    for (;;) {
      const batch = await playRpc(session, "play-commands");
      if (batch === null) break;
      let ack;
      try { ack = await session.audio.commands(batch.commands); }
      catch (error) {
        if (session.phase !== "closing" && Number.isInteger(error.admitted)) {
          try { await playRpc(session, "play-ack", { sequence: batch.sequence, admitted: error.admitted, success: false }); }
          catch { /* Actual rejection fences the core; preserve the original audio error. */ }
        }
        throw error;
      }
      await playRpc(session, "play-ack", { sequence: batch.sequence, admitted: ack.admitted, success: true });
    }
    const clock = session.audio.controlClock();
    session.startFrame = session.audio.currentFrame + BigInt(Math.ceil(session.audio.sampleRate / 4));
    session.origin = startProjection(clock, session.startFrame);
    await session.audio.arm(session.startFrame);
    await playRpc(session, "play-activate", { hostNs: session.origin, startFrame: session.startFrame });
    if (millisecondsToNanos(performance.now()) >= session.origin) throw new Error("Playback activation missed its chosen start. Start a fresh session.");
    session.phase = "playing";
    ui.rate.value = String(session.audio.sampleRate);
    controls();
    ui.stop.focus();
    status(mode === "replay" ? "Playing recorded replay. Stop ends this session." : "Playing. Stop ends this session; leaving the page stops playback.");
    session.timer = setInterval(() => { if (session.mode === "live") pumpInput(session); void pumpAudio(session); }, 8);
  } catch (error) {
    if (activePlay === session && session.phase !== "closing") await stopPlay(`Playback failed: ${String(error.message).slice(0, 4096)}`, true);
  }
}

function key(event, down) {
  const session = activePlay;
  if (!session || session.phase !== "playing") return;
  if (event.code === "Escape" && down) { event.preventDefault(); void stopPlay("Playback stopped."); return; }
  const binding = session.bindings.find(row => row[1] === event.code);
  if (!binding) return;
  event.preventDefault();
  if (event.repeat || (down && session.pressed.has(event.code)) || (!down && !session.pressed.has(event.code))) return;
  try {
    if (session.events.length >= 1024) throw new Error("Pending keyboard input capacity exceeded.");
    const hostNs = millisecondsToNanos(event.timeStamp);
    if (hostNs < session.lastHost) throw new Error("Keyboard input arrived behind the accepted gameplay watermark.");
    if (down) session.pressed.add(event.code); else session.pressed.delete(event.code);
    session.sequence++;
    if (session.sequence > 18446744073709551615n) throw new Error("Keyboard sequence exhausted.");
    session.events.push({ hostNs, key: binding[2], down, sequence: session.sequence });
    session.completionReady = false;
    pumpInput(session);
  } catch (error) { void stopPlay(`Playback failed: ${error.message}`, true); }
}

function audioSchedule(session) {
  const frame = session.audio.currentFrame + BigInt(Math.ceil(session.audio.sampleRate / 50));
  return frameNanos(frame > session.startFrame ? frame - session.startFrame : 0n, session.audio.sampleRate);
}
function presentedPoint(session) {
  let timestamp;
  try { timestamp = session.audio.outputTimestamp(); }
  catch (error) {
    if (error.code === "unsupported" || error.code === "unavailable") return null;
    throw error;
  }
  const pair = presentationPair(timestamp, session.startFrame, session.audio.sampleRate, performance.now());
  const previous = session.lastPresentation;
  if (pair === null || (previous !== null && (pair.outputNs < previous.outputNs
    || pair.hostNs < previous.hostNs || (pair.outputNs > previous.outputNs && pair.hostNs === previous.hostNs)))) return null;
  // A repeated output position must not refresh the clock observer's age.
  if (previous === null || pair.outputNs > previous.outputNs) session.lastPresentation = pair;
  return pair;
}
function finishPlay(session) {
  if (activePlay === session && session.phase === "playing" && session.completionReady
    && session.events.length === 0 && session.tickPending === null && session.renderPending === null
    && session.batch === null && !session.audioBusy) {
    void stopPlay(session.mode === "replay" ? "Recorded replay ended." : "Song completed.", false, true);
  }
}
function pumpInput(session) {
  if (activePlay !== session || session.mode !== "live" || session.phase !== "playing" || session.tickPending !== null) return;
  try {
    const events = session.events.splice(0, 256);
    let watermark = null;
    if (!session.events.length) {
      watermark = millisecondsToNanos(Math.max(0, performance.now() - 12));
      if (events.length && watermark < events.at(-1).hostNs) watermark = events.at(-1).hostNs;
      if (watermark < session.lastHost) watermark = session.lastHost;
    }
    const tickId = ++session.tickId;
    if (!Number.isSafeInteger(tickId)) throw new Error("Gameplay step identity exhausted.");
    const timer = setTimeout(() => { if (session.tickPending?.tickId === tickId) void stopPlay("Gameplay Worker stopped responding.", true); }, 10000);
    session.tickPending = { tickId, timer, watermark, lastInput: events.at(-1)?.hostNs ?? session.lastHost };
    worker.postMessage({ kind: "play-step", playId: session.id, tickId, events, watermark, audioNs: audioSchedule(session) });
  } catch (error) { void stopPlay(`Playback failed: ${error.message}`, true); }
}

async function pumpAudio(session) {
  if (activePlay !== session || session.phase !== "playing" || session.audioBusy) return;
  if (session.renderPending && !session.batch) return;
  session.audioBusy = true;
  let batch = null;
  try {
    if (session.batch) {
      batch = session.batch;
      session.batch = null;
      const ack = await session.audio.commands(batch.commands);
      if (session.phase !== "playing") return;
      worker.postMessage({ kind: "play-ack", playId: session.id, sequence: batch.sequence, admitted: ack.admitted, success: true });
    } else {
      const report = await session.audio.poll();
      if (session.phase === "playing") {
        const renderId = ++session.renderId;
        if (!Number.isSafeInteger(renderId)) throw new Error("Audio report identity exhausted.");
        const timer = setTimeout(() => { if (session.renderPending?.renderId === renderId) void stopPlay("Audio report Worker stopped responding.", true); }, 10000);
        session.renderPending = { renderId, timer };
        const presentation = presentedPoint(session);
        worker.postMessage({ kind: "play-render", playId: session.id, renderId, report,
          presentedNs: presentation?.outputNs ?? null, presentedHostNs: presentation?.hostNs ?? null });
      }
    }
  } catch (error) {
    if (session.phase === "playing") {
      let reason = `Playback failed: ${String(error.message).slice(0, 4096)}`;
      try {
        if (batch && Number.isInteger(error.admitted)) worker.postMessage({ kind: "play-ack", playId: session.id,
          sequence: batch.sequence, admitted: error.admitted, success: false });
      } catch { reason += " The rejected audio prefix could not reach the gameplay Worker."; }
      void stopPlay(reason, true);
    }
  } finally {
    session.audioBusy = false;
    if (session.batch && session.phase === "playing") void pumpAudio(session);
    finishPlay(session);
  }
}

function receivePlay(data) {
  const session = activePlay;
  if (!session || data.playId !== session.id) return;
  if (data.kind === "play-reply") {
    const request = session.rpc;
    if (!request || data.rpcId !== request.rpcId) return;
    session.rpc = null;
    clearTimeout(request.timer);
    if (typeof data.error === "string") request.reject(new Error(data.error));
    else request.resolve(data.result);
  } else if (data.kind === "play-stopped") {
    session.finalScore = data;
    replayReceipt(session, data);
    releasePlayWorker(session);
    if (session.phase !== "closing") void stopPlay("Gameplay stopped without the Window cleanup request.", true);
  } else if (data.kind === "play-error") {
    session.finalScore = data;
    replayReceipt(session, data);
    if (data.released === false) {
      session.cleanupError = String(data.message).slice(0, 4096);
      stop();
    } else releasePlayWorker(session);
    void stopPlay(`Playback failed: ${data.message} · Hits ${data.hits}, misses ${data.misses}`, true);
  } else if (data.kind === "play-commands" && session.phase === "playing") {
    if (session.batch) { void stopPlay("More than one outgoing audio batch was published.", true); return; }
    session.batch = data.batch;
    session.completionReady = false;
    void pumpAudio(session);
  } else if (data.kind === "play-render-done" && session.phase === "playing") {
    if (!session.renderPending || data.renderId !== session.renderPending.renderId) { void stopPlay("Audio report response was not correlated.", true); return; }
    if (typeof data.completed !== "boolean") { void stopPlay("Song completion evidence was malformed.", true); return; }
    clearTimeout(session.renderPending.timer);
    session.renderPending = null;
    session.completionReady = data.completed;
    if (session.mode === "replay") {
      if (typeof data.songNs === "bigint") ui.position.value = seconds(data.songNs.toString());
      if (performance.now() - session.lastStatus >= 100) {
        session.lastStatus = performance.now();
        status(`Replay · Hits ${data.hits ?? "unavailable"} · Misses ${data.misses ?? "unavailable"} · Combo ${data.combo ?? "unavailable"}`);
      }
    }
    finishPlay(session);
  } else if (data.kind === "play-step-done" && session.phase === "playing") {
    const pending = session.tickPending;
    if (!pending || data.tickId !== pending.tickId) { void stopPlay("Gameplay step response was not correlated.", true); return; }
    clearTimeout(pending.timer);
    session.tickPending = null;
    session.lastHost = pending.watermark ?? pending.lastInput;
    ui.position.value = seconds(data.songNs.toString());
    if (performance.now() - session.lastStatus >= 100) {
      session.lastStatus = performance.now();
      status(`Playing · Hits ${data.hits} · Misses ${data.misses} · Combo ${data.combo}`);
    }
    if (session.events.length) pumpInput(session);
    finishPlay(session);
  }
}

function releasePlayWorker(session) {
  session.workerReleased = true;
  if (session.workerStop) {
    clearTimeout(session.workerStop.timer);
    session.workerStop.resolve();
    session.workerStop = null;
  }
}

function stopPlay(reason, failed = false, completed = false) {
  const session = activePlay;
  if (!session) return Promise.resolve();
  if (session.stopping) return session.stopping;
  session.naturalFinishRequested = completed;
  session.phase = "closing";
  session.controller.abort();
  clearInterval(session.timer);
  if (session.tickPending) clearTimeout(session.tickPending.timer);
  session.tickPending = null;
  if (session.renderPending) clearTimeout(session.renderPending.timer);
  session.renderPending = null;
  if (session.rpc) {
    clearTimeout(session.rpc.timer);
    session.rpc.reject(new Error("Playback operation cancelled."));
    session.rpc = null;
  }
  session.events.length = 0;
  session.pressed.clear();
  session.batch = null;
  let workerStopped = Promise.resolve();
  if (session.workerStarted && !session.workerReleased && worker) {
    workerStopped = new Promise(resolve => {
      const timer = setTimeout(() => {
        // Termination establishes ownership release if the stop receipt never arrives.
        stop();
        failed = true;
        reason = "Gameplay cleanup timed out. Reload the page before playing again.";
      }, 10000);
      session.workerStop = { timer, resolve };
    });
    try { worker.postMessage({ kind: "play-stop", playId: session.id, completed }); }
    catch { stop(); failed = true; reason = "Gameplay Worker could not stop. Reload the page."; }
  }
  controls();
  status(reason, failed);
  session.stopping = (async () => {
    try {
      const audio = session.audio ?? await session.opening?.catch(error => {
        if (error?.cleanupError) throw error.cleanupError;
        return null;
      });
      await audio?.stop();
    }
    catch (error) {
      stop();
      failed = true;
      reason += ` Audio cleanup failed: ${String(error.message).slice(0, 4096)} Reload the page before playing again.`;
    }
    finally {
      await workerStopped;
      if (session.cleanupError !== null) {
        failed = true;
        reason = `Gameplay cleanup failed: ${session.cleanupError} Reload the page before playing again.`;
      }
      if (activePlay === session) {
        if (session.owner === owner) {
          ui.title.textContent = session.preview.title;
          ui.details.textContent = session.preview.details;
          ui.position.value = session.preview.position;
          ui.keys.textContent = "";
        }
        const score = session.finalScore;
        const result = score && typeof score.hits === "bigint" && typeof score.misses === "bigint"
          ? ` Hits ${score.hits} · Misses ${score.misses} · Combo ${score.combo ?? "unavailable"}.` : "";
        if (session.replayError !== null) {
          failed = true;
          reason += ` Replay export failed: ${session.replayError}`;
        }
        if (session.replay !== null) {
          revokeReplayURL();
          lastReplay = { bytes: session.replay.bytes, complete: session.replay.complete && !failed, id: session.id,
            chartPath: session.chartPath, hits: score?.hits ?? null, misses: score?.misses ?? null, combo: score?.combo ?? null };
          ui.export.textContent = `Download last replay (${lastReplay.complete ? "complete" : "prefix"})`;
        }
        activePlay = null;
        controls();
        if (session.owner === owner || failed) status(reason + result, failed);
      }
    }
  })();
  return session.stopping;
}

function replayReceipt(session, data) {
  try {
    // A non-recording older peer may omit export fields, but cannot publish bytes.
    if (!session.recordReplay && data.replay === undefined && data.replayComplete === undefined
      && data.replayError === undefined) return;
    if (typeof data.replayComplete !== "boolean" || !(data.replayError === null
      || (typeof data.replayError === "string" && data.replayError.length <= 4096))) throw new Error("Invalid replay export metadata.");
    if (data.replay === null) {
      if (data.replayComplete) throw new Error("Complete replay has no encoded data.");
    } else {
      const bytes = data.replay;
      if (!session.recordReplay || !(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
        || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
        || bytes.length === 0 || bytes.length > 64 * 1024 * 1024
        || (data.replayComplete && (data.kind !== "play-stopped" || !session.naturalFinishRequested))) {
        throw new Error("Invalid replay export ownership or layout.");
      }
      session.replay = { bytes, complete: data.replayComplete };
    }
    if (data.replayError !== null) session.replayError = data.replayError;
  } catch (error) {
    session.replay = null;
    session.replayError = String(error.message).slice(0, 4096);
  }
}
function revokeReplayURL() {
  clearTimeout(replayURLTimer);
  replayURLTimer = null;
  if (replayURL !== null) URL.revokeObjectURL(replayURL);
  replayURL = null;
}
function downloadReplay() {
  if (activePlay !== null || recordsOperation !== null || lastReplay === null) return;
  let link = null;
  try {
    revokeReplayURL();
    replayURL = URL.createObjectURL(new Blob([lastReplay.bytes], { type: "application/octet-stream" }));
    link = document.createElement("a");
    link.href = replayURL;
    link.download = `beatkernel-${lastReplay.id}-${lastReplay.complete ? "complete" : "prefix"}.bkr`;
    document.body.appendChild(link);
    link.click();
    replayURLTimer = setTimeout(revokeReplayURL, 60000);
  } catch (error) {
    revokeReplayURL();
    status(`Replay download failed: ${String(error.message).slice(0, 4096)}`, true);
  } finally { link?.remove(); }
}

function closeRecords() {
  recordsOperation?.controller.abort();
  recordsOperation = null;
  const previous = recordsStore;
  recordsStore = null;
  previous?.close();
}
function recordCurrent(operation) {
  return recordsOperation === operation && operation.owner === owner && !operation.controller.signal.aborted;
}
async function openRecords(operation) {
  if (recordsStore && !recordsStore.closed) return recordsStore;
  const opened = await RecordsStore.open({ signal: operation.controller.signal });
  if (!recordCurrent(operation)) {
    opened.close();
    throw new Error("Record library operation was cancelled.");
  }
  recordsStore = opened;
  return opened;
}
function showRecords(entries) {
  const previous = ui.records.value;
  const options = document.createDocumentFragment();
  for (const record of entries) {
    options.append(new Option(`${record.name} · ${record.complete ? "complete capture" : "prefix"} · Hits ${record.hits ?? "unavailable"} · Misses ${record.misses ?? "unavailable"}`, String(record.id)));
  }
  ui.records.replaceChildren(options);
  if (!entries.length) ui.records.append(new Option("No saved records", ""));
  else if (entries.some(record => String(record.id) === previous)) ui.records.value = previous;
}
async function recordAction(action) {
  if (!initialized || importing || preparing || activePlay || recordsOperation) return;
  const captured = lastReplay;
  if (action === "save" && captured === null) return;
  const id = Number(ui.records.value);
  if ((action === "use" || action === "delete") && (!Number.isSafeInteger(id) || id < 1)) return;
  const operation = { owner, controller: new AbortController() };
  recordsOperation = operation;
  controls();
  status(action === "save" ? "Saving the captured recording…" : "Opening saved records…");
  let committed = "";
  try {
    const store = await openRecords(operation);
    if (!recordCurrent(operation)) return;
    if (action === "use") {
      const loaded = await store.load(id);
      if (!recordCurrent(operation)) return;
      const file = new File([loaded.bytes], loaded.metadata.name, { type: "application/octet-stream" });
      selectedReplay = file;
      ui["replay-file"].value = "";
      ui["replay-name"].textContent = `${file.name} · ${file.size} bytes · matching chart: ${loaded.metadata.chartPath}`;
      status("Saved replay selected. Prepare its matching chart, then choose Play replay.");
    } else {
      if (action === "save") {
        await store.save({ bytes: captured.bytes, name: `beatkernel-${captured.id}-${captured.complete ? "complete" : "prefix"}.bkr`,
          chartPath: captured.chartPath, complete: captured.complete,
          hits: captured.hits, misses: captured.misses, combo: captured.combo });
        if (!recordCurrent(operation)) return;
        committed = "Recording saved. ";
      } else if (action === "delete") {
        const removed = await store.remove(id);
        if (!recordCurrent(operation)) return;
        committed = removed ? "Selected record deleted. " : "Selected record was already absent. ";
      }
      const entries = await store.list();
      if (!recordCurrent(operation)) return;
      showRecords(entries);
      status(`${committed}${entries.length} saved record(s).`);
    }
  } catch (error) {
    if (recordCurrent(operation)) status(`${committed}Record library failed: ${String(error.message).slice(0, 4096)} Current replay and download remain available.`, true);
  } finally {
    if (recordsOperation === operation) {
      recordsOperation = null;
      controls();
    }
  }
}
start();

import { snapshotFiles, nanoseconds, seconds } from "./host_model.mjs";
import { AudioHost } from "./audio-host.mjs";
import { KEY_BINDINGS, bindingsFor, millisecondsToNanos, frameNanos, startProjection } from "./play-model.mjs";

const byId = id => document.getElementById(id);
const ui = Object.fromEntries(["folder", "files", "chart", "rate", "seed", "prepare", "position", "seek", "title", "details", "status", "viewport", "play", "stop", "keys"].map(id => [id, byId(id)]));
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

function status(text, error = false) {
  ui.status.textContent = text;
  ui.status.dataset.error = String(error);
}
function controls() {
  const playing = activePlay !== null;
  ui.folder.disabled = !initialized || preparing || playing || !("webkitdirectory" in ui.folder);
  ui.files.disabled = !initialized || preparing || playing;
  for (const field of [ui.chart, ui.rate, ui.seed, ui.prepare]) field.disabled = !initialized || !libraryId || importing || preparing || playing;
  ui.position.disabled = ui.seek.disabled = !initialized || !hasPreview || importing || preparing || playing;
  ui.play.disabled = !initialized || !hasPreview || importing || preparing || playing || !audioModule;
  ui.stop.disabled = !playing || activePlay.phase === "closing";
}
function stop() {
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
  if (!worker || !libraryId || importing || preparing || activePlay) return;
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
  if (!initialized || preparing || !worker || activePlay) return;
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
  if (!worker || !hasPreview || preparing || importing || activePlay) return;
  try {
    const ns = nanoseconds(ui.position.value);
    seekId = ++serial;
    worker.postMessage({ kind: "seek", id: seekId, selectedId, ns });
  } catch (error) { status(error.message, true); }
});
window.addEventListener("pagehide", stop);
window.addEventListener("pageshow", event => { if (event.persisted) start(); });
window.addEventListener("resize", resize);
ui.play.addEventListener("click", play);
ui.stop.addEventListener("click", () => { void stopPlay("Playback stopped."); });
window.addEventListener("blur", () => { void stopPlay("Playback stopped after losing focus."); });
document.addEventListener("visibilitychange", () => { if (document.hidden) void stopPlay("Playback stopped while the page is hidden."); });
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

async function play() {
  if (!initialized || !hasPreview || !audioModule || importing || preparing || activePlay) return;
  const session = { id: ++serial, owner, phase: "preparing", controller: new AbortController(), audio: null, opening: null,
    rpc: null, timer: null, events: [], pressed: new Set(), bindings: [], sequence: 0n,
    tickId: 0, tickPending: null, audioBusy: false, batch: null, startFrame: null,
    origin: null, lastHost: 0n, lastStatus: 0, stopping: null, renderId: 0, renderPending: null,
    workerStarted: false, workerReleased: false, workerStop: null, finalScore: null,
    preview: { title: ui.title.textContent, details: ui.details.textContent, position: ui.position.value } };
  activePlay = session;
  controls();
  status("Preparing playable chart and audio…");
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
    const prepared = await playRpc(session, "play-start", { libraryId, path: ui.chart.value,
      rate: session.audio.sampleRate, seed: ui.seed.value, keyPairs: Uint32Array.from(KEY_BINDINGS.flatMap(row => [row[0], row[2]])) });
    ui.title.textContent = prepared.title || ui.chart.value;
    ui.details.textContent = `${prepared.artist || "Unknown artist"} · ${prepared.notes} notes · ${prepared.samples} sounds · ${session.audio.sampleRate} Hz output`;
    session.bindings = bindingsFor(prepared.lanes);
    ui.keys.textContent = session.bindings.map(row => `${row[0].toString(16).toUpperCase()}: ${row[1]}`).join(" · ");
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
    status("Playing. Stop ends this session; leaving the page stops playback.");
    session.timer = setInterval(() => { pumpInput(session); void pumpAudio(session); }, 8);
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
    pumpInput(session);
  } catch (error) { void stopPlay(`Playback failed: ${error.message}`, true); }
}

function audioSchedule(session) {
  const frame = session.audio.currentFrame + BigInt(Math.ceil(session.audio.sampleRate / 50));
  return frameNanos(frame > session.startFrame ? frame - session.startFrame : 0n, session.audio.sampleRate);
}
function pumpInput(session) {
  if (activePlay !== session || session.phase !== "playing" || session.tickPending !== null) return;
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
        worker.postMessage({ kind: "play-render", playId: session.id, renderId, report });
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
    releasePlayWorker(session);
  } else if (data.kind === "play-error") {
    session.finalScore = data;
    releasePlayWorker(session);
    void stopPlay(`Playback failed: ${data.message} · Hits ${data.hits}, misses ${data.misses}`, true);
  } else if (data.kind === "play-commands" && session.phase === "playing") {
    if (session.batch) { void stopPlay("More than one outgoing audio batch was published.", true); return; }
    session.batch = data.batch;
    void pumpAudio(session);
  } else if (data.kind === "play-render-done" && session.phase === "playing") {
    if (!session.renderPending || data.renderId !== session.renderPending.renderId) { void stopPlay("Audio report response was not correlated.", true); return; }
    clearTimeout(session.renderPending.timer);
    session.renderPending = null;
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

function stopPlay(reason, failed = false) {
  const session = activePlay;
  if (!session) return Promise.resolve();
  if (session.stopping) return session.stopping;
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
    try { worker.postMessage({ kind: "play-stop", playId: session.id }); }
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
        activePlay = null;
        controls();
        if (session.owner === owner || failed) status(reason + result, failed);
      }
    }
  })();
  return session.stopping;
}
start();

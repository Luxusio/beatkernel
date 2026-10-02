import init, { BrowserLibrary, BrowserView } from "./pkg/beatkernel_bms_runtime.js";
import { LIMITS, preflight, previewNanos } from "./host_model.mjs";
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

function report(kind, fields = {}) { self.postMessage({ kind, ...fields }); }
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
  stopRedraw();
  report("fatal", { message: message(error) });
  // Main terminates this owner. Do not try to reuse potentially failed GPU state.
}

function scheduleDraw(reset = true) {
  if (failed || !view || !selectedId || extent.includes(0)) return;
  if (reset) retries = 0;
  if (redraw !== null) return;
  const draw = () => {
    redraw = null;
    try {
      view.draw();
      if (view.needs_redraw()) {
        if (++retries <= 3) scheduleDraw(false);
        else report("render-wait", { selectedId });
      } else report("drawn", { selectedId });
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

self.addEventListener("message", event => {
  if (failed) return;
  const request = event.data;
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
  if (request.kind === "import") queueImport(request);
  else if (request.kind === "accept-library") acceptLibrary(request);
  else if (request.kind === "select") void selectChart(request).catch(fatal);
  else if (request.kind === "seek") void seek(request).catch(fatal);
  else if (request.kind === "resize") void resize(request).catch(fatal);
});

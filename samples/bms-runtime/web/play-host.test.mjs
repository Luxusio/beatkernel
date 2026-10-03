// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/play-host.test.mjs
// Actual main.js and numeric helpers; controlled DOM/Worker/AudioHost endpoints.
// No browser, audio device, generated binding or WASM instance is used.
import assert from "node:assert/strict";
import { Blob, File } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function flush() {
  for (let index = 0; index < 48; index++) await Promise.resolve();
}
function command(voice = 1n) {
  return { kind: 0, voice, sample: 1n, at: 100000000n, gain: 0.5, value: 0n, denominator: 0n };
}
function finalScore(playId, overrides = {}) {
  return { kind: "play-stopped", playId, songNs: 2350000000n,
    hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0, ...overrides };
}
function selectedRecording(size = 6) {
  const file = new File([Uint8Array.from([66, 75, 82, 0, 255, 1])], "recorded-prefix.bkr");
  Object.defineProperty(file, "size", { value: size });
  let reads = 0;
  file.arrayBuffer = () => { reads++; throw new Error("Window must not acquire replay bytes"); };
  return { file, get reads() { return reads; } };
}
function chooseRecording(h, files) {
  h.get("replay-file").files = files;
  h.get("replay-file").emit("change");
}
function savedRecord(fields = {}) {
  return { id: 41, name: "saved-prefix.bkr", chartPath: "Songs/曲/chart.bms", complete: false,
    hits: 18446744073709551615n, misses: 2n, combo: null, createdAt: 1234567890, byteLength: 4, ...fields };
}

async function harness(faults = {}) {
  const elements = new Map();
  const workers = [];
  const traces = [];
  const unexpected = [];
  const timers = new Map();
  const opens = [];
  const urls = [];
  const revoked = [];
  const downloads = [];
  const recordOpens = [];
  const recordCalls = [];
  const recordOwners = [];
  let now = 1000;
  let nextTimer = 0;
  let gesture = false;

  class Events {
    constructor() { this.listeners = new Map(); }
    addEventListener(type, handler) {
      if (!this.listeners.has(type)) this.listeners.set(type, new Set());
      this.listeners.get(type).add(handler);
    }
    removeEventListener(type, handler) { this.listeners.get(type)?.delete(handler); }
    emit(type, fields = {}) {
      const event = { type, target: this, defaultPrevented: false,
        preventDefault() { this.defaultPrevented = true; }, ...fields };
      for (const handler of [...(this.listeners.get(type) ?? [])]) {
        const result = handler.call(this, event);
        if (result?.then) result.catch(error => unexpected.push(error));
      }
      return event;
    }
  }
  class Element extends Events {
    constructor(tag, id = "") {
      super();
      this.tagName = tag;
      this.id = id;
      this.value = "";
      this.textContent = "";
      this.dataset = {};
      this.disabled = false;
      this.checked = false;
      this.hidden = false;
      this.children = [];
      this.files = [];
      this.focuses = 0;
    }
    append(...children) {
      for (const child of children) {
        this.children.push(...(child.tagName === "#fragment" ? child.children : [child]));
      }
      if (this.tagName === "select" && !this.children.some(child => child.value === this.value)) {
        this.value = this.children[0]?.value ?? "";
      }
    }
    appendChild(child) { this.append(child); child.parent = this; return child; }
    click() {
      if (this.tagName === "a") downloads.push({ href: this.href, filename: this.download, link: this });
      else this.emit("click");
    }
    remove() {
      if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this);
      this.parent = null;
      this.removed = true;
    }
    replaceChildren(...children) { this.children = []; this.value = ""; this.append(...children); }
    replaceWith(fresh) { elements.set(this.id, fresh); }
    setAttribute(name, value) { this[name] = value; }
    getBoundingClientRect() { return { width: 960, height: 720 }; }
    transferControlToOffscreen() { return { surface: this.id }; }
    focus() { this.focuses++; }
  }
  class Option extends Element {
    constructor(text, value) { super("option"); this.textContent = text; this.value = value; }
  }
  for (const id of ["folder", "files", "chart", "rate", "seed", "prepare", "position", "seek",
    "title", "details", "status", "viewport", "play", "stop", "record", "export", "keys", "canvas", "prepare-form", "seek-form",
    "replay-file", "replay-play", "replay-name", "records", "records-refresh", "records-save", "records-use", "records-delete",
    "multiplayer", "multiplayer-url", "multiplayer-role", "multiplayer-status",
    "opponents-kind", "opponents-label", "opponents-add", "records-opponent", "opponents-clear",
    "opponents-list", "opponents-status", "opponents-results", "judge-early", "judge-late", "judge-offset", "live-start"]) {
    elements.set(id, new Element(id === "chart" || id === "records" ? "select" : id, id));
  }
  elements.get("folder").webkitdirectory = true;
  elements.get("rate").value = "44100";
  elements.get("seed").value = "7";
  elements.get("multiplayer-role").value = "join";
  elements.get("opponents-kind").value = "own";
  elements.get("judge-early").value = "50";
  elements.get("judge-late").value = "50";
  elements.get("judge-offset").value = "0";
  elements.get("live-start").value = "0";

  const document = new Events();
  document.body = new Element("body");
  document.hidden = false;
  document.getElementById = id => elements.get(id);
  document.createElement = tag => new Element(tag);
  document.createDocumentFragment = () => new Element("#fragment");

  class Worker extends Events {
    constructor(url, options) {
      super();
      this.url = String(url);
      this.options = options;
      this.posts = [];
      this.terminations = 0;
      this.failKind = null;
      workers.push(this);
    }
    postMessage(value, transfer = []) {
      traces.push(["post", value.kind]);
      if (value.kind === this.failKind) throw new Error("injected Worker post failure");
      assert.equal(this.terminations, 0, "posting after Worker termination");
      // The fake canvas has no native transferable. Other data follows the real
      // structured-clone shape, including BigInt and typed event arrays.
      const posted = structuredClone(value);
      // Node versions may clone File as Blob. Preserve the selected immutable
      // File endpoint here; this fake Worker never acquires its bytes.
      if (value.replayFile instanceof File) posted.replayFile = value.replayFile;
      if (Array.isArray(value.opponents)) posted.opponents = value.opponents.map((entry, index) => ({
        ...posted.opponents[index], file: entry.file,
      }));
      this.posts.push({ value: posted, transferCount: transfer.length });
    }
    terminate() { this.terminations++; traces.push(["terminate"]); }
    messages(kind) { return this.posts.map(entry => entry.value).filter(value => value.kind === kind); }
    last(kind) { return this.messages(kind).at(-1); }
  }
  class ResizeObserver {
    constructor(callback) { this.callback = callback; }
    observe() {}
    disconnect() { traces.push(["observer-disconnect"]); }
  }
  const window = new Events();
  Object.assign(window, { isSecureContext: true, devicePixelRatio: 1, Worker,
    OffscreenCanvas: class {}, ResizeObserver, matchMedia: () => new Events() });

  function createAudio() { return {
    sampleRate: 48000,
    samples: [], commandsSeen: [], arms: [], polls: 0, finishes: 0, outputReads: 0,
    stopCalls: 0, stopStarts: 0, stopping: null,
    get currentFrame() { return BigInt(Math.floor(now * 48)); },
    controlClock() { return faults.controlClock ?? { beforeMs: now, afterMs: now, contextTime: now / 1000, sampleRate: 48000 }; },
    outputTimestamp() {
      this.outputReads++;
      traces.push(["output-timestamp"]);
      if (faults.outputFailure) throw faults.outputFailure;
      if (faults.outputEvidence) return { ...faults.outputEvidence };
      throw Object.assign(new Error("output evidence is not available"), { code: "unavailable" });
    },
    async sample(value) {
      this.samples.push(value);
      traces.push(["sample", value.id]);
      return { admitted: 0 };
    },
    async finish() { this.finishes++; traces.push(["finish"]); },
    async arm(frame) { this.arms.push(frame); traces.push(["arm", frame]); },
    async commands(commands) {
      this.commandsSeen.push(structuredClone(commands));
      traces.push(["commands", commands.length]);
      if (faults.commandFailure) throw faults.commandFailure;
      if (faults.commandGate) await faults.commandGate.promise;
      return { admitted: commands.length };
    },
    async poll() {
      this.polls++;
      traces.push(["poll"]);
      if (faults.pollGate) await faults.pollGate.promise;
      if (faults.renderReport) return structuredClone(faults.renderReport);
      const words = new Uint32Array(56);
      words[50] = 1;
      const start = this.arms.at(-1) ?? 0n;
      words[52] = Number(start & 0xffffffffn);
      words[53] = Number(start >> 32n);
      return { available: false, words };
    },
    stop() {
      this.stopCalls++;
      if (!this.stopping) {
        this.stopStarts++;
        traces.push(["audio-stop"]);
        this.stopping = faults.stopFailure ? Promise.reject(faults.stopFailure)
          : faults.stopGate?.promise ?? Promise.resolve();
      }
      return this.stopping;
    },
  }; }
  let audio = createAudio();
  let audioOpened = false;
  class AudioHost {
    static open(options) {
      if (audioOpened) audio = createAudio();
      audioOpened = true;
      opens.push({ options, gesture });
      traces.push(["open", gesture]);
      return faults.openGate?.promise ?? Promise.resolve(audio);
    }
  }
  const moduleToken = {};
  class ControlledURL extends URL {
    static createObjectURL(blob) {
      if (faults.downloadError) throw new Error(faults.downloadError);
      assert.ok(blob instanceof Blob);
      const url = `blob:deferred-fixture/${urls.length + 1}`;
      urls.push({ url, blob });
      return url;
    }
    static revokeObjectURL(url) { revoked.push(url); }
  }
  const context = createContext({
    document, window, Worker, ResizeObserver, Option,
    AbortController, AbortSignal, URL: ControlledURL, Blob, TextEncoder, File, Uint8Array, Uint32Array, Float32Array,
    ArrayBuffer, structuredClone, performance: { timeOrigin: 9000, now: () => now },
    WebAssembly: { compile: async binary => {
      assert.deepEqual(Array.from(binary), [0, 97, 115, 109, 1, 0, 0, 0]);
      return moduleToken;
    } },
    fetch: async () => {
      let read = false;
      return { ok: true, body: { getReader: () => ({
        async read() {
          if (read) return { done: true };
          read = true;
          return { done: false, value: Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0]) };
        },
        async cancel() {}, releaseLock() {},
      }) } };
    },
    setTimeout(callback, delay) {
      const id = ++nextTimer;
      timers.set(id, { callback, at: now + delay, interval: null });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
    setInterval(callback, delay) {
      const id = ++nextTimer;
      timers.set(id, { callback, at: now + delay, interval: delay });
      return id;
    },
    clearInterval(id) { timers.delete(id); },
  });
  const audioModule = new SyntheticModule(["AudioHost"], function () {
    this.setExport("AudioHost", AudioHost);
  }, { context });
  class RecordsStore {
    constructor() { this.closed = false; this.closes = 0; recordOwners.push(this); }
    static async open(options) {
      recordOpens.push(options);
      const store = new RecordsStore();
      if (faults.recordsOpenGate) await faults.recordsOpenGate.promise;
      if (faults.recordsOpenError) throw faults.recordsOpenError;
      return store;
    }
    async list() {
      assert.equal(this.closed, false);
      recordCalls.push({ method: "list", store: this });
      if (faults.recordsListGate) await faults.recordsListGate.promise;
      if (faults.recordsListError) throw faults.recordsListError;
      return structuredClone(faults.recordsList ?? []);
    }
    async save(value) {
      assert.equal(this.closed, false);
      recordCalls.push({ method: "save", value: structuredClone(value), store: this });
      if (faults.recordsSaveGate) await faults.recordsSaveGate.promise;
      if (faults.recordsSaveError) throw faults.recordsSaveError;
      return savedRecord();
    }
    async load(id) {
      assert.equal(this.closed, false);
      recordCalls.push({ method: "load", id, store: this });
      if (faults.recordsLoadGate) await faults.recordsLoadGate.promise;
      if (faults.recordsLoadError) throw faults.recordsLoadError;
      return structuredClone(faults.recordsLoaded ?? { metadata: savedRecord(), bytes: Uint8Array.from([1, 2, 3, 4]) });
    }
    async remove(id) {
      assert.equal(this.closed, false);
      recordCalls.push({ method: "remove", id, store: this });
      if (faults.recordsRemoveGate) await faults.recordsRemoveGate.promise;
      if (faults.recordsRemoveError) throw faults.recordsRemoveError;
      return faults.recordsRemoved ?? true;
    }
    close() { if (!this.closed) { this.closed = true; this.closes++; } }
  }
  const recordsModule = new SyntheticModule(["RecordsStore"], function () {
    this.setExport("RecordsStore", RecordsStore);
  }, { context });
  const modules = new Map();
  for (const name of ["host_model.mjs", "play-model.mjs", "saved-opponents.mjs", "main.js"]) {
    const url = new URL(name, import.meta.url);
    modules.set(name, new SourceTextModule(await readFile(url, "utf8"), {
      context, identifier: url.href, initializeImportMeta(meta) { meta.url = url.href; },
    }));
  }
  const main = modules.get("main.js");
  await main.link(specifier => {
    if (specifier === "./audio-host.mjs") return audioModule;
    if (specifier === "./record-store.mjs") return recordsModule;
    const linked = modules.get(specifier.replace(/^\.\//, ""));
    assert.ok(linked, `unexpected import ${specifier}`);
    return linked;
  });
  await main.evaluate();
  const get = id => elements.get(id);
  function click(id) {
    const element = get(id);
    if (element.disabled) return;
    gesture = true;
    try { element.emit("click"); } finally { gesture = false; }
  }
  async function receive(value, target = workers.at(-1)) {
    target.emit("message", { data: structuredClone(value) });
    await flush();
  }
  async function reply(request, result) {
    assert.ok(request, "expected an actual setup request");
    await receive({ kind: "play-reply", playId: request.playId, rpcId: request.rpcId, result });
  }
  async function preview() {
    const worker = workers.at(-1);
    await receive({ kind: "ready" });
    get("files").files = [new File(["#BPM 120"], "chart.bms")];
    get("files").emit("change");
    const imported = worker.last("import");
    await receive({ kind: "catalog", id: imported.id, charts: ["chart.bms"] });
    get("prepare-form").emit("submit");
    const selected = worker.last("select");
    await receive({ kind: "selected", id: selected.id, libraryId: imported.id, path: "chart.bms",
      title: "Accepted preview", artist: "Preview artist", notes: 6, samples: 1, images: 0, duration: "6000000000" });
    await receive({ kind: "drawn", selectedId: selected.id });
    get("position").value = "12.345678901";
    get("seek-form").emit("submit");
    const seek = worker.last("seek");
    await receive({ kind: "position", id: seek.id, selectedId: selected.id, ns: "12345678901" });
    assert.equal(get("play").disabled, false);
    return { title: get("title").textContent, details: get("details").textContent, position: get("position").value };
  }
  async function begin(mode = "live") {
    click(mode === "replay" ? "replay-play" : "play");
    await flush();
    return workers.at(-1).last("play-start");
  }
  async function prepared(start, sampleCount = 0) {
    const worker = workers.at(-1);
    await reply(start, { kind: "prepared", title: "Actual runtime", artist: "Runtime artist",
      notes: 6, samples: sampleCount, lanes: [0x11], opponentCount: start.opponents?.length ?? 0,
      startNs: start.mode === "replay" ? faults.replayStart ?? 0n : start.startNs ?? 0n,
      ...(start.mode === "replay" ? { mode: "replay", recordedUntilNs: 2350000000n } : {}) });
    for (let index = 0; index < sampleCount; index++) {
      await reply(worker.last("play-sample"), { kind: "sample", id: BigInt(index + 1), rate: 44100,
        channels: 2, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) });
    }
    await reply(worker.last("play-sample"), { kind: "samples-end" });
    return worker.last("play-commands");
  }
  async function launch(sampleCount = 0, mode = "live") {
    const worker = workers.at(-1);
    const start = await begin(mode);
    const commands = await prepared(start, sampleCount);
    await reply(commands, null);
    const activation = worker.last("play-activate");
    await reply(activation, null);
    assert.equal(get("stop").disabled, false);
    return { id: start.playId, start, activation };
  }
  async function advance(milliseconds) {
    const until = now + milliseconds;
    let callbacks = 0;
    for (;;) {
      const next = [...timers.entries()].filter(([, timer]) => timer.at <= until)
        .sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
      if (!next) break;
      assert.ok(++callbacks < 5000, "fixture timer loop remained bounded");
      const [id, timer] = next;
      now = Math.max(now, timer.at);
      if (timer.interval === null) timers.delete(id);
      else timer.at = now + timer.interval;
      timer.callback();
      await flush();
    }
    now = until;
    await flush();
  }
  return { get, workers, get audio() { return audio; }, opens, traces, faults, timers, moduleToken, window, document, urls, revoked, downloads,
    recordOpens, recordCalls, recordOwners,
    click, receive, reply, preview, begin, prepared, launch, advance,
    setNow(value) { assert.ok(value >= now); now = value; },
    async close() {
      window.emit("pagehide");
      await flush();
      assert.deepEqual(unexpected, []);
      assert.equal(timers.size, 0, "page teardown must clear host deadlines and intervals");
    },
  };
}

test("the library opens only explicitly and saves joined capture bytes and actual score without autosave", async () => {
  const stopGate = deferred();
  const saveGate = deferred();
  const h = await harness({ stopGate, recordsSaveGate: saveGate, recordsList: [savedRecord()] });
  await h.preview();
  assert.equal(h.recordOpens.length, 0);
  assert.equal(h.recordCalls.length, 0);
  const selected = selectedRecording();
  chooseRecording(h, [selected.file]);
  const label = h.get("replay-name").textContent;
  h.get("record").checked = true;
  const session = await h.launch();
  assert.equal(h.get("records-save").disabled, true);
  h.click("stop");
  await flush();
  const bytes = Uint8Array.from([7, 8, 9, 255]);
  await h.receive(finalScore(session.id, { replay: bytes, replayComplete: false, replayError: null,
    hits: 18446744073709551615n, misses: 0n, combo: 7n }));
  assert.equal(h.get("records-save").disabled, true, "Worker receipt alone cannot release capture ownership");
  stopGate.resolve();
  await flush();
  assert.equal(h.recordOpens.length, 0);
  assert.equal(h.get("records-save").disabled, false);
  h.click("records-save");
  await flush();
  assert.equal(h.recordOpens.length, 1);
  const saved = h.recordCalls.find(call => call.method === "save").value;
  assert.deepEqual(saved.bytes, bytes);
  assert.equal(saved.name, `beatkernel-${session.id}-prefix.bkr`);
  assert.equal(saved.chartPath, "chart.bms");
  assert.equal(saved.complete, false);
  assert.equal(saved.hits, 18446744073709551615n);
  assert.equal(saved.misses, 0n);
  assert.equal(saved.combo, 7n);
  for (const id of ["play", "replay-play", "files", "seek", "export", "records-save", "records-refresh"]) {
    assert.equal(h.get(id).disabled, true, `${id} conflicts with the pending library operation`);
  }
  h.click("records-save");
  h.click("play");
  assert.equal(h.recordCalls.filter(call => call.method === "save").length, 1);
  assert.equal(h.opens.length, 1);
  saveGate.resolve();
  await flush();
  assert.match(h.get("status").textContent, /Recording saved/);
  assert.equal(h.recordCalls.filter(call => call.method === "list").length, 1);
  assert.equal(h.get("export").disabled, false);
  h.faults.recordsSaveError = Object.assign(new Error("actual IndexedDB quota exhausted"), { code: "quota" });
  h.click("records-save");
  await flush();
  assert.match(h.get("status").textContent, /actual IndexedDB quota exhausted/);
  assert.equal(h.get("replay-name").textContent, label);
  assert.equal(h.get("export").disabled, false);
  assert.equal(h.get("records-save").disabled, false);
  delete h.faults.recordsSaveError;
  h.faults.recordsListError = new Error("metadata refresh failed after commit");
  h.click("records-save");
  await flush();
  assert.match(h.get("status").textContent, /Recording saved\..*metadata refresh failed after commit/);
  assert.equal(h.recordOpens.length, 1, "ordinary quota/list failure does not silently open a replacement connection");
  h.click("export");
  assert.deepEqual(new Uint8Array(await h.urls[0].blob.arrayBuffer()), bytes);
  assert.equal(selected.reads, 0);
  await h.close();
  assert.equal(h.recordOwners[0].closes, 1);
});

test("metadata listing does not load bytes and explicit use preserves the original replay gesture and delete scope", async () => {
  const listGate = deferred();
  const loadGate = deferred();
  const row = savedRecord();
  const bytes = Uint8Array.from([66, 75, 82, 255]);
  const h = await harness({ recordsListGate: listGate, recordsLoadGate: loadGate,
    recordsList: [row], recordsLoaded: { metadata: row, bytes } });
  await h.preview();
  h.click("records-refresh");
  await flush();
  assert.equal(h.recordCalls.filter(call => call.method === "list").length, 1);
  assert.equal(h.recordCalls.filter(call => call.method === "load").length, 0);
  assert.equal(h.get("play").disabled, true);
  listGate.resolve();
  await flush();
  assert.equal(h.get("records").value, "41");
  assert.match(h.get("records").children[0].textContent, /prefix.*18446744073709551615/);
  assert.equal(h.opens.length, 0);
  h.click("records-use");
  await flush();
  assert.equal(h.recordCalls.find(call => call.method === "load").id, 41);
  assert.equal(h.get("replay-play").disabled, true);
  loadGate.resolve();
  await flush();
  assert.match(h.get("replay-name").textContent, /saved-prefix\.bkr.*matching chart: Songs\/曲\/chart\.bms/);
  assert.equal(h.get("chart").value, "chart.bms", "a metadata hint does not pretend the loaded assets match");
  assert.equal(h.opens.length, 0, "selecting saved bytes cannot resume audio automatically");
  const replay = await h.launch(0, "replay");
  assert.equal(h.opens[0].gesture, true);
  assert.equal(replay.start.replayFile.name, row.name);
  assert.equal(replay.start.replayFile.size, bytes.length);
  assert.deepEqual(new Uint8Array(await replay.start.replayFile.arrayBuffer()), bytes);
  h.click("stop");
  await flush();
  await h.receive(finalScore(replay.id));
  const label = h.get("replay-name").textContent;
  h.faults.recordsList = [];
  h.click("records-delete");
  await flush();
  assert.equal(h.recordCalls.find(call => call.method === "remove").id, 41);
  assert.equal(h.get("records").value, "");
  assert.match(h.get("status").textContent, /Selected record deleted/);
  assert.equal(h.get("replay-name").textContent, label);
  assert.equal(h.get("replay-play").disabled, false, "deleting storage does not revoke an already selected File");
  assert.equal(h.recordCalls.filter(call => call.method === "save").length, 0);
  assert.equal(h.opens.length, 1);
  await h.close();
});

test("hidden-page and reopened-page cancellation cannot publish stale library results or retain late connections", async () => {
  for (const transition of ["hidden", "pagehide"]) {
    const loadGate = deferred();
    const h = await harness({ recordsList: [savedRecord()], recordsLoadGate: loadGate });
    await h.preview();
    const selected = selectedRecording();
    chooseRecording(h, [selected.file]);
    const previous = h.get("replay-name").textContent;
    h.click("records-refresh");
    await flush();
    h.click("records-use");
    await flush();
    const old = h.recordOwners[0];
    if (transition === "hidden") { h.document.hidden = true; h.document.emit("visibilitychange"); }
    else h.window.emit("pagehide");
    await flush();
    assert.equal(old.closed, true);
    assert.equal(old.closes, 1);
    if (transition === "pagehide") {
      h.window.emit("pageshow", { persisted: true });
      await h.preview();
      chooseRecording(h, [selected.file]);
    } else { h.document.hidden = false; h.document.emit("visibilitychange"); }
    const currentLabel = h.get("replay-name").textContent;
    if (transition === "hidden") assert.equal(currentLabel, previous);
    loadGate.resolve();
    await flush();
    assert.equal(h.get("replay-name").textContent, currentLabel);
    assert.equal(h.opens.length, 0);
    delete h.faults.recordsLoadGate;
    h.click("records-refresh");
    await flush();
    assert.equal(h.recordOpens.length, 2, "only a new explicit action replaces the closed owner");
    assert.notEqual(h.recordOwners[1], old);
    await h.close();
  }
  const gate = deferred();
  const h = await harness({ recordsOpenGate: gate });
  await h.preview();
  h.click("records-refresh");
  await flush();
  const signal = h.recordOpens[0].signal;
  h.window.emit("pagehide");
  await flush();
  assert.equal(signal.aborted, true);
  gate.resolve();
  await flush();
  assert.equal(h.recordOwners[0].closed, true, "late open must be released before any catalog call");
  assert.equal(h.recordCalls.length, 0);
  assert.equal(h.opens.length, 0);
  await h.close();
});

test("replay selection retains bounded File metadata, opens in the gesture and pumps audio without live keys", async () => {
  const stopGate = deferred();
  const h = await harness({ stopGate, outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
  await h.preview();
  const file = selectedRecording();
  assert.equal(h.get("replay-play").disabled, true);
  chooseRecording(h, [file.file]);
  const label = h.get("replay-name").textContent;
  assert.match(label, /recorded-prefix\.bkr.*6 bytes/);
  assert.equal(h.get("replay-play").disabled, false);
  const invalidFiles = [selectedRecording(0), selectedRecording(64 * 1024 * 1024 + 1), selectedRecording(1.5)];
  for (const files of [...invalidFiles.map(value => [value.file]), [file.file, file.file],
    [{ size: 6, name: "unbranded.bkr", arrayBuffer() { assert.fail("unbranded file read"); } }]]) {
    chooseRecording(h, files);
    assert.equal(h.get("replay-name").textContent, label);
    assert.equal(h.get("replay-play").disabled, false, "invalid metadata leaves the prior admitted selection usable");
    assert.equal(h.get("replay-file").value, "");
  }
  h.get("seed").value = "not a live seed";
  h.get("record").checked = true;
  h.get("multiplayer").checked = true;
  h.get("multiplayer-url").value = "invalid for live multiplayer";
  h.click("replay-play");
  assert.equal(h.opens.length, 1);
  assert.equal(h.opens[0].gesture, true);
  assert.equal(file.reads, 0);
  await flush();
  const worker = h.workers[0];
  const start = worker.last("play-start");
  assert.equal(start.mode, "replay");
  assert.equal(start.multiplayer, undefined);
  assert.equal(worker.messages("play-network-ready").length, 0);
  assert.equal(start.replayFile, file.file);
  assert.equal(start.rate, 48000);
  for (const field of ["seed", "keyPairs", "recordReplay"]) assert.equal(Object.hasOwn(start, field), false);
  assert.equal(h.get("replay-file").disabled, true);
  assert.equal(h.get("play").disabled, true);
  const commands = await h.prepared(start, 1);
  await h.reply(commands, null);
  const activation = worker.last("play-activate");
  assert.equal(activation.startFrame, 60000n);
  assert.deepEqual(h.audio.arms, [60000n]);
  await h.reply(activation, null);
  assert.equal(h.audio.samples[0].rate, 44100);
  assert.match(h.get("keys").textContent, /Recorded input playback/);
  h.setNow(1300);
  const key = h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  assert.equal(key.defaultPrevented, false);
  await h.advance(8);
  assert.equal(worker.messages("play-step").length, 0);
  const render = worker.last("play-render");
  assert.equal(render.presentedNs, 50000000n);
  await h.receive({ kind: "play-render-done", playId: start.playId, renderId: render.renderId,
    completed: false, songNs: 2350000000n, hits: 23n, misses: 4n, combo: 11n, preOriginInputs: 0 });
  assert.match(h.get("status").textContent, /Replay.*Hits 23.*Misses 4.*Combo 11/);
  assert.match(h.get("position").value, /^2\.35(?:0*)$/);
  const escape = h.window.emit("keydown", { code: "Escape", repeat: false, timeStamp: 1308 });
  assert.equal(escape.defaultPrevented, true);
  await flush();
  assert.equal(worker.last("play-stop").completed, false);
  await h.receive(finalScore(start.playId, { replay: null, replayComplete: false, replayError: null }));
  assert.equal(h.get("replay-play").disabled, true);
  stopGate.resolve();
  await flush();
  assert.equal(h.get("replay-play").disabled, false);
  assert.equal(h.get("export").disabled, true, "a played file does not become a newly captured export");
  assert.equal(file.reads, 0);
  assert.ok(invalidFiles.every(value => value.reads === 0));
  await h.close();
});

test("recorded-prefix completion joins both owners and a fresh live session regains ordinary input routing", async () => {
  const stopGate = deferred();
  const h = await harness({ stopGate, outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
  const saved = await h.preview();
  const file = selectedRecording();
  chooseRecording(h, [file.file]);
  const replay = await h.launch(0, "replay");
  const worker = h.workers[0];
  const replayAudio = h.audio;
  h.setNow(1300);
  await h.advance(8);
  const render = worker.last("play-render");
  await h.receive({ kind: "play-render-done", playId: replay.id, renderId: render.renderId,
    completed: true, songNs: 2350000000n, hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0 });
  assert.equal(worker.last("play-stop").completed, true);
  assert.equal(worker.messages("play-step").length, 0);
  await h.receive(finalScore(replay.id, { replay: null, replayComplete: false, replayError: null }));
  assert.equal(h.get("play").disabled, true);
  stopGate.resolve();
  await flush();
  assert.match(h.get("status").textContent, /Recorded replay ended\..*Hits 3/);
  assert.doesNotMatch(h.get("status").textContent, /Song completed/);
  assert.equal(h.get("title").textContent, saved.title);
  assert.equal(h.get("position").value, saved.position);
  assert.equal(h.get("export").disabled, true);
  const live = await h.launch();
  assert.ok(live.id > replay.id);
  assert.equal(live.start.mode, "live");
  assert.equal(Object.hasOwn(live.start, "replayFile"), false);
  assert.ok(live.start.keyPairs instanceof Uint32Array);
  assert.notEqual(h.audio, replayAudio);
  assert.equal(h.opens.length, 2);
  assert.match(h.get("keys").textContent, /KeyZ/);
  h.setNow(1600);
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1600 });
  assert.equal(worker.last("play-step").playId, live.id);
  assert.equal(worker.last("play-step").events[0].down, true);
  h.click("stop");
  await flush();
  await h.receive(finalScore(live.id));
  assert.equal(file.reads, 0);
  await h.close();
});

test("replay preparation cancellation, mode mismatch and rejected audio prefixes release the original session", async () => {
  for (const mode of [undefined, "live"]) {
    const h = await harness();
    await h.preview();
    chooseRecording(h, [selectedRecording().file]);
    const start = await h.begin("replay");
    await h.reply(start, { kind: "prepared", mode, startNs: 0n, title: "Wrong mode", notes: 1, samples: 1, lanes: [0x11] });
    assert.equal(h.workers[0].messages("play-sample").length, 0);
    assert.equal(h.audio.samples.length, 0);
    assert.equal(h.audio.arms.length, 0);
    await h.receive(finalScore(start.playId));
    assert.match(h.get("status").textContent, /preparation mode changed/);
    assert.equal(h.get("replay-play").disabled, false);
    await h.close();
  }
  const cancelled = await harness();
  await cancelled.preview();
  const file = selectedRecording();
  chooseRecording(cancelled, [file.file]);
  const start = await cancelled.begin("replay");
  cancelled.click("stop");
  await flush();
  await cancelled.reply(start, { kind: "prepared", mode: "replay", samples: 2, lanes: [] });
  assert.equal(cancelled.workers[0].messages("play-sample").length, 0);
  assert.equal(cancelled.get("play").disabled, true);
  await cancelled.receive(finalScore(start.playId, { songNs: null, hits: null, misses: null, combo: null }));
  assert.equal(cancelled.get("play").disabled, false);
  assert.equal(file.reads, 0);
  await cancelled.close();

  const failure = Object.assign(new Error("actual replay output queue rejected prefix"), { admitted: 1 });
  const rejected = await harness({ commandFailure: failure });
  await rejected.preview();
  chooseRecording(rejected, [selectedRecording().file]);
  const replay = await rejected.launch(0, "replay");
  const worker = rejected.workers[0];
  await rejected.receive({ kind: "play-commands", playId: replay.id,
    batch: { sequence: 91n, commands: [command(8n), command(9n)] } });
  assert.equal(worker.last("play-ack").sequence, 91n);
  assert.equal(worker.last("play-ack").admitted, 1);
  assert.equal(worker.last("play-ack").success, false);
  assert.equal(worker.messages("play-ack").length, 1);
  assert.equal(rejected.audio.commandsSeen.length, 1);
  await rejected.receive(finalScore(replay.id, { kind: "play-error", released: true,
    message: "retained rejected replay commands", replay: null, replayComplete: false, replayError: null }));
  assert.match(rejected.get("status").textContent, /actual replay output queue rejected prefix/);
  assert.equal(rejected.get("export").disabled, true);
  assert.equal(worker.messages("play-step").length, 0);
  await rejected.close();
});

test("recording locks its session choice and exposes prefix downloads only after both cleanup joins", async () => {
  for (const audioFirst of [true, false]) {
    const stopGate = deferred();
    const h = await harness({ stopGate });
    assert.equal(h.get("record").checked, false);
    assert.equal(h.get("export").disabled, true);
    await h.preview();
    h.get("record").checked = true;
    const session = await h.launch();
    assert.equal(session.start.recordReplay, true);
    assert.equal(h.get("record").disabled, true);
    assert.equal(h.get("export").disabled, true);
    h.click("export");
    assert.equal(h.urls.length, 0);
    h.get("record").checked = false; // A later DOM value cannot change the session's choice.
    h.click("stop");
    await flush();
    assert.equal(h.workers[0].last("play-stop").completed, false);
    const bytes = Uint8Array.from([66, 75, 82, 0, 255]);
    const receipt = finalScore(session.id, { replay: bytes, replayComplete: false, replayError: null });
    if (audioFirst) { stopGate.resolve(); await flush(); }
    else await h.receive(receipt);
    assert.equal(h.get("export").disabled, true, "one cleanup receipt cannot expose owned capture bytes");
    if (audioFirst) await h.receive(receipt);
    else { stopGate.resolve(); await flush(); }
    assert.equal(h.get("record").disabled, false);
    assert.equal(h.get("export").disabled, false);
    assert.match(h.get("export").textContent, /prefix/);
    assert.equal(h.urls.length, 0, "receiving a capture does not create a URL or start a download");
    h.click("export");
    assert.equal(h.urls.length, 1);
    assert.equal(h.urls[0].blob.type, "application/octet-stream");
    assert.deepEqual(new Uint8Array(await h.urls[0].blob.arrayBuffer()), bytes);
    assert.equal(h.downloads[0].filename, `beatkernel-${session.id}-prefix.bkr`);
    assert.equal(h.downloads[0].href, h.urls[0].url);
    assert.equal(h.downloads[0].link.removed, true);
    assert.equal(h.document.body.children.length, 0);
    h.click("export");
    assert.deepEqual(h.revoked, [h.urls[0].url]);
    await h.advance(59999);
    assert.equal(h.revoked.length, 1);
    await h.advance(1);
    assert.deepEqual(h.revoked, [h.urls[0].url, h.urls[1].url]);
    assert.equal(h.get("export").disabled, false, "URL expiry does not discard the bounded recorded result");
    h.click("export");
    await h.close();
    assert.deepEqual(h.revoked, h.urls.map(entry => entry.url));
  }
});

test("only natural completion labels a joined replay complete and real cleanup failures downgrade it", async () => {
  for (const cleanup of ["clean", "audio", "worker"]) {
    const stopGate = deferred();
    const h = await harness({ stopGate, outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
    await h.preview();
    h.get("record").checked = true;
    const session = await h.launch();
    const worker = h.workers[0];
    h.setNow(1300);
    await h.advance(8);
    await h.receive({ kind: "play-render-done", playId: session.id,
      renderId: worker.last("play-render").renderId, completed: true });
    await h.receive({ kind: "play-step-done", playId: session.id,
      tickId: worker.last("play-step").tickId, songNs: 50000000n,
      hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0 });
    assert.equal(worker.last("play-stop").completed, true);
    const receipt = finalScore(session.id, { replay: Uint8Array.from([1, 2, 3]),
      replayComplete: cleanup !== "worker", replayError: null,
      ...(cleanup === "worker" ? { kind: "play-error", released: false, message: "game free failed" } : {}) });
    await h.receive(receipt);
    assert.equal(h.get("export").disabled, true);
    if (cleanup === "audio") stopGate.reject(new Error("actual context close failed"));
    else stopGate.resolve();
    await flush();
    assert.equal(h.get("export").disabled, false);
    assert.match(h.get("export").textContent, cleanup === "clean" ? /complete/ : /prefix/);
    if (cleanup === "clean") assert.match(h.get("status").textContent, /Song completed/);
    else {
      assert.equal(h.get("status").dataset.error, "true");
      assert.match(h.get("status").textContent, /cleanup failed/);
      assert.equal(worker.terminations, 1);
      assert.equal(h.get("play").disabled, true, "download remains possible while graphics ownership requires reload");
    }
    assert.equal(h.urls.length, 0);
    h.click("export");
    assert.match(h.downloads[0].filename, cleanup === "clean" ? /-complete\.bkr$/ : /-prefix\.bkr$/);
    await h.close();
  }
});

test("missing, unowned and malformed export evidence cannot become a downloadable complete record", async () => {
  const bytes = () => Uint8Array.from([1, 2, 3]);
  for (const [record, fields] of [
    [true, {}],
    [true, { replay: null, replayComplete: true, replayError: null }],
    [true, { replay: bytes(), replayComplete: true, replayError: null }],
    [true, { replay: bytes().subarray(1), replayComplete: false, replayError: null }],
    [true, { replay: new Uint8Array(0), replayComplete: false, replayError: null }],
    [false, { replay: bytes(), replayComplete: false, replayError: null }],
    [true, { replay: null, replayComplete: false, replayError: "actual codec refused the byte limit" }],
  ]) {
    const h = await harness();
    await h.preview();
    h.get("record").checked = record;
    const session = await h.launch();
    h.click("stop");
    await flush();
    await h.receive(finalScore(session.id, fields));
    assert.equal(h.get("export").disabled, true);
    assert.equal(h.get("status").dataset.error, "true");
    assert.match(h.get("status").textContent, /Replay export failed/);
    assert.equal(h.workers[0].terminations, 0, "serialization evidence alone does not imply a resource leak");
    assert.equal(h.get("play").disabled, false);
    h.click("export");
    assert.equal(h.urls.length, 0);
    await h.close();
  }
});

test("one retained replay survives a failed export and its URLs are replaced only by a newer valid result", async () => {
  const h = await harness();
  await h.preview();
  h.get("record").checked = true;
  const first = await h.launch();
  h.click("stop");
  await flush();
  await h.receive(finalScore(first.id, { replay: Uint8Array.from([7, 8]), replayComplete: false, replayError: null }));
  h.click("export");
  assert.equal(h.urls.length, 1);
  h.faults.downloadError = "browser denied URL allocation";
  h.click("export");
  assert.deepEqual(h.revoked, [h.urls[0].url]);
  assert.match(h.get("status").textContent, /browser denied URL allocation/);
  assert.equal(h.get("export").disabled, false);
  delete h.faults.downloadError;
  h.click("export");
  assert.equal(h.urls.length, 2);

  const failed = await h.launch();
  assert.equal(h.get("export").disabled, true);
  h.click("stop");
  await flush();
  await h.receive(finalScore(failed.id, { replay: null, replayComplete: false, replayError: "capture export failed" }));
  assert.equal(h.get("export").disabled, false);
  assert.deepEqual(h.revoked, [h.urls[0].url], "a missing newer capture retains the older result and URL");
  h.click("export");
  assert.equal(h.downloads.at(-1).filename, `beatkernel-${first.id}-prefix.bkr`);
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), Uint8Array.from([7, 8]));

  const replacement = await h.launch();
  h.click("stop");
  await flush();
  await h.receive(finalScore(replacement.id, { replay: Uint8Array.from([9, 10]), replayComplete: false, replayError: null }));
  assert.deepEqual(h.revoked, h.urls.map(entry => entry.url));
  const count = h.urls.length;
  h.click("export");
  assert.equal(h.urls.length, count + 1);
  assert.equal(h.downloads.at(-1).filename, `beatkernel-${replacement.id}-prefix.bkr`);
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), Uint8Array.from([9, 10]));
  await h.close();
  assert.deepEqual(h.revoked, h.urls.map(entry => entry.url));
});

test("user gesture opens real host boundary before awaits, then transfers source PCM and arms after setup", async () => {
  const h = await harness();
  await h.preview();
  h.click("play");
  assert.equal(h.opens.length, 1, "open must run in the synchronous click task");
  assert.equal(h.opens[0].gesture, true);
  assert.equal(h.opens[0].options.module, h.moduleToken);
  await flush();
  const worker = h.workers[0];
  const start = worker.last("play-start");
  assert.equal(start.rate, 48000, "preparation follows actual AudioContext rate");
  assert.equal(start.seed, "7");
  assert.equal(start.recordReplay, false, "recording remains disabled unless explicitly selected");
  assert.equal(start.multiplayer, undefined, "solo remains the default automatic audio path");
  assert.equal(worker.messages("play-network-ready").length, 0);
  assert.ok(start.keyPairs instanceof Uint32Array);
  const commands = await h.prepared(start, 1);
  assert.equal(h.audio.samples[0].rate, 44100, "original source rate survives transfer");
  assert.deepEqual(Array.from(h.audio.samples[0].pcm), [0.25, -0.25, 0.5, -0.5]);
  assert.equal(h.audio.finishes, 1);
  assert.deepEqual(h.audio.arms, []);
  await h.reply(commands, null);
  assert.deepEqual(h.audio.arms, [60000n]);
  const activation = worker.last("play-activate");
  assert.equal(activation.hostNs, 1250000000n);
  assert.equal(activation.startFrame, 60000n);
  await h.reply(activation, null);
  assert.match(h.get("keys").textContent, /KeyZ/);
  assert.equal(h.get("play").disabled, true);
  assert.equal(h.get("stop").disabled, false);
  await h.close();
});

test("stop waits for both audio cleanup and exact Worker receipt in either order and restores accepted preview", async () => {
  for (const audioFirst of [true, false]) {
    const stopGate = deferred();
    const h = await harness({ stopGate });
    const saved = await h.preview();
    const session = await h.launch();
    h.click("stop");
    h.window.emit("blur"); // Another stop reason must not create another owner cleanup.
    await flush();
    const worker = h.workers[0];
    assert.equal(worker.messages("play-stop").length, 1);
    assert.equal(h.audio.stopStarts, 1);
    await h.receive(finalScore(session.id + 1));
    assert.equal(h.get("play").disabled, true, "a different play generation cannot release ownership");
    if (audioFirst) stopGate.resolve();
    else await h.receive(finalScore(session.id));
    await flush();
    assert.equal(h.get("play").disabled, true);
    assert.equal(h.get("files").disabled, true);
    if (audioFirst) await h.receive(finalScore(session.id));
    else { stopGate.resolve(); await flush(); }
    assert.equal(h.get("play").disabled, false);
    assert.equal(h.get("title").textContent, saved.title);
    assert.equal(h.get("details").textContent, saved.details);
    assert.equal(h.get("position").value, "12.345678901");
    assert.equal(h.get("keys").textContent, "");
    assert.match(h.get("status").textContent, /Hits 3.*Misses 1.*Combo 2/);
    assert.doesNotMatch(h.get("status").textContent, /completed|acoustic/i);
    await h.close();
  }
});

test("cancelled pending open retains ownership through settled or failed cleanup and a late returned audio owner", async () => {
  for (const outcome of ["rejected", "late-owner", "cleanup-failure"]) {
    const openGate = deferred();
    const stopGate = deferred();
    const h = await harness({ openGate, stopGate });
    await h.preview();
    h.click("play");
    h.click("stop");
    await flush();
    assert.equal(h.opens[0].options.signal.aborted, true);
    assert.equal(h.get("play").disabled, true);
    assert.equal(h.workers[0].messages("play-start").length, 0);
    let openingError;
    if (outcome === "late-owner") {
      openGate.resolve(h.audio);
      await flush();
      assert.equal(h.audio.stopStarts, 1, "late owner shares its one cleanup promise");
      assert.equal(h.get("play").disabled, true);
      stopGate.resolve();
    } else {
      openingError = new Error("original opening failure");
      if (outcome === "cleanup-failure") {
        openingError.cleanupError = new Error("opening close remained unproven");
      }
      openGate.reject(openingError);
    }
    await flush();
    if (outcome === "cleanup-failure") {
      assert.equal(h.workers[0].terminations, 1);
      assert.equal(h.get("play").disabled, true);
      assert.equal(h.get("files").disabled, true);
      assert.equal(h.get("status").dataset.error, "true");
      assert.match(h.get("status").textContent, /Audio cleanup failed: opening close remained unproven/);
      assert.match(h.get("status").textContent, /Reload the page/i);
      assert.doesNotMatch(h.get("status").textContent, /original opening failure/);
      assert.equal(openingError.message, "original opening failure");
      assert.equal(openingError.cleanupError.message, "opening close remained unproven");
      h.click("play");
      assert.equal(h.opens.length, 1, "failed opening cleanup must fence another audio owner");
    } else assert.equal(h.get("play").disabled, false);
    assert.equal(h.workers[0].messages("play-start").length, 0);
    assert.equal(h.workers[0].messages("play-stop").length, 0, "there was no Worker game to stop");
    await h.close();
  }
});

test("pending Worker preparation cancels without inventing score and ignores late prepared replies", async () => {
  const h = await harness();
  const saved = await h.preview();
  const start = await h.begin();
  h.click("stop");
  await flush();
  assert.equal(h.get("play").disabled, true);
  await h.receive(finalScore(start.playId, { songNs: null, hits: null, misses: null, combo: null }));
  assert.equal(h.get("play").disabled, false);
  assert.doesNotMatch(h.get("status").textContent, /Hits|Misses|Combo|null/);
  await h.reply(start, { kind: "prepared", title: "stale prepared title", artist: "stale", notes: 2, samples: 1, lanes: [0x11] });
  assert.equal(h.get("title").textContent, saved.title);
  assert.equal(h.get("position").value, saved.position);
  assert.equal(h.workers[0].messages("play-sample").length, 0);
  await h.close();
});

test("missing stop receipt and rejected audio cleanup terminate Worker and require page reload", async () => {
  for (const audioFailure of [false, true]) {
    const h = await harness(audioFailure ? { stopFailure: new Error("close remained unproven") } : {});
    await h.preview();
    await h.launch();
    h.click("stop");
    await flush();
    const worker = h.workers[0];
    if (!audioFailure) {
      await h.advance(9999);
      assert.equal(worker.terminations, 0);
      assert.equal(h.get("play").disabled, true);
      await h.advance(1);
    }
    assert.equal(worker.terminations, 1);
    assert.equal(h.get("play").disabled, true);
    assert.equal(h.get("files").disabled, true);
    assert.equal(h.get("status").dataset.error, "true");
    assert.match(h.get("status").textContent, /Reload the page/i);
    h.click("play");
    assert.equal(h.opens.length, 1, "unproven release cannot silently admit another owner");
    await h.close();
  }
  for (const natural of [false, true]) {
    const h = await harness();
    await h.preview();
    const session = await h.launch();
    const worker = h.workers[0];
    if (natural) {
      await h.advance(8);
      await h.receive({ kind: "play-render-done", playId: session.id,
        renderId: worker.last("play-render").renderId, completed: true });
      await h.receive({ kind: "play-step-done", playId: session.id,
        tickId: worker.last("play-step").tickId, songNs: 2350000000n,
        hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0 });
    } else { h.click("stop"); await flush(); }
    assert.equal(worker.messages("play-stop").length, 1);
    await h.receive(finalScore(session.id, { kind: "play-error", released: false,
      message: "actual game disposal failed" }));
    assert.equal(worker.terminations, 1);
    assert.equal(h.audio.stopStarts, 1);
    assert.equal(h.get("play").disabled, true);
    assert.equal(h.get("files").disabled, true);
    assert.equal(h.get("status").dataset.error, "true");
    assert.match(h.get("status").textContent, /Gameplay cleanup failed: actual game disposal failed/);
    assert.match(h.get("status").textContent, /Reload the page/);
    assert.doesNotMatch(h.get("status").textContent, /Song completed|Playback stopped/);
    h.click("play");
    assert.equal(h.opens.length, 1);
    await h.close();
  }
});

test("pagehide releases the Worker waiter and late audio cleanup cannot restore an older page generation", async () => {
  const stopGate = deferred();
  const h = await harness({ stopGate });
  await h.preview();
  const session = await h.launch();
  h.click("stop");
  await flush();
  const oldWorker = h.workers[0];
  h.window.emit("pagehide");
  h.window.emit("pageshow", { persisted: true });
  await flush();
  assert.equal(oldWorker.terminations, 1);
  assert.equal(h.workers.length, 2);
  const freshCanvas = h.get("canvas");
  assert.equal(h.get("title").textContent, "No chart prepared");
  await h.receive(finalScore(session.id), oldWorker);
  stopGate.resolve();
  await flush();
  assert.equal(h.get("canvas"), freshCanvas);
  assert.equal(h.get("title").textContent, "No chart prepared");
  assert.equal(h.get("position").value, "0");
  assert.doesNotMatch(h.get("status").textContent, /Hits 3|Playback stopped/);
  assert.equal(h.get("play").disabled, true);
  assert.equal(h.timers.size, 0, "pagehide releases the stop deadline rather than waiting ten seconds");
  await h.close();
});

test("input and render reports each have one in-flight request and watermarks cannot cross unsent input", async () => {
  const h = await harness();
  await h.preview();
  const session = await h.launch();
  const worker = h.workers[0];
  await h.advance(40);
  assert.equal(worker.messages("play-step").length, 1);
  assert.equal(worker.messages("play-render").length, 1);
  assert.equal(h.audio.polls, 1);
  h.setNow(1300);
  for (let index = 0; index < 600; index++) {
    h.window.emit(index % 2 === 0 ? "keydown" : "keyup", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  }
  assert.equal(worker.messages("play-step").length, 1);
  async function done(step) {
    await h.receive({ kind: "play-step-done", playId: session.id, tickId: step.tickId,
      songNs: 50000000n, hits: 2n, misses: 0n, combo: 2n, preOriginInputs: 0 });
  }
  await done(worker.last("play-step"));
  const first = worker.last("play-step");
  assert.equal(first.events.length, 256);
  assert.equal(first.watermark, null);
  assert.equal(first.events[0].sequence, 1n);
  assert.equal(first.events.at(-1).sequence, 256n);
  await done(first);
  const second = worker.last("play-step");
  assert.equal(second.events.length, 256);
  assert.equal(second.watermark, null);
  assert.equal(second.events[0].sequence, 257n);
  await done(second);
  const final = worker.last("play-step");
  assert.equal(final.events.length, 88);
  assert.equal(final.events.at(-1).sequence, 600n);
  assert.equal(final.watermark, 1300000000n);
  const report = worker.last("play-render");
  await h.receive({ kind: "play-render-done", playId: session.id, renderId: report.renderId, completed: false });
  await h.advance(8);
  assert.equal(h.audio.polls, 2);
  assert.equal(worker.messages("play-render").length, 2);
  assert.equal(worker.messages("play-step").length, 4, "the unacknowledged final input batch still fences another step");
  await h.close();
});

test("setup and active command rejection forward the exact admitted prefix once and preserve original failure", async () => {
  for (const setup of [true, false]) {
    const failure = Object.assign(new Error("original audio admission failure"), { admitted: 1 });
    const h = await harness({ commandFailure: failure });
    await h.preview();
    const worker = h.workers[0];
    let playId;
    const batch = { sequence: 9n, commands: [command(3n), command(4n)] };
    if (setup) {
      const start = await h.begin();
      playId = start.playId;
      const requested = await h.prepared(start);
      await h.reply(requested, batch);
      const acknowledgement = worker.last("play-ack");
      assert.ok(acknowledgement.rpcId, "setup waits for rejection evidence to reach the real game owner");
      await h.receive({ kind: "play-reply", playId, rpcId: acknowledgement.rpcId, error: "secondary Worker rejection" });
    } else {
      const session = await h.launch();
      playId = session.id;
      await h.receive({ kind: "play-commands", playId, batch });
    }
    const acknowledgement = worker.last("play-ack");
    assert.equal(acknowledgement.sequence, 9n);
    assert.equal(acknowledgement.admitted, 1);
    assert.equal(acknowledgement.success, false);
    assert.equal(worker.messages("play-ack").length, 1);
    assert.equal(h.audio.commandsSeen.length, 1, "neither admitted prefix nor remainder is retried");
    assert.ok(worker.last("play-stop"));
    await h.receive(finalScore(playId, { kind: "play-error", released: true, message: "Worker retained exact rejected batch", hits: 2n }));
    assert.match(h.get("status").textContent, /original audio admission failure/);
    assert.doesNotMatch(h.get("status").textContent, /secondary Worker rejection/);
    assert.match(h.get("status").textContent, /Hits 2/);
    await h.close();
  }
});

test("natural completion joins captured input and command admission before the normal release handshake", async () => {
  const commandGate = deferred();
  const stopGate = deferred();
  const h = await harness({ commandGate, stopGate, outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
  const saved = await h.preview();
  const session = await h.launch();
  h.setNow(1300);
  await h.advance(8);
  const worker = h.workers[0];
  const firstTick = worker.last("play-step");
  const firstReport = worker.last("play-render");
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1308 });
  await h.receive({ kind: "play-render-done", playId: session.id,
    renderId: firstReport.renderId, completed: true });
  assert.equal(worker.messages("play-stop").length, 0, "captured input and its earlier watermark must join");
  h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1308 });
  const stepDone = request => h.receive({ kind: "play-step-done", playId: session.id,
    tickId: request.tickId, songNs: 58000000n, hits: 4n, misses: 1n, combo: 3n, preOriginInputs: 0 });
  await stepDone(firstTick);
  const captured = worker.last("play-step");
  assert.deepEqual(captured.events, [
    { hostNs: 1308000000n, key: 2, down: true, sequence: 1n },
    { hostNs: 1308000000n, key: 2, down: false, sequence: 2n },
  ]);
  await h.receive({ kind: "play-commands", playId: session.id,
    batch: { sequence: 9n, commands: [command(3n)] } });
  await stepDone(captured);
  assert.equal(worker.messages("play-stop").length, 0);
  assert.equal(worker.messages("play-ack").length, 0, "ordinary audio work is still pending");
  commandGate.resolve();
  await flush();
  assert.equal(worker.last("play-ack").sequence, 9n);
  assert.equal(worker.last("play-ack").admitted, 1);
  assert.equal(worker.messages("play-stop").length, 0, "new input/audio invalidated the older completion receipt");
  await h.advance(8);
  const finalReport = worker.last("play-render");
  const finalTick = worker.last("play-step");
  assert.ok(finalReport.renderId > firstReport.renderId);
  await h.receive({ kind: "play-render-done", playId: session.id,
    renderId: finalReport.renderId, completed: true });
  assert.equal(worker.messages("play-stop").length, 0);
  await stepDone(finalTick);
  assert.equal(worker.messages("play-stop").length, 1);
  assert.equal(h.audio.stopStarts, 1);
  assert.equal(h.get("play").disabled, true);
  await h.receive(finalScore(session.id, { hits: 4n, combo: 3n }));
  assert.equal(h.get("play").disabled, true, "Worker completion is not audio cleanup");
  stopGate.resolve();
  await flush();
  assert.equal(h.get("play").disabled, false);
  assert.match(h.get("status").textContent, /Song completed\..*Hits 4.*Misses 1.*Combo 3/);
  assert.equal(h.get("title").textContent, saved.title);
  assert.equal(h.get("position").value, saved.position);
  await h.close();
});

test("output observations follow poll, retain actual time without extrapolation and drop regressing points", async () => {
  const pollGate = deferred();
  const h = await harness({ pollGate, outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
  await h.preview();
  const session = await h.launch();
  h.setNow(1300);
  await h.advance(8);
  const worker = h.workers[0];
  assert.equal(h.audio.polls, 1);
  assert.equal(h.audio.outputReads, 0, "pending poll has no matching presentation query yet");
  assert.equal(worker.messages("play-render").length, 0);
  pollGate.resolve();
  await flush();
  const first = worker.last("play-render");
  assert.equal(first.presentedNs, 50000000n, "8 ms of host delay does not advance output evidence");
  assert.equal(first.presentedHostNs, 1300000000n);
  const pollIndex = h.traces.findIndex(row => row[0] === "poll");
  const outputIndex = h.traces.findIndex(row => row[0] === "output-timestamp");
  assert.ok(outputIndex > pollIndex);
  await h.receive({ kind: "play-render-done", playId: session.id, renderId: first.renderId, completed: false });
  h.faults.outputEvidence = { contextTime: 1.29, performanceTime: 1308 };
  await h.advance(8);
  const regressed = worker.last("play-render");
  assert.equal(regressed.presentedNs, null);
  assert.equal(regressed.presentedHostNs, null);
  await h.receive({ kind: "play-render-done", playId: session.id, renderId: regressed.renderId, completed: false });
  h.faults.outputEvidence = { contextTime: 1.3, performanceTime: 1316 };
  await h.advance(8);
  assert.equal(worker.last("play-render").presentedNs, 50000000n, "a missing point does not reset the accepted frontier");
  assert.equal(worker.messages("play-stop").length, 0);
  await h.close();
});

test("unavailable output keeps completion pending while malformed evidence and receipts fail visibly", async () => {
  for (const code of ["unsupported", "unavailable", "state", "transport"]) {
    const h = await harness({ outputFailure: Object.assign(new Error(`actual output ${code}`), { code }) });
    await h.preview();
    const session = await h.launch();
    await h.advance(8);
    const worker = h.workers[0];
    if (code === "unsupported" || code === "unavailable") {
      const report = worker.last("play-render");
      assert.equal(report.presentedNs, null);
      assert.equal(report.presentedHostNs, null);
      await h.receive({ kind: "play-render-done", playId: session.id, renderId: report.renderId, completed: false });
      await h.receive({ kind: "play-step-done", playId: session.id,
        tickId: worker.last("play-step").tickId, songNs: 604800000000000n,
        hits: 10000n, misses: 0n, combo: 10000n, preOriginInputs: 0 });
      assert.equal(worker.messages("play-stop").length, 0, "song age and finished score do not invent presentation");
      h.click("stop");
      await flush();
      await h.receive(finalScore(session.id));
      assert.match(h.get("status").textContent, /Playback stopped/);
      assert.doesNotMatch(h.get("status").textContent, /Song completed/);
    } else {
      assert.equal(worker.messages("play-render").length, 0);
      assert.equal(worker.messages("play-stop").length, 1);
      await h.receive(finalScore(session.id));
      assert.equal(h.get("status").dataset.error, "true");
      assert.match(h.get("status").textContent, new RegExp(`actual output ${code}`));
    }
    await h.close();
  }
  for (const completed of [undefined, "true"]) {
    const h = await harness();
    await h.preview();
    const session = await h.launch();
    await h.advance(8);
    const worker = h.workers[0];
    await h.receive({ kind: "play-render-done", playId: session.id,
      renderId: worker.last("play-render").renderId, completed });
    assert.equal(worker.messages("play-stop").length, 1);
    await h.receive(finalScore(session.id));
    assert.equal(h.get("status").dataset.error, "true");
    assert.match(h.get("status").textContent, /completion evidence was malformed/);
    await h.close();
  }
});

test("Window retains only progressing clock pairs and defers coarse or regressing coordinates", async () => {
  const h = await harness({ outputEvidence: { contextTime: 1.5, performanceTime: 1500 } });
  await h.preview();
  const session = await h.launch();
  const worker = h.workers[0];
  h.setNow(1500);
  await h.advance(8);
  const initial = worker.last("play-render");
  assert.equal(initial.presentedNs, 250000000n);
  assert.equal(initial.presentedHostNs, 1500000000n);
  let previous = initial;
  for (const [contextTime, performanceTime, outputNs, hostNs] of [
    // The repeated output forwards its actual host coordinate without retaining
    // it as new progress. A subsequent host below 1504 ms must remain admissible.
    [1.5, 1504, 250000000n, 1504000000n],
    [1.5009765625, 1500, null, null],
    [1.5009765625, 1502.125, 250976562n, 1502125000n],
    [1.5, 1510, null, null],
    [1.501953125, 1501, null, null],
    [1.501953125, 1502.125, null, null],
    [1.501953125, 1510, 251953125n, 1510000000n],
  ]) {
    await h.receive({ kind: "play-render-done", playId: session.id,
      renderId: previous.renderId, completed: false });
    h.faults.outputEvidence = { contextTime, performanceTime };
    await h.advance(8);
    const next = worker.last("play-render");
    assert.ok(next.renderId > previous.renderId);
    assert.equal(next.presentedNs, outputNs);
    assert.equal(next.presentedHostNs, hostNs);
    previous = next;
  }
  assert.equal(worker.messages("play-stop").length, 0);
  assert.equal(h.audio.stopStarts, 0);
  await h.close();
});

function chooseMultiplayer(h, host = true) {
  h.get("multiplayer").checked = true;
  h.get("multiplayer-url").value = "https://example.test:4433/competition";
  h.get("multiplayer-role").value = host ? "host" : "join";
  h.get("multiplayer").emit("change");
}

function content(element) {
  return [element.textContent, ...element.children.map(content)].join(" ");
}
function selectOpponent(h, selected, { own = true, label = "" } = {}) {
  chooseRecording(h, [selected.file]);
  h.get("opponents-kind").value = own ? "own" : "other";
  h.get("opponents-label").value = label;
  h.click("opponents-add");
}
function comparison(playId, fields = {}) {
  return { kind: "play-opponents", playId, error: null, opponents: [{
    kind: "other", label: "<img src=x>", songNs: 999999990n, recordedUntilNs: -1n,
    hits: 7n, misses: 2n, combo: 3n, maxCombo: 5n, ...fields,
  }] };
}

test("live opponent selection retains immutable Files through retry, opens audio in the gesture and stays inactive for replay", async () => {
  const h = await harness(); await h.preview();
  const selected = selectedRecording();
  selectOpponent(h, selected, { own: false, label: "<img src=x>" });
  assert.equal(h.get("opponents-list").children.length, 1);
  assert.match(content(h.get("opponents-list")), /Other.*<img src=x>/);
  h.click("opponents-add");
  assert.equal(h.get("opponents-list").children.length, 1);
  assert.match(h.get("opponents-status").textContent, /already selected/i);
  assert.equal(selected.reads, 0);
  assert.equal(h.recordCalls.length, 0);
  assert.equal(h.opens.length, 0);
  const session = await h.launch();
  const worker = h.workers[0];
  assert.equal(h.opens[0].gesture, true);
  assert.equal(session.start.opponents.length, 1);
  assert.equal(session.start.opponents[0].file, selected.file);
  assert.equal(session.start.opponents[0].own, false);
  assert.equal(session.start.opponents[0].label, "<img src=x>");
  assert.equal(worker.posts.find(post => post.value === session.start).transferCount, 0);
  for (const id of ["opponents-add", "opponents-clear", "opponents-kind", "opponents-label", "records-opponent"]) {
    assert.equal(h.get(id).disabled, true);
  }
  const local = { title: h.get("title").textContent, details: h.get("details").textContent,
    status: h.get("status").textContent, network: h.get("multiplayer-status").textContent };
  await h.receive(comparison(session.id));
  const retainedRow = h.get("opponents-results").children[0];
  assert.match(retainedRow.textContent, /Other.*<img src=x>.*Hits 7.*recorded through -0\.000000001/);
  assert.deepEqual({ title: h.get("title").textContent, details: h.get("details").textContent,
    status: h.get("status").textContent, network: h.get("multiplayer-status").textContent }, local);
  await h.receive(comparison(session.id, { hits: 8n, combo: 4n, maxCombo: 5n, recordedUntilNs: null }));
  assert.equal(h.get("opponents-results").children[0], retainedRow);
  assert.match(retainedRow.textContent, /Hits 8.*empty recording/);
  h.click("stop"); await flush();
  await h.receive(finalScore(session.id));
  assert.equal(h.get("opponents-list").children.length, 1);
  const retry = await h.launch();
  assert.equal(retry.start.opponents[0].file, selected.file);
  assert.equal(retry.start.opponents[0].sourceKey, session.start.opponents[0].sourceKey);
  assert.equal(selected.reads, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(retry.id));
  const replay = await h.launch(0, "replay");
  assert.equal(replay.start.opponents?.length ?? 0, 0);
  assert.equal(replay.start.replayFile, selected.file);
  const inactive = h.get("opponents-status").textContent;
  await h.receive(comparison(session.id));
  await h.receive(comparison(replay.id));
  assert.equal(h.get("opponents-status").textContent, inactive);
  assert.equal(h.get("opponents-results").children.length, 0);
  await h.close();
  assert.equal(h.get("opponents-list").children.length, 0);
});

test("stored opponent admission is explicit and cancellable, with stable record identity and selection quota preserved on refusal", async () => {
  const gate = deferred();
  const h = await harness({ recordsList: [savedRecord()], recordsLoadGate: gate });
  await h.preview();
  const original = selectedRecording();
  selectOpponent(h, original);
  h.click("records-refresh"); await flush();
  assert.equal(h.recordCalls.filter(call => call.method === "load").length, 0);
  h.get("opponents-kind").value = "other";
  h.click("records-opponent"); await flush();
  assert.equal(h.recordCalls.filter(call => call.method === "load").length, 1);
  assert.equal(h.get("play").disabled, true);
  assert.equal(h.get("opponents-clear").disabled, true);
  h.document.hidden = true; h.document.emit("visibilitychange"); await flush();
  assert.equal(h.recordOwners[0].closed, true);
  gate.resolve(); await flush();
  assert.equal(h.get("opponents-list").children.length, 1);
  assert.equal(h.opens.length, 0);
  h.document.hidden = false; h.document.emit("visibilitychange");
  delete h.faults.recordsLoadGate;
  h.click("records-refresh"); await flush();
  h.click("records-opponent"); await flush();
  assert.equal(h.get("opponents-list").children.length, 2);
  assert.match(content(h.get("opponents-list")), /Other.*saved-prefix\.bkr/);
  assert.equal(h.opens.length, 0);
  h.click("records-use"); await flush();
  h.click("opponents-add");
  assert.equal(h.get("opponents-list").children.length, 2, "same record loaded through Use remains the same selection identity");
  assert.match(h.get("opponents-status").textContent, /already selected/i);
  const tooLargeForRemaining = selectedRecording(64 * 1024 * 1024);
  selectOpponent(h, tooLargeForRemaining);
  assert.equal(h.get("opponents-list").children.length, 2);
  assert.match(h.get("opponents-status").textContent, /64 MiB/);
  assert.equal(tooLargeForRemaining.reads, 0);
  const firstRemove = h.get("opponents-list").children[0].children.find(child => child.tagName === "button");
  firstRemove.emit("click");
  assert.equal(h.get("opponents-list").children.length, 1);
  h.click("opponents-clear");
  assert.equal(h.get("opponents-list").children.length, 0);
  assert.equal(h.get("opponents-clear").disabled, true);
  assert.equal(h.recordCalls.filter(call => call.method === "remove" || call.method === "save").length, 0);

  const late = deferred(); h.faults.recordsLoadGate = late;
  h.click("records-opponent"); await flush();
  h.window.emit("pagehide"); await flush();
  h.window.emit("pageshow", { persisted: true }); await h.preview();
  late.resolve(); await flush();
  assert.equal(h.get("opponents-list").children.length, 0);
  assert.equal(h.opens.length, 0);
  await h.close();
});

test("opponent preparation mismatches stop setup while malformed or failed live comparisons preserve capture and natural completion", async () => {
  const mismatch = await harness(); await mismatch.preview();
  selectOpponent(mismatch, selectedRecording());
  const start = await mismatch.begin();
  await mismatch.reply(start, { kind: "prepared", title: "Mismatch", artist: "Fixture", notes: 1,
    samples: 0, lanes: [0x11], opponentCount: 0 });
  assert.equal(mismatch.audio.arms.length, 0);
  assert.equal(mismatch.workers[0].messages("play-activate").length, 0);
  assert.equal(mismatch.workers[0].last("play-stop").playId, start.playId);
  await mismatch.receive(finalScore(start.playId));
  assert.equal(mismatch.get("opponents-list").children.length, 1);
  await mismatch.close();

  for (const failure of ["binding", "malformed"]) {
    const h = await harness({ outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
    await h.preview(); selectOpponent(h, selectedRecording()); h.get("record").checked = true;
    const session = await h.launch(); const worker = h.workers[0];
    const data = failure === "binding" ? { kind: "play-opponents", playId: session.id,
      opponents: null, error: "actual comparison failure" } : comparison(session.id, { hits: "7" });
    await h.receive(data);
    assert.match(h.get("opponents-status").textContent, /stopped.*Local play continues/i);
    assert.equal(worker.messages("play-stop").length, 0);
    assert.equal(h.audio.stopStarts, 0);
    const comparisonFailure = h.get("opponents-status").textContent;
    await h.receive(comparison(session.id));
    assert.equal(h.get("opponents-status").textContent, comparisonFailure);
    h.setNow(1300); await h.advance(8);
    await h.receive({ kind: "play-render-done", playId: session.id,
      renderId: worker.last("play-render").renderId, completed: true });
    await h.receive({ kind: "play-step-done", playId: session.id, tickId: worker.last("play-step").tickId,
      songNs: 50000000n, hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0 });
    assert.equal(worker.last("play-stop").completed, true);
    await h.receive(finalScore(session.id, { replay: Uint8Array.from([66, 75, 82]), replayComplete: true, replayError: null }));
    assert.match(h.get("status").textContent, /Song completed/);
    assert.match(h.get("export").textContent, /complete/);
    assert.equal(h.get("export").disabled, false);
    const ended = h.get("opponents-status").textContent;
    await h.receive(comparison(session.id));
    assert.equal(h.get("opponents-status").textContent, ended);
    await h.close();
  }
});

test("live timing is captured before synchronous audio open and drafts stay locked until all owners join", async () => {
  const opening = deferred(), loading = deferred(), stopping = deferred();
  const h = await harness({ openGate: opening, recordsList: [savedRecord()], recordsLoadGate: loading, stopGate: stopping });
  await h.preview();
  const fields = ["judge-early", "judge-late", "judge-offset"].map(id => h.get(id));
  assert.deepEqual(fields.map(field => field.value), ["50", "50", "0"]);
  h.click("records-refresh"); await flush();
  h.click("records-use"); await flush();
  assert.equal(h.recordCalls.at(-1).method, "load");
  assert.ok(fields.every(field => field.disabled), "record acquisition locks session drafts");
  assert.equal(h.opens.length, 0);
  loading.resolve(); await flush();
  assert.ok(fields.every(field => !field.disabled));
  fields[0].value = "12.345678";
  fields[1].value = "87.654321";
  fields[2].value = "-12.500001";
  h.click("play");
  assert.equal(h.opens.length, 1, "opening begins in the original click stack");
  assert.equal(h.opens[0].gesture, true);
  assert.ok(fields.every(field => field.disabled));
  const worker = h.workers[0];
  assert.equal(worker.messages("play-start").length, 0);
  // Disabled DOM values remain script-mutable; the admitted session owns a
  // separate snapshot before any asynchronous audio or chart preparation.
  fields[0].value = "1"; fields[1].value = "2"; fields[2].value = "3";
  opening.resolve(h.audio); await flush();
  const start = worker.last("play-start");
  assert.deepEqual(start.timing, { earlyNs: 12345678n, lateNs: 87654321n, offsetNs: -12500001n });
  await h.reply(await h.prepared(start), null);
  await h.reply(worker.last("play-activate"), null);
  assert.ok(fields.every(field => field.disabled));
  h.click("stop"); await flush();
  await h.receive(finalScore(start.playId));
  assert.ok(fields.every(field => field.disabled), "Worker completion cannot unlock pending audio cleanup");
  stopping.resolve(); await flush();
  assert.ok(fields.every(field => !field.disabled));
  assert.deepEqual(fields.map(field => field.value), ["1", "2", "3"]);
  await h.close();
});

test("invalid timing never opens audio, setup refusal preserves drafts for retry, and replay ignores live drafts", async () => {
  const h = await harness();
  await h.preview();
  const fields = ["judge-early", "judge-late", "judge-offset"].map(id => h.get(id));
  const worker = h.workers[0];
  for (const values of [["-0.000001", "50", "0"], ["50", "50", "0.0000001"],
    ["50", "50", "9223372036854.775808"]]) {
    fields.forEach((field, index) => { field.value = values[index]; });
    h.click("play"); await flush();
    assert.equal(h.opens.length, 0);
    assert.equal(worker.messages("play-start").length, 0);
    assert.equal(worker.messages("play-stop").length, 0);
    assert.equal(h.get("status").dataset.error, "true");
    assert.deepEqual(fields.map(field => field.value), values);
    assert.ok(fields.every(field => !field.disabled));
  }
  fields[0].value = "0"; fields[1].value = "25.000001"; fields[2].value = "-0.000001";
  const refused = await h.begin();
  assert.equal(h.opens[0].gesture, true);
  await h.receive({ kind: "play-reply", playId: refused.playId, rpcId: refused.rpcId,
    error: "recorded opponent uses a different judge profile" });
  assert.equal(worker.last("play-stop").playId, refused.playId);
  await h.receive(finalScore(refused.playId));
  assert.deepEqual(fields.map(field => field.value), ["0", "25.000001", "-0.000001"]);
  const retry = await h.launch();
  assert.notEqual(retry.id, refused.playId);
  assert.deepEqual(retry.start.timing, { earlyNs: 0n, lateNs: 25000001n, offsetNs: -1n });
  assert.equal(h.opens[1].gesture, true);
  h.click("stop"); await flush(); await h.receive(finalScore(retry.id));
  const recorded = selectedRecording();
  chooseRecording(h, [recorded.file]);
  fields[0].value = "bad draft"; fields[1].value = "-1"; fields[2].value = "1e100";
  const replay = await h.launch(0, "replay");
  assert.equal(replay.start.mode, "replay");
  assert.equal(Object.hasOwn(replay.start, "timing"), false);
  assert.equal(h.opens[2].gesture, true);
  assert.equal(recorded.reads, 0, "Window still leaves replay bytes to the actual Worker owner");
  assert.ok(fields.every(field => field.disabled));
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  assert.deepEqual(fields.map(field => field.value), ["bad draft", "-1", "1e100"]);
  assert.ok(fields.every(field => !field.disabled));
  await h.close();
});

test("live section start snapshots before audio opens, retains drafts and prepares a fresh original-source session", async () => {
  const opening = deferred(), loading = deferred(), stopping = deferred();
  const h = await harness({ openGate: opening, recordsList: [savedRecord()], recordsLoadGate: loading, stopGate: stopping });
  await h.preview();
  const draft = h.get("live-start");
  assert.equal(draft.value, "0");
  h.click("records-refresh"); await flush(); h.click("records-use"); await flush();
  assert.equal(draft.disabled, true);
  assert.equal(h.opens.length, 0);
  loading.resolve(); await flush();
  assert.equal(draft.disabled, false);
  draft.value = "2.125000001";
  h.click("play");
  assert.equal(h.opens.length, 1);
  assert.equal(h.opens[0].gesture, true);
  assert.equal(draft.disabled, true);
  assert.equal(h.opens[0].options.pcmLimits.maxSamples, 5392);
  assert.equal(h.opens[0].options.pcmLimits.maxAssetBytes, 64 * 1024 * 1024);
  assert.equal(h.opens[0].options.pcmLimits.maxTotalBytes, 256 * 1024 * 1024);
  draft.value = "7.250000001";
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0];
  const start = worker.last("play-start");
  assert.equal(start.startNs, 2125000001n);
  await h.reply(await h.prepared(start), null);
  const activation = worker.last("play-activate");
  assert.equal(activation.hostNs, 1250000000n, "section song coordinates do not offset the chosen host/output start");
  assert.equal(activation.startFrame, 60000n);
  await h.reply(activation, null);
  assert.equal(draft.disabled, true);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.equal(draft.disabled, true, "pending output cleanup still owns the section draft");
  stopping.resolve(); await flush();
  assert.equal(draft.disabled, false);
  assert.equal(draft.value, "7.250000001");
  delete h.faults.openGate;
  const next = await h.launch();
  assert.notEqual(next.id, start.playId);
  assert.equal(next.start.startNs, 7250000001n);
  assert.equal(next.start.libraryId, start.libraryId);
  assert.equal(next.start.path, start.path);
  assert.equal(h.opens[1].gesture, true);
  h.click("stop"); await flush(); await h.receive(finalScore(next.id));
  await h.close();
});

test("section draft and preparation errors stay recoverable while replay uses only its recorded start", async () => {
  const h = await harness({ replayStart: 604800000000001n });
  await h.preview();
  const draft = h.get("live-start"), worker = h.workers[0];
  for (const text of ["-1", "1.0000000001", "9223372034.854775808"]) {
    draft.value = text;
    h.click("play"); await flush();
    assert.equal(h.opens.length, 0);
    assert.equal(worker.messages("play-start").length, 0);
    assert.equal(draft.disabled, false);
    assert.equal(draft.value, text);
    assert.equal(h.get("status").dataset.error, "true");
  }
  draft.value = "2.000000001";
  for (const startNs of [undefined, 0n, "2000000001"]) {
    const start = await h.begin();
    await h.reply(start, { kind: "prepared", title: "Wrong section", notes: 1, samples: 1,
      lanes: [0x11], opponentCount: 0, ...(startNs === undefined ? {} : { startNs }) });
    assert.equal(worker.messages("play-sample").length, 0);
    assert.deepEqual(h.audio.arms, []);
    assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(finalScore(start.playId));
    assert.equal(draft.disabled, false);
    assert.equal(draft.value, "2.000000001");
  }
  const retried = await h.launch();
  assert.equal(retried.start.startNs, 2000000001n);
  h.click("stop"); await flush(); await h.receive(finalScore(retried.id));
  const recording = selectedRecording();
  chooseRecording(h, [recording.file]);
  draft.value = "invalid live start";
  const replay = await h.launch(0, "replay");
  assert.equal(Object.hasOwn(replay.start, "startNs"), false);
  assert.equal(recording.reads, 0);
  assert.equal(h.opens.at(-1).gesture, true);
  assert.equal(draft.disabled, true);
  assert.equal(worker.messages("play-stop").some(message => message.playId === replay.id), false);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  assert.equal(draft.value, "invalid live start");
  assert.equal(draft.disabled, false);
  await h.close();
});

test("multiplayer readiness follows real audio setup and one committed grid arms both game and output", async () => {
  const commandGate = deferred();
  const h = await harness({ commandGate });
  await h.preview();
  assert.equal(h.get("multiplayer").checked, false);
  chooseMultiplayer(h);
  const start = await h.begin();
  const worker = h.workers[0];
  assert.equal(h.opens[0].gesture, true);
  assert.deepEqual(start.multiplayer, { url: "https://example.test:4433/competition", host: true,
    windowOriginNs: 9000000000n });
  assert.equal(h.get("multiplayer").disabled, true);
  const commands = await h.prepared(start, 1);
  assert.equal(h.audio.finishes, 1);
  assert.equal(worker.messages("play-network-ready").length, 0);
  await h.reply(commands, { sequence: 41n, commands: [command()] });
  assert.equal(worker.messages("play-network-ready").length, 0, "pending actual PCM command write is not readiness");
  commandGate.resolve(); await flush();
  await h.reply(worker.last("play-ack"), null);
  await h.reply(worker.last("play-commands"), null);
  const ready = worker.last("play-network-ready");
  assert.ok(ready);
  assert.equal(worker.messages("play-network-ready").length, 1);
  assert.deepEqual(h.audio.arms, []);
  await h.reply(ready, { kind: "multiplayer-start", targetHostNs: 1500000001n,
    songTargetHostNs: 1600000001n, uncertaintyNs: 0n });
  assert.deepEqual(h.audio.arms, [72001n]);
  const activation = worker.last("play-activate");
  assert.equal(activation.startFrame, 72001n);
  assert.equal(activation.hostNs, 1500020833n);
  assert.equal(activation.targetHostNs, 1500000001n);
  assert.ok(activation.hostNs >= activation.targetHostNs);
  assert.ok(activation.hostNs - activation.targetHostNs <= 20834n);
  await h.reply(activation, null);
  assert.equal(h.get("stop").disabled, false);
  await h.close();
});

test("remote summaries stay separate and active disconnect does not stop local play or fake final ACK", async () => {
  const stopGate = deferred();
  const h = await harness({ stopGate });
  await h.preview(); chooseMultiplayer(h, false);
  const start = await h.begin();
  const worker = h.workers[0];
  await h.reply(await h.prepared(start), null);
  await h.reply(worker.last("play-network-ready"), { kind: "multiplayer-start", targetHostNs: 1500000000n,
    songTargetHostNs: 1600000000n, uncertaintyNs: 0n });
  await h.reply(worker.last("play-activate"), null);
  const before = { title: h.get("title").textContent, details: h.get("details").textContent,
    status: h.get("status").textContent };
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "progress",
    songNs: -1n, hits: 18446744073709551615n, misses: 0n, combo: 7n, maxCombo: 99n } });
  assert.match(h.get("multiplayer-status").textContent, /self-reported.*-0\.000000001.*18446744073709551615/);
  assert.deepEqual({ title: h.get("title").textContent, details: h.get("details").textContent,
    status: h.get("status").textContent }, before);
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "progress",
    songNs: 0n, hits: "4", misses: 0n, combo: 0n, maxCombo: 0n } });
  assert.match(h.get("multiplayer-status").textContent, /malformed.*continues/i);
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "disconnected", error: "peer left" } });
  assert.match(h.get("multiplayer-status").textContent, /peer left.*local play continues/i);
  assert.equal(h.audio.stopStarts, 0);
  assert.equal(worker.messages("play-stop").length, 0);
  await h.advance(8);
  assert.ok(worker.messages("play-step").length > 0);
  h.click("stop"); await flush();
  await h.receive(finalScore(start.playId, { multiplayer: { finalWritten: true, finalAcknowledged: false, error: "peer ACK timed out" } }));
  assert.equal(h.get("play").disabled, true, "network receipt does not release outstanding audio cleanup");
  stopGate.resolve(); await flush();
  assert.match(h.get("multiplayer-status").textContent, /written.*ACK unavailable.*timed out/);
  const ended = h.get("multiplayer-status").textContent;
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "final-acknowledged" } });
  assert.equal(h.get("multiplayer-status").textContent, ended);
  h.get("multiplayer").checked = false;
  const solo = await h.launch();
  assert.equal(solo.start.multiplayer, undefined);
  const soloStatus = h.get("multiplayer-status").textContent;
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "disconnected", error: "late owner" } });
  assert.equal(h.get("multiplayer-status").textContent, soloStatus);
  await h.close();
});

test("invalid committed schedules and cancelled readiness never arm or revive a later play owner", async () => {
  for (const schedule of [
    { kind: "multiplayer-start", targetHostNs: 1500000000n, songTargetHostNs: 1500000000n, uncertaintyNs: 0n },
    { kind: "multiplayer-start", targetHostNs: 1000000000n, songTargetHostNs: 1100000000n, uncertaintyNs: 0n },
    { kind: "multiplayer-start", targetHostNs: 1500000000n, songTargetHostNs: 1600000000n, uncertaintyNs: 100000001n },
    { kind: "multiplayer-start", targetHostNs: 1500000000, songTargetHostNs: 1600000000n, uncertaintyNs: 0n },
  ]) {
    const h = await harness(); await h.preview(); chooseMultiplayer(h);
    const start = await h.begin(); const worker = h.workers[0];
    await h.reply(await h.prepared(start), null);
    await h.reply(worker.last("play-network-ready"), schedule);
    assert.deepEqual(h.audio.arms, []);
    assert.equal(worker.messages("play-activate").length, 0);
    assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(finalScore(start.playId, { multiplayer: { finalWritten: false, finalAcknowledged: false, error: "setup refused" } }));
    await h.close();
  }
  const h = await harness(); await h.preview(); chooseMultiplayer(h);
  const start = await h.begin(); const worker = h.workers[0];
  await h.reply(await h.prepared(start), null);
  const pending = worker.last("play-network-ready");
  const escape = h.window.emit("keydown", { code: "Escape", repeat: false, timeStamp: 1000 });
  assert.equal(escape.defaultPrevented, true);
  await flush();
  await h.reply(pending, { kind: "multiplayer-start", targetHostNs: 1500000000n,
    songTargetHostNs: 1600000000n, uncertaintyNs: 0n });
  assert.deepEqual(h.audio.arms, []);
  await h.receive(finalScore(start.playId, { multiplayer: { finalWritten: false, finalAcknowledged: false, error: "cancelled" } }));
  assert.equal(h.get("play").disabled, false);
  await h.close();
});

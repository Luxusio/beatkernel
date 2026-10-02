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
    "replay-file", "replay-play", "replay-name"]) {
    elements.set(id, new Element(id === "chart" ? "select" : id, id));
  }
  elements.get("folder").webkitdirectory = true;
  elements.get("rate").value = "44100";
  elements.get("seed").value = "7";

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
    controlClock() { return { beforeMs: now, afterMs: now, contextTime: now / 1000, sampleRate: 48000 }; },
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
    ArrayBuffer, structuredClone, performance: { now: () => now },
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
  const modules = new Map();
  for (const name of ["host_model.mjs", "play-model.mjs", "main.js"]) {
    const url = new URL(name, import.meta.url);
    modules.set(name, new SourceTextModule(await readFile(url, "utf8"), {
      context, identifier: url.href, initializeImportMeta(meta) { meta.url = url.href; },
    }));
  }
  const main = modules.get("main.js");
  await main.link(specifier => {
    if (specifier === "./audio-host.mjs") return audioModule;
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
      notes: 6, samples: sampleCount, lanes: [0x11],
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
  h.click("replay-play");
  assert.equal(h.opens.length, 1);
  assert.equal(h.opens[0].gesture, true);
  assert.equal(file.reads, 0);
  await flush();
  const worker = h.workers[0];
  const start = worker.last("play-start");
  assert.equal(start.mode, "replay");
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
    await h.reply(start, { kind: "prepared", mode, title: "Wrong mode", notes: 1, samples: 1, lanes: [0x11] });
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

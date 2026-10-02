// Deferred: node --experimental-vm-modules --test samples/bms-runtime/web/play-host.test.mjs
// Actual main.js and numeric helpers; controlled DOM/Worker/AudioHost endpoints.
// No browser, audio device, generated binding or WASM instance is used.
import assert from "node:assert/strict";
import { File } from "node:buffer";
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

async function harness(faults = {}) {
  const elements = new Map();
  const workers = [];
  const traces = [];
  const unexpected = [];
  const timers = new Map();
  const opens = [];
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
    "title", "details", "status", "viewport", "play", "stop", "keys", "canvas", "prepare-form", "seek-form"]) {
    elements.set(id, new Element(id === "chart" ? "select" : id, id));
  }
  elements.get("folder").webkitdirectory = true;
  elements.get("rate").value = "44100";
  elements.get("seed").value = "7";

  const document = new Events();
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
      this.posts.push({ value: structuredClone(value), transferCount: transfer.length });
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

  const audio = {
    sampleRate: 48000,
    samples: [], commandsSeen: [], arms: [], polls: 0, finishes: 0,
    stopCalls: 0, stopStarts: 0, stopping: null,
    get currentFrame() { return BigInt(Math.floor(now * 48)); },
    controlClock() { return { beforeMs: now, afterMs: now, contextTime: now / 1000, sampleRate: 48000 }; },
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
      return { admitted: commands.length };
    },
    async poll() {
      this.polls++;
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
  };
  class AudioHost {
    static open(options) {
      opens.push({ options, gesture });
      traces.push(["open", gesture]);
      return faults.openGate?.promise ?? Promise.resolve(audio);
    }
  }
  const moduleToken = {};
  const context = createContext({
    document, window, Worker, ResizeObserver, Option,
    AbortController, AbortSignal, URL, TextEncoder, File, Uint8Array, Uint32Array, Float32Array,
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
  async function begin() {
    click("play");
    await flush();
    return workers.at(-1).last("play-start");
  }
  async function prepared(start, sampleCount = 0) {
    const worker = workers.at(-1);
    await reply(start, { kind: "prepared", title: "Actual runtime", artist: "Runtime artist",
      notes: 6, samples: sampleCount, lanes: [0x11] });
    for (let index = 0; index < sampleCount; index++) {
      await reply(worker.last("play-sample"), { kind: "sample", id: BigInt(index + 1), rate: 44100,
        channels: 2, pcm: new Float32Array([0.25, -0.25, 0.5, -0.5]) });
    }
    await reply(worker.last("play-sample"), { kind: "samples-end" });
    return worker.last("play-commands");
  }
  async function launch(sampleCount = 0) {
    const worker = workers.at(-1);
    const start = await begin();
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
  return { get, workers, audio, opens, traces, faults, timers, moduleToken, window, document,
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
  await h.receive({ kind: "play-render-done", playId: session.id, renderId: report.renderId });
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
    await h.receive(finalScore(playId, { kind: "play-error", message: "Worker retained exact rejected batch", hits: 2n }));
    assert.match(h.get("status").textContent, /original audio admission failure/);
    assert.doesNotMatch(h.get("status").textContent, /secondary Worker rejection/);
    assert.match(h.get("status").textContent, /Hits 2/);
    await h.close();
  }
});

// Deferred: node --experimental-vm-modules --test app/web/play-host.test.mjs
// Actual main.js and numeric helpers; controlled DOM/Worker/AudioHost endpoints.
// No browser, audio device, generated binding or WASM instance is used.
import assert from "node:assert/strict";
import { Blob, File } from "node:buffer";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";
import { BMS_TIMING_PRESET_ID } from "./play-model.mjs";

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
function selectedControllerProfile(size = 128) {
  const file = new File(["{\"version\":1}"], "controller-profile.json");
  Object.defineProperty(file, "size", { value: size });
  let reads = 0;
  file.arrayBuffer = () => { reads++; throw new Error("Window must not read or interpret HID profiles"); };
  return { file, get reads() { return reads; } };
}
function chooseControllerProfile(h, file) {
  h.get("hid-profile").files = [file]; h.get("hid-profile").emit("change");
}
function selectedGamepadProfile(size = 128) {
  const file = new File(["{\"version\":1}"], "nonstandard-gamepad.json");
  Object.defineProperty(file, "size", { value: size });
  let reads = 0;
  file.arrayBuffer = () => { reads++; throw new Error("Window must not read or interpret Gamepad profiles"); };
  return { file, get reads() { return reads; } };
}
function chooseGamepadProfile(h, file) {
  h.get("gamepad-profile").files = [file]; h.get("gamepad-profile").emit("change");
}
function nativeGamepad(index = 0, fields = {}) {
  return { index, id: "standard gamepad", mapping: "standard", connected: true, timestamp: 1000,
    axes: [0.12345678901234568],
    buttons: Array.from({ length: 9 }, () => ({ value: 0, pressed: false, touched: false })), ...fields };
}
function watchPlayDisplay(h) {
  const writes = [];
  for (const [id, property] of [["status", "textContent"], ["position", "value"],
    ["title", "textContent"], ["details", "textContent"], ["keys", "textContent"]]) {
    const element = h.get(id);
    let value = element[property];
    Object.defineProperty(element, property, { configurable: true, get() { return value; },
      set(next) { writes.push({ id, property, value: next }); value = next; } });
  }
  return writes;
}
function savedRecord(fields = {}) {
  return { id: 41, name: "saved-prefix.bkr", chartPath: "Songs/曲/chart.bms", complete: false,
    hits: 18446744073709551615n, misses: 2n, combo: null, createdAt: 1234567890, byteLength: 4, ...fields };
}

function portableSettings() {
  return { kind: "beatkernel-browser-settings", version: 1,
    timing: { earlyMs: "12.345678", lateMs: "87.654321", offsetMs: "-12.500001" },
    output: { latency: "custom", latencyMs: "10.000001", rate: "44100" },
    capacities: { queueCapacity: "257", maxVoices: "17", pendingCapacity: "31", maxFrames: "257", maxCommandsPerRender: "7" },
    section: { startSeconds: "1.000000001", endSeconds: "2.000000002" },
    bindings: [[17, "KeyA"], [18, ""], [19, "KeyX"], [20, "KeyD"], [21, "KeyC"],
      [22, "ShiftLeft"], [23, "Space"], [24, "KeyF"], [25, "KeyV"], [33, "KeyN"],
      [34, "KeyJ"], [35, "KeyM"], [36, "KeyK"], [37, "Comma"], [38, "ShiftRight"],
      [39, "Slash"], [40, "KeyL"], [41, "Period"]] };
}
const settingsFields = [
  ["timing", "earlyMs", "judge-early"], ["timing", "lateMs", "judge-late"], ["timing", "offsetMs", "judge-offset"],
  ["output", "latency", "output-latency"], ["output", "latencyMs", "output-latency-ms"], ["output", "rate", "output-rate"],
  ["capacities", "queueCapacity", "audio-queue"], ["capacities", "maxVoices", "audio-voices"],
  ["capacities", "pendingCapacity", "audio-pending"], ["capacities", "maxFrames", "audio-frames"],
  ["capacities", "maxCommandsPerRender", "audio-commands"],
  ["section", "startSeconds", "live-start"], ["section", "endSeconds", "live-end"],
];
function settingsDraft(h) {
  return [...settingsFields.map(([, , id]) => [id, h.get(id).value]),
    ...portableSettings().bindings.map(([lane]) => [`binding-${lane.toString(16)}`, h.get(`binding-${lane.toString(16)}`).value])];
}
function dispatchKeyboard(h, type, event) {
  // Events.emit spreads its fields; call the actual registered handler so native
  // accessor side effects happen inside acquisition, not in the DOM fixture.
  const handlers = [...(h.window.listeners.get(type) ?? [])];
  assert.equal(handlers.length, 1);
  handlers[0].call(h.window, event);
}
function observedKeyboard(values = {}, effects = {}) {
  const fields = { code: "KeyZ", repeat: false, timeStamp: 1300.125, ...values };
  const reads = { code: 0, repeat: 0, timeStamp: 0, preventDefault: 0 };
  let calls = 0;
  const event = {};
  for (const field of ["code", "repeat", "timeStamp"]) Object.defineProperty(event, field, { get() {
    assert.equal(++reads[field], 1, `${field} is acquired once`);
    const value = fields[field]; effects[field]?.(); return value;
  } });
  Object.defineProperty(event, "preventDefault", { get() {
    assert.equal(++reads.preventDefault, 1);
    effects.preventDefault?.();
    return function () { assert.equal(this, event); assert.equal(++calls, 1); effects.call?.(); };
  } });
  Object.defineProperty(event, "key", { get() { assert.fail("physical binding does not read the layout-dependent key"); } });
  return { event, fields, reads, get calls() { return calls; } };
}
function chooseSettings(h, size = 128) {
  const file = new File(["settings bytes remain Worker-owned"], "portable-settings.json");
  Object.defineProperty(file, "size", { value: size });
  file.arrayBuffer = () => { assert.fail("Window must not read the selected settings File"); };
  h.get("settings-load").files = [file]; h.get("settings-load").emit("change");
  return file;
}

async function harness(faults = {}) {
  const elements = new Map();
  const workers = [];
  const renderers = [];
  const allWorkers = [];
  const channels = [];
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
  const resizeObservers = [];
  const captures = [];
  const releases = [];
  let viewport = { width: 960, height: 720 };
  let layoutReads = 0;
  let now = 1000;
  let nextTimer = 0;
  let gesture = false;
  let gamepadReads = 0;

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
      this.capturedPointers = new Set();
      if (faults.missingPointerCapture) this.setPointerCapture = undefined;
      if (faults.missingPointerRelease) this.releasePointerCapture = undefined;
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
    getBoundingClientRect() { layoutReads++; return { left: 0, top: 0, ...viewport }; }
    get options() { return this.tagName === "select" ? this.children.filter(child => child.tagName === "option") : undefined; }
    setPointerCapture(id) {
      if (faults.captureFailure) throw faults.captureFailure;
      this.capturedPointers.add(id);
      captures.push({ surface: this, id });
      traces.push(["capture", id]);
    }
    releasePointerCapture(id) {
      this.capturedPointers.delete(id);
      releases.push({ surface: this, id });
      traces.push(["release", id]);
      if (faults.pointerReleaseError) throw faults.pointerReleaseError;
      // Exercise synchronous native notification too: ownership must be gone
      // before this can attempt to create another cancellation.
      this.emit("lostpointercapture", { pointerId: id, timeStamp: now });
    }
    transferControlToOffscreen() {
      assert.equal(this.transferred, undefined, "a transferred HTML canvas cannot be transferred twice");
      this.transferred = { surface: this.id, width: this.width, height: this.height };
      return this.transferred;
    }
    focus() { this.focuses++; }
  }
  class HidDevice extends Events {
    constructor(index, metadata) {
      super(); Object.assign(this, metadata);
      this.index = index; this.opened = false; this.opens = 0; this.closes = 0;
    }
    async open() {
      this.opens++; traces.push(["hid-open", this.index]);
      if (faults.hidOpenGate) await faults.hidOpenGate.promise;
      this.opened = true;
    }
    async close() {
      this.closes++; traces.push(["hid-close", this.index]);
      if (faults.hidCloseGate) await faults.hidCloseGate.promise;
      if (faults.hidCloseError) throw faults.hidCloseError;
      this.opened = false;
    }
  }
  const hidDevices = (faults.hidDescriptors ?? [{ vendorId: 1, productId: 2 }, { vendorId: 9, productId: 9 }])
    .map((metadata, index) => new HidDevice(index, metadata));
  const hid = new Events();
  hid.gets = 0; hid.requests = [];
  hid.getDevices = () => {
    hid.gets++; traces.push(["hid-discover"]);
    return faults.hidConnectGate?.promise ?? Promise.resolve(hidDevices);
  };
  hid.requestDevice = options => {
    hid.requests.push({ options, gesture }); traces.push(["hid-authorize", gesture]);
    return faults.hidAuthorizeGate?.promise ?? Promise.resolve(hidDevices);
  };
  class Option extends Element {
    constructor(text, value) { super("option"); this.textContent = text; this.value = value; }
  }
  for (const id of ["folder", "files", "chart", "rate", "seed", "prepare", "position", "seek",
    "title", "details", "status", "viewport", "play", "stop", "record", "export", "keys", "canvas", "prepare-form", "seek-form",
    "replay-file", "replay-play", "replay-name", "records", "records-refresh", "records-save", "records-use", "records-delete",
    "multiplayer", "multiplayer-mode", "multiplayer-url", "multiplayer-role", "multiplayer-status",
    "room-seal", "room-ready", "room-leave", "room-score-prev", "room-score-next", "room-score-page",
    "opponents-kind", "opponents-label", "opponents-add", "records-opponent", "opponents-clear",
    "opponents-list", "opponents-status", "opponents-results", "judge-early", "judge-late", "judge-offset", "judge-preset", "judge-precedence", "judge-gauge", "live-start", "live-end",
    "bindings", "bindings-reset", "settings-save", "settings-load", "settings-status", "output-latency", "output-latency-ms", "output-rate",
    "audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands", "touch-input", "pointer-input", "pointer-bindings",
    "hid-input", "hid-authorize", "hid-profile", "hid-profile-name", "hid-status",
    "gamepad-profile", "gamepad-profile-name", "gamepad-profile-clear",
    "local-count", "local-discover", "local-release", "local-sources", "local-status", "local-page", "local-results", "captured-replay",
    "historical-grade-prev", "historical-grade-next", "historical-grade-page",
    "menu-open", "menu-editor", "menu-back"]) {
    elements.set(id, new Element(["chart", "records", "local-page", "captured-replay", "multiplayer-mode"].includes(id) ? "select" : id, id));
  }
  elements.get("folder").webkitdirectory = true;
  elements.get("rate").value = "44100";
  elements.get("seed").value = "7";
  elements.get("multiplayer-role").value = "join";
  elements.get("multiplayer-mode").value = "peer";
  elements.get("opponents-kind").value = "own";
  elements.get("judge-early").value = "50";
  elements.get("judge-late").value = "50";
  elements.get("judge-offset").value = "0";
  elements.get("judge-preset").value = "";
  elements.get("judge-precedence").value = "rank-first";
  elements.get("judge-gauge").value = "beatkernel";
  elements.get("live-start").value = "0";
  elements.get("live-end").value = "";
  elements.get("local-count").value = "1";
  elements.get("output-latency").value = "interactive";
  elements.get("output-latency-ms").value = "10";
  elements.get("output-rate").value = "";
  for (const id of ["audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands"]) {
    elements.get(id).value = "4096";
  }

  const document = new Events();
  document.body = new Element("body");
  document.hidden = false;
  document.getElementById = id => {
    const find = node => node.id === id ? node : node.children?.map(find).find(Boolean);
    return elements.get(id) ?? [...elements.values()].map(find).find(Boolean);
  };
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
      this.geometryVersion = 0n;
      this.role = /renderer-worker\.js(?:$|\?)/.test(this.url) ? "renderer" : "game";
      allWorkers.push(this);
      (this.role === "renderer" ? renderers : workers).push(this);
    }
    postMessage(value, transfer = []) {
      if (value.kind === "menu-motion") (this.motionArguments ??= []).push(value);
      traces.push(["post", value.kind]);
      if (value.kind === this.failKind) throw new Error("injected Worker post failure");
      assert.equal(this.terminations, 0, "posting after Worker termination");
      // The fake canvas has no native transferable. Other data follows the real
      // structured-clone shape, including BigInt and typed event arrays.
      const portKey = value.renderPort ? "renderPort" : value.port ? "port" : null;
      const portHandoff = portKey !== null;
      const posted = portHandoff ? { ...structuredClone({ ...value, [portKey]: undefined, canvas: undefined }),
        [portKey]: value[portKey], ...(value.canvas ? { canvas: value.canvas } : {}) }
        : structuredClone(value);
      if (portHandoff) {
        assert.ok(transfer.includes(value[portKey]), "native channel endpoint is transferred");
        value[portKey].transfers++;
        if (value.canvas) assert.ok(transfer.includes(value.canvas), "only renderer receives the native canvas transfer");
      }
      // Node versions may clone File as Blob. Preserve the selected immutable
      // File endpoint here; this fake Worker never acquires its bytes.
      if (value.replayFile instanceof File) posted.replayFile = value.replayFile;
      if (value.hidProfileFile instanceof File) posted.hidProfileFile = value.hidProfileFile;
      if (value.gamepadProfileFile instanceof File) posted.gamepadProfileFile = value.gamepadProfileFile;
      if (value.kind === "settings-profile-load" && value.file instanceof File) posted.file = value.file;
      if (Array.isArray(value.opponents)) posted.opponents = value.opponents.map((entry, index) => ({
        ...posted.opponents[index], file: entry.file,
      }));
      this.posts.push({ value: posted, transferCount: transfer.length, transfer: [...transfer] });
      if (this.role === "game" && ["resize", "play-page"].includes(value.kind)) this.geometryVersion++;
      if (value.kind === "dispose" && !faults.holdDispose) queueMicrotask(() => this.emit("message", { data: { kind: "disposed" } }));
      // Mechanical legacy compatibility: the actual Worker acknowledges clear
      // with unavailable metadata. Page RPC replies remain explicitly scripted.
      if (value.kind === "historical-record-clear") queueMicrotask(() => this.emit("message", { data: {
        kind: "historical-record-result", id: value.id, available: false, error: null, gradePage: null, gradePages: 0,
      } }));
    }
    terminate() { this.terminations++; traces.push(["terminate"]); }
    messages(kind) { return this.posts.map(entry => entry.value).filter(value => value.kind === kind); }
    last(kind) { return this.messages(kind).at(-1); }
  }
  class MessageChannel {
    constructor() {
      class Port extends Events {
        constructor() { super(); this.transfers = 0; this.closes = 0; this.posts = []; }
        start() {}
        postMessage(value) {
          this.posts.push(structuredClone(value));
          queueMicrotask(() => this.peer.emit("message", { data: structuredClone(value) }));
        }
        close() { this.closes++; }
      }
      this.port1 = new Port(); this.port2 = new Port();
      this.port1.peer = this.port2; this.port2.peer = this.port1;
      channels.push(this);
    }
  }
  class ResizeObserver {
    constructor(callback) { this.callback = callback; resizeObservers.push(this); }
    observe() {}
    disconnect() { traces.push(["observer-disconnect"]); }
  }
  const window = new Events();
  Object.assign(window, { isSecureContext: true, devicePixelRatio: 1, Worker, MessageChannel,
    OffscreenCanvas: class {}, ResizeObserver, matchMedia: () => new Events() });
  if (faults.touchSupported || faults.pointerSupported) window.PointerEvent = class {};
  const addWindowListener = window.addEventListener.bind(window);
  window.addEventListener = (kind, listener) => {
    addWindowListener(kind, listener);
    if (faults.gamepadListenerError && kind === "gamepaddisconnected") throw faults.gamepadListenerError;
  };
  const removeWindowListener = window.removeEventListener.bind(window);
  window.removeEventListener = (kind, listener) => {
    removeWindowListener(kind, listener);
    if (faults.gamepadCleanupError && ["gamepadconnected", "gamepaddisconnected"].includes(kind)) throw faults.gamepadCleanupError;
  };

  function createAudio() { return {
    sampleRate: faults.actualRate ?? 48000,
    samples: [], commandsSeen: [], arms: [], polls: 0, finishes: 0, finishArgs: [], outputReads: 0, frameReads: 0,
    stopCalls: 0, stopStarts: 0, stopping: null,
    commandPorts: [], attachments: 0, samplePorts: [], sampleAttachments: 0, configuration: null,
    get channels() { return this.configuration.channels; },
    get currentFrame() {
      this.frameReads++;
      if (faults.frameFailure) throw faults.frameFailure;
      if (Object.hasOwn(faults, "contextFrame")) return faults.contextFrame;
      const frames = faults.actualRate === undefined ? now * 48 : now * faults.actualRate / 1000;
      return BigInt(Math.floor(frames));
    },
    controlClock() { return faults.controlClock ?? { beforeMs: now, afterMs: now, contextTime: now / 1000, sampleRate: faults.actualRate ?? 48000 }; },
    outputTimestamp() {
      this.outputReads++;
      traces.push(["output-timestamp"]);
      if (faults.outputFailure) throw faults.outputFailure;
      if (faults.outputObject) return faults.outputObject;
      if (faults.outputEvidence) return { ...faults.outputEvidence };
      throw Object.assign(new Error("output evidence is not available"), { code: "unavailable" });
    },
    async sample(value) {
      assert.fail("Window must never acquire or relay a PCM sample");
    },
    async openSamplePort() {
      this.sampleAttachments++; traces.push(["open-sample-port"]);
      if (faults.samplePortFailure) throw faults.samplePortFailure;
      const port = { closes: 0, transfers: 0, start() {}, postMessage() { assert.fail("Window must not send PCM"); },
        close() { this.closes++; } };
      this.samplePorts.push(port);
      const descriptor = { port, generation: this.configuration.generation, channels: this.configuration.channels,
        pcmLimits: { ...this.configuration.pcmLimits }, timeoutMs: this.configuration.timeoutMs };
      if (faults.samplePortGate) await faults.samplePortGate.promise;
      return faults.sampleDescriptor ? faults.sampleDescriptor(descriptor) : descriptor;
    },
    async finish(...args) {
      this.finishes++; this.finishArgs.push(args); traces.push(["finish", ...args]);
      if (faults.finishFailure) throw faults.finishFailure;
    },
    async arm(frame) { this.arms.push(frame); traces.push(["arm", frame]); },
    commands() { assert.fail("Window must never copy commands or relay their per-batch ACK"); },
    async openCommandPort() {
      this.attachments++; traces.push(["open-command-port"]);
      if (faults.commandPortFailure) throw faults.commandPortFailure;
      const port = { closes: 0, transfers: 0, start() {}, postMessage() { assert.fail("Window must not post audio commands"); },
        close() { this.closes++; } };
      this.commandPorts.push(port);
      const descriptor = { port, generation: this.configuration.generation,
        queueCapacity: this.configuration.audioLimits.queueCapacity, timeoutMs: 50 };
      if (faults.commandPortGate) await faults.commandPortGate.promise;
      return faults.commandDescriptor ? faults.commandDescriptor(descriptor) : descriptor;
    },
    poll() { this.polls++; assert.fail("Window must not poll or relay Worklet report words"); },
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
      audio.configuration = options;
      if (faults.missingCommandPort) audio.openCommandPort = undefined;
      if (faults.missingSamplePort) audio.openSamplePort = undefined;
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
  const browserNavigator = { maxTouchPoints: faults.touchPoints ?? (faults.touchSupported ? 2 : 0), ...(faults.hidSupported ? { hid } : {}) };
  if (Object.hasOwn(faults, "gamepads")) browserNavigator.getGamepads = function () {
    assert.equal(this, browserNavigator);
    gamepadReads++; traces.push(["gamepad-poll"]);
    faults.onGamepadPoll?.(gamepadReads);
    if (faults.gamepadReadError) throw faults.gamepadReadError;
    return faults.gamepads;
  };
  const context = createContext({
    document, window, Worker, MessageChannel, ResizeObserver, Option,
    navigator: browserNavigator,
    AbortController, AbortSignal, URL: ControlledURL, Blob, TextEncoder, TextDecoder, File, Uint8Array, Uint32Array, Float32Array, DataView,
    ArrayBuffer, structuredClone, performance: { timeOrigin: 9000, now: () => now },
    JSON: faults.noWindowSettingsJson ? {
      parse() { assert.fail("Window must not parse settings JSON"); },
      stringify() { assert.fail("Window must not encode settings JSON"); },
    } : JSON,
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
  for (const name of ["render-protocol.mjs", "presentation-status.mjs", "completed-results-model.mjs", "host_model.mjs", "play-model.mjs", "settings-profile.mjs", "saved-opponents.mjs", "hid-input.mjs", "hid-profile.mjs", "gamepad-input.mjs", "pointer-input.mjs", "local-play-host.mjs", "main.js"]) {
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
  await flush();
  const get = id => document.getElementById(id);
  function click(id) {
    const element = get(id);
    if (element.disabled) return;
    gesture = true;
    try { element.emit("click"); } finally { gesture = false; }
  }
  async function receive(value, target = workers.at(-1)) {
    if (value.kind === "play-render-done" || value.kind === "play-step-done") {
      value = { commandsPending: false,
        ...(value.kind === "play-render-done" ? { observedTick: target.last("play-step")?.tickId ?? 0 } : {}), ...value };
    }
    target.emit("message", { data: structuredClone(value) });
    await flush();
  }
  async function reply(request, result, echoInputMode = true) {
    assert.ok(request, "expected an actual setup request");
    if (request.kind === "play-audio" && result === null) result = { kind: "audio-ready", commandsPending: false };
    // Normal controlled setup replies echo the admitted route. Negative route
    // fixtures can explicitly retain missing metadata with echoInputMode=false.
    if (echoInputMode && request.kind === "play-start" && ["physical", "physical-contact"].includes(request.inputMode)
      && result?.kind === "prepared" && !Object.hasOwn(result, "inputMode")) result = { ...result, inputMode: request.inputMode };
    await receive({ kind: "play-reply", playId: request.playId, rpcId: request.rpcId, result });
  }
  async function admitSamples(request, count) {
    assert.equal(request?.kind, "play-samples-upload");
    await receive({ kind: "play-samples-admitted", playId: request.playId, rpcId: request.rpcId, count });
  }
  async function preview() {
    await flush();
    const worker = workers.at(-1);
    await receive({ kind: "ready" });
    if (!faults.holdRendererReady) await receive({ kind: "ready" }, renderers.at(-1));
    get("files").files = [new File(["#BPM 120"], "chart.bms")];
    get("files").emit("change");
    const imported = worker.last("import");
    await receive({ kind: "catalog", id: imported.id, charts: ["chart.bms"] });
    get("prepare-form").emit("submit");
    const selected = worker.last("select");
    await receive({ kind: "selected", id: selected.id, libraryId: imported.id, path: "chart.bms",
      title: "Accepted preview", artist: "Preview artist", notes: 6, samples: 1, images: 0, duration: "6000000000" });
    await receive({ kind: "drawn", selectedId: selected.id, generation: 1n, content: 1n });
    if (!faults.holdGeometry) await geometry({ selectedId: selected.id });
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
  async function prepared(start, sampleCount = 0, completeUpload = true) {
    const worker = workers.at(-1);
    const localPlayers = start.localPlanWords === undefined ? null
      : Array.from(start.localPlanWords).filter((_, index) => index % 4 === 0);
    const localSources = start.localPlanWords === undefined
      || (start.localPlanWords.length === 4 && start.localPlanWords[1] === 0) ? null
      : Array.from({ length: start.localPlanWords.length / 4 }, (_, index) =>
        BigInt(start.localPlanWords[index * 4 + 2]) | BigInt(start.localPlanWords[index * 4 + 3]) << 32n);
    await reply(start, { kind: "prepared", title: "Actual runtime", artist: "Runtime artist",
      notes: 6, samples: sampleCount, lanes: [0x11], opponentCount: start.opponents?.length ?? 0,
      startNs: start.mode === "replay" ? faults.replayStart ?? 0n : start.startNs ?? 0n,
      ...(start.hidProfileFile ? { hidSources: faults.hidAdmittedSources ?? start.hidDevices.map(device => device.source),
        hidSourceCount: (faults.hidAdmittedSources ?? start.hidDevices).length } : {}),
      ...(start.gamepadDevices !== undefined ? { gamepadSources: faults.gamepadAdmittedSources
        ?? start.gamepadDevices.filter(device => device.mapping === "standard" && device.buttons >= 9
          && (localSources === null || localSources.includes(device.source))).map(device => device.source) } : {}),
      ...(start.pointerSetup === undefined ? {} : { pointerDevices: faults.pointerAdmittedDevices ?? start.pointerSetup.devices }),
      ...(localPlayers === null ? {} : { localPlayers, localPage: start.localPage ?? 0,
        ...(start.recordReplay ? { recordLimits: { bytes: Math.floor(64 * 1024 * 1024 / localPlayers.length),
          records: Math.floor(1000000 / localPlayers.length) } } : {}) }),
      ...(start.mode === "replay" ? { mode: "replay", recordedUntilNs: 2350000000n } : {}) });
    if (!completeUpload) return worker.last("play-samples-upload");
    await admitSamples(worker.last("play-samples-upload"), sampleCount);
    await reply(worker.last("play-samples-upload"), { kind: "samples-uploaded", count: sampleCount, bytes: sampleCount * 16 });
    return worker.last("play-audio");
  }
  async function launch(sampleCount = 0, mode = "live") {
    const worker = workers.at(-1);
    const start = await begin(mode);
    const commands = await prepared(start, sampleCount);
    await reply(commands, null);
    const activation = worker.last("play-activate");
    await reply(activation, null);
    assert.equal(get("stop").disabled, false);
    if (!faults.holdGeometry) await geometry({ playId: start.playId, page: start.localPage ?? 0 });
    return { id: start.playId, start, activation };
  }
  async function geometry(fields = {}, target = workers.at(-1)) {
    const resize = target.last("resize");
    const play = target.last("play-start");
    await receive({ kind: "render-geometry", generation: 1n, content: 1n,
      geometryVersion: fields.geometryVersion ?? target.geometryVersion,
      selectedId: target.last("select")?.id,
      mode: fields.playId === undefined ? "preview" : play?.localPlanWords ? "local" : play?.mode ?? "live",
      page: 0, width: resize?.width ?? 960, height: resize?.height ?? 720,
      ...fields, geometryVersion: fields.geometryVersion ?? target.geometryVersion }, target);
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
  return { get, workers, renderers, allWorkers, channels, get audio() { return audio; }, opens, traces, faults, timers, moduleToken, window, document, urls, revoked, downloads,
    requestMenuMotion(...args) { return main.namespace.requestMenuMotion(...args); },
    recordOpens, recordCalls, recordOwners, captures, releases, hid, hidDevices, get layoutReads() { return layoutReads; },
    get gamepadReads() { return gamepadReads; },
    resize(width, height) { viewport = { width, height }; resizeObservers.at(-1).callback(); },
    click, receive, reply, admitSamples, preview, begin, prepared, launch, geometry, advance,
    setNow(value) { assert.ok(value >= now); now = value; },
    setTimeOrigin(value) { context.performance.timeOrigin = value; },
    async close() {
      window.emit("pagehide");
      await flush();
      const game = workers.at(-1), pending = game?.last("play-stop");
      if (pending && game.terminations === 0) await receive(finalScore(pending.playId), game);
      assert.deepEqual(unexpected, []);
      assert.equal(timers.size, 0, "page teardown must clear host deadlines and intervals");
    },
  };
}

const menuMotionEndpoints = () => new Float32Array([0, 0, 1, 1, 1, 40, 10, 1.25, 0.75, 0.5]);
async function submittedMotionHost() {
  const h = await harness(); await h.preview(); h.click("menu-open"); await flush();
  await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n, route: 2, fields: ["draft"], selected: 0 });
  await h.receive({ kind: "render-geometry", mode: "menu", generation: 7n, content: 9n,
    geometryVersion: 3n, page: 0, width: 960, height: 720, menuGeneration: 77n, screen: 3n, revision: 5n });
  return h;
}
function menuMotionReply(request, fields = {}) {
  const { requestId, hostOwner, menuGeneration, screen, revision, generation, content, geometryVersion } = request;
  return { kind: "menu-motion-reply", requestId, hostOwner, menuGeneration, screen, revision,
    generation, content, geometryVersion, admitted: true, admittedGeometryVersion: 4n, ...fields };
}

test("Window explicit motion copies endpoints and correlates every owner field before settling admission", async () => {
  const h = await submittedMotionHost(); const game = h.workers[0];
  try {
    const transform = menuMotionEndpoints(), expected = [...transform];
    let settled = 0; const promise = h.requestMenuMotion(1000n, transform, 1000).then(value => { settled++; return value; });
    transform.fill(999); const request = game.last("menu-motion");
    assert.deepEqual([...game.motionArguments.at(-1).transforms], expected, "host copy precedes postMessage cloning");
    assert.equal(request.control, 1000n); assert.equal(request.geometryVersion, 3n);
    assert.equal(request.generation, 7n); assert.equal(request.content, 9n);
    for (const wrong of [{ requestId: request.requestId + 1n }, { hostOwner: request.hostOwner + 1 },
      { menuGeneration: 78n }, { screen: 4n }, { revision: 4n }, { generation: 8n }, { content: 10n }, { geometryVersion: 4n }]) {
      await h.receive(menuMotionReply(request, wrong)); assert.equal(settled, 0);
    }
    await h.receive(menuMotionReply(request)); await promise; assert.equal(settled, 1);
    await h.receive(menuMotionReply(request)); assert.equal(settled, 1);
    assert.equal(h.opens.length, 0); assert.equal(game.messages("play-step").length, 0);
    assert.equal(game.messages("resize").at(-1)?.geometryVersion > 3n, false, "Window does not allocate renderer motion counters");
  } finally { await h.close(); }
});

test("Window explicit motion validates endpoint bounds before consuming an RPC", async () => {
  const h = await submittedMotionHost(); const game = h.workers[0];
  try {
    for (const [control, values, duration, easing] of [[0n, menuMotionEndpoints(), 1000, 0],
      [1n, new Float32Array(9), 1000, 0], [1n, menuMotionEndpoints(), Infinity, 0], [1n, menuMotionEndpoints(), 1, 4],
      [1n, new Float32Array([0, 0, 0, 1, 1, 0, 0, 1, 1, 1]), 1, 0],
      [1n, new Float32Array([16777218, 0, 1, 1, 1, 0, 0, 1, 1, 1]), 1, 0]]) {
      await assert.rejects(h.requestMenuMotion(control, values, duration, easing));
      assert.equal(game.messages("menu-motion").length, 0);
    }
  } finally { await h.close(); }
});

test("Window motion deadline settles once and a late admitted reply cannot revive it", async () => {
  const h = await submittedMotionHost(); const game = h.workers[0];
  try {
    const pending = h.requestMenuMotion(1000n, menuMotionEndpoints(), 1000);
    const result = assert.rejects(pending, /timed|deadline/i); const request = game.last("menu-motion");
    await h.advance(10000); await result;
    await h.receive(menuMotionReply(request));
    const next = h.requestMenuMotion(1001n, menuMotionEndpoints(), 1000);
    await h.receive(menuMotionReply(game.last("menu-motion"))); await next;
  } finally { await h.close(); }
});

test("Window outstanding motion bound includes in-flight and independent targets settle separately", async () => {
  const h = await submittedMotionHost(); const game = h.workers[0];
  try {
    const pending = Array.from({ length: 64 }, (_, i) => h.requestMenuMotion(BigInt(1000 + i), menuMotionEndpoints(), 1000));
    const settled = Promise.allSettled(pending);
    await assert.rejects(h.requestMenuMotion(9999n, menuMotionEndpoints(), 1000), /limit|full|capacity|outstanding/i);
    assert.equal(game.messages("menu-motion").length, 64);
    const requests = game.messages("menu-motion"); assert.equal(new Set(requests.map(m => m.requestId)).size, 64);
    for (const request of requests) await h.receive(menuMotionReply(request));
    const results = await settled; assert.equal(results.filter(r => r.status === "fulfilled").length, 64);
  } finally { await h.close(); }
});

for (const transition of ["replacement", "fatal", "dispose"]) test(`Window ${transition} settles owned motion and clears its deadline`, async () => {
  const h = await submittedMotionHost(); const game = h.workers[0];
  try {
    const pending = h.requestMenuMotion(1000n, menuMotionEndpoints(), 1000); const result = assert.rejects(pending);
    const request = game.last("menu-motion");
    if (transition === "replacement") await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 4n, revision: 6n, route: 3, fields: ["0"], selected: 0 });
    else if (transition === "fatal") await h.receive({ kind: "fatal", message: "renderer failed" });
    else await h.close();
    await result; await h.receive(menuMotionReply(request));
  } finally { if (transition !== "dispose") await h.close(); }
});

async function localCount(h, count) {
  h.get("local-count").value = String(count); h.get("local-count").emit("change"); await flush();
}
function localAssign(h, player, source) {
  const field = h.get(`local-source-${player}`);
  assert.ok(field, "retained player source selector");
  field.value = String(source); field.emit("change");
}
function localFinal(start, overrides = {}) {
  const players = Array.from(start.localPlanWords).filter((_, index) => index % 4 === 0);
  return finalScore(start.playId, { primaryPlayer: players[0], hits: BigInt(players[0]), misses: 0n, combo: 0n,
    maxCombo: BigInt(players[0]), replay: null, replayError: null, replayComplete: false,
    localScores: players.map(player => ({ player, songNs: 2350000000n, hits: BigInt(player), misses: 0n, combo: 0n, maxCombo: BigInt(player) })),
    replays: players.map(player => ({ player, replay: null, replayError: null, replayComplete: false })), ...overrides });
}

async function pagedTouchSession() {
  const h = await harness({ touchSupported: true, gamepads: [nativeGamepad(0), nativeGamepad(1), nativeGamepad(2)] });
  await h.preview(); await localCount(h, 5); h.click("local-discover"); await flush();
  const sources = h.get("local-source-1").children.filter(option => option.textContent.startsWith("Gamepad ")).map(option => BigInt(option.value));
  assert.equal(sources.length, 3);
  localAssign(h, 1, 1n); localAssign(h, 2, 2n);
  sources.forEach((source, index) => localAssign(h, index + 3, source));
  const session = await h.launch();
  h.setNow(1300);
  return { h, session, worker: h.workers[0], surface: h.get("canvas") };
}

test("Window menu editor and Back bridge preserve exact owner token without audio or gameplay effects", async () => {
  const h = await harness(); await h.preview(); const game = h.workers[0];
  h.click("menu-open"); await flush();
  assert.deepEqual(structuredClone(game.last("menu-open")), { kind: "menu-open", fields: ["chart.bms"],
    roster: { players: [1], nextPlayerId: 2, assignments: [] }, opponents: [] });
  const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 3, fields: ["1.000000001", "2.000000002"], selected: 0 };
  await h.receive(state);
  h.get("menu-editor").value = "604800.000000001"; h.get("menu-editor").emit("input");
  assert.deepEqual(game.last("menu-edit"), { kind: "menu-edit", menuGeneration: 77n,
    screen: 3n, revision: 5n, index: 0, value: "604800.000000001" });
  h.click("menu-back"); await flush();
  assert.equal(game.messages("menu-action").length, 0, "Back waits for the pending edit transaction");
  await h.receive({ ...state, revision: 6n, fields: ["604800.000000001", "2.000000002"] });
  const back = game.last("menu-action");
  assert.equal(back.menuGeneration, 77n); assert.equal(back.screen, 3n); assert.equal(back.revision, 6n);
  assert.equal(back.control, 72n); assert.equal(typeof back.actionId, "bigint"); assert.ok(back.actionId > 0n);
  assert.equal(game.messages("play-start").length, 0); assert.equal(h.opens.length, 0);
  await h.receive({ ...state, revision: 4n, fields: ["stale overwritten field"] });
  assert.equal(h.get("menu-editor").value, "604800.000000001", "old state cannot replace a committed editor acquisition");
  await h.close();
});

test("menu editing during active gameplay cannot turn keyboard acquisition into UI business actions", async () => {
  const h = await harness({ gamepads: [nativeGamepad()] }); await h.preview();
  const session = await h.launch(), game = h.workers[0]; h.setNow(1301);
  await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 3, fields: ["0", ""], selected: 0 });
  const edits = game.messages("menu-edit").length, actions = game.messages("menu-action").length;
  h.get("menu-editor").value = "different song offset"; h.get("menu-editor").emit("input");
  h.click("menu-open"); h.click("menu-back");
  const reads = h.gamepadReads;
  dispatchKeyboard(h, "keydown", observedKeyboard({ timeStamp: 1300.125 }).event);
  assert.equal(h.gamepadReads, reads);
  assert.equal(game.messages("menu-edit").length, edits); assert.equal(game.messages("menu-action").length, actions);
  const key = game.last("play-step").events.find(event => event.key === 2);
  assert.equal(key.hostNs, 1300125000n); assert.equal(key.down, true);
  h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
});

test("Window menu clicks use only submitted menu geometry and preserve original resize acquisition", async () => {
  const h = await harness(); await h.preview(); const game = h.workers[0];
  h.click("menu-open"); await flush();
  const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 2, fields: ["retained setting"], selected: 0 };
  await h.receive(state); const surface = h.get("canvas");
  surface.emit("click", { clientX: 120.25, clientY: 180.5 });
  assert.equal(game.messages("menu-input").length, 0, "model update alone cannot establish a visible hit target");
  const evidence = { kind: "render-geometry", mode: "menu", generation: 7n, content: 9n,
    geometryVersion: 3n, page: 0, width: 960, height: 720,
    menuGeneration: 77n, screen: 3n, revision: 5n };
  await h.receive({ ...evidence, revision: 4n });
  surface.emit("click", { clientX: 120.25, clientY: 180.5 });
  assert.equal(game.messages("menu-input").length, 0);
  await h.receive(evidence);
  h.resize(480, 360); const reads = h.gamepadReads;
  for (const [clientX, clientY] of [[NaN, 180.5], [120.25, Infinity], [undefined, 180.5], [120.25, undefined]]) {
    surface.emit("click", { clientX, clientY });
    assert.equal(game.messages("menu-input").length, 0, "malformed acquired coordinates cannot enter menu semantics");
  }
  surface.emit("click", { clientX: 120.25, clientY: 180.5 });
  const input = game.last("menu-input"); assert.ok(input);
  assert.equal(input.menuGeneration, 77n); assert.equal(input.screen, 3n); assert.equal(input.revision, 5n);
  assert.equal(input.geometryVersion, 3n); assert.equal(input.x, 240.5); assert.equal(input.y, 361);
  assert.equal(h.gamepadReads, reads); assert.equal(h.opens.length, 0);
  assert.equal(game.messages("play-step").length, 0);
  await h.close();
});

test("menu IME bridge commits text once and ignores composition and retired owner input", async () => {
  const h = await harness(); await h.preview(); const game = h.workers[0]; h.click("menu-open"); await flush();
  await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 3, fields: [""], selected: 0 });
  const editor = h.get("menu-editor");
  editor.emit("compositionstart"); editor.value = "きょ"; editor.emit("input", { isComposing: true });
  assert.equal(game.messages("menu-edit").length, 0);
  editor.value = "曲🎵"; editor.emit("compositionend"); editor.emit("input", { isComposing: false });
  assert.equal(game.messages("menu-edit").length, 1);
  assert.equal(game.last("menu-edit").value, "曲🎵");
  h.window.emit("pagehide"); await flush(); const count = game.messages("menu-edit").length;
  editor.value = "obsolete"; editor.emit("input", { isComposing: false });
  assert.equal(game.messages("menu-edit").length, count);
  await h.close();
});

test("compositionend alone commits the final editor value and Enter waits for its correlated state", async () => {
  const h = await harness();
  try {
    await h.preview(); const game = h.workers[0];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 3, fields: ["0", "2"], selected: 1 };
    await h.receive(state);
    const editor = h.get("menu-editor");
    editor.emit("compositionstart"); editor.value = "7";
    editor.emit("input", { isComposing: true }); editor.emit("input", { isComposing: false });
    editor.emit("keydown", { code: "Enter", isComposing: true });
    assert.equal(game.messages("menu-edit").length, 0, "composition ownership also suppresses an intermediate input whose flag is false");
    assert.equal(game.messages("menu-action").length, 0);
    editor.value = "73"; editor.emit("compositionend");
    assert.deepEqual(game.last("menu-edit"), { kind: "menu-edit", menuGeneration: 77n,
      screen: 3n, revision: 5n, index: 1, value: "73" });
    editor.emit("keydown", { code: "Enter", isComposing: false });
    assert.equal(game.messages("menu-action").length, 0, "Apply cannot overtake the final edit");
    await h.receive({ ...state, revision: 4n, fields: ["0", "73"] });
    await h.receive({ ...state, fields: ["0", "2"] });
    assert.equal(game.messages("menu-action").length, 0, "stale or uncommitted state cannot release Apply");
    await h.receive({ ...state, revision: 6n, fields: ["0", "73"] });
    const action = game.last("menu-action");
    assert.equal(game.messages("menu-action").length, 1);
    assert.equal(action.menuGeneration, 77n); assert.equal(action.screen, 3n);
    assert.equal(action.revision, 6n); assert.equal(action.control, 71n);
    assert.equal(game.messages("play-start").length, 0); assert.equal(h.opens.length, 0);
  } finally { await h.close(); }
});

test("correlated Settings Apply changes all thirteen scalar controls and preserves every keyboard binding", async () => {
  const h = await harness();
  try {
    await h.preview(); const game = h.workers[0];
    const lanes = [...Array.from({ length: 9 }, (_, index) => 0x11 + index),
      ...Array.from({ length: 9 }, (_, index) => 0x21 + index)];
    const bindingValues = () => lanes.map(lane => [lane, h.get(`binding-${lane.toString(16)}`).value]);
    h.get("binding-11").value = "KeyA"; h.get("binding-12").value = "";
    const bindings = bindingValues(); assert.equal(bindings.length, 18);
    const ids = ["judge-early", "judge-late", "judge-offset", "output-latency", "output-latency-ms", "output-rate",
      "audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands", "live-start", "live-end"];
    const fields = ["73", "12.345678", "-0.000001", "balanced", "10.000001", "044100",
      "65536", "4096", "4096", "4096", "65536", "1.000000001", "2.000000002"];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 6n,
      route: 2, fields, selected: 0 };
    await h.receive(state);
    const oldScalars = ids.map(id => h.get(id).value);
    const effect = { ...state, kind: "menu-effect", effect: 1n, control: 10n };
    for (const wrong of [{ menuGeneration: 78n }, { screen: 4n }, { revision: 5n }]) {
      await h.receive({ ...effect, ...wrong });
      assert.deepEqual(ids.map(id => h.get(id).value), oldScalars);
      assert.deepEqual(bindingValues(), bindings);
    }
    await h.receive(effect);
    assert.deepEqual(ids.map(id => h.get(id).value), fields);
    assert.equal(h.get("judge-early").value, "73");
    assert.deepEqual(bindingValues(), bindings, "scalar Apply has no binding ownership");
    assert.equal(game.messages("settings-profile-save").length, 0);
    assert.equal(game.messages("play-start").length, 0); assert.equal(h.opens.length, 0);
  } finally { await h.close(); }
});

test("multiple composition commits coalesce to the latest value before a deferred Apply", async () => {
  const h = await harness();
  try {
    await h.preview(); const game = h.workers[0];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 3, fields: ["0", "2"], selected: 0 };
    await h.receive(state); const editor = h.get("menu-editor");
    for (const value of ["73", "74", "75"]) {
      editor.emit("compositionstart"); editor.value = value;
      editor.emit("input", { isComposing: true }); editor.emit("compositionend");
    }
    assert.equal(game.messages("menu-edit").length, 1);
    assert.equal(game.last("menu-edit").value, "73");
    editor.emit("input", { isComposing: false });
    assert.equal(game.messages("menu-edit").length, 1, "a trailing final input joins the same pending commit");
    editor.emit("keydown", { code: "Enter", isComposing: false });
    await h.receive({ ...state, revision: 6n, fields: ["73", "2"] });
    assert.deepEqual(game.last("menu-edit"), { kind: "menu-edit", menuGeneration: 77n,
      screen: 3n, revision: 6n, index: 0, value: "75" });
    assert.equal(game.messages("menu-edit").length, 2, "the intermediate committed value is coalesced");
    assert.equal(game.messages("menu-action").length, 0);
    assert.equal(editor.value, "75", "an earlier edit reply cannot replace the most recent composition");
    await h.receive({ ...state, revision: 7n, fields: ["75", "2"] });
    assert.equal(game.messages("menu-action").length, 1);
    assert.equal(game.last("menu-action").revision, 7n); assert.equal(game.last("menu-action").control, 71n);
    await h.receive({ ...state, revision: 7n, fields: ["75", "2"] });
    assert.equal(game.messages("menu-action").length, 1);
  } finally { await h.close(); }
});

test("Escape cancels an active composition and a late compositionend cannot commit its discarded text", async () => {
  const h = await harness();
  try {
    await h.preview(); const game = h.workers[0];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 3, fields: ["0", "2"], selected: 0 };
    await h.receive(state); const editor = h.get("menu-editor");
    editor.emit("compositionstart"); editor.value = "discarded text";
    editor.emit("input", { isComposing: true });
    editor.emit("keydown", { code: "Escape", isComposing: false });
    assert.equal(game.messages("menu-edit").length, 0);
    assert.equal(game.last("menu-action").control, 72n);
    assert.equal(game.last("menu-action").screen, 3n); assert.equal(game.last("menu-action").revision, 5n);
    editor.emit("compositionend");
    assert.equal(game.messages("menu-edit").length, 0, "cancelled composition cannot commit even before navigation replies");
    await h.receive({ ...state, screen: 4n, revision: 6n, route: 2, fields: ["destination"] });
    editor.value = "discarded text"; editor.emit("compositionend");
    assert.equal(game.messages("menu-edit").length, 0);
    assert.equal(game.messages("menu-action").length, 1);
  } finally { await h.close(); }
});

for (const change of ["screen", "selected field", "cancel", "owner"]) test(`composition ${change} change fences a stale end and deferred Apply`, async () => {
  const h = await harness();
  try {
    await h.preview(); const game = h.workers[0];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 3, fields: ["0", "2"], selected: 0 };
    await h.receive(state); const editor = h.get("menu-editor");
    editor.value = "73"; editor.emit("input", { isComposing: false });
    editor.emit("keydown", { code: "Enter", isComposing: false });
    editor.emit("compositionstart"); editor.value = "obsolete composition";
    if (change === "owner") {
      h.window.emit("pagehide"); await flush();
      h.window.emit("pageshow", { persisted: true }); await flush();
      await h.receive({ kind: "ready" });
    } else if (change === "cancel") h.click("menu-back");
    const destination = { ...state, revision: 6n, screen: change === "selected field" ? 3n : 4n,
      selected: change === "selected field" ? 1 : 0, fields: ["destination", "retained"] };
    await h.receive(destination);
    const target = h.workers.at(-1), edits = target.messages("menu-edit").length;
    const actions = target.messages("menu-action").length;
    editor.value = "obsolete composition"; editor.emit("compositionend");
    assert.equal(target.messages("menu-edit").length, edits, "old composition cannot edit the destination field");
    assert.equal(target.messages("menu-action").length, actions);
    await h.receive({ ...state, revision: 5n, fields: ["73", "2"] }, game);
    assert.equal(target.messages("menu-edit").length, edits);
    assert.equal(target.messages("menu-action").length, actions, "old edit reply cannot Apply the destination");
  } finally { await h.close(); }
});

test("canonical Players count ACK refreshes discovery controls and removal restores automatic one-player guidance", async () => {
  const h = await harness({ touchSupported: true });
  try {
    await h.preview(); const game = h.workers[0]; h.click("menu-open");
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 5, fields: ["0", "1", "1", "1", "", "0"], selected: 0,
      roster: { players: [1], nextPlayerId: 2, assignments: [] } };
    await h.receive(state);
    assert.equal(h.get("local-discover").disabled, true);
    h.get("local-count").value = "2"; h.get("local-count").emit("change");
    assert.deepEqual(game.last("menu-roster-count"), { kind: "menu-roster-count", menuGeneration: 77n,
      screen: 3n, revision: 5n, count: 2 });
    assert.equal(h.get("local-discover").disabled, true, "the count request cannot change the canonical roster before ACK");
    const two = { ...state, revision: 6n, fields: ["0", "1", "2", "1", "", "2", "", "0"],
      roster: { players: [1, 2], nextPlayerId: 3, assignments: [] } };
    await h.receive(two);
    assert.equal(h.get("local-count").value, "2"); assert.equal(h.get("local-discover").disabled, false);
    assert.match(h.get("local-status").textContent, /discover.*sources/i);
    assert.ok(h.get("local-source-1")); assert.ok(h.get("local-source-2"));
    assert.equal(h.get("local-source-1").disabled, true, "assignment still needs acquired sources");
    h.click("local-discover"); await flush();
    assert.match(h.get("local-status").textContent, /acquired source/);
    assert.equal(h.get("local-source-1").disabled, true, "source assignment waits for canonical inventory admission");
    const inventory = game.last("menu-fields");
    assert.equal(inventory.fields[0], "1"); assert.equal(inventory.fields[1], "1");
    assert.equal(inventory.fields[7], "2"); assert.equal(inventory.fields[8], "1");
    assert.equal(inventory.fields[9], "keyboard");
    await h.receive({ ...two, revision: 7n, fields: inventory.fields });
    assert.equal(h.get("local-source-1").disabled, false);
    const acquiredStatus = h.get("local-status").textContent;
    const assignedFields = [...inventory.fields]; assignedFields[4] = "1";
    await h.receive({ ...two, revision: 8n, fields: assignedFields, roster: { ...two.roster, assignments: [[1, 1n]] } });
    assert.equal(h.get("local-source-1").value, "1");
    assert.equal(h.get("local-source-1").disabled, false);
    assert.equal(h.get("local-status").textContent, acquiredStatus, "assignment ACK retains acquired-inventory guidance");
    h.get("local-count").value = "1"; h.get("local-count").emit("change");
    await h.receive({ ...state, revision: 9n, roster: { players: [1], nextPlayerId: 3, assignments: [] } });
    assert.equal(h.get("local-count").value, "1"); assert.equal(h.get("local-discover").disabled, true);
    assert.equal(h.get("local-source-1"), undefined); assert.equal(h.get("local-source-2"), undefined);
    assert.match(h.get("local-status").textContent, /one player.*automatically/i);
    await h.receive(two);
    assert.equal(h.get("local-count").value, "1"); assert.equal(h.get("local-discover").disabled, true);
  } finally { await h.close(); }
});

test("canonical Players refresh preserves busy controls and old Worker ownership fencing", async () => {
  const h = await harness();
  try {
    await h.preview(); const oldGame = h.workers[0];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 5, fields: ["0", "1", "1", "1", "", "0"], selected: 0,
      roster: { players: [1], nextPlayerId: 2, assignments: [] } };
    await h.receive(state); h.click("settings-save");
    assert.ok(oldGame.last("settings-profile-save"));
    const two = { ...state, revision: 6n,
      roster: { players: [1, 2], nextPlayerId: 3, assignments: [] } };
    await h.receive(two);
    for (const id of ["local-count", "local-discover", "local-source-1", "local-source-2"]) {
      assert.equal(h.get(id).disabled, true, `${id} stays disabled while the settings operation owns the UI`);
    }
    h.get("local-discover").emit("click"); await flush();
    assert.doesNotMatch(h.get("local-status").textContent, /acquired source/);
    h.window.emit("pagehide"); await flush(); h.window.emit("pageshow", { persisted: true }); await flush();
    await h.receive({ kind: "ready" });
    await h.receive({ ...state, revision: 7n, roster: { players: [1], nextPlayerId: 3, assignments: [] } });
    const before = h.get("local-status").textContent;
    await h.receive({ ...two, revision: 8n }, oldGame);
    assert.equal(h.get("local-count").value, "1"); assert.equal(h.get("local-discover").disabled, true);
    assert.equal(h.get("local-status").textContent, before);
  } finally { await h.close(); }
});

test("released inventory and rediscovery serialize behind exact capability and source-table ACKs", async () => {
  const h = await harness({ touchSupported: true });
  try {
    await h.preview(); const game = h.workers[0];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 5, fields: ["0", "1", "2", "1", "", "2", "", "0"], selected: 0,
      roster: { players: [1, 2], nextPlayerId: 3, assignments: [] } };
    await h.receive(state); h.click("local-discover"); await flush();
    const first = game.last("menu-fields");
    assert.equal(first.revision, 5n); assert.equal(first.fields[0], "1");
    assert.equal(first.fields[8], "1"); assert.equal(first.fields[9], "keyboard");
    assert.equal(h.get("local-source-1").disabled, true);
    localAssign(h, 1, 1n);
    assert.equal(game.messages("menu-roster-assign").length, 0, "even direct events cannot assign an unacknowledged inventory");
    await h.receive({ ...state, revision: 6n, fields: first.fields });
    assert.equal(h.get("local-source-1").disabled, false);
    h.click("local-release"); await flush();
    const empty = game.last("menu-fields");
    assert.equal(empty.revision, 6n); assert.deepEqual(empty.fields, state.fields);
    assert.equal(h.get("local-source-1").disabled, true);
    const publications = game.messages("menu-fields").length;
    h.click("local-discover"); await flush();
    assert.equal(game.messages("menu-fields").length, publications, "rediscovery coalesces behind the outstanding release transaction");
    assert.equal(h.get("local-source-1").disabled, true);
    const wrongCapability = [...empty.fields]; wrongCapability[1] = "0";
    await h.receive({ ...state, revision: 7n, fields: wrongCapability });
    assert.equal(game.messages("menu-fields").length, publications);
    assert.equal(h.get("local-source-1").disabled, true, "a mismatched capability is not release admission");
    await h.receive({ ...state, revision: 8n, fields: empty.fields });
    const latest = game.last("menu-fields");
    assert.equal(game.messages("menu-fields").length, publications + 1);
    assert.equal(latest.revision, 8n); assert.deepEqual(latest.fields, first.fields);
    assert.equal(h.get("local-source-1").disabled, true, "the new inventory has its own ACK gate");
    const wrongTable = [...latest.fields]; wrongTable[10] = "unrelated source label";
    await h.receive({ ...state, revision: 9n, fields: wrongTable });
    assert.equal(h.get("local-source-1").disabled, true, "matching capabilities alone cannot admit a different source table");
    await h.receive({ ...state, revision: 10n, fields: latest.fields });
    assert.equal(h.get("local-source-1").disabled, false);
    localAssign(h, 1, 1n);
    assert.deepEqual(game.last("menu-roster-assign"), { kind: "menu-roster-assign", menuGeneration: 77n,
      screen: 3n, revision: 10n, player: 1, source: 1n });
  } finally { await h.close(); }
});

test("navigation cancels an old inventory transaction and publishes acquired sources for the current assignment screen", async () => {
  const h = await harness();
  try {
    await h.preview(); const game = h.workers[0];
    const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 5, fields: ["0", "1", "2", "1", "", "2", "", "0"], selected: 0,
      roster: { players: [1, 2], nextPlayerId: 3, assignments: [] } };
    await h.receive(state); h.click("local-discover"); await flush();
    const old = game.last("menu-fields");
    await h.receive({ ...state, screen: 4n, revision: 6n, route: 2, fields: ["retained settings"] });
    const publications = game.messages("menu-fields").length;
    await h.receive({ ...state, revision: 5n, fields: old.fields });
    assert.equal(game.messages("menu-fields").length, publications, "old state cannot republish an inventory onto Settings");
    await h.receive({ ...state, screen: 9n, revision: 7n, route: 9 });
    const current = game.last("menu-fields");
    assert.equal(game.messages("menu-fields").length, publications + 1);
    assert.equal(current.screen, 9n); assert.equal(current.revision, 7n);
    assert.deepEqual(current.fields, old.fields, "the new screen receives the actual retained acquired inventory");
    assert.equal(h.get("local-source-1").disabled, true);
    await h.receive({ ...state, screen: 3n, revision: 6n, fields: old.fields });
    assert.equal(h.get("local-source-1").disabled, true);
    await h.receive({ ...state, screen: 9n, revision: 8n, route: 9, fields: current.fields });
    assert.equal(h.get("local-source-1").disabled, false);
    localAssign(h, 1, 1n);
    assert.equal(game.last("menu-roster-assign").screen, 9n);
    assert.equal(game.last("menu-roster-assign").revision, 8n);
  } finally { await h.close(); }
});

test("two owners transfer only renderer canvas and CPU readiness does not wait for GPU readiness", async () => {
  const h = await harness({ holdRendererReady: true });
  assert.equal(h.workers.length, 1); assert.equal(h.renderers.length, 1);
  const game = h.workers[0], renderer = h.renderers[0];
  const gameInit = game.last("init"), renderInit = renderer.last("init");
  assert.equal(gameInit.canvas, undefined);
  assert.equal(renderInit.canvas, h.get("canvas").transferred);
  assert.equal(gameInit.renderPort.peer, renderInit.port);
  assert.equal(h.channels.length, 1);
  assert.equal(gameInit.renderPort.transfers, 1); assert.equal(renderInit.port.transfers, 1);
  assert.equal(gameInit.maxPacketBytes, renderInit.maxPacketBytes);
  assert.equal(gameInit.maxDiagnosticBytes, renderInit.maxDiagnosticBytes);
  await h.preview(); const session = await h.launch();
  assert.equal(h.audio.arms.length, 1, "real setup continues while renderer readiness is pending");
  h.setNow(1301); h.window.emit("keydown", { code: "KeyZ", timeStamp: 1300.125 });
  const tick = game.last("play-step");
  assert.equal(tick.events.find(event => event.key === 2).hostNs, 1300125000n);
  assert.equal(renderer.messages("play-step").length, 0);
  await h.receive({ kind: "ready" }, renderer);
  assert.equal(h.audio.arms.length, 1, "late GPU readiness does not restart audio");
  h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
  assert.equal(game.terminations, 1); assert.equal(renderer.terminations, 1);
});

test("requested resize and invalid geometry never reinterpret touch before an actual submitted tuple", async () => {
  const { h, session, worker, surface } = await pagedTouchSession();
  const oldResize = worker.last("resize");
  const oldVersion = worker.geometryVersion;
  const pointer = (id, timeStamp) => surface.emit("pointerdown", { pointerType: "touch", pointerId: id,
    timeStamp, offsetX: 80.25, offsetY: 120.5, pressure: 0.375 });
  h.resize(400, 300); const resize = worker.last("resize");
  const requestedVersion = worker.geometryVersion;
  assert.ok(requestedVersion > oldVersion);
  pointer(201, 1300); const first = worker.last("play-step");
  const acquired = first.events.find(event => event.kind === "touch");
  assert.equal(acquired.width, 400); assert.equal(acquired.height, 300);
  assert.equal(acquired.surfaceWidth, oldResize.width); assert.equal(acquired.surfaceHeight, oldResize.height);
  assert.equal(acquired.page, 0); assert.equal(acquired.hostNs, 1300000000n);
  await h.receive({ kind: "play-step-done", playId: session.id, tickId: first.tickId, pendingInputs: 0,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  for (const fields of [
    { playId: session.id + 1 }, { selectedId: -1 }, { width: 0 }, { height: 0 },
    { geometryVersion: oldVersion },
    { mode: "history" }, { mode: "results" }, { mode: "room" }, { mode: "live" }, { mode: "replay" },
  ]) await h.geometry({ playId: session.id, geometryVersion: requestedVersion,
    width: 400, height: 300, ...fields });
  pointer(202, 1300.125); const second = worker.last("play-step");
  assert.equal(second.events.find(event => event.kind === "touch").surfaceWidth, oldResize.width);
  await h.receive({ kind: "play-step-done", playId: session.id, tickId: second.tickId, pendingInputs: 0,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  await h.geometry({ playId: session.id, geometryVersion: requestedVersion, width: 400, height: 300 });
  pointer(203, 1300.25); const third = worker.last("play-step");
  const submitted = third.events.find(event => event.kind === "touch");
  assert.equal(submitted.surfaceWidth, 400); assert.equal(submitted.surfaceHeight, 300);
  assert.equal(submitted.hostNs, 1300250000n);
  h.click("stop"); await flush(); await h.receive(localFinal(session.start)); await h.close();
});

test("renderer transport failure joins capture receipt and audio cleanup before terminating gameplay", async () => {
  const stopping = deferred(), h = await harness({ stopGate: stopping });
  await h.preview(); const session = await h.launch();
  const game = h.workers[0], renderer = h.renderers[0];
  renderer.emit("error", { message: "renderer transport lost", preventDefault() {} }); await flush();
  assert.equal(game.last("play-stop").playId, session.id);
  assert.equal(h.audio.stopStarts, 1); assert.equal(game.terminations, 0);
  await h.receive(finalScore(session.id, { kind: "play-error", message: "renderer transport lost",
    replay: Uint8Array.from([9, 8, 7]), replayComplete: false, replayError: null }));
  assert.equal(game.terminations, 0, "capture receipt alone cannot release pending audio cleanup");
  stopping.resolve(); await flush();
  assert.equal(game.terminations, 1); assert.equal(renderer.terminations, 1);
  assert.match(h.get("status").textContent, /renderer transport lost/);
  await h.close();
});

test("local coalesced movement and lost capture retain the acquired page through an unsubmitted choice", async () => {
  const { h, session, worker, surface } = await pagedTouchSession();
  const ack = request => h.receive({ kind: "play-step-done", playId: session.id, tickId: request.tickId, pendingInputs: 0,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  surface.emit("pointerdown", { pointerType: "touch", pointerId: 8, timeStamp: 1300,
    isPrimary: true, clientX: 100, clientY: 200,
    offsetX: 100, offsetY: 200, pressure: 0.5 }); const down = worker.last("play-step");
  h.get("local-page").value = "1"; h.get("local-page").emit("change"); await flush();
  const children = [
    { pointerType: "touch", pointerId: 8, isPrimary: true, timeStamp: 1300.125, clientX: 110, clientY: 210, pressure: 0.25 },
    { pointerType: "touch", pointerId: 8, isPrimary: true, timeStamp: 1300.25, clientX: 120, clientY: 220, pressure: 0.75 },
  ];
  surface.emit("pointermove", { pointerType: "touch", pointerId: 8, timeStamp: 1300.375,
    isPrimary: true, clientX: 150, clientY: 250,
    offsetX: 150, offsetY: 250, pressure: 1, getCoalescedEvents() { return children; } });
  children[0].timeStamp = 9999; children[0].pressure = 0;
  surface.emit("lostpointercapture", { pointerId: 8, timeStamp: 1300.5 });
  await ack(down); const batch = worker.last("play-step");
  assert.deepEqual(batch.events.map(event => [event.phase, event.page, event.hostNs]), [
    [1, 0, 1300125000n], [1, 0, 1300250000n], [3, 0, 1300500000n],
  ]);
  assert.equal(batch.events[0].pressure, 0.25);
  assert.ok(batch.events.every(event => event.contact === down.events[0].contact));
  await ack(batch); const page = worker.last("play-page"); assert.ok(page);
  await h.reply(page, { kind: "local-page", page: 1, touchVisible: false });
  surface.emit("pointerdown", { pointerType: "touch", pointerId: 9, timeStamp: 1300.625,
    offsetX: 10, offsetY: 20, pressure: 0.5 }); const fresh = worker.last("play-step");
  assert.equal(fresh.events[0].page, 0, "RPC acknowledgement alone never means page was submitted");
  assert.equal(fresh.events[0].hostNs, 1300625000n);
  h.click("stop"); await flush(); await h.receive(localFinal(session.start)); await h.close();
});

test("pre-first-live touch may use matching preview submission but never static record or stale selection geometry", async () => {
  for (const mode of ["preview", "history", "results", "room", "stale-preview"]) {
    const h = await harness({ touchSupported: true, holdGeometry: true });
    await h.preview(); const game = h.workers[0];
    await h.geometry({ mode: mode === "stale-preview" ? "preview" : mode,
      ...(mode === "stale-preview" ? { selectedId: game.last("select").id + 1 } : {}) });
    const session = await h.launch(); h.setNow(1300);
    h.get("canvas").emit("pointerdown", { pointerType: "touch", pointerId: 55,
      timeStamp: 1300, offsetX: 100, offsetY: 200, pressure: 0.5 }); await flush();
    if (mode === "preview") {
      const contact = game.last("play-step").events.find(event => event.kind === "touch");
      assert.ok(contact); assert.equal(contact.hostNs, 1300000000n);
      assert.equal(contact.surfaceWidth, game.last("resize").width);
      assert.equal(game.messages("play-stop").length, 0);
      h.click("stop"); await flush();
    } else {
      assert.equal(game.messages("play-step").flatMap(request => request.events).some(event => event.kind === "touch"), false);
      assert.equal(game.last("play-stop").playId, session.id, "missing valid acquisition geometry fails explicitly");
    }
    await h.receive(finalScore(session.id)); await h.close();
  }
});

test("actual Main portable settings preserve explicit policy fields and old profiles reset the selection", async () => {
  const h = await harness(); await h.preview();
  const worker = h.workers[0];
  h.get("judge-preset").value = BMS_TIMING_PRESET_ID;
  h.get("judge-precedence").value = "defexrank-first";
  h.get("judge-gauge").value = "groove";
  h.click("settings-save"); await flush();
  const saved = worker.last("settings-profile-save");
  assert.deepEqual(saved.settings.timing, { earlyMs: "50", lateMs: "50", offsetMs: "0",
    presetId: BMS_TIMING_PRESET_ID, rankPrecedence: "defexrank-first", gauge: "groove" });
  await h.receive({ kind: "settings-profile-error", id: saved.id, message: "Fixture ends download" });
  const selected = portableSettings();
  Object.assign(selected.timing, { presetId: BMS_TIMING_PRESET_ID, rankPrecedence: "rank-first", gauge: "hard" });
  chooseSettings(h); await flush();
  await h.receive({ kind: "settings-profile-loaded", id: worker.last("settings-profile-load").id, settings: selected });
  assert.equal(h.get("judge-gauge").value, "hard");
  assert.equal(h.get("judge-precedence").value, "rank-first");
  chooseSettings(h); await flush();
  await h.receive({ kind: "settings-profile-loaded", id: worker.last("settings-profile-load").id, settings: portableSettings() });
  assert.equal(h.get("judge-preset").value, "");
  assert.equal(h.get("judge-precedence").value, "rank-first");
  assert.equal(h.get("judge-gauge").value, "beatkernel");
  await h.close();
});

test("portable settings save downloads exact Worker bytes and a correlated load governs the next actual launch", async () => {
  const h = await harness({ noWindowSettingsJson: true, actualRate: 44100 });
  await h.preview();
  const worker = h.workers[0], original = settingsDraft(h);
  h.click("settings-save"); await flush();
  const saved = worker.last("settings-profile-save");
  assert.ok(Number.isSafeInteger(saved.id) && saved.id > 0);
  assert.equal(saved.settings.kind, "beatkernel-browser-settings");
  assert.equal(saved.settings.version, 1);
  assert.deepEqual(saved.settings.timing, { earlyMs: "50", lateMs: "50", offsetMs: "0" });
  assert.equal(saved.settings.bindings.length, 18);
  assert.deepEqual(Object.keys(saved.settings).sort(), ["bindings", "capacities", "kind", "output", "section", "timing", "version"]);
  assert.equal(h.get("play").disabled, true);
  assert.equal(h.get("settings-load").disabled, true);
  const encoded = Uint8Array.from([123, 10, 32, 34, 111, 107, 34, 58, 49, 125]);
  await h.receive({ kind: "settings-profile-saved", id: saved.id, bytes: encoded });
  assert.equal(h.downloads.length, 1);
  assert.equal(h.downloads[0].filename, "beatkernel-browser-settings.json");
  assert.equal(h.urls[0].blob.type, "application/json");
  assert.deepEqual(Array.from(new Uint8Array(await h.urls[0].blob.arrayBuffer())), Array.from(encoded));
  assert.deepEqual(settingsDraft(h), original);
  assert.equal(h.get("play").disabled, false);
  h.click("settings-save"); await flush();
  const second = worker.last("settings-profile-save");
  await h.receive({ kind: "settings-profile-saved", id: second.id, bytes: Uint8Array.from([123, 125]) });
  assert.equal(h.revoked.filter(url => url === h.urls[0].url).length, 1);
  await h.advance(60000);
  assert.equal(h.revoked.filter(url => url === h.urls[1].url).length, 1, "download ownership has a bounded expiry");
  h.click("settings-save"); await flush();
  const third = worker.last("settings-profile-save");
  await h.receive({ kind: "settings-profile-saved", id: third.id, bytes: Uint8Array.from([123, 125]) });

  const selected = chooseSettings(h); await flush();
  const load = worker.last("settings-profile-load");
  assert.equal(load.file, selected);
  assert.ok(load.id > third.id);
  assert.deepEqual(settingsDraft(h), original, "no optimistic draft assignment");
  const loaded = portableSettings();
  await h.receive({ kind: "settings-profile-loaded", id: load.id, settings: loaded });
  for (const [group, key, id] of settingsFields) assert.equal(h.get(id).value, loaded[group][key]);
  for (const [lane, code] of loaded.bindings) assert.equal(h.get(`binding-${lane.toString(16)}`).value, code);
  assert.equal(h.get("chart").value, "chart.bms");
  assert.equal(h.get("seed").value, "7");
  assert.equal(h.get("multiplayer-url").value, "");
  const start = await h.begin();
  assert.deepEqual(start.timing, { earlyNs: 12345678n, lateNs: 87654321n, offsetNs: -12500001n });
  assert.equal(start.startNs, 1000000001n); assert.equal(start.endNs, 2000000002n);
  assert.ok(Array.from(start.keyPairs).some((value, index, words) => index % 2 === 0 && value === 17 && words[index + 1] === 19));
  assert.ok(!Array.from(start.keyPairs).some((value, index) => index % 2 === 0 && value === 18));
  assert.deepEqual(structuredClone(h.opens[0].options.contextOptions), { latencyHint: 0.010000001, sampleRate: 44100 });
  assert.deepEqual(structuredClone(h.audio.configuration.audioLimits), {
    queueCapacity: 257, maxVoices: 17, pendingCapacity: 31, maxFrames: 257, maxCommandsPerRender: 7,
  });
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  await h.close();
  assert.equal(h.revoked.filter(url => url === h.urls[2].url).length, 1);
});

test("malformed settings responses and timeouts preserve the whole draft without disturbing the live preview owner", async () => {
  for (const fault of ["missing-binding", "foreign-kind", "unknown-field", "wrong-type", "worker-error", "malformed-error", "timeout", "draft-change", "bad-save-bytes"]) {
    const h = await harness({ noWindowSettingsJson: true }); await h.preview();
    const worker = h.workers[0];
    if (fault === "bad-save-bytes") h.click("settings-save");
    else chooseSettings(h);
    await flush();
    const request = worker.last(fault === "bad-save-bytes" ? "settings-profile-save" : "settings-profile-load");
    assert.ok(request);
    const payload = portableSettings();
    if (fault === "missing-binding") payload.bindings.pop();
    if (fault === "foreign-kind") payload.kind = "native-settings";
    if (fault === "unknown-field") payload.output.token = "must not apply";
    if (fault === "wrong-type") payload.capacities.maxVoices = 17;
    if (fault === "draft-change") h.get("judge-early").value = "51";
    const before = settingsDraft(h);
    if (fault === "timeout") await h.advance(10000);
    else if (fault === "worker-error") await h.receive({ kind: "settings-profile-error", id: request.id, message: "strict settings decode failed" });
    else if (fault === "malformed-error") await h.receive({ kind: "settings-profile-error", id: request.id, message: [] });
    else if (fault === "bad-save-bytes") await h.receive({ kind: "settings-profile-saved", id: request.id, bytes: new Uint8Array(16385) });
    else await h.receive({ kind: "settings-profile-loaded", id: request.id, settings: payload });
    assert.deepEqual(settingsDraft(h), before, fault);
    assert.equal(h.get("settings-save").disabled, false);
    assert.equal(h.get("play").disabled, false);
    assert.equal(worker.terminations, 0);
    assert.equal(worker.messages("play-stop").length, 0);
    assert.equal(h.opens.length, 0);
    assert.equal(h.downloads.length, 0);
    assert.ok(h.get("settings-status").textContent.length > 0);
    assert.equal(h.get("settings-status").dataset.error, "true");
    if (fault === "timeout") {
      await h.receive({ kind: "settings-profile-loaded", id: request.id, settings: portableSettings() });
      assert.deepEqual(settingsDraft(h), before, "late read completion cannot revive a timed-out draft");
    }
    await h.close();
  }
});

test("settings pending ownership blocks overlapping actions and stale responses cannot clear deadlines or apply after shutdown", async () => {
  const h = await harness({ noWindowSettingsJson: true }); await h.preview();
  const worker = h.workers[0], before = settingsDraft(h);
  chooseSettings(h); await flush();
  const first = worker.last("settings-profile-load");
  for (const [, , id] of settingsFields) assert.equal(h.get(id).disabled, true, id);
  h.click("settings-save"); chooseSettings(h); h.click("play");
  h.get("prepare-form").emit("submit"); h.get("seek-form").emit("submit"); await flush();
  assert.equal(worker.messages("settings-profile-load").length, 1);
  assert.equal(worker.messages("settings-profile-save").length, 0);
  assert.equal(h.opens.length, 0);
  assert.equal(worker.messages("select").length, 1);
  assert.equal(worker.messages("seek").length, 1);
  await h.receive({ kind: "settings-profile-loaded", id: first.id + 1, settings: portableSettings() });
  assert.equal(h.get("settings-save").disabled, true);
  await h.advance(9999); assert.equal(h.get("settings-save").disabled, true);
  await h.advance(1); assert.equal(h.get("settings-save").disabled, false);
  assert.deepEqual(settingsDraft(h), before);
  chooseSettings(h); await flush(); const second = worker.last("settings-profile-load");
  assert.ok(second.id > first.id);
  await h.receive({ kind: "settings-profile-loaded", id: first.id, settings: portableSettings() });
  assert.equal(h.get("settings-save").disabled, true);
  assert.deepEqual(settingsDraft(h), before);
  await h.close();
  await h.receive({ kind: "settings-profile-loaded", id: second.id, settings: portableSettings() }, worker);
  assert.deepEqual(settingsDraft(h), before);
  assert.equal(worker.terminations, 1);
  assert.equal(h.downloads.length, 0);
  const replacement = await harness({ noWindowSettingsJson: true }); await replacement.preview();
  assert.deepEqual(settingsDraft(replacement), before);
  await replacement.close();

  const local = await harness({ gamepads: [nativeGamepad()] }); await local.preview();
  await localCount(local, 2); local.click("local-discover"); await flush();
  assert.equal(local.get("settings-save").disabled, true);
  assert.equal(local.get("settings-load").disabled, true);
  local.get("settings-save").emit("click"); chooseSettings(local); await flush();
  assert.equal(local.workers[0].messages("settings-profile-save").length, 0);
  assert.equal(local.workers[0].messages("settings-profile-load").length, 0);
  local.click("local-release"); await flush();
  assert.equal(local.get("settings-save").disabled, false);
  await local.close();
});

test("touch paging retains new contacts and original page while waiting for acquired prefix and submitted geometry", async () => {
  const { h, session, worker, surface } = await pagedTouchSession();
  const pointer = (type, fields = {}) => surface.emit(type, { pointerType: "touch", pointerId: -2,
    timeStamp: 1300, offsetX: 120.25, offsetY: 180.5, pressure: 0.375, ...fields });
  const ack = (request, pendingInputs = 0) => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs, playId: session.id, tickId: request.tickId,
    songNs: 1n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  assert.equal(h.get("local-page").disabled, false);
  const layoutReads = h.layoutReads;
  pointer("pointerdown"); const down = worker.last("play-step"), original = down.events.find(event => event.kind === "touch");
  pointer("pointermove", { timeStamp: 1300.125, offsetX: 200.5 });
  assert.equal(worker.last("play-step"), down);
  h.get("local-page").value = "1"; h.get("local-page").emit("change"); await flush();
  assert.equal(h.get("local-page").disabled, true); assert.equal(worker.messages("play-page").length, 0);
  const captures = h.captures.length;
  pointer("pointerdown", { pointerId: 91, timeStamp: 1300.25 });
  assert.equal(h.captures.length, captures + 1, "a new contact is acquired during the page transition");
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id - 1, tickId: down.tickId,
    songNs: 1n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  assert.equal(worker.messages("play-page").length, 0);
  await ack(down);
  const moved = worker.last("play-step"), move = moved.events.find(event => event.kind === "touch");
  assert.ok(moved.tickId > down.tickId); assert.equal(move.phase, 1); assert.equal(move.contact, original.contact);
  assert.equal(move.hostNs, 1300125000n); assert.equal(move.x, 200.5);
  const admittedDown = moved.events.find(event => event.kind === "touch" && event.phase === 0);
  assert.ok(admittedDown); assert.equal(admittedDown.hostNs, 1300250000n);
  assert.equal(admittedDown.page, 0); assert.equal(move.page, 0);
  assert.equal(worker.messages("play-page").length, 0, "the first ACK alone does not cover the already queued Move");
  await ack(moved, 1);
  assert.equal(worker.messages("play-page").length, 0, "admission ACK cannot remap a touch still held for audio correspondence");
  await h.advance(8);
  const joinedTick = worker.last("play-step");
  if (joinedTick.tickId > moved.tickId) await ack(joinedTick, 1);
  const joined = worker.last("play-render");
  await h.receive({ kind: "play-render-done", playId: session.id, renderId: joined.renderId,
    completed: false, commandsPending: false, observedTick: worker.last("play-step").tickId, pendingInputs: 0 });
  const page = worker.last("play-page"); assert.ok(page); assert.equal(page.page, 1);
  const posts = worker.posts.map(post => post.value);
  assert.ok(posts.indexOf(page) > posts.indexOf(moved));
  assert.equal(h.releases.length, 0, "remapping cannot synthesize contact release");
  pointer("pointerup", { timeStamp: 1300.5, offsetX: -10.25, pressure: 0 });
  const released = worker.last("play-step"), up = released.events.find(event => event.kind === "touch");
  assert.equal(up.phase, 2); assert.equal(up.contact, original.contact); assert.equal(up.code, 0xfffffffe);
  assert.equal(up.hostNs, 1300500000n); assert.equal(up.x, -10.25); assert.equal(up.width, original.width);
  assert.equal(h.releases.length, 1); assert.equal(h.releases[0].id, -2);
  assert.equal(released.events.filter(event => event.kind === "touch").length, 1, "synchronous lost capture must not manufacture Cancel");
  pointer("pointerdown", { pointerId: 92, timeStamp: 1300.625 });
  assert.equal(h.captures.length, captures + 2, "pending page submission cannot discard the next contact");
  await ack(released); await h.reply(page, { kind: "local-page", page: 1, touchVisible: false });
  const pendingDown = worker.last("play-step");
  assert.equal(pendingDown.events.find(event => event.kind === "touch" && event.phase === 0).page, 0);
  await ack(pendingDown);
  await h.geometry({ playId: session.id, page: 1, geometryVersion: page.geometryVersion });
  assert.equal(h.get("local-page").value, "1"); assert.equal(h.get("local-page").disabled, false);
  assert.match(h.get("local-status").textContent, /unbound.*offscreen.*held contacts retain/);
  pointer("pointerdown", { pointerId: 93, timeStamp: 1300.75 });
  const hidden = worker.last("play-step"), hiddenDown = hidden.events.find(event => event.kind === "touch");
  assert.equal(hiddenDown.phase, 0); assert.equal(hiddenDown.contact, original.contact + 3n);
  assert.equal(hiddenDown.page, 1);
  assert.equal(hiddenDown.hostNs, 1300750000n); assert.equal(hiddenDown.sequence > up.sequence, true);
  await ack(hidden);
  h.get("local-page").value = "0"; h.get("local-page").emit("change"); await flush();
  const back = worker.last("play-page"); await h.reply(back, { kind: "local-page", page: 0, touchVisible: true });
  await h.geometry({ playId: session.id, page: 0, geometryVersion: back.geometryVersion });
  pointer("pointerup", { pointerId: 93, timeStamp: 1300.875 });
  const hiddenUp = worker.last("play-step");
  assert.equal(hiddenUp.events.find(event => event.kind === "touch").contact, hiddenDown.contact);
  await ack(hiddenUp);
  assert.equal(h.layoutReads, layoutReads); assert.equal(worker.messages("play-stop").length, 0);
  h.click("stop"); await flush(); await h.receive(localFinal(session.start)); await h.close();
});

test("Window keeps the existing twelve millisecond prefix behind a newer batch and accepts another source within that lag", async () => {
  const { h, session, worker, surface } = await pagedTouchSession();
  const initialPages = worker.messages("play-page").length;
  const ack = (request, pendingInputs) => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs, playId: session.id, tickId: request.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  h.setNow(1300.125); const key = observedKeyboard({ timeStamp: 1300.125 }); dispatchKeyboard(h, "keydown", key.event);
  const first = worker.last("play-step"); const keyboard = first.events.find(event => event.key === 2);
  assert.ok(keyboard); assert.equal(keyboard.hostNs, 1300125000n); assert.equal(first.nowNs, 1300125000n);
  assert.equal(first.watermark, 1288125000n); assert.ok(first.watermark < keyboard.hostNs);
  await ack(first, 1);
  h.setNow(1300.25); surface.emit("pointerdown", { pointerType: "touch", pointerId: 91, timeStamp: 1300,
    offsetX: 120.25, offsetY: 180.5, pressure: 0.375 });
  const second = worker.last("play-step"); const touch = second.events.find(event => event.kind === "touch");
  assert.ok(second.tickId > first.tickId); assert.ok(touch); assert.equal(touch.hostNs, 1300000000n);
  assert.equal(touch.x, 120.25); assert.equal(touch.y, 180.5); assert.equal(second.nowNs, 1300250000n);
  assert.equal(second.watermark, 1288250000n); assert.ok(touch.hostNs > first.watermark && touch.hostNs < keyboard.hostNs);
  assert.equal(worker.messages("play-stop").length, 0, "a different original source within the lag must not close the session");
  h.get("local-page").value = "1"; h.get("local-page").emit("change"); await flush(); await ack(second, 2);
  assert.equal(worker.messages("play-page").length, initialPages, "admission of newer events is not a drained-prefix proof");
  assert.equal(second.events.some(event => event.kind === "gamepad"), false, "touch and ACK callbacks do not sample Gamepads");
  const sources = session.start.gamepadDevices.map(event => event.source);
  assert.equal(sources.length, 3);
  const assertPolling = (request, previous) => {
    assert.ok(request.tickId > previous.tickId);
    assert.deepEqual(request.events, sources.map((source, index) => ({ kind: "gamepad", source, index,
      id: "standard gamepad", mapping: "standard", hostNs: 1000000000n, timestampMs: 1000,
      sequence: previous.events.at(-1).sequence + BigInt(index + 1), axes: [0.12345678901234568],
      buttons: Array.from({ length: 9 }, () => ({ value: 0, pressed: false, touched: false })) })));
  };
  await h.advance(8);
  const polling = worker.last("play-step"); assertPolling(polling, second); await ack(polling, 2);
  assert.equal(worker.messages("play-page").length, initialPages, "unchanged device polling does not drain held key and touch input");
  h.setNow(1313); await h.advance(8); const later = worker.last("play-step");
  assertPolling(later, polling); assert.ok(later.watermark >= keyboard.hostNs);
  await ack(later, 0);
  if (worker.messages("play-page").length === initialPages) { const render = worker.last("play-render"); await h.receive({ kind: "play-render-done", playId: session.id,
    renderId: render.renderId, completed: false, commandsPending: false, observedTick: later.tickId, pendingInputs: 0 }); }
  assert.equal(worker.messages("play-page").length, initialPages + 1);
  const page = worker.last("play-page"); assert.ok(page); assert.equal(page.page, 1);
  await h.reply(page, { kind: "local-page", page: 1, touchVisible: false });
  h.click("stop"); await flush(); await h.receive(localFinal(session.start)); await h.close();
});

test("page choice errors preserve held input but cancellation and invalid visibility cannot apply a late page", async () => {
  for (const fault of ["choice-error", "wrong-visibility", "cancel-before-ack", "cancel-pending-rpc"]) {
    const { h, session, worker, surface } = await pagedTouchSession();
    surface.emit("pointerdown", { pointerType: "touch", pointerId: 7, timeStamp: 1300,
      offsetX: 100, offsetY: 200, pressure: 0.5 });
    const down = worker.last("play-step");
    h.get("local-page").value = "1"; h.get("local-page").emit("change"); await flush();
    const ack = () => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: down.tickId,
      songNs: 1n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
    if (fault === "cancel-before-ack") {
      h.click("stop"); await flush();
      const status = h.get("local-status").textContent;
      await ack();
      assert.equal(worker.messages("play-page").length, 0); assert.equal(h.get("local-status").textContent, status);
      await h.receive(localFinal(session.start));
    } else {
      await ack(); const page = worker.last("play-page"); assert.ok(page);
      if (fault === "choice-error") {
        await h.receive({ kind: "play-reply", playId: session.id, rpcId: page.rpcId, error: "actual remap refused" });
        assert.equal(h.get("local-page").value, "0"); assert.equal(h.get("local-page").disabled, false);
        assert.match(h.get("local-status").textContent, /Page unchanged.*actual remap refused/);
        assert.equal(h.releases.length, 0); assert.equal(worker.messages("play-stop").length, 0);
        surface.emit("pointerup", { pointerType: "touch", pointerId: 7, timeStamp: 1300.5,
          offsetX: 800, offsetY: 700, pressure: 0 });
        const release = worker.last("play-step");
        assert.equal(release.events.find(event => event.kind === "touch").contact, down.events.find(event => event.kind === "touch").contact);
        await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: release.tickId,
          songNs: 2n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
        h.click("stop"); await flush(); await h.receive(localFinal(session.start));
      } else if (fault === "wrong-visibility") {
        await h.reply(page, { kind: "local-page", page: 1, touchVisible: true });
        assert.equal(worker.last("play-stop").playId, session.id);
        await h.receive(localFinal(session.start));
        assert.match(h.get("status").textContent, /page response.*identity/i);
      } else {
        h.click("stop"); await flush();
        const status = h.get("local-status").textContent;
        await h.reply(page, { kind: "local-page", page: 1, touchVisible: false });
        assert.equal(h.get("local-status").textContent, status);
        await h.receive(localFinal(session.start));
      }
    }
    assert.equal(h.releases.filter(release => release.id === 7).length, 1);
    assert.equal(worker.messages("play-page").length, fault === "cancel-before-ack" ? 0 : 1);
    assert.match(h.get("local-status").textContent, /sources released/);
    assert.equal(h.get("play").disabled, false);
    await h.close();
  }
});

test("local discovery retains actual mixed-device owners through one synchronous plan snapshot and filters unassigned acquisitions", async () => {
  const opening = deferred(), chosen = nativeGamepad(0), ignored = nativeGamepad(1, { id: chosen.id });
  const h = await harness({ gamepads: [chosen, ignored], hidSupported: true,
    hidDescriptors: [{ vendorId: 1, productId: 2 }], openGate: opening });
  const preview = await h.preview(); const profile = selectedControllerProfile(); chooseControllerProfile(h, profile.file);
  await localCount(h, 3); h.click("local-discover"); await flush();
  assert.equal(h.opens.length, 0); assert.equal(h.hid.requests.length, 0); assert.equal(h.hid.gets, 1);
  assert.equal(h.hidDevices[0].opens, 1); assert.equal(h.hidDevices[0].closes, 0);
  const options = h.get("local-source-1").children;
  const padSources = options.filter(option => option.textContent.startsWith("Gamepad ")).map(option => BigInt(option.value));
  const hidSource = BigInt(options.find(option => option.textContent.startsWith("HID ")).value);
  assert.equal(padSources.length, 2); assert.notEqual(padSources[0], padSources[1]);
  assert.ok(hidSource > padSources[1]);
  localAssign(h, 1, 1n); localAssign(h, 2, padSources[0]); localAssign(h, 3, hidSource);
  const native = h.hidDevices[0], staleDisconnect = [...h.window.listeners.get("gamepaddisconnected")][0];
  h.click("play");
  assert.equal(h.opens.length, 1); assert.equal(h.opens[0].gesture, true);
  for (const id of ["local-count", "local-discover", "local-source-1", "local-source-2", "local-source-3"]) assert.equal(h.get(id).disabled, true);
  h.get("local-source-2").value = padSources[1].toString(); h.get("local-source-2").emit("change");
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  assert.deepEqual(Array.from(start.localPlanWords), [1, 1, 1, 0, 2, 1, Number(padSources[0]), 0, 3, 1, Number(hidSource), 0]);
  assert.equal(start.localPage, 0); assert.equal(start.hidProfileFile, profile.file);
  assert.equal(h.hid.gets, 1); assert.equal(native.opens, 1); assert.equal(native.closes, 0);
  assert.equal(h.window.listeners.get("gamepaddisconnected").size, 1);
  const handoff = await h.prepared(start, 1); await h.reply(handoff, null); await h.reply(worker.last("play-activate"), null);
  const display = watchPlayDisplay(h), layout = h.layoutReads;
  h.setNow(1300.125); chosen.timestamp = 1300.0625; ignored.timestamp = 1300.0625;
  chosen.buttons[0] = { value: 0.25, pressed: true, touched: true };
  ignored.buttons[0] = { value: 1, pressed: true, touched: true };
  h.faults.onGamepadPoll = () => {
    delete h.faults.onGamepadPoll;
    native.emit("inputreport", { device: native, reportId: 7, timeStamp: 1300.125, data: new DataView(Uint8Array.from([7, 5]).buffer) });
  };
  await h.advance(8);
  const input = worker.last("play-step");
  assert.deepEqual(input.events.map(event => [event.kind, event.source]), [["hid", hidSource], ["gamepad", padSources[0]]]);
  assert.equal(input.events[0].hostNs, 1300125000n); assert.equal(input.events[1].hostNs, 1300062500n);
  assert.equal(h.layoutReads, layout); assert.deepEqual(display, []);
  assert.equal(profile.reads, 0); assert.equal(h.audio.samples.length, 0); assert.equal(h.audio.sampleAttachments, 1);
  h.click("stop"); await flush(); await h.receive(localFinal(start));
  assert.equal(native.closes, 1); assert.equal(h.window.listeners.get("gamepaddisconnected")?.size ?? 0, 0);
  assert.equal(h.get("position").value, preview.position);
  const posts = worker.posts.length;
  staleDisconnect({ gamepad: chosen }); await flush(); assert.equal(worker.posts.length, posts);
  await h.close();
});

test("local preparation refuses stale or contradictory ownership before PCM and cancelled discovery cannot resurrect acquired devices", async () => {
  const missing = await harness({ touchSupported: true }); await missing.preview();
  await localCount(missing, 2); missing.click("play"); await flush();
  assert.equal(missing.opens.length, 0); assert.equal(missing.workers[0].messages("play-start").length, 0);
  await missing.close();
  for (const conflict of ["invalid network", "opponents"]) {
    const h = await harness({ touchSupported: true }); await h.preview();
    await localCount(h, 2); h.click("local-discover"); await flush();
    localAssign(h, 1, 1n); localAssign(h, 2, 2n);
    if (conflict === "invalid network") { h.get("multiplayer").checked = true; h.get("multiplayer-url").value = "http://room.example/rooms/local"; }
    else selectOpponent(h, selectedRecording());
    h.click("play"); await flush();
    assert.equal(h.opens.length, 0); assert.equal(h.workers[0].messages("play-start").length, 0);
    await h.close();
  }
  for (const changed of [{ localPlayers: [2, 1] }, { localPlayers: [1, 1] },
    { recordLimits: { bytes: 33554433, records: 500000 } }]) {
    const h = await harness({ touchSupported: true }); await h.preview();
    await localCount(h, 2); h.click("local-discover"); await flush();
    localAssign(h, 1, 1n); localAssign(h, 2, 2n); h.get("record").checked = true;
    const start = await h.begin(), worker = h.workers[0];
    await h.reply(start, { kind: "prepared", title: "Contradictory local preparation", samples: 1, lanes: [0x11],
      startNs: 0n, opponentCount: 0, localPlayers: [1, 2], localPage: 0,
      recordLimits: { bytes: 33554432, records: 500000 }, ...changed });
    assert.equal(worker.messages("play-samples-upload").length, 0); assert.equal(h.audio.samples.length, 0);
    assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(localFinal(start)); assert.equal(h.get("play").disabled, false);
    await h.close();
  }
  const opening = deferred(), closing = deferred();
  const pending = await harness({ hidSupported: true, hidDescriptors: [{ vendorId: 1, productId: 2 }],
    hidOpenGate: opening, hidCloseGate: closing });
  await pending.preview(); chooseControllerProfile(pending, selectedControllerProfile().file);
  await localCount(pending, 2); pending.click("local-discover"); await flush();
  assert.equal(pending.hidDevices[0].opens, 1); assert.equal(pending.get("play").disabled, true);
  pending.window.emit("pagehide"); await flush();
  const status = pending.get("local-status").textContent;
  opening.resolve(); await flush(); assert.equal(pending.hidDevices[0].closes, 1);
  closing.resolve(); await flush();
  assert.equal(pending.opens.length, 0); assert.equal(pending.hidDevices[0].listeners.get("inputreport")?.size ?? 0, 0);
  assert.equal(pending.get("local-status").textContent, status, "old discovery completion cannot publish into another page owner");
  assert.equal(pending.workers[0].messages("play-start").length, 0);
  await pending.close();
});

test("joined local capture retains each valid member prefix for explicit download and save while replay ignores the roster", async () => {
  const stopping = deferred(), pad = nativeGamepad();
  const h = await harness({ touchSupported: true, gamepads: [pad], stopGate: stopping });
  await h.preview(); await localCount(h, 3); h.click("local-discover"); await flush();
  const source = BigInt(h.get("local-source-1").children.find(option => option.textContent.startsWith("Gamepad ")).value);
  localAssign(h, 1, 1n); localAssign(h, 2, 2n); localAssign(h, 3, source);
  h.get("record").checked = true;
  const session = await h.launch(), worker = h.workers[0];
  assert.equal(session.start.recordReplay, true);
  const first = Uint8Array.from([66, 75, 82, 1]), third = Uint8Array.from([66, 75, 82, 3, 255]);
  const result = localFinal(session.start, {
    localScores: [
      { player: 1, songNs: 2350000000n, hits: 1n, misses: 0n, combo: 0n, maxCombo: 1n },
      { player: 2, songNs: 2350000000n, hits: 2n, misses: 0n, combo: 0n, maxCombo: 2n },
      { player: 3, songNs: 2350000000n, hits: 18446744073709551615n, misses: 0n, combo: 0n, maxCombo: 18446744073709551615n },
    ],
    replays: [
      { player: 1, replay: first, replayError: null, replayComplete: false },
      { player: 2, replay: null, replayError: "member 2 codec capacity refused", replayComplete: false },
      { player: 3, replay: third, replayError: null, replayComplete: false },
    ],
  });
  h.click("stop"); await flush(); await h.receive(result);
  assert.equal(h.get("export").disabled, true); assert.equal(h.get("captured-replay").disabled, true);
  assert.equal(h.recordCalls.length, 0); assert.equal(h.urls.length, 0);
  stopping.resolve(); await flush();
  assert.equal(h.get("play").disabled, false); assert.equal(worker.terminations, 0);
  const choices = h.get("captured-replay").children.filter(option => option.value !== "");
  assert.equal(choices.length, 2); assert.match(choices[0].textContent, /1/); assert.match(choices[1].textContent, /3/);
  const rows = h.get("local-results").children.map(row => row.textContent).join("\n");
  assert.match(rows, /member 2 codec capacity refused/); assert.match(rows, /18446744073709551615/);
  assert.equal(h.get("export").disabled, true, "a local recording requires an explicit member choice");
  assert.equal(h.get("records-save").disabled, true); assert.equal(h.recordCalls.length, 0, "capture never saves itself");
  h.get("captured-replay").value = choices[1].value; h.get("captured-replay").emit("change");
  assert.equal(h.get("export").disabled, false);
  h.click("export"); await flush();
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), third);
  assert.equal(h.downloads.at(-1).filename, `beatkernel-${session.id}-player-3-prefix.bkr`);
  h.click("records-save"); await flush();
  const saved = h.recordCalls.find(call => call.method === "save");
  assert.ok(saved); assert.deepEqual(saved.value.bytes, third); assert.equal(saved.value.complete, false);
  assert.equal(saved.value.hits, 18446744073709551615n); assert.equal(saved.value.misses, 0n); assert.equal(saved.value.combo, 0n);
  h.get("captured-replay").value = choices[0].value; h.get("captured-replay").emit("change");
  h.click("export"); await flush();
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), first);
  const reads = h.gamepadReads; chooseRecording(h, [selectedRecording().file]);
  const replay = await h.launch(0, "replay");
  assert.equal(Object.hasOwn(replay.start, "localPlanWords"), false); assert.equal(Object.hasOwn(replay.start, "localPage"), false);
  assert.equal(Object.hasOwn(replay.start, "gamepadDevices"), false); assert.equal(h.gamepadReads, reads);
  const currentRows = h.get("local-results").children.map(row => row.textContent);
  await h.receive({ ...result, replays: [] });
  assert.deepEqual(h.get("local-results").children.map(row => row.textContent), currentRows);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id)); await h.close();
});

test("missing sample-port admission has one finite deadline and joins or forcibly releases the still-unacknowledged Worker", async () => {
  for (const workerReleases of [true, false]) {
    const h = await harness(); await h.preview(); const start = await h.begin(), worker = h.workers[0];
    const upload = await h.prepared(start, 2, false);
    assert.equal(upload.port.transfers, 1); assert.equal(h.audio.finishes, 0);
    await h.advance(9999);
    assert.equal(worker.messages("play-stop").length, 0); assert.equal(h.audio.stopStarts, 0);
    await h.advance(1);
    assert.equal(worker.messages("play-stop").length, 1); assert.equal(h.audio.stopStarts, 1);
    assert.equal(h.get("play").disabled, true); assert.equal(h.audio.finishes, 0); assert.deepEqual(h.audio.arms, []);
    // A late admission/final result cannot discharge the separate cleanup join.
    await h.admitSamples(upload, 2);
    await h.reply(upload, { kind: "samples-uploaded", count: 2, bytes: 24 });
    assert.equal(h.audio.finishes, 0); assert.equal(h.get("play").disabled, true);
    if (workerReleases) {
      await h.receive(finalScore(start.playId));
      assert.equal(h.get("play").disabled, false); assert.equal(worker.terminations, 0);
      assert.match(h.get("status").textContent, /timed out|admission/i);
    } else {
      await h.advance(9999); assert.equal(worker.terminations, 0);
      await h.advance(1); assert.equal(worker.terminations, 1);
      assert.equal(h.get("play").disabled, true); assert.match(h.get("status").textContent, /Reload the page/i);
    }
    assert.equal(worker.messages("play-samples-upload").length, 1);
    assert.equal(worker.messages("play-audio").length, 0); assert.equal(worker.messages("play-sample").length, 0);
    await h.close();
  }
});

test("malformed or duplicate current admission and success before admission cannot authorize finish or command handoff", async () => {
  const cases = [
    ...[undefined, null, -1, 1, 2.5, "2", NaN, 7941].map(count => ({ count })),
    { duplicate: true }, { earlySuccess: true }, { earlyError: true },
  ];
  for (const scenario of cases) {
    const h = await harness(); await h.preview(); const start = await h.begin(), worker = h.workers[0];
    const upload = await h.prepared(start, 2, false);
    if (scenario.duplicate) {
      await h.admitSamples(upload, 2);
      assert.equal(h.audio.finishes, 0); assert.equal(worker.messages("play-stop").length, 0);
      await h.admitSamples(upload, 2);
    } else if (scenario.earlySuccess) await h.reply(upload, { kind: "samples-uploaded", count: 2, bytes: 24 });
    else if (scenario.earlyError) await h.receive({ kind: "play-reply", playId: start.playId,
      rpcId: upload.rpcId, error: "sample endpoint could not start" });
    else await h.receive({ kind: "play-samples-admitted", playId: start.playId, rpcId: upload.rpcId, count: scenario.count });
    assert.equal(worker.messages("play-stop").length, 1); assert.equal(h.audio.stopStarts, 1);
    assert.equal(h.audio.finishes, 0); assert.equal(h.audio.attachments, 0); assert.deepEqual(h.audio.arms, []);
    await h.admitSamples(upload, 2); await h.reply(upload, { kind: "samples-uploaded", count: 2, bytes: 24 });
    assert.equal(worker.messages("play-stop").length, 1); assert.equal(h.audio.finishes, 0);
    await h.receive(finalScore(start.playId)); assert.equal(h.get("play").disabled, false);
    if (scenario.earlyError) assert.match(h.get("status").textContent, /sample endpoint could not start/);
    assert.equal(h.get("status").dataset.error, "true"); await h.close();
  }
});

test("stale admission cannot clear a replacement deadline and cancelled admission waiters cannot revive setup", async () => {
  for (const staleField of ["playId", "rpcId"]) {
    const h = await harness(); await h.preview(); const first = await h.begin(), worker = h.workers[0];
    const abandoned = await h.prepared(first, 2, false), originalAudio = h.audio;
    h.click("stop"); await flush(); await h.receive(finalScore(first.playId));
    await h.advance(10001);
    assert.equal(worker.terminations, 0); assert.equal(worker.messages("play-stop").length, 1);
    assert.equal(originalAudio.stopStarts, 1); assert.equal(originalAudio.finishes, 0);
    const next = await h.begin(), current = await h.prepared(next, 1, false);
    assert.ok(next.playId > first.playId); assert.ok(current.rpcId > abandoned.rpcId);
    await h.receive({ kind: "play-samples-admitted", playId: next.playId, rpcId: current.rpcId, count: 1,
      ...(staleField === "playId" ? { playId: first.playId } : { rpcId: abandoned.rpcId }) });
    await h.admitSamples(abandoned, 2);
    await h.reply(abandoned, { kind: "samples-uploaded", count: 2, bytes: 24 });
    assert.equal(h.audio.finishes, 0); assert.equal(h.audio.stopStarts, 0);
    await h.advance(9999); assert.equal(h.audio.stopStarts, 0);
    await h.advance(1);
    assert.equal(h.audio.stopStarts, 1); assert.equal(worker.last("play-stop").playId, next.playId);
    assert.equal(worker.messages("play-stop").length, 2); assert.equal(h.audio.finishes, 0);
    await h.receive(finalScore(next.playId)); assert.equal(h.get("play").disabled, false);
    assert.equal(originalAudio.finishes, 0); assert.equal(originalAudio.stopStarts, 1);
    assert.equal(worker.messages("play-audio").length, 0); await h.close();
  }
});

test("Window transfers one bounded sample descriptor and waits beyond a whole-bank timeout before exact upload receipt and finish", async () => {
  for (const mode of ["live", "replay"]) for (const count of [0, 2]) {
    const h = await harness(); await h.preview();
    if (mode === "replay") chooseRecording(h, [selectedRecording().file]);
    const start = await h.begin(mode), worker = h.workers[0];
    const upload = await h.prepared(start, count, false), audio = h.audio;
    assert.equal(upload.kind, "play-samples-upload"); assert.equal(upload.playId, start.playId);
    assert.equal(upload.generation, start.playId); assert.equal(upload.channels, 2);
    assert.equal(upload.timeoutMs, 10000);
    assert.deepEqual(upload.pcmLimits, { maxAssetBytes: 67108864, maxTotalBytes: 268435456, maxSamples: 7940 });
    assert.equal(upload.port, audio.samplePorts[0]); assert.equal(upload.port.transfers, 1);
    assert.equal(audio.sampleAttachments, 1); assert.deepEqual(worker.posts.find(row => row.value === upload).transfer, [upload.port]);
    assert.equal(Object.hasOwn(upload, "pcm"), false); assert.equal(worker.messages("play-sample").length, 0);
    assert.equal(audio.samples.length, 0); assert.equal(audio.finishes, 0); assert.equal(audio.attachments, 0);
    await h.admitSamples(upload, count);
    assert.equal(audio.finishes, 0); assert.equal(audio.attachments, 0);
    await h.advance(10001);
    assert.equal(worker.messages("play-stop").length, 0, "individual Worklet ACK deadlines do not become a whole-bank Window timer");
    assert.equal(audio.stopStarts, 0); assert.equal(audio.finishes, 0); assert.deepEqual(audio.arms, []);
    await h.reply(upload, { kind: "samples-uploaded", count, bytes: count ? 24 : 0 });
    assert.equal(audio.finishes, 1); assert.equal(audio.attachments, 1);
    assert.equal(worker.messages("play-samples-upload").length, 1);
    const kinds = h.traces.filter(row => ["open-sample-port", "finish", "open-command-port"].includes(row[0])).map(row => row[0]);
    assert.deepEqual(kinds, ["open-sample-port", "finish", "open-command-port"]);
    await h.reply(worker.last("play-audio"), null); await h.reply(worker.last("play-activate"), null);
    assert.equal(audio.arms.length, 1); assert.equal(audio.samples.length, 0);
    h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
    assert.equal(audio.stopStarts, 1); await h.close();
  }
});

test("Window refuses malformed sample handoff and aggregate count or byte evidence without PCM fallback or premature finish", async () => {
  const cases = [
    { missingSamplePort: true }, { samplePortFailure: new Error("actual sample handoff failed") },
    { sampleDescriptor: value => ({ ...value, generation: value.generation + 1 }) },
    { sampleDescriptor: value => ({ ...value, channels: 1 }) },
    { sampleDescriptor: value => ({ ...value, timeoutMs: 0 }) },
    { sampleDescriptor: value => ({ ...value, pcmLimits: { ...value.pcmLimits, maxSamples: 7939 } }) },
    { transferFailure: true },
    ...[null, { kind: "sample", count: 2, bytes: 24 }, { kind: "samples-uploaded", count: 1, bytes: 24 },
      { kind: "samples-uploaded", count: 2.5, bytes: 24 }, { kind: "samples-uploaded", count: 2, bytes: -1 },
      { kind: "samples-uploaded", count: 2, bytes: 1 }, { kind: "samples-uploaded", count: 2, bytes: 134217736 },
      { kind: "samples-uploaded", count: 2, bytes: 268435457 }, { kind: "samples-uploaded", count: 2, bytes: NaN },
      { kind: "samples-uploaded", count: 2 }].map(receipt => ({ receipt })),
    { remoteFailure: true },
  ];
  for (const faults of cases) {
    const h = await harness(faults); await h.preview(); const start = await h.begin(), worker = h.workers[0];
    if (faults.transferFailure) worker.failKind = "play-samples-upload";
    const upload = await h.prepared(start, 2, false);
    if (Object.hasOwn(faults, "receipt")) {
      await h.admitSamples(upload, 2);
      await h.reply(upload, faults.receipt);
    }
    if (faults.remoteFailure) await h.receive({ kind: "play-reply", playId: start.playId,
      rpcId: upload.rpcId, error: "actual sample ACK timed out" });
    assert.equal(h.audio.finishes, 0); assert.equal(h.audio.attachments, 0); assert.deepEqual(h.audio.arms, []);
    assert.equal(h.audio.samples.length, 0); assert.equal(worker.messages("play-sample").length, 0);
    assert.equal(worker.messages("play-activate").length, 0); assert.equal(worker.last("play-stop").playId, start.playId);
    const port = h.audio.samplePorts[0];
    if (port && port.transfers === 0) assert.equal(port.closes, 1, "untransferred endpoint is still Window-owned cleanup");
    await h.receive(finalScore(start.playId)); assert.equal(h.audio.stopStarts, 1);
    assert.equal(h.get("play").disabled, false); await h.close();
  }
});

test("cancelled sample acquisition or pending upload cannot let late descriptors and receipts finish a replacement owner", async () => {
  for (const phase of ["acquire", "upload"]) {
    const gate = deferred(), h = await harness(phase === "acquire" ? { samplePortGate: gate } : {});
    await h.preview(); const start = await h.begin(), worker = h.workers[0];
    const upload = await h.prepared(start, 2, false), oldAudio = h.audio, port = oldAudio.samplePorts[0];
    assert.equal(port.transfers, phase === "acquire" ? 0 : 1);
    h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
    assert.equal(oldAudio.stopStarts, 1); assert.equal(oldAudio.finishes, 0);
    delete h.faults.samplePortGate;
    const next = await h.begin(); assert.ok(next.playId > start.playId);
    if (phase === "acquire") { gate.resolve(); await flush(); assert.equal(port.closes, 1); assert.equal(port.transfers, 0); }
    else await h.reply(upload, { kind: "samples-uploaded", count: 2, bytes: 24 });
    assert.equal(oldAudio.finishes, 0); assert.equal(h.audio.finishes, 0); assert.equal(h.audio.sampleAttachments, 0);
    assert.equal(worker.messages("play-audio").length, 0); assert.equal(worker.messages("play-activate").length, 0);
    const handoff = await h.prepared(next, 0);
    await h.reply(handoff, null); await h.reply(worker.last("play-activate"), null);
    assert.equal(h.audio.sampleAttachments, 1); assert.equal(h.audio.finishes, 1);
    assert.equal(worker.messages("play-sample").length, 0); assert.equal(oldAudio.stopStarts, 1);
    h.click("stop"); await flush(); await h.receive(finalScore(next.playId)); await h.close();
  }
});

test("live and replay transfer one real command descriptor after PCM finish and wait for initial Worker drain before arm", async () => {
  for (const mode of ["live", "replay"]) {
    const stopGate = deferred(), h = await harness({ stopGate });
    const preview = await h.preview();
    if (mode === "replay") chooseRecording(h, [selectedRecording().file]);
    const start = await h.begin(mode), worker = h.workers[0];
    const attachment = await h.prepared(start, 2), audio = h.audio;
    assert.equal(h.opens[0].gesture, true);
    assert.equal(attachment.kind, "play-audio");
    assert.equal(attachment.generation, start.playId);
    assert.equal(attachment.queueCapacity, h.opens[0].options.audioLimits.queueCapacity);
    assert.equal(attachment.timeoutMs, 50);
    assert.equal(attachment.port, audio.commandPorts[0]);
    assert.equal(attachment.port.transfers, 1); assert.equal(audio.attachments, 1);
    const transfer = worker.posts.find(entry => entry.value === attachment);
    assert.deepEqual(transfer.transfer, [attachment.port]);
    assert.equal(transfer.transferCount, 1);
    assert.equal(audio.samples.length, 0); assert.equal(audio.sampleAttachments, 1); assert.deepEqual(audio.finishArgs, [[]]);
    const finishIndex = h.traces.findIndex(row => row[0] === "finish");
    const attachIndex = h.traces.findIndex(row => row[0] === "open-command-port");
    const postIndex = h.traces.findIndex(row => row[0] === "post" && row[1] === "play-audio");
    assert.ok(finishIndex < attachIndex && attachIndex < postIndex);
    assert.deepEqual(audio.arms, []); assert.equal(worker.messages("play-activate").length, 0);
    await h.reply(attachment, { kind: "audio-ready", commandsPending: false });
    const activation = worker.last("play-activate");
    assert.equal(audio.arms.length, 1); assert.equal(activation.startFrame, audio.arms[0]);
    await h.reply(activation, null);
    h.setNow(1300); await h.advance(8);
    assert.equal(audio.polls, 0); assert.ok(worker.last("play-render"));
    assert.equal(worker.messages("play-step").length, mode === "live" ? 1 : 0);
    assert.equal(worker.messages("play-commands").length, 0);
    assert.equal(worker.messages("play-ack").length, 0); assert.deepEqual(audio.commandsSeen, []);
    h.click("stop"); await flush();
    await h.receive(finalScore(start.playId));
    assert.equal(h.get("play").disabled, true, "Worker release does not prove host cleanup");
    stopGate.resolve(); await flush();
    assert.equal(h.get("play").disabled, false);
    assert.equal(h.get("title").textContent, preview.title);
    assert.equal(h.get("position").value, preview.position);
    assert.equal(audio.stopStarts, 1); assert.equal(audio.attachments, 1);
    await h.close();
  }
});

test("unavailable or refused direct handoff and late cancelled endpoints never fall back to Window command ownership", async () => {
  for (const failure of ["missing", "open", "generation", "capacity", "transfer", "ready"]) {
    const faults = failure === "missing" ? { missingCommandPort: true }
      : failure === "open" ? { commandPortFailure: new Error("actual attachment refused") }
      : failure === "generation" ? { commandDescriptor: value => ({ ...value, generation: value.generation + 1 }) }
      : failure === "capacity" ? { commandDescriptor: value => ({ ...value, queueCapacity: value.queueCapacity - 1 }) } : {};
    const h = await harness(faults); await h.preview();
    const start = await h.begin(), worker = h.workers[0];
    if (failure === "transfer") worker.failKind = "play-audio";
    const attachment = await h.prepared(start);
    if (failure === "ready") await h.reply(attachment, { kind: "audio-ready", commandsPending: true });
    assert.equal(worker.messages("play-commands").length, 0);
    assert.equal(worker.messages("play-ack").length, 0); assert.deepEqual(h.audio.commandsSeen, []);
    assert.equal(worker.messages("play-activate").length, 0); assert.deepEqual(h.audio.arms, []);
    assert.equal(worker.last("play-stop").playId, start.playId);
    if (h.audio.commandPorts.length) assert.equal(h.audio.commandPorts[0].closes, 1);
    await h.receive(finalScore(start.playId));
    assert.equal(h.get("play").disabled, false);
    assert.equal(h.audio.stopStarts, 1);
    await h.close();
  }
  const gate = deferred(), h = await harness({ commandPortGate: gate }); await h.preview();
  const start = await h.begin(), worker = h.workers[0];
  await h.prepared(start);
  const oldAudio = h.audio, port = oldAudio.commandPorts[0];
  assert.equal(port.transfers, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  // A late descriptor belongs to its cancelled setup even after a new play owns the page.
  delete h.faults.commandPortGate;
  const next = await h.begin(); assert.ok(next.playId > start.playId);
  gate.resolve(); await flush();
  assert.equal(port.closes, 1); assert.equal(port.transfers, 0);
  assert.equal(worker.messages("play-audio").length, 0);
  assert.equal(worker.messages("play-activate").length, 0);
  const nextAttachment = await h.prepared(next);
  await h.reply(nextAttachment, null); await h.reply(worker.last("play-activate"), null);
  assert.equal(h.audio.attachments, 1); assert.equal(h.audio.commandPorts[0].transfers, 1);
  assert.equal(oldAudio.stopStarts, 1);
  h.click("stop"); await flush(); await h.receive(finalScore(next.playId));
  await h.close();
});

test("natural completion requires the latest issued tick and settled direct commands without receiving a Window batch", async () => {
  const stopGate = deferred(), h = await harness({ stopGate }); await h.preview();
  const session = await h.launch(), worker = h.workers[0];
  h.setNow(1300); await h.advance(8);
  const done = (request, commandsPending = false) => h.receive({ kind: "play-step-done", pendingInputs: 0, playId: session.id,
    tickId: request.tickId, commandsPending, songNs: 0n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  const firstTick = worker.last("play-step"), firstRender = worker.last("play-render");
  await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: session.id, renderId: firstRender.renderId,
    completed: true, commandsPending: false, observedTick: 0 });
  await done(firstTick);
  assert.equal(worker.messages("play-stop").length, 0, "earlier completion cannot cover even a newer empty watermark");
  await h.advance(8);
  const pendingTick = worker.last("play-step"), pendingRender = worker.last("play-render");
  await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: session.id, renderId: pendingRender.renderId,
    completed: false, commandsPending: true, observedTick: pendingTick.tickId });
  await done(pendingTick, true);
  assert.equal(worker.messages("play-stop").length, 0);
  await h.advance(8);
  const beforeInput = worker.last("play-step"), beforeInputRender = worker.last("play-render");
  await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: session.id, renderId: beforeInputRender.renderId,
    completed: true, commandsPending: false, observedTick: beforeInput.tickId });
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1324 });
  await done(beforeInput);
  const captured = worker.last("play-step");
  assert.ok(captured.tickId > beforeInput.tickId); assert.equal(captured.events.length, 1);
  await done(captured);
  assert.equal(worker.messages("play-stop").length, 0, "issuing input invalidates the retained completion");
  await h.advance(8);
  const finalTick = worker.last("play-step"), finalRender = worker.last("play-render");
  await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: session.id, renderId: finalRender.renderId,
    completed: true, commandsPending: false, observedTick: finalTick.tickId });
  assert.equal(worker.messages("play-stop").length, 0, "the correlated input receipt still has to join");
  await done(finalTick);
  assert.equal(worker.last("play-stop").completed, true);
  assert.equal(worker.messages("play-commands").length, 0);
  assert.equal(worker.messages("play-ack").length, 0); assert.deepEqual(h.audio.commandsSeen, []);
  await h.receive(finalScore(session.id)); assert.equal(h.get("play").disabled, true);
  stopGate.resolve(); await flush();
  assert.match(h.get("status").textContent, /Song completed/);
  await h.close();

  for (const malformed of [{ commandsPending: null, observedTick: 0 },
    { commandsPending: false, observedTick: 1 }, { commandsPending: true, observedTick: 0 }]) {
    const replay = await harness(); await replay.preview(); chooseRecording(replay, [selectedRecording().file]);
    const playing = await replay.launch(0, "replay"); await replay.advance(8);
    const endpoint = replay.workers[0];
    await replay.receive({ kind: "play-render-done", commandsPending: false, pendingInputs: 0, playId: playing.id, renderId: endpoint.last("play-render").renderId,
      completed: true, ...malformed });
    assert.equal(endpoint.last("play-stop").completed, false);
    assert.equal(endpoint.messages("play-step").length, 0);
    await replay.receive(finalScore(playing.id));
    assert.match(replay.get("status").textContent, /malformed/);
    await replay.close();
  }
});

test("live and replay send only original raw presentation observations while one Worker report remains outstanding", async () => {
  for (const mode of ["live", "replay"]) {
    const h = await harness({ outputEvidence: { contextTime: 1.5, performanceTime: 1499.875 } });
    await h.preview(); if (mode === "replay") chooseRecording(h, [selectedRecording().file]);
    const session = await h.launch(0, mode), worker = h.workers[0];
    h.setNow(1500); await h.advance(8);
    const first = worker.last("play-render");
    assert.deepEqual(first, { kind: "play-render", playId: session.id, renderId: 1,
      timestamp: { contextTime: 1.5, performanceTime: 1499.875 }, observedNowMs: 1500 });
    assert.equal(h.audio.polls, 0); assert.equal(h.audio.outputReads, 1);
    const transfer = worker.posts.find(entry => entry.value === first);
    assert.equal(transfer.transferCount, 0, "Window has no Worklet report buffer to transfer");
    h.faults.outputEvidence = { contextTime: 1.75, performanceTime: 1750 };
    await h.advance(248);
    assert.equal(worker.messages("play-render").length, 1);
    assert.equal(h.audio.outputReads, 1, "a pending observation is neither replaced nor reacquired");
    assert.equal(h.audio.polls, 0);
    if (mode === "live") {
      const firstTick = worker.last("play-step");
      h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1756 });
      await h.receive({ kind: "play-step-done", pendingInputs: 0, playId: session.id, tickId: firstTick.tickId,
        commandsPending: false, songNs: 0n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
      const input = worker.last("play-step");
      assert.equal(input.events[0].hostNs, 1756000000n); assert.equal(input.events[0].sequence, 1n);
      assert.equal(worker.last("play-render"), first, "input keeps its own provenance while the output read waits");
    } else assert.equal(worker.messages("play-step").length, 0);
    await h.receive({ kind: "play-render-done", observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: first.renderId,
      completed: false, commandsPending: mode === "live" });
    await h.advance(8);
    const second = worker.last("play-render");
    assert.equal(second.renderId, 2); assert.deepEqual(second.timestamp, { contextTime: 1.75, performanceTime: 1750 });
    assert.equal(Object.hasOwn(second, "presentedNs"), false); assert.equal(Object.hasOwn(second, "report"), false);
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: second.renderId, completed: false });
    h.faults.outputFailure = Object.assign(new Error("temporarily unavailable"), { code: "unavailable" });
    await h.advance(8);
    const absent = worker.last("play-render");
    assert.equal(absent.timestamp, null); assert.equal(typeof absent.observedNowMs, "number");
    assert.equal(Object.hasOwn(absent, "report"), false);
    assert.equal(h.audio.polls, 0); assert.deepEqual(h.audio.commandsSeen, []);
    assert.equal(worker.messages("play-commands").length, 0); assert.equal(worker.messages("play-ack").length, 0);
    h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
  }
});

test("report request transport failures and Worker deadlines use joined cleanup without a host poll fallback", async () => {
  for (const failure of ["worker", "timeout", "post"]) {
    const stopGate = deferred(), h = await harness({ stopGate }); await h.preview();
    chooseRecording(h, [selectedRecording().file]);
    const session = await h.launch(0, "replay"), worker = h.workers[0];
    if (failure === "post") worker.failKind = "play-render";
    await h.advance(8);
    if (failure === "worker") await h.receive({ ...finalScore(session.id), kind: "play-error", released: true,
      message: "Actual direct report shape was refused" });
    else if (failure === "timeout") {
      const first = worker.last("play-render");
      await h.advance(10000);
      assert.equal(worker.messages("play-render").length, 1);
      assert.equal(worker.last("play-render"), first);
    }
    assert.equal(h.audio.polls, 0); assert.equal(h.audio.outputReads, 1);
    assert.equal(h.audio.stopStarts, 1); assert.equal(h.get("replay-play").disabled, true);
    assert.equal(worker.messages("play-commands").length, 0); assert.equal(worker.messages("play-ack").length, 0);
    if (failure !== "worker") {
      assert.equal(worker.last("play-stop").completed, false);
      await h.receive(finalScore(session.id));
    }
    assert.equal(h.get("play").disabled, true);
    stopGate.resolve(); await flush();
    assert.equal(h.get("play").disabled, false);
    assert.equal(h.get("status").dataset.error, "true");
    assert.match(h.get("status").textContent, failure === "worker" ? /Actual direct report shape was refused/
      : failure === "timeout" ? /Audio report Worker stopped responding/ : /injected Worker post failure/);
    await h.close();
  }
});

test("cancelled direct observations cannot finish a closing session or a later playback owner", async () => {
  for (const transition of ["stop", "pagehide"]) {
    const stopGate = deferred(), h = await harness({ stopGate }); await h.preview();
    const session = await h.launch(), oldWorker = h.workers[0], oldAudio = h.audio;
    await h.advance(8);
    const pending = oldWorker.last("play-render");
    const late = { kind: "play-render-done", pendingInputs: 0, playId: session.id, renderId: pending.renderId,
      completed: true, commandsPending: false, observedTick: oldWorker.last("play-step").tickId };
    if (transition === "stop") { h.click("stop"); await flush(); }
    else { h.window.emit("pagehide"); h.window.emit("pageshow", { persisted: true }); await flush(); }
    await h.receive(late, oldWorker);
    assert.equal(oldAudio.polls, 0); assert.equal(oldAudio.stopStarts, 1);
    assert.equal(oldWorker.messages("play-stop").at(-1)?.completed ?? false, false);
    await h.receive(finalScore(session.id), oldWorker);
    stopGate.resolve(); await flush();
    if (transition === "pagehide") { assert.equal(oldWorker.terminations, 1); await h.preview(); }
    chooseRecording(h, [selectedRecording().file]);
    const next = await h.launch(0, "replay"), currentWorker = h.workers.at(-1), audio = h.audio;
    assert.ok(next.id > session.id);
    await h.advance(8);
    const current = currentWorker.last("play-render");
    const stops = currentWorker.messages("play-stop").length;
    await h.receive(late, oldWorker);
    assert.equal(currentWorker.messages("play-stop").length, stops);
    assert.equal(audio.stopStarts, 0); assert.equal(audio.polls, 0);
    await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: next.id, renderId: current.renderId,
      completed: true, commandsPending: false, observedTick: 0 });
    assert.equal(currentWorker.last("play-stop").completed, true);
    await h.receive(finalScore(next.id));
    assert.match(h.get("status").textContent, /Recorded replay ended/);
    assert.equal(audio.stopStarts, 1); assert.equal(audio.polls, 0);
    await h.close();
  }
});

test("Window snapshots exact raw frames and output observations while leaving projection to the actual-rate Worker owner", async () => {
  const original = { contextTime: 1.5, performanceTime: 1499.875 }, reads = { context: 0, host: 0 };
  const outputObject = {
    get contextTime() { reads.context++; return original.contextTime; },
    get performanceTime() { reads.host++; return original.performanceTime; },
  };
  const h = await harness({ actualRate: 44100, contextFrame: 4294967299n, outputObject });
  await h.preview(); h.get("output-rate").value = "48000";
  const session = await h.launch(), worker = h.workers[0];
  assert.equal(session.start.rate, 44100); assert.equal(h.opens[0].options.contextOptions.sampleRate, 48000);
  assert.equal(h.audio.frameReads, 1); // Establish the actual output arm frame.
  h.setNow(1500);
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1500.125 });
  const down = worker.last("play-step");
  assert.equal(down.contextFrame, 4294967299n); assert.equal(Object.hasOwn(down, "audioNs"), false);
  assert.deepEqual(down.events, [{ hostNs: 1500125000n, key: 2, down: true, sequence: 1n }]);
  h.faults.contextFrame = 4294967300n;
  h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1500.25 });
  await h.receive({ kind: "play-step-done", pendingInputs: 0, playId: session.id, tickId: down.tickId,
    commandsPending: false, songNs: 0n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  const up = worker.last("play-step");
  assert.equal(up.contextFrame, 4294967300n); assert.equal(down.contextFrame, 4294967299n);
  assert.deepEqual(up.events, [{ hostNs: 1500250000n, key: 2, down: false, sequence: 2n }]);
  assert.equal(up.watermark, up.nowNs - 12000000n); assert.equal(Object.hasOwn(up, "audioNs"), false);
  await h.advance(8);
  const render = worker.last("play-render");
  assert.deepEqual(render.timestamp, { contextTime: 1.5, performanceTime: 1499.875 });
  assert.equal(render.observedNowMs, 1500); assert.deepEqual(reads, { context: 1, host: 1 });
  for (const field of ["presentedNs", "presentedHostNs", "report"]) assert.equal(Object.hasOwn(render, field), false);
  original.contextTime = 604800.125; original.performanceTime = 604800000.125;
  await h.advance(248);
  assert.equal(worker.messages("play-render").length, 1); assert.deepEqual(reads, { context: 1, host: 1 });
  assert.deepEqual(render.timestamp, { contextTime: 1.5, performanceTime: 1499.875 });
  assert.equal(h.audio.polls, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
});

test("raw observation acquisition failures never fall back and replay never acquires a live scheduling frame", async () => {
  for (const mode of ["live", "replay"]) {
    const faults = { outputFailure: Object.assign(new Error("no presentation yet"), { code: "unavailable" }) };
    const h = await harness(faults); await h.preview();
    if (mode === "replay") chooseRecording(h, [selectedRecording().file]);
    const session = await h.launch(0, mode), worker = h.workers[0];
    faults.frameFailure = new Error("actual context frame unavailable");
    await h.advance(8);
    assert.equal(worker.messages("play-step").length, 0);
    if (mode === "live") {
      assert.equal(h.audio.frameReads, 2); assert.equal(worker.messages("play-render").length, 0);
      assert.equal(worker.last("play-stop").completed, false);
      await h.receive(finalScore(session.id));
      assert.match(h.get("status").textContent, /actual context frame unavailable/);
    } else {
      assert.equal(h.audio.frameReads, 1);
      const request = worker.last("play-render");
      assert.deepEqual(request, { kind: "play-render", playId: session.id, renderId: 1,
        timestamp: null, observedNowMs: 1008 });
      assert.equal(worker.messages("play-stop").length, 0);
      await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: session.id, renderId: request.renderId,
        completed: false, commandsPending: false, observedTick: 0 });
      h.faults.outputFailure = Object.assign(new Error("actual output clock failed"), { code: "state" });
      await h.advance(8);
      assert.equal(worker.messages("play-render").length, 1);
      assert.equal(worker.last("play-stop").completed, false); assert.equal(h.audio.frameReads, 1);
      await h.receive(finalScore(session.id));
      assert.match(h.get("status").textContent, /actual output clock failed/);
    }
    assert.equal(h.audio.polls, 0); assert.deepEqual(h.audio.commandsSeen, []);
    assert.equal(h.audio.stopStarts, 1); assert.equal(h.get("play").disabled, false);
    await h.close();
  }
});

test("live page discovers Gamepads before gesture audio and shares genuine sources and acquisition sequences with HID and keyboard", async () => {
  const standard = nativeGamepad(1), ignored = nativeGamepad(3, { mapping: "" });
  const slots = [null, standard, null, ignored];
  const h = await harness({ gamepads: slots, hidSupported: true, hidDescriptors: [{ vendorId: 1, productId: 2 }] });
  const preview = await h.preview(); chooseControllerProfile(h, selectedControllerProfile().file);
  assert.equal(h.gamepadReads, 0);
  const start = await h.begin(), worker = h.workers[0];
  assert.equal(h.gamepadReads, 1); assert.equal(h.opens[0].gesture, true);
  assert.ok(h.traces.findIndex(row => row[0] === "gamepad-poll") < h.traces.findIndex(row => row[0] === "open"));
  assert.equal(h.hid.requests.length, 0); assert.equal(h.hid.gets, 1);
  assert.equal(start.gamepadDevices.length, 2);
  const [padSource, ignoredSource] = start.gamepadDevices.map(device => device.source), hidSource = start.hidDevices[0].source;
  assert.ok(padSource >= 3n && ignoredSource > padSource && hidSource > ignoredSource);
  assert.deepEqual(start.gamepadDevices, [
    { source: padSource, index: 1, id: standard.id, mapping: "standard", buttons: 9, axes: 1 },
    { source: ignoredSource, index: 3, id: ignored.id, mapping: "", buttons: 9, axes: 1 },
  ]);
  const attachment = await h.prepared(start); await h.reply(attachment, null); await h.reply(worker.last("play-activate"), null);
  const native = h.hidDevices[0], display = watchPlayDisplay(h), layoutReads = h.layoutReads;
  h.setNow(1300.125);
  standard.timestamp = 1300.0625;
  standard.buttons[0] = { value: 0.12345678901234566, pressed: true, touched: false };
  h.faults.onGamepadPoll = () => {
    delete h.faults.onGamepadPoll;
    native.emit("inputreport", { device: native, reportId: 7, timeStamp: 1300.125, data: new DataView(Uint8Array.from([7, 255]).buffer) });
  };
  await h.advance(8);
  const first = worker.last("play-step");
  assert.deepEqual(first.events.map(event => event.kind), ["hid", "gamepad"], "Window preserves acquisition order; Worker orders native timestamps");
  const acquired = first.events[1];
  assert.equal(acquired.source, padSource); assert.equal(first.events[0].source, hidSource);
  assert.equal(acquired.hostNs, 1300062500n); assert.equal(acquired.timestampMs, 1300.0625);
  assert.equal(first.events[0].hostNs, 1300125000n);
  assert.equal(acquired.sequence, first.events[0].sequence + 1n);
  assert.equal(acquired.axes[0], 0.12345678901234568);
  assert.equal(acquired.buttons[0].value, 0.12345678901234566);
  assert.equal(Object.hasOwn(acquired, "bytes"), false); assert.equal(Object.hasOwn(acquired, "key"), false);
  standard.axes[0] = 0.75; standard.buttons[0].value = 0.5;
  assert.equal(acquired.axes[0], 0.12345678901234568); assert.equal(acquired.buttons[0].value, 0.12345678901234566);
  standard.timestamp = 1300.1875;
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.25 });
  const done = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: request.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  await done(first);
  const second = worker.last("play-step");
  assert.ok(second.tickId > first.tickId);
  assert.equal(second.events[0].key, 2); assert.equal(second.events.length, 1);
  assert.ok(second.events[0].sequence > acquired.sequence);
  assert.equal(h.gamepadReads, 2, "key and ACK callbacks perform zero acquisition reads");
  await done(second); await h.advance(8);
  const third = worker.last("play-step");
  assert.equal(third.events.length, 1); assert.equal(third.events[0].kind, "gamepad");
  assert.equal(third.events[0].sequence, second.events[0].sequence + 1n);
  assert.equal(third.events[0].hostNs, 1300187500n, "poll time never replaces the browser sample timestamp");
  assert.equal(h.gamepadReads, 3, "only eligible cadence acquires the next snapshot");
  assert.equal(h.layoutReads, layoutReads); assert.deepEqual(display, []);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.equal(h.window.listeners.get("gamepadconnected")?.size ?? 0, 0);
  assert.equal(h.window.listeners.get("gamepaddisconnected")?.size ?? 0, 0);
  assert.equal(native.closes, 1); assert.equal(h.get("position").value, preview.position);
  await h.close();
});

test("keyboard HID touch pointer and ACK dispatch acquire zero Gamepad reads while eligible cadence alone polls", async () => {
  const pad = nativeGamepad(0, { timestamp: 1300.0625 });
  const h = await harness({ touchSupported: true, pointerSupported: true, gamepads: [pad], hidSupported: true,
    hidDescriptors: [{ vendorId: 1, productId: 2 }] });
  await h.preview(); enablePointers(h); chooseControllerProfile(h, selectedControllerProfile().file);
  const session = await h.launch(), worker = h.workers[0], reads = h.gamepadReads;
  h.setNow(1300.125);
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.125 });
  const first = worker.last("play-step");
  assert.equal(h.gamepadReads, reads, "keyboard callback must not query unrelated devices");
  const backing = Uint8Array.from([99, 7, 255, 88]), native = h.hidDevices[0];
  native.emit("inputreport", { device: native, reportId: 7, timeStamp: 1300.25, data: new DataView(backing.buffer, 1, 2) });
  assert.equal(h.gamepadReads, reads, "HID callback must not query unrelated devices");
  h.get("canvas").emit("pointerdown", { pointerType: "touch", pointerId: 91, timeStamp: 1300.375,
    offsetX: 120.25, offsetY: 180.5, pressure: 0.375 });
  assert.equal(h.gamepadReads, reads, "touch callback must not query unrelated devices");
  h.get("canvas").emit("pointerdown", nativePointer({ timeStamp: 1300.5 }));
  assert.equal(h.gamepadReads, reads, "pointer callback must not query unrelated devices");
  backing.fill(0);
  await h.advance(8);
  assert.equal(h.gamepadReads, reads, "pending tick suppresses cadence acquisition");
  assert.equal(worker.last("play-step"), first);
  await pointerStepDone(h, first);
  const acquired = worker.last("play-step");
  assert.equal(h.gamepadReads, reads, "ACK-triggered dispatch must not query unrelated devices");
  assert.deepEqual(acquired.events.map(event => event.kind), ["hid", "touch", "pointer", "pointer-button"]);
  assert.deepEqual(Array.from(acquired.events[0].data), [7, 255]);
  assert.deepEqual(acquired.events.map(event => event.hostNs), [1300250000n, 1300375000n, 1300500000n, 1300500000n]);
  assert.deepEqual([acquired.events[1].x, acquired.events[1].y, acquired.events[1].pressure], [120.25, 180.5, 0.375]);
  assert.ok(acquired.events.every(event => event.sequence > first.events[0].sequence));
  await pointerStepDone(h, acquired);
  assert.equal(h.gamepadReads, reads);
  await h.advance(8);
  const sampled = worker.last("play-step");
  assert.equal(h.gamepadReads, reads + 1, "the existing eligible interval reads once");
  assert.equal(sampled.events.length, 1); assert.equal(sampled.events[0].kind, "gamepad");
  assert.equal(sampled.events[0].hostNs, 1300062500n);
  await h.advance(8); assert.equal(h.gamepadReads, reads + 1, "unacknowledged sample applies backpressure");
  const retiredCadence = [...h.timers.values()].find(timer => timer.interval === 8).callback;
  h.click("stop"); await flush(); await h.receive(finalScore(session.id));
  retiredCadence(); await flush(); assert.equal(h.gamepadReads, reads + 1, "retired cadence cannot sample");
  await h.close();
});

test("cadence holds publication across native getter key reentry and stop cancels the entire snapshot", async () => {
  for (const action of ["key", "stop"]) {
    const firstPad = nativeGamepad(0), lastPad = nativeGamepad(1);
    const h = await harness({ gamepads: [firstPad, lastPad] }); await h.preview();
    const session = await h.launch(), worker = h.workers[0], reads = h.gamepadReads;
    assert.equal(reads, 1, "the genuine setup snapshot precedes cadence acquisition");
    const initialSequence = BigInt(session.start.gamepadDevices.length);
    h.setNow(1300.125);
    const cadence = [...h.timers.values()].find(timer => timer.interval === 8).callback;
    let getterReads = 0;
    Object.defineProperty(lastPad, "timestamp", { get() {
      getterReads++;
      if (action === "key") h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.125 });
      else h.click("stop");
      cadence();
      assert.equal(h.gamepadReads, reads + 1, "reentrant cadence cannot enter an active native poll");
      assert.equal(worker.messages("play-step").length, 0, "native snapshot acquisition cannot publish a partial or nested batch");
      return 1300.0625;
    } });
    await h.advance(8);
    assert.equal(getterReads, 1); assert.equal(h.gamepadReads, reads + 1);
    if (action === "key") {
      assert.equal(worker.messages("play-step").length, 1);
      const request = worker.last("play-step");
      assert.deepEqual(request.events.map(event => event.kind ?? "keyboard"), ["keyboard", "gamepad", "gamepad"]);
      assert.deepEqual(request.events.map(event => event.hostNs), [1300125000n, 1000000000n, 1300062500n]);
      assert.deepEqual(request.events.map(event => event.sequence), [1n, 2n, 3n].map(offset => initialSequence + offset));
      await pointerStepDone(h, request); assert.equal(h.gamepadReads, reads + 1);
      h.click("stop"); await flush();
    } else {
      assert.equal(worker.messages("play-step").length, 0);
      assert.equal(worker.last("play-stop").playId, session.id);
    }
    await h.receive(finalScore(session.id)); await h.close();
  }
});

test("actual main stop inside early or middle native getters ends acquisition before downstream controls and slots", async () => {
  const readOrder = ["index", "id", "mapping", "connected", "timestamp", "axes", "buttons",
    "axis0", "axis1", "button0", "pressed", "touched", "value", "button1"];
  for (const slot of [0, 1]) for (const boundary of ["id", "timestamp", "axis0", "pressed"]) {
    const pads = [0, 1, 2].map(index => nativeGamepad(index, { axes: [0, 0.5] }));
    const h = await harness({ gamepads: pads }); await h.preview();
    const session = await h.launch(), worker = h.workers[0], initialReads = h.gamepadReads;
    const native = pads[slot], reads = new Map(), axes = [0, 0.5];
    const firstButton = { pressed: false, touched: false, value: 0 };
    const buttons = Array.from({ length: 9 }, () => ({ pressed: false, touched: false, value: 0 }));
    buttons[0] = firstButton;
    const getter = (target, property, label, value) => Object.defineProperty(target, property, { get() {
      reads.set(label, (reads.get(label) ?? 0) + 1);
      if (label === boundary) h.click("stop");
      return value;
    } });
    for (const property of ["index", "id", "mapping", "connected", "timestamp"]) getter(native, property, property, native[property]);
    getter(native, "axes", "axes", axes); getter(native, "buttons", "buttons", buttons);
    getter(axes, 0, "axis0", 0); getter(axes, 1, "axis1", 0.5);
    getter(buttons, 0, "button0", firstButton); getter(buttons, 1, "button1", buttons[1]);
    for (const property of ["pressed", "touched", "value"]) getter(firstButton, property, property, firstButton[property]);
    let laterSlotReads = 0;
    Object.defineProperty(pads, slot + 1, { get() { laterSlotReads++; return nativeGamepad(slot + 1); } });
    h.setNow(1300); await h.advance(8);
    assert.equal(h.gamepadReads, initialReads + 1);
    assert.equal(reads.get(boundary), 1, `${slot}:${boundary}`);
    for (const downstream of readOrder.slice(readOrder.indexOf(boundary) + 1)) {
      assert.equal(reads.get(downstream) ?? 0, 0, `${slot}:${boundary} must not read ${downstream}`);
    }
    assert.equal(laterSlotReads, 0, "stopped session cannot acquire another native slot");
    assert.equal(worker.messages("play-step").length, 0, "neither the valid earlier device nor cancelled device may publish");
    assert.equal(worker.last("play-stop").playId, session.id);
    assert.equal(h.window.listeners.get("gamepaddisconnected")?.size ?? 0, 0);
    await h.receive(finalScore(session.id)); await h.close();
  }
});

test("unavailable or unsupported Gamepads preserve keyboard play while replay never acquires live device snapshots", async () => {
  const unavailable = await harness(); await unavailable.preview();
  const solo = await unavailable.launch();
  assert.equal(Object.hasOwn(solo.start, "gamepadDevices"), false);
  assert.equal(unavailable.gamepadReads, 0);
  unavailable.click("stop"); await flush(); await unavailable.receive(finalScore(solo.id)); await unavailable.close();

  for (const pads of [[], [nativeGamepad(0, { mapping: "" }), nativeGamepad(1, { buttons: nativeGamepad().buttons.slice(0, 8) })]]) {
    const h = await harness({ gamepads: pads }); await h.preview();
    const start = await h.begin(), worker = h.workers[0];
    assert.equal(start.gamepadDevices.length, pads.length);
    if (pads.length) {
      pads[0].connected = false;
      h.window.emit("gamepaddisconnected", { gamepad: pads[0] }); await flush();
      assert.equal(worker.messages("play-stop").length, 0, "unsupported devices do not participate even while preparation is pending");
    }
    const attachment = await h.prepared(start); await h.reply(attachment, null); await h.reply(worker.last("play-activate"), null);
    h.setNow(1300); await h.advance(8);
    assert.deepEqual(worker.last("play-step").events, [], "unsupported layouts never become guessed key bindings");
    assert.match(h.get("keys").textContent, /0 automatic standard Gamepad/);
    h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
    h.faults.gamepads = [nativeGamepad()];
    chooseRecording(h, [selectedRecording().file]);
    const reads = h.gamepadReads, replay = await h.launch(0, "replay");
    assert.equal(Object.hasOwn(replay.start, "gamepadDevices"), false);
    h.setNow(1500); await h.advance(8);
    assert.equal(h.gamepadReads, reads);
    assert.equal(h.window.listeners.get("gamepadconnected")?.size ?? 0, 0);
    assert.equal(worker.messages("play-step").filter(request => request.playId === replay.id).length, 0);
    h.click("stop"); await flush(); await h.receive(finalScore(replay.id)); await h.close();
  }
});

test("Gamepad source admission, disconnection, reentrant cancellation and failed detach preserve current ownership and cleanup barriers", async () => {
  for (const sources of [undefined, [], [3], [99n], [3n, 3n]]) {
    const h = await harness({ gamepads: [nativeGamepad()] }); await h.preview();
    const start = await h.begin(), worker = h.workers[0];
    await h.reply(start, { kind: "prepared", title: "Bad Gamepad source receipt", samples: 1, lanes: [0x11],
      startNs: 0n, opponentCount: 0, ...(sources === undefined ? {} : { gamepadSources: sources }) });
    assert.equal(worker.messages("play-samples-upload").length, 0); assert.equal(h.audio.samples.length, 0);
    assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(finalScore(start.playId)); assert.equal(h.get("play").disabled, false); await h.close();
  }
  const native = nativeGamepad(), h = await harness({ gamepads: [native] }); await h.preview();
  const start = await h.begin(), worker = h.workers[0];
  const stale = [...h.window.listeners.get("gamepaddisconnected")][0];
  native.connected = false; h.window.emit("gamepaddisconnected", { gamepad: native }); await flush();
  assert.equal(worker.last("play-stop").playId, start.playId);
  assert.equal(worker.messages("play-step").length, 0); await h.receive(finalScore(start.playId));
  const replacement = nativeGamepad(); h.faults.gamepads = [replacement];
  const next = await h.launch(); const messages = worker.posts.length;
  stale({ gamepad: native }); await flush();
  assert.equal(worker.posts.length, messages, "a retired native callback cannot close the replacement session");
  replacement.connected = false; h.window.emit("gamepaddisconnected", { gamepad: replacement }); await flush();
  assert.equal(worker.last("play-stop").playId, next.id);
  assert.equal(worker.messages("play-step").length, 0, "disconnect does not synthesize releases");
  await h.receive(finalScore(next.id)); await h.close();

  const reentrant = await harness({ gamepads: [nativeGamepad()] }); await reentrant.preview();
  const active = await reentrant.launch();
  reentrant.faults.onGamepadPoll = () => reentrant.click("stop");
  reentrant.setNow(1300); await reentrant.advance(8);
  assert.equal(reentrant.workers[0].messages("play-step").length, 0);
  assert.equal(reentrant.workers[0].last("play-stop").playId, active.id);
  await reentrant.receive(finalScore(active.id)); await reentrant.close();

  const dirty = await harness({ gamepads: [nativeGamepad()] }); await dirty.preview();
  const playing = await dirty.launch();
  dirty.faults.gamepadReadError = new Error("native polling failed");
  dirty.faults.gamepadCleanupError = new Error("listener removal also failed");
  dirty.setNow(1300); await dirty.advance(8);
  assert.equal(dirty.workers[0].messages("play-step").length, 0);
  await dirty.receive(finalScore(playing.id));
  assert.match(dirty.get("status").textContent, /Gamepad input failed/);
  assert.match(dirty.get("status").textContent, /Gamepad cleanup failed.*Reload/i);
  assert.equal(dirty.get("play").disabled, true);
  assert.equal(dirty.window.listeners.get("gamepaddisconnected")?.size ?? 0, 0);
  await dirty.close();
  const denied = await harness({ gamepads: [], gamepadReadError: new Error("browser denied Gamepad acquisition") });
  await denied.preview(); assert.equal(await denied.begin(), undefined);
  assert.equal(denied.opens.length, 0); assert.equal(denied.workers[0].messages("play-start").length, 0);
  await denied.close();
  const setup = await harness({ gamepads: [], gamepadListenerError: new Error("native listener setup refused"),
    gamepadCleanupError: new Error("partially registered listener removal failed") });
  await setup.preview(); assert.equal(await setup.begin(), undefined);
  assert.equal(setup.opens.length, 0);
  assert.equal(setup.get("play").disabled, true, "failed construction still owns its failed listener cleanup");
  assert.match(setup.get("status").textContent, /cleanup failed.*Reload/i);
  assert.equal(setup.window.listeners.get("gamepadconnected")?.size ?? 0, 0);
  assert.equal(setup.window.listeners.get("gamepaddisconnected")?.size ?? 0, 0);
  await setup.close();
});

test("custom Gamepad profiles remain immutable metadata across gesture setup, replay isolation and an explicit return to automatic mapping", async () => {
  const opening = deferred(), pad = nativeGamepad(0, { id: "custom pad", mapping: "", buttons: nativeGamepad().buttons.slice(0, 2) });
  const h = await harness({ gamepads: [pad, nativeGamepad(1)], gamepadAdmittedSources: [3n], openGate: opening });
  const preview = await h.preview(), selected = selectedGamepadProfile(), replacement = selectedGamepadProfile(256);
  assert.equal(h.get("gamepad-profile").disabled, false); assert.equal(h.get("gamepad-profile-clear").disabled, true);
  chooseGamepadProfile(h, selected.file);
  const label = h.get("gamepad-profile-name").textContent;
  for (const file of [selectedGamepadProfile(0).file, selectedGamepadProfile(1048577).file, { size: 128, name: "not a File" }]) {
    chooseGamepadProfile(h, file); assert.equal(h.get("gamepad-profile-name").textContent, label);
  }
  for (const lane of [...Array.from({ length: 9 }, (_, index) => 0x11 + index), ...Array.from({ length: 9 }, (_, index) => 0x21 + index)]) {
    h.get(`binding-${lane.toString(16)}`).value = "";
  }
  const worker = h.workers[0]; h.click("play");
  assert.equal(h.opens[0].gesture, true); assert.equal(h.gamepadReads, 1);
  assert.equal(worker.messages("play-start").length, 0);
  assert.equal(h.get("gamepad-profile").disabled, true); assert.equal(h.get("gamepad-profile-clear").disabled, true);
  chooseGamepadProfile(h, replacement.file); h.get("gamepad-profile-clear").emit("click");
  opening.resolve(h.audio); await flush(); delete h.faults.openGate;
  const start = worker.last("play-start");
  assert.equal(start.gamepadProfileFile, selected.file); assert.equal(start.keyPairs.length, 0);
  assert.deepEqual(start.gamepadDevices[0], { source: 3n, index: 0, id: "custom pad", mapping: "", buttons: 2, axes: 1 });
  const attachment = await h.prepared(start); await h.reply(attachment, null); await h.reply(worker.last("play-activate"), null);
  assert.match(h.get("keys").textContent, /1 profile-configured Gamepad.*1 unmatched/);
  const layout = h.layoutReads, writes = watchPlayDisplay(h);
  h.setNow(1300.125); pad.timestamp = 1300.0625;
  pad.buttons[1] = { value: 0.12345678901234566, pressed: true, touched: true };
  await h.advance(8);
  const input = worker.last("play-step").events;
  assert.equal(input.length, 1); assert.equal(input[0].kind, "gamepad"); assert.equal(input[0].source, 3n);
  assert.equal(input[0].hostNs, 1300062500n); assert.equal(input[0].buttons[1].value, 0.12345678901234566);
  assert.equal(Object.hasOwn(input[0], "key"), false); assert.equal(Object.hasOwn(input[0], "bytes"), false);
  assert.equal(h.layoutReads, layout); assert.deepEqual(writes, []);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.equal(h.get("position").value, preview.position); assert.equal(h.get("gamepad-profile-name").textContent, label);
  assert.equal(h.get("gamepad-profile-clear").disabled, false);
  chooseRecording(h, [selectedRecording().file]);
  const reads = h.gamepadReads, replay = await h.launch(0, "replay");
  assert.equal(Object.hasOwn(replay.start, "gamepadProfileFile"), false); assert.equal(Object.hasOwn(replay.start, "gamepadDevices"), false);
  await h.advance(8); assert.equal(h.gamepadReads, reads);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  assert.equal(h.get("gamepad-profile-name").textContent, label);
  h.click("gamepad-profile-clear"); h.click("bindings-reset"); delete h.faults.gamepadAdmittedSources;
  assert.equal(h.get("gamepad-profile-clear").disabled, true);
  const automatic = await h.launch();
  assert.equal(Object.hasOwn(automatic.start, "gamepadProfileFile"), false);
  assert.match(h.get("keys").textContent, /1 automatic standard Gamepad/);
  assert.equal(selected.reads, 0); assert.equal(replacement.reads, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(automatic.id)); await h.close();
});

test("custom Gamepad admission refuses missing source proof before PCM and joins failed or cancelled owners without fallback or stale callbacks", async () => {
  const custom = index => nativeGamepad(index, { mapping: "", buttons: nativeGamepad().buttons.slice(0, 2) });
  const unsupported = await harness(); await unsupported.preview();
  assert.equal(unsupported.get("gamepad-profile").disabled, true);
  chooseGamepadProfile(unsupported, selectedGamepadProfile().file);
  const ordinary = await unsupported.launch(); assert.equal(Object.hasOwn(ordinary.start, "gamepadProfileFile"), false);
  unsupported.click("stop"); await flush(); await unsupported.receive(finalScore(ordinary.id)); await unsupported.close();
  for (const sources of [undefined, [], [3], [99n], [3n, 3n]]) {
    const h = await harness({ gamepads: [custom(0)] }); await h.preview();
    const selected = selectedGamepadProfile(); chooseGamepadProfile(h, selected.file);
    const start = await h.begin(), worker = h.workers[0];
    await h.reply(start, { kind: "prepared", title: "Invalid custom source proof", samples: 1, lanes: [0x11],
      startNs: 0n, opponentCount: 0, ...(sources === undefined ? {} : { gamepadSources: sources }) });
    assert.equal(worker.messages("play-samples-upload").length, 0); assert.equal(h.audio.samples.length, 0);
    assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(finalScore(start.playId)); assert.equal(h.get("play").disabled, false);
    assert.match(h.get("gamepad-profile-name").textContent, /nonstandard-gamepad/); assert.equal(selected.reads, 0); await h.close();
  }
  const first = custom(0), ignored = custom(1), h = await harness({ gamepads: [first, ignored], gamepadAdmittedSources: [3n] });
  await h.preview(); const selected = selectedGamepadProfile(); chooseGamepadProfile(h, selected.file);
  const worker = h.workers[0], refused = await h.begin();
  await h.receive({ kind: "play-reply", playId: refused.playId, rpcId: refused.rpcId, error: "No Gamepad profile matched the actual device." });
  assert.equal(worker.messages("play-start").length, 1); assert.equal(worker.messages("play-samples-upload").length, 0);
  await h.receive(finalScore(refused.playId));
  const pending = await h.begin(), stale = [...h.window.listeners.get("gamepaddisconnected")][0];
  assert.equal(pending.gamepadProfileFile, selected.file, "a refused profile remains selected for an explicit retry");
  first.connected = false; h.window.emit("gamepaddisconnected", { gamepad: first }); await flush();
  assert.equal(worker.last("play-stop").playId, pending.playId, "a nonstandard candidate still owns preparation before matching returns");
  await h.reply(pending, { kind: "prepared", inputMode: "physical", startNs: 0n, samples: 1, lanes: [0x11], gamepadSources: [3n] });
  assert.equal(worker.messages("play-samples-upload").length, 0, "late preparation cannot resurrect the cancelled owner");
  await h.receive(finalScore(pending.playId));
  const nextPad = custom(0), nextIgnored = custom(1); h.faults.gamepads = [nextPad, nextIgnored];
  const next = await h.launch(), messages = worker.posts.length;
  stale({ gamepad: first }); await flush(); assert.equal(worker.posts.length, messages);
  nextIgnored.connected = false; h.window.emit("gamepaddisconnected", { gamepad: nextIgnored }); await flush();
  assert.equal(worker.messages("play-stop").filter(request => request.playId === next.id).length, 0);
  nextPad.connected = false; h.window.emit("gamepaddisconnected", { gamepad: nextPad }); await flush();
  assert.equal(worker.last("play-stop").playId, next.id);
  assert.equal(worker.messages("play-step").length, 0, "disconnect does not invent a release or substitute input");
  await h.receive(finalScore(next.id)); assert.equal(selected.reads, 0); await h.close();
});

test("HID permission remains an explicit gesture, retains profile metadata only and joins cancelled native ownership without stale page updates", async () => {
  const unsupported = await harness(); await unsupported.preview();
  assert.equal(unsupported.get("hid-authorize").disabled, true);
  assert.equal(unsupported.get("hid-input").disabled, true);
  await unsupported.close();
  for (const cancel of [false, true]) {
    const discover = deferred(), closing = deferred(), opening = deferred();
    const h = await harness({ hidSupported: true, hidAuthorizeGate: discover, hidCloseGate: closing,
      ...(cancel ? { hidOpenGate: opening } : {}) }); await h.preview();
    const selected = selectedControllerProfile(); chooseControllerProfile(h, selected.file);
    assert.equal(h.get("hid-input").checked, true); assert.equal(selected.reads, 0);
    const name = h.get("hid-profile-name").textContent;
    for (const size of [0, 1048577]) chooseControllerProfile(h, selectedControllerProfile(size).file);
    assert.equal(h.get("hid-profile-name").textContent, name);
    h.click("hid-authorize");
    assert.equal(h.hid.requests.length, 1, "the native chooser starts inside the synchronous click gesture");
    assert.equal(h.hid.requests[0].gesture, true); assert.deepEqual(Array.from(h.hid.requests[0].options.filters), []);
    assert.equal(h.hid.gets, 0); assert.equal(h.get("play").disabled, true);
    h.click("hid-authorize"); assert.equal(h.hid.requests.length, 1);
    if (cancel) {
      discover.resolve(h.hidDevices); await flush();
      assert.equal(h.hidDevices[0].opens, 1);
      h.window.emit("pagehide");
    }
    const staleStatus = h.get("hid-status").textContent;
    if (cancel) opening.resolve(); else discover.resolve(h.hidDevices);
    await flush();
    if (!cancel) {
      assert.ok(h.hidDevices.every(device => device.opens === 1 && device.closes === 1));
      assert.equal(h.get("play").disabled, true, "permission ownership remains busy until native closes settle");
    }
    closing.resolve(); await flush();
    assert.equal(selected.reads, 0); assert.equal(h.opens.length, 0);
    assert.ok(h.hidDevices.every(device => device.opens === device.closes));
    if (cancel) assert.equal(h.get("hid-status").textContent, staleStatus);
    else assert.equal(h.get("play").disabled, false);
    await h.close();
  }
});

function enablePointers(h) {
  h.get("pointer-input").checked = true; h.get("pointer-input").emit("change");
}
function nativePointer(fields = {}) {
  return { pointerType: "mouse", pointerId: -7, timeStamp: 1300.125, offsetX: 123.5, offsetY: -2.25, buttons: 1,
    getPredictedEvents() { assert.fail("predicted pointer samples cannot enter gameplay"); }, ...fields };
}
function nativePointerChild(fields = {}) {
  const sample = { pointerType: "mouse", pointerId: -7, isPrimary: true, timeStamp: 1300,
    clientX: 1000, clientY: 2000, buttons: 0, ...fields };
  for (const key of ["offsetX", "offsetY"]) Object.defineProperty(sample, key, {
    get() { assert.fail("coalesced pointer children have no authoritative canvas offset"); },
  });
  return sample;
}
function nativePointerHistory(children, fields = {}) {
  return nativePointer({ timeStamp: 1400, buttons: 0, isPrimary: true,
    offsetX: 100, offsetY: 200, clientX: 1000, clientY: 2000,
    getCoalescedEvents() { return children; }, ...fields });
}
function expandedPointerHistory() {
  return Array.from({ length: 256 }, (_, index) => nativePointerChild({ timeStamp: 1300 + index / 8,
    clientX: 1000 + index, buttons: index % 2 ? 0 : 7 }));
}
function pointerListenerCount(canvas) {
  return ["pointerdown", "pointermove", "pointerup", "pointercancel", "lostpointercapture"]
    .reduce((sum, kind) => sum + (canvas.listeners.get(kind)?.size ?? 0), 0);
}
async function pointerStepDone(h, request) {
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: request.playId, tickId: request.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
}

test("enabled Window pointers snapshot explicit button choices and forward original mixed batches without layout or score rendering", async () => {
  const h = await harness({ pointerSupported: true }); await h.preview();
  assert.equal(h.get("pointer-input").checked, false);
  for (const [type, lane, value] of [["mouse", "11", "1"], ["mouse", "12", "2"], ["mouse", "13", "3"],
    ["pen", "11", "1"], ["pen", "12", "2"], ["pen", "13", ""], ["mouse", "29", ""], ["pen", "29", ""]])
    assert.equal(h.get(`pointer-${type}-${lane}`).value, value);
  const plain = await h.launch();
  assert.equal(Object.hasOwn(plain.start, "pointerSetup"), false);
  assert.equal(h.get("canvas").emit("contextmenu").defaultPrevented, false, "ordinary keyboard play retains the browser menu");
  h.click("stop"); await flush(); await h.receive(finalScore(plain.id));
  enablePointers(h);
  const canvas = h.get("canvas"), baseline = pointerListenerCount(canvas);
  const priorMoves = new Set(canvas.listeners.get("pointermove") ?? []);
  const start = await h.begin(), worker = h.workers[0];
  assert.equal(canvas.emit("contextmenu").defaultPrevented, false, "pending pointer preparation has no menu authority");
  assert.equal(h.opens.at(-1).gesture, true, "audio acquisition remains in the original click gesture");
  assert.equal(start.inputMode, "physical");
  assert.deepEqual(start.pointerSetup.devices, [{ source: 3n, pointerType: "mouse" }, { source: 4n, pointerType: "pen" }]);
  const rows = Array.from({ length: start.pointerSetup.bindingWords.length / 4 }, (_, index) =>
    Array.from(start.pointerSetup.bindingWords.slice(index * 4, index * 4 + 4))).sort((a, b) => a[1] - b[1] || a[0] - b[0]);
  assert.deepEqual(rows, [[0x11, 3, 0, 1], [0x12, 3, 0, 2], [0x13, 3, 0, 3], [0x11, 4, 0, 1], [0x12, 4, 0, 2]]);
  assert.equal(h.get("pointer-input").disabled, true); assert.equal(h.get("pointer-mouse-11").disabled, true);
  h.get("pointer-mouse-11").value = "4"; h.get("pointer-mouse-11").emit("change");
  const commands = await h.prepared(start);
  assert.equal(canvas.emit("contextmenu").defaultPrevented, false, "admitted setup still waits for actual activation");
  await h.reply(commands, null); await h.reply(worker.last("play-activate"), null);
  assert.equal(canvas.emit("contextmenu").defaultPrevented, true);
  assert.equal(worker.messages("play-step").length, 0, "menu suppression does not invent pointer input");
  const staleMove = [...canvas.listeners.get("pointermove")].find(handler => !priorMoves.has(handler));
  assert.ok(staleMove);
  h.setNow(1300.125); const layout = h.layoutReads, display = watchPlayDisplay(h);
  canvas.emit("pointerdown", nativePointer({ buttons: 9 }));
  const first = worker.last("play-step");
  assert.deepEqual(first.events, [
    { kind: "pointer", pointerType: "mouse", hostNs: 1300125000n, source: 3n, sequence: 1n,
      code: 4294967289, control: 0, mode: 0, x: 123.5, y: -2.25 },
    { kind: "pointer-button", pointerType: "mouse", hostNs: 1300125000n, source: 3n, sequence: 2n,
      code: 4294967289, control: 1, state: 0 },
  ], "unbound control4 is filtered; editing a disabled field cannot redirect the frozen launch binding");
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.25 });
  canvas.emit("pointerdown", nativePointer({ pointerType: "pen", pointerId: 23, buttons: 2, timeStamp: 1300.5, offsetX: -8 }));
  await pointerStepDone(h, first);
  const mixed = worker.last("play-step");
  assert.deepEqual(mixed.events.map(row => [row.kind ?? "keyboard", row.sequence, row.hostNs]), [
    ["keyboard", 4n, 1300250000n], ["pointer", 5n, 1300500000n], ["pointer-button", 6n, 1300500000n],
  ]);
  assert.equal(mixed.events[1].source, 4n); assert.equal(mixed.events[1].mode, 0);
  assert.equal(mixed.events[1].x, -8); assert.equal(mixed.events[2].control, 2);
  await pointerStepDone(h, mixed);
  canvas.emit("pointerup", nativePointer({ buttons: 0, timeStamp: 1300.75 }));
  const release = worker.last("play-step");
  assert.deepEqual(release.events.map(row => [row.kind, row.control, row.state]), [["pointer", 0, undefined], ["pointer-button", 1, 1]]);
  await pointerStepDone(h, release);
  const count = worker.messages("play-step").length;
  canvas.emit("lostpointercapture", { pointerType: "mouse", pointerId: -7 });
  assert.equal(worker.messages("play-step").length, count); assert.equal(h.layoutReads, layout); assert.deepEqual(display, []);
  h.click("stop");
  assert.equal(canvas.emit("contextmenu").defaultPrevented, false, "closing immediately relinquishes menu authority");
  await flush(); await h.receive(finalScore(start.playId));
  assert.equal(canvas.emit("contextmenu").defaultPrevented, false);
  assert.equal(pointerListenerCount(canvas), baseline);
  const afterStop = worker.messages("play-step").length;
  staleMove(nativePointer({ buttons: 0, timeStamp: 1400 })); assert.equal(worker.messages("play-step").length, afterStop);
  chooseRecording(h, [selectedRecording().file]);
  const replay = await h.launch(0, "replay");
  assert.equal(Object.hasOwn(replay.start, "pointerSetup"), false); assert.equal(pointerListenerCount(h.get("canvas")), baseline);
  assert.equal(h.get("canvas").emit("contextmenu").defaultPrevented, false, "replay never owns live pointer menu suppression");
  h.get("canvas").emit("pointerdown", nativePointer({ timeStamp: 1400 }));
  assert.equal(worker.messages("play-step").length, afterStop);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  h.window.emit("pagehide"); h.window.emit("pageshow", { persisted: true }); await flush(); await h.preview();
  enablePointers(h);
  const replacement = await h.launch(), currentCanvas = h.get("canvas");
  assert.notEqual(currentCanvas, canvas);
  assert.equal(currentCanvas.emit("contextmenu").defaultPrevented, true);
  assert.equal(canvas.emit("contextmenu").defaultPrevented, false, "a retained old canvas cannot suppress its replacement's menu");
  h.click("stop"); await flush(); await h.receive(finalScore(replacement.id)); await h.close();
});

test("local discovery owns distinct pointer aggregates beside HID and Gamepad and only assigned descriptors reach the Worker", async () => {
  const h = await harness({ pointerSupported: true, gamepads: [nativeGamepad()], hidSupported: true,
    hidDescriptors: [{ vendorId: 1, productId: 2 }] });
  await h.preview(); enablePointers(h); chooseControllerProfile(h, selectedControllerProfile().file);
  await localCount(h, 2); h.click("local-discover"); await flush();
  assert.equal(h.opens.length, 0); assert.equal(h.hid.gets, 1);
  const choices = h.get("local-source-1").children;
  const mouse = choices.find(option => /mouse/i.test(option.textContent));
  const pen = choices.find(option => /pen/i.test(option.textContent));
  const pad = choices.find(option => option.textContent.startsWith("Gamepad "));
  const hid = choices.find(option => option.textContent.startsWith("HID "));
  assert.ok(mouse && pen && pad && hid);
  const allSources = [mouse, pen, pad, hid].map(option => BigInt(option.value));
  assert.equal(new Set(allSources).size, 4); assert.ok(allSources.every(source => source >= 3n));
  assert.equal(h.get("pointer-input").disabled, true); assert.equal(h.get("pointer-pen-11").disabled, true);
  const canvas = h.get("canvas"), worker = h.workers[0];
  canvas.emit("pointermove", nativePointer({ buttons: 0, timeStamp: 1000 }));
  assert.equal(worker.messages("play-step").length, 0, "discovery observations are not another player's input");
  localAssign(h, 1, BigInt(mouse.value)); localAssign(h, 2, BigInt(hid.value));
  const start = await h.begin();
  assert.deepEqual(start.pointerSetup.devices, [{ source: BigInt(mouse.value), pointerType: "mouse" }]);
  assert.ok(Array.from({ length: start.pointerSetup.bindingWords.length / 4 }, (_, index) =>
    BigInt(start.pointerSetup.bindingWords[index * 4 + 1]) | BigInt(start.pointerSetup.bindingWords[index * 4 + 2]) << 32n)
    .every(source => source === BigInt(mouse.value)));
  assert.deepEqual(Array.from(start.localPlanWords), [1, 1, Number(BigInt(mouse.value)), 0, 2, 1, Number(BigInt(hid.value)), 0]);
  assert.equal(h.hidDevices[0].opens, 1, "launch reuses the retained discovery resource");
  await h.reply(await h.prepared(start), null); await h.reply(worker.last("play-activate"), null);
  h.setNow(1300);
  canvas.emit("pointerdown", nativePointer({ pointerType: "pen", pointerId: 8, timeStamp: 1300 }));
  assert.equal(worker.messages("play-step").length, 0, "an unassigned aggregate channel stays outside the exact local roster");
  canvas.emit("pointerdown", nativePointer({ timeStamp: 1300 }));
  const accepted = worker.last("play-step");
  assert.equal(accepted.events.length, 2); assert.ok(accepted.events.every(row => row.source === BigInt(mouse.value)));
  await pointerStepDone(h, accepted);
  h.click("stop"); await flush(); await h.receive(localFinal(start));
  assert.equal(h.hidDevices[0].closes, 1); assert.equal(h.get("pointer-input").disabled, false);
  assert.match(h.get("local-status").textContent, /sources released/);
  h.click("local-discover"); await flush();
  const secondCanvas = h.get("canvas"), beforeRelease = worker.messages("play-step").length;
  secondCanvas.emit("pointerdown", nativePointer({ timeStamp: 1400 }));
  h.click("local-release"); await flush();
  assert.equal(worker.messages("play-step").length, beforeRelease, "release never fabricates queued gameplay input");
  assert.equal(h.get("pointer-input").disabled, false); await h.close();
});

test("pointer admission, whole-batch queue limits and capture cleanup refuse safely without reviving replacement ownership", async () => {
  for (const mutate of [() => undefined, devices => [], devices => devices.slice().reverse(),
    devices => [devices[0], devices[0]], devices => [{ ...devices[0], source: 99n }, devices[1]],
    devices => [{ ...devices[0], pointerType: "pen" }, devices[1]], devices => [{ ...devices[0], source: 3 }, devices[1]]]) {
    const h = await harness({ pointerSupported: true }); await h.preview(); enablePointers(h);
    const start = await h.begin(), worker = h.workers[0], pointerDevices = mutate(start.pointerSetup.devices);
    await h.reply(start, { kind: "prepared", title: "untrusted pointer ownership", samples: 0, lanes: [0x11], startNs: 0n,
      opponentCount: 0, ...(pointerDevices === undefined ? {} : { pointerDevices }) });
    assert.equal(worker.messages("play-samples-upload").length, 0); assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(finalScore(start.playId)); assert.equal(h.audio.stopStarts, 1); await h.close();
  }
  const duplicate = await harness({ pointerSupported: true }); await duplicate.preview(); enablePointers(duplicate);
  duplicate.get("pointer-mouse-12").value = "1";
  duplicate.click("play"); await flush();
  assert.equal(duplicate.workers[0].messages("play-start").length, 0); await duplicate.close();

  const h = await harness({ pointerSupported: true }); await h.preview(); enablePointers(h);
  const session = await h.launch(), worker = h.workers[0], canvas = h.get("canvas");
  h.setNow(1300); canvas.emit("pointerdown", nativePointer({ timeStamp: 1300 }));
  const pending = worker.last("play-step");
  for (let index = 0; index < 1023; index++) canvas.emit("pointermove", nativePointer({ timeStamp: 1300, offsetX: index }));
  assert.equal(worker.messages("play-step").length, 1);
  canvas.emit("pointermove", nativePointer({ timeStamp: 1300, buttons: 3 }));
  await flush();
  assert.equal(worker.last("play-stop").playId, session.id);
  await pointerStepDone(h, pending);
  assert.equal(worker.messages("play-step").length, 1, "a two-row overflow event publishes no partial position prefix");
  await h.receive(finalScore(session.id));
  const replacement = await h.launch();
  const moves = [...(h.get("canvas").listeners.get("pointermove") ?? [])];
  h.setNow(1500); h.get("canvas").emit("pointermove", nativePointer({ timeStamp: 1500, buttons: 0 }));
  const frontier = worker.last("play-step"); await pointerStepDone(h, frontier);
  h.get("canvas").emit("pointermove", nativePointer({ timeStamp: Number(frontier.watermark) / 1000000 - 0.125, buttons: 1 })); await flush();
  assert.equal(worker.last("play-stop").playId, replacement.id);
  await h.receive(finalScore(replacement.id));
  const count = worker.messages("play-step").length;
  for (const stale of moves) stale(nativePointer({ timeStamp: 1600, buttons: 0 }));
  assert.equal(worker.messages("play-step").length, count); await h.close();

  const capture = await harness({ pointerSupported: true, captureFailure: new Error("native pointer capture denied") });
  await capture.preview(); enablePointers(capture); const active = await capture.launch();
  capture.setNow(1300); capture.get("canvas").emit("pointerdown", nativePointer({ timeStamp: 1300 })); await flush();
  assert.equal(capture.workers[0].messages("play-step").length, 0);
  assert.equal(capture.workers[0].last("play-stop").playId, active.id);
  await capture.receive(finalScore(active.id)); await capture.close();

  const dirty = await harness({ pointerSupported: true }); await dirty.preview(); enablePointers(dirty);
  const held = await dirty.launch(); dirty.setNow(1300);
  dirty.get("canvas").emit("pointerdown", nativePointer({ timeStamp: 1300 }));
  const beforeClose = dirty.workers[0].messages("play-step").length;
  dirty.faults.pointerReleaseError = new Error("native pointer release remained unproven");
  dirty.click("stop"); await flush(); await dirty.receive(finalScore(held.id));
  assert.match(dirty.get("status").textContent, /Pointer cleanup failed.*Reload/i);
  assert.equal(dirty.get("play").disabled, true);
  assert.equal(dirty.workers[0].messages("play-step").length, beforeClose, "failed cleanup still cannot publish synthetic releases");
  await dirty.close();
});

test("Window coalesced pointer histories preserve every original DTO across bounded Worker chunks and null intermediate watermarks", async () => {
  const h = await harness({ pointerSupported: true }); await h.preview(); enablePointers(h);
  const session = await h.launch(), worker = h.workers[0], canvas = h.get("canvas");
  const display = watchPlayDisplay(h), layout = h.layoutReads;
  h.setNow(1400);
  const children = expandedPointerHistory(); let calls = 0, receiver;
  const dispatched = canvas.emit("pointermove", nativePointerHistory(children, {
    getCoalescedEvents() { calls++; receiver = this; return children; },
  }));
  assert.equal(calls, 1); assert.equal(receiver, dispatched);
  assert.equal(worker.messages("play-step").length, 1, "one frozen acquisition triggers one initial pump");
  children[0].clientX = -999; children.length = 0;
  const chunks = [];
  for (let index = 0; index < 4; index++) {
    const request = worker.last("play-step"); chunks.push(request);
    assert.equal(request.events.length, 256);
    assert.equal(request.watermark, index === 3 ? 1388000000n : null);
    if (index) assert.ok(request.tickId > chunks[index - 1].tickId);
    await pointerStepDone(h, request);
  }
  assert.equal(worker.messages("play-step").length, 4);
  const events = chunks.flatMap(request => request.events);
  assert.equal(events.length, 1024); assert.equal(events[0].sequence, 1n); assert.equal(events.at(-1).sequence, 1024n);
  assert.deepEqual(events.slice(0, 4).map(row => [row.kind, row.control, row.state, row.hostNs]), [
    ["pointer", 0, undefined, 1300000000n], ["pointer-button", 1, 0, 1300000000n],
    ["pointer-button", 2, 0, 1300000000n], ["pointer-button", 3, 0, 1300000000n],
  ]);
  assert.deepEqual(events.slice(-4).map(row => [row.control, row.state, row.hostNs]),
    [[0, undefined, 1331875000n], [1, 1, 1331875000n], [2, 1, 1331875000n], [3, 1, 1331875000n]]);
  assert.equal(events[0].x, 100); assert.equal(events[1020].x, 355);
  assert.ok(events.every(row => row.source === 3n && row.pointerType === "mouse" && row.code === 4294967289));
  assert.equal(events.some(row => row.hostNs === 1400000000n), false, "the dispatched parent is not appended to nonempty history");
  h.setNow(1401);
  canvas.emit("pointermove", nativePointerHistory([
    nativePointerChild({ pointerType: "pen", pointerId: 17, isPrimary: false, timeStamp: 1400.25, clientX: 998, buttons: 2 }),
    nativePointerChild({ pointerType: "pen", pointerId: 17, isPrimary: false, timeStamp: 1400.5, clientX: 1003, buttons: 0 }),
  ], { pointerType: "pen", pointerId: 17, isPrimary: false, timeStamp: 1401 }));
  const pen = worker.last("play-step");
  assert.deepEqual(pen.events.map(row => [row.kind, row.control, row.state, row.hostNs, row.sequence]), [
    ["pointer", 0, undefined, 1400250000n, 1025n], ["pointer-button", 2, 0, 1400250000n, 1026n],
    ["pointer", 0, undefined, 1400500000n, 1027n], ["pointer-button", 2, 1, 1400500000n, 1028n],
  ]);
  assert.equal(pen.events[0].x, 98); assert.equal(pen.events[2].x, 103);
  assert.ok(pen.events.every(row => row.source === 4n && row.code === 17));
  await pointerStepDone(h, pen);
  assert.equal(h.layoutReads, layout); assert.deepEqual(display, []);
  h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
});

test("Window rejects malformed, over-budget, late or retired pointer histories without publishing a valid prefix", async () => {
  const malformed = [
    children => { children[1].pointerType = "pen"; }, children => { children[1].buttons = 1; },
    children => { children[0].timeStamp = 1300.0000002; children[1].timeStamp = 1300.0000001; },
    (children, parent) => { parent.getCoalescedEvents = () => ({ length: 2 }); },
    (children, parent) => { parent.getCoalescedEvents = () => Array.from({ length: 257 }, () => nativePointerChild()); },
    (children, parent) => { const tooMany = expandedPointerHistory(); tooMany[255].buttons = 8;
      parent.buttons = 8; parent.getCoalescedEvents = () => tooMany; },
  ];
  for (const mutate of malformed) {
    const h = await harness({ pointerSupported: true }); await h.preview(); enablePointers(h);
    const session = await h.launch(), worker = h.workers[0]; h.setNow(1400);
    const children = [nativePointerChild(), nativePointerChild({ timeStamp: 1300.125 })], parent = nativePointerHistory(children);
    mutate(children, parent); h.get("canvas").emit("pointermove", parent); await flush();
    assert.equal(worker.messages("play-step").length, 0); assert.equal(worker.last("play-stop").playId, session.id);
    await h.receive(finalScore(session.id)); await h.close();
  }
  for (const overflow of [false, true]) {
    const h = await harness({ pointerSupported: true }); await h.preview(); enablePointers(h);
    const session = await h.launch(), worker = h.workers[0], canvas = h.get("canvas");
    h.setNow(1300);
    h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300 });
    const first = worker.last("play-step");
    h.window.emit("keyup", { code: "KeyZ", timeStamp: 1300.125 });
    const children = expandedPointerHistory();
    for (const row of children) row.timeStamp += 0.25;
    children[255].buttons = 1; // 255 four-DTO samples + final position and two actual Up edges = 1023.
    h.setNow(1400); canvas.emit("pointermove", nativePointerHistory(children, { buttons: 1 }));
    assert.equal(worker.messages("play-stop").length, 0);
    assert.equal(worker.messages("play-step").length, 1, "the in-flight keyboard request retains the exact shared pending boundary");
    if (overflow) {
      canvas.emit("pointermove", nativePointer({ timeStamp: 1400, buttons: 1 })); await flush();
      assert.equal(worker.last("play-stop").playId, session.id);
      await pointerStepDone(h, first);
      assert.equal(worker.messages("play-step").length, 1, "overflow discards the queued history instead of flushing a partial prefix");
    } else {
      await pointerStepDone(h, first);
      const accepted = [];
      for (let index = 0; index < 4; index++) {
        const request = worker.last("play-step"); assert.equal(request.events.length, 256);
        accepted.push(...request.events); assert.equal(request.watermark, index === 3 ? 1388000000n : null);
        await pointerStepDone(h, request);
      }
      assert.equal(accepted.length, 1024); assert.equal(accepted[0].key, 2); assert.equal(accepted[0].down, false);
      assert.equal(accepted.filter(row => row.kind === "pointer" || row.kind === "pointer-button").length, 1023);
      h.click("stop"); await flush();
    }
    await h.receive(finalScore(session.id)); await h.close();
  }
  const late = await harness({ pointerSupported: true }); await late.preview(); enablePointers(late);
  const committed = await late.launch(), worker = late.workers[0]; late.setNow(1400);
  late.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1400 });
  await pointerStepDone(late, worker.last("play-step"));
  late.get("canvas").emit("pointermove", nativePointerHistory([nativePointerChild(), nativePointerChild({ timeStamp: 1400 })])); await flush();
  assert.equal(worker.messages("play-step").length, 1, "a current parent cannot retime a child behind the global frontier");
  await late.receive(finalScore(committed.id));
  const next = await late.launch(), canvas = late.get("canvas"), before = worker.messages("play-step").length;
  canvas.emit("pointermove", nativePointerHistory([nativePointerChild()], {
    getCoalescedEvents() { late.click("stop"); return [nativePointerChild()]; },
  })); await flush();
  assert.equal(worker.messages("play-step").length, before, "native callback cancellation cannot publish its stale history");
  await late.receive(finalScore(next.id)); await late.close();
});

test("live HID discovers authorized interfaces automatically and queues original reports beside touch and keyboard without Window interpretation", async () => {
  const h = await harness({ hidSupported: true, touchSupported: true, hidAdmittedSources: [3n] });
  const preview = await h.preview(), selected = selectedControllerProfile(); chooseControllerProfile(h, selected.file);
  const start = await h.begin(), worker = h.workers[0];
  assert.equal(start.hidProfileFile, selected.file);
  assert.deepEqual(start.hidDevices, [{ source: 3n, vendorId: 1, productId: 2 }, { source: 4n, vendorId: 9, productId: 9 }]);
  assert.equal(h.hid.gets, 1); assert.equal(h.hid.requests.length, 0); assert.equal(h.opens[0].gesture, true);
  assert.ok(h.hidDevices.every(device => device.opens === 1));
  const native = h.hidDevices[0], buffer = Uint8Array.from([99, 7, 255, 0, 88]);
  const report = (timeStamp = 1300.125) => native.emit("inputreport", { device: native, timeStamp, reportId: 7, data: new DataView(buffer.buffer, 1, 3) });
  report(1000); assert.equal(worker.messages("play-step").length, 0, "preparation does not acquire gameplay sequence or forward raw reports");
  const commands = await h.prepared(start); await h.reply(commands, null);
  await h.reply(worker.last("play-activate"), null);
  assert.equal(h.get("hid-input").disabled, true); assert.equal(h.get("hid-profile").disabled, true);
  h.setNow(1300.125); const display = watchPlayDisplay(h), reads = h.layoutReads;
  report(); const first = worker.last("play-step");
  assert.deepEqual(first.events, [{ kind: "hid", hostNs: 1300125000n, source: 3n, sequence: 1n, reportId: 7, data: Uint8Array.from([7, 255, 0]) }]);
  buffer.fill(0); assert.deepEqual(Array.from(first.events[0].data), [7, 255, 0]);
  h.get("canvas").emit("pointerdown", { pointerType: "touch", pointerId: 1, timeStamp: 1300.25, offsetX: 120, offsetY: 180, pressure: 0.5 });
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.5 });
  const done = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: request.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  await done(first); const mixed = worker.last("play-step");
  assert.deepEqual(mixed.events.map(event => event.sequence), [2n, 3n]);
  assert.deepEqual(mixed.events.map(event => event.hostNs), [1300250000n, 1300500000n]);
  assert.equal(mixed.events[0].kind, "touch"); assert.equal(mixed.events[1].key, 2);
  await done(mixed);
  const before = worker.messages("play-step").length, ignored = h.hidDevices[1];
  ignored.emit("inputreport", { device: ignored, timeStamp: 1301, reportId: 1, data: new DataView(new ArrayBuffer(0)) });
  assert.equal(worker.messages("play-step").length, before);
  assert.equal(h.layoutReads, reads); assert.deepEqual(display, []); assert.equal(selected.reads, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.ok(h.hidDevices.every(device => device.closes === 1));
  assert.equal(h.get("position").value, preview.position);
  chooseRecording(h, [selectedRecording().file]);
  const replay = await h.launch(0, "replay");
  assert.equal(Object.hasOwn(replay.start, "hidProfileFile"), false); assert.equal(Object.hasOwn(replay.start, "hidDevices"), false);
  assert.equal(h.hid.gets, 1, "recorded playback never reopens a live HID owner");
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id)); await h.close();
});

test("HID source receipts precede PCM and matched disconnect or failed cleanup fences ownership while stale and unmatched sources cannot stop another play", async () => {
  for (const metadata of [{ hidSources: [3n, 3n], hidSourceCount: 2 }, { hidSources: [5n], hidSourceCount: 1 },
    { hidSources: [3], hidSourceCount: 1 }, { hidSources: [3n], hidSourceCount: 2 }, {}]) {
    const h = await harness({ hidSupported: true }); await h.preview(); chooseControllerProfile(h, selectedControllerProfile().file);
    const start = await h.begin(), worker = h.workers[0];
    await h.reply(start, { kind: "prepared", title: "Bad sources", samples: 1, lanes: [0x11], startNs: 0n, opponentCount: 0, ...metadata });
    assert.equal(worker.messages("play-samples-upload").length, 0); assert.equal(h.audio.samples.length, 0);
    assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(finalScore(start.playId)); assert.ok(h.hidDevices.every(device => device.closes === 1)); await h.close();
  }
  const connecting = deferred(), cancelled = await harness({ hidSupported: true, hidConnectGate: connecting });
  await cancelled.preview(); chooseControllerProfile(cancelled, selectedControllerProfile().file);
  assert.equal(await cancelled.begin(), undefined);
  cancelled.click("stop"); await flush(); assert.equal(cancelled.get("play").disabled, true);
  connecting.resolve(cancelled.hidDevices); await flush();
  assert.equal(cancelled.workers[0].messages("play-start").length, 0);
  assert.ok(cancelled.hidDevices.every(device => device.opens === 0 && device.closes === 0));
  assert.equal(cancelled.audio.stopStarts, 1);
  assert.equal(cancelled.get("play").disabled, false);
  await cancelled.close();
  for (const failClose of [false, true]) {
    const closing = deferred(), faults = { hidSupported: true, hidAdmittedSources: [3n], hidCloseGate: closing };
    if (failClose) faults.hidCloseError = new Error("owned HID close rejected");
    const h = await harness(faults); await h.preview(); chooseControllerProfile(h, selectedControllerProfile().file);
    for (const lane of [...Array.from({ length: 9 }, (_, index) => 0x11 + index), ...Array.from({ length: 9 }, (_, index) => 0x21 + index)]) h.get(`binding-${lane.toString(16)}`).value = "";
    const session = await h.launch(), worker = h.workers[0], matched = h.hidDevices[0];
    assert.equal(session.start.keyPairs.length, 0, "actual HID union coverage permits an explicitly unbound keyboard");
    const retiredListener = [...matched.listeners.get("inputreport")][0];
    h.hid.emit("disconnect", { device: h.hidDevices[1], timeStamp: 1300 }); await flush();
    assert.equal(worker.messages("play-stop").length, 0);
    h.hid.emit("disconnect", { device: matched, timeStamp: 1301 }); await flush();
    assert.equal(worker.last("play-stop").playId, session.id);
    assert.equal(worker.messages("play-step").length, 0, "disconnect never fabricates typed releases");
    assert.equal(matched.listeners.get("inputreport")?.size ?? 0, 0);
    await h.receive(finalScore(session.id)); assert.equal(h.get("play").disabled, true);
    closing.resolve(); await flush();
    assert.ok(h.hidDevices.every(device => device.closes === 1));
    if (failClose) {
      assert.match(h.get("status").textContent, /cleanup|close|reload/i);
      assert.equal(h.get("play").disabled, true);
    } else {
      assert.equal(h.get("play").disabled, false);
      const next = await h.launch(); const messages = worker.posts.length;
      retiredListener({ device: matched, reportId: 1, timeStamp: 1400, data: new DataView(new ArrayBuffer(0)) });
      await flush(); assert.equal(worker.posts.length, messages);
      h.click("stop"); await flush(); await h.receive(finalScore(next.id));
    }
    await h.close();
  }
});

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
  assert.equal(h.audio.samples.length, 0); assert.equal(h.audio.sampleAttachments, 1);
  assert.match(h.get("keys").textContent, /Recorded input playback/);
  const retainedDisplay = { status: h.get("status").textContent, position: h.get("position").value };
  h.setNow(1300);
  const key = h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  assert.equal(key.defaultPrevented, false);
  await h.advance(8);
  assert.equal(worker.messages("play-step").length, 0);
  const render = worker.last("play-render");
  assert.deepEqual(render.timestamp, { contextTime: 1.3, performanceTime: 1300 });
  assert.equal(Object.hasOwn(render, "presentedNs"), false);
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: start.playId, renderId: render.renderId,
    completed: false, songNs: 2350000000n, hits: 23n, misses: 4n, combo: 11n, preOriginInputs: 0 });
  assert.equal(h.get("status").textContent, retainedDisplay.status);
  assert.equal(h.get("position").value, retainedDisplay.position);
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

test("finite replay metadata snapshots the actual output grid before samples and unlimited replay keeps zero-argument finish", async () => {
  const h = await harness({ actualRate: 44100 });
  await h.preview();
  chooseRecording(h, [selectedRecording().file]);
  h.get("output-rate").value = "96000";
  h.get("live-start").value = "invalid live draft";
  const start = await h.begin("replay"), worker = h.workers[0];
  assert.equal(h.opens[0].gesture, true);
  assert.equal(h.opens[0].options.contextOptions.sampleRate, 96000);
  assert.equal(start.rate, 44100, "the applied AudioHost rate, not the requested preference, validates the recorded endpoint");
  let endNs = 1000000001n, endFrame = 4411n;
  const reads = { end: 0, frame: 0 };
  const metadata = { kind: "prepared", mode: "replay", title: "Finite recording", artist: "Fixture",
    notes: 1, samples: 1, lanes: [0x11], opponentCount: 0, startNs: 1000000000n, recordedUntilNs: 1000000000n,
    get endNs() { assert.equal(++reads.end, 1); return endNs; },
    get endFrame() { assert.equal(++reads.frame, 1); return endFrame; },
  };
  // Deliver the controlled endpoint object directly so a later property reread is observable.
  worker.emit("message", { data: { kind: "play-reply", playId: start.playId, rpcId: start.rpcId, result: metadata } });
  await flush();
  assert.deepEqual(reads, { end: 1, frame: 1 });
  assert.equal(h.audio.samples.length, 0);
  assert.equal(h.audio.finishes, 0);
  const sampleRequest = worker.last("play-samples-upload");
  assert.ok(sampleRequest);
  endNs = null; endFrame = 0n;
  await h.admitSamples(sampleRequest, 1);
  await h.reply(sampleRequest, { kind: "samples-uploaded", count: 1, bytes: 8 });
  assert.deepEqual(h.audio.finishArgs, [[4411n]]);
  assert.equal(h.audio.samples.length, 0, "Window receives only aggregate sample completion");
  await h.reply(worker.last("play-audio"), null);
  await h.reply(worker.last("play-activate"), null);
  assert.deepEqual(reads, { end: 1, frame: 1 });
  assert.match(h.get("details").textContent, /recorded end 1\.000000001 s/);
  assert.equal(h.audio.arms.length, 1);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  const unlimited = await h.launch(0, "replay");
  assert.deepEqual(h.audio.finishArgs, [[]], "unlimited replay uses the original finish payload without an undefined argument");
  assert.equal(unlimited.start.rate, 44100);
  h.click("stop"); await flush(); await h.receive(finalScore(unlimited.id));
  await h.close();
});

test("finite replay admission and finish failures clean up without transferring invalid setup or retrying unlimited output", async () => {
  for (const fields of [
    { mode: "replay", endNs: 1n }, { mode: "replay", endFrame: 4801n },
    { mode: "replay", endNs: null, endFrame: null }, { mode: "replay", endNs: 1n, endFrame: 4800n },
    { mode: "replay", endNs: 1n, endFrame: 4801 },
    { mode: "live", endNs: 1n, endFrame: 4801n }, // An unlimited live request cannot gain an unrequested endpoint.
  ]) {
    const h = await harness();
    await h.preview();
    chooseRecording(h, [selectedRecording().file]);
    const start = await h.begin(fields.mode), worker = h.workers[0];
    await h.reply(start, { kind: "prepared", title: "Invalid endpoint", notes: 1, samples: 1,
      lanes: [0x11], opponentCount: 0, startNs: 0n, ...fields });
    assert.equal(worker.messages("play-samples-upload").length, 0);
    assert.equal(h.audio.samples.length, 0);
    assert.deepEqual(h.audio.finishArgs, []);
    assert.deepEqual(h.audio.arms, []);
    assert.equal(h.audio.stopStarts, 1);
    assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(finalScore(start.playId));
    assert.equal(h.get("status").dataset.error, "true");
    await h.close();
  }
  const failure = new Error("actual finite finish rejected");
  const h = await harness({ finishFailure: failure });
  await h.preview();
  chooseRecording(h, [selectedRecording().file]);
  const start = await h.begin("replay"), worker = h.workers[0];
  await h.reply(start, { kind: "prepared", mode: "replay", title: "Finite endpoint", notes: 1, samples: 0,
    lanes: [0x11], opponentCount: 0, startNs: 0n, endNs: 1000000000n, endFrame: 52800n });
  await h.admitSamples(worker.last("play-samples-upload"), 0);
  await h.reply(worker.last("play-samples-upload"), { kind: "samples-uploaded", count: 0, bytes: 0 });
  assert.deepEqual(h.audio.finishArgs, [[52800n]]);
  assert.equal(h.audio.finishes, 1);
  assert.equal(worker.messages("play-commands").length, 0);
  assert.equal(worker.messages("play-activate").length, 0);
  assert.deepEqual(h.audio.arms, []);
  assert.equal(h.audio.stopStarts, 1);
  await h.receive(finalScore(start.playId));
  assert.match(h.get("status").textContent, /actual finite finish rejected/);
  assert.equal(h.get("replay-play").disabled, false);
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
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: replay.id, renderId: render.renderId,
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
    assert.equal(h.workers[0].messages("play-samples-upload").length, 0);
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
  assert.equal(cancelled.workers[0].messages("play-samples-upload").length, 0);
  assert.equal(cancelled.get("play").disabled, true);
  await cancelled.receive(finalScore(start.playId, { songNs: null, hits: null, misses: null, combo: null }));
  assert.equal(cancelled.get("play").disabled, false);
  assert.equal(file.reads, 0);
  await cancelled.close();

  const rejected = await harness();
  await rejected.preview();
  chooseRecording(rejected, [selectedRecording().file]);
  const replay = await rejected.launch(0, "replay");
  const worker = rejected.workers[0];
  await rejected.receive(finalScore(replay.id, { kind: "play-error", released: true,
    message: "actual replay output queue rejected prefix", replay: null, replayComplete: false, replayError: null }));
  assert.equal(worker.messages("play-ack").length, 0);
  assert.equal(rejected.audio.commandsSeen.length, 0);
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
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
      renderId: worker.last("play-render").renderId, completed: true });
    await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id,
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

test("touch acquisition snapshots cached CSS and backing extents across resize and lost capture without Window projection or rendering", async () => {
  const h = await harness({ touchSupported: true }); await h.preview();
  const session = await h.launch(), worker = h.workers[0], surface = h.get("canvas");
  for (const field of ["width", "height"]) Object.defineProperty(surface, field, {
    configurable: true, get: () => 1, set() { assert.fail("only the renderer owns backing dimensions"); },
  });
  h.window.devicePixelRatio = 1.5; h.resize(1280, 720);
  assert.deepEqual(worker.last("resize"), { kind: "resize", width: 1920, height: 1080 });
  await h.geometry({ playId: session.id });
  h.setNow(1300);
  const pointer = (kind, fields = {}) => surface.emit(kind, { pointerType: "touch", pointerId: 11,
    offsetX: 100.125, offsetY: 300.5, pressure: 0.375, timeStamp: 1300, ...fields });
  const ack = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: request.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  const display = watchPlayDisplay(h), firstReads = h.layoutReads;
  const renderRequests = worker.messages("play-render").length;
  pointer("pointerdown");
  const first = worker.last("play-step"), raw = first.events[0];
  assert.deepEqual(raw, { kind: "touch", hostNs: 1300000000n, sequence: 1n, contact: 1n, phase: 0,
    code: 11, x: 100.125, y: 300.5, pressure: 0.375, width: 1280, height: 720, surfaceWidth: 1920, surfaceHeight: 1080 });
  assert.equal(h.captures.length, 1, "a bar point is still acquired; common routing alone decides its destination");
  pointer("pointermove", { offsetX: 1200.25, offsetY: 700.5, timeStamp: 1300.125 });
  assert.equal(h.layoutReads, firstReads);
  h.resize(1001.5, 701.25);
  assert.deepEqual(worker.last("resize"), { kind: "resize", width: 1502, height: 1052 });
  await h.geometry({ playId: session.id });
  const resizedReads = h.layoutReads;
  pointer("pointerup", { offsetX: -25.5, offsetY: 900.25, timeStamp: 1300.25 });
  assert.equal(h.layoutReads, resizedReads);
  await ack(first);
  const queued = worker.last("play-step");
  assert.deepEqual(queued.events.map(event => [event.phase, event.width, event.height, event.surfaceWidth, event.surfaceHeight]), [
    [1, 1280, 720, 1920, 1080], [2, 1001.5, 701.25, 1502, 1052],
  ]);
  assert.deepEqual(queued.events.map(event => [event.x, event.y]), [[1200.25, 700.5], [-25.5, 900.25]]);
  assert.ok(queued.events.every(event => event.contact === 1n));
  assert.deepEqual(first.events[0], raw);
  await ack(queued);
  h.setNow(1301);
  pointer("pointerdown", { timeStamp: 1301 }); const held = worker.last("play-step");
  h.resize(0, 0); const zeroReads = h.layoutReads;
  pointer("lostpointercapture", { offsetX: NaN, offsetY: undefined, pressure: undefined, timeStamp: 1301.125 });
  pointer("lostpointercapture", { timeStamp: 1301.25 });
  await ack(held);
  const cancelled = worker.last("play-step");
  assert.deepEqual(cancelled.events, [{ kind: "touch", hostNs: 1301125000n, sequence: 5n, contact: 2n,
    phase: 3, code: 11, x: 100.125, y: 300.5, pressure: 0.375,
    width: 1001.5, height: 701.25, surfaceWidth: 1502, surfaceHeight: 1052 }]);
  assert.equal(h.layoutReads, zeroReads); assert.deepEqual(display, []);
  await ack(cancelled);
  assert.equal(worker.messages("play-render").length, renderRequests, "input acquisition does not request presentation work");
  assert.equal(h.audio.polls, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
});

test("touch capture preserves original samples and shared keyboard order without pointer-time layout or duplicate cancellation", async () => {
  const h = await harness({ touchSupported: true });
  const preview = await h.preview();
  assert.equal(h.get("touch-input").checked, true);
  const session = await h.launch(), worker = h.workers[0], surface = h.get("canvas");
  assert.equal(session.start.inputMode, "physical-contact");
  assert.equal(h.opens[0].gesture, true);
  assert.equal(h.get("touch-input").disabled, true);
  assert.equal(surface.dataset.touchInput, "true");
  h.get("touch-input").checked = false; // A programmatic draft change cannot change this owner.
  h.resize(480, 360);
  await h.geometry({ playId: session.id });
  const layoutReads = h.layoutReads, display = watchPlayDisplay(h);
  h.setNow(1300);
  const pointer = (type, overrides = {}) => surface.emit(type, { pointerType: "touch", pointerId: -2,
    offsetX: 120, offsetY: 90, pressure: 0.5, timeStamp: 1300, ...overrides });
  const done = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: request.tickId,
    songNs: 50000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  assert.equal(pointer("pointerdown", { pointerType: "mouse" }).defaultPrevented, false);
  assert.equal(pointer("pointerdown", { pointerType: "pen" }).defaultPrevented, false);
  assert.equal(pointer("pointermove").defaultPrevented, false);
  assert.equal(worker.messages("play-step").length, 0);
  assert.equal(pointer("pointerdown").defaultPrevented, true);
  const down = worker.last("play-step");
  assert.deepEqual(down.events, [{ kind: "touch", hostNs: 1300000000n, sequence: 1n, contact: 1n,
    phase: 0, code: 4294967294, x: 120, y: 90, pressure: 0.5, width: 480, height: 360, surfaceWidth: 480, surfaceHeight: 360 }]);
  assert.equal(h.captures.length, 1);
  assert.ok(h.traces.findIndex(row => row[0] === "capture") < h.traces.findIndex(row => row[1] === "play-step"));
  assert.equal(pointer("pointerdown", { offsetX: 400 }).defaultPrevented, false);
  pointer("pointermove", { offsetX: 400, offsetY: -10, pressure: 1.25, timeStamp: 1300.125 });
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.25 });
  pointer("pointerup", { offsetX: 500, timeStamp: 1300.5 });
  assert.equal(h.releases.length, 1);
  assert.equal(worker.last("play-step").tickId, down.tickId);
  assert.equal(h.layoutReads, layoutReads);
  await done(down);
  const mixed = worker.last("play-step");
  assert.deepEqual(mixed.events.map(event => event.sequence), [2n, 3n, 4n]);
  assert.deepEqual(mixed.events.map(event => event.hostNs), [1300125000n, 1300250000n, 1300500000n]);
  assert.deepEqual(mixed.events[0], { kind: "touch", hostNs: 1300125000n, sequence: 2n, contact: 1n,
    phase: 1, code: 4294967294, x: 400, y: -10, pressure: 1.25, width: 480, height: 360, surfaceWidth: 480, surfaceHeight: 360 });
  assert.deepEqual(mixed.events[1], { hostNs: 1300250000n, key: 2, down: true, sequence: 3n });
  assert.equal(mixed.events[2].phase, 2);
  assert.equal(mixed.events[2].contact, 1n);
  assert.equal(mixed.events.length, 3, "release-triggered lost capture must not append a second terminal event");
  await done(mixed);
  h.resize(960, 720);
  await h.geometry({ playId: session.id });
  const resizedReads = h.layoutReads;
  h.setNow(1301);
  pointer("pointerdown", { timeStamp: 1301 });
  const reused = worker.last("play-step");
  assert.equal(reused.events[0].contact, 2n);
  assert.equal(reused.events[0].sequence, 5n);
  assert.equal(reused.events[0].width, 960);
  pointer("lostpointercapture", { timeStamp: 1301.125, offsetX: NaN, offsetY: undefined, pressure: undefined });
  pointer("lostpointercapture", { timeStamp: 1301.25 });
  await done(reused);
  const canceled = worker.last("play-step");
  assert.deepEqual(canceled.events, [{ kind: "touch", hostNs: 1301125000n, sequence: 6n, contact: 2n,
    phase: 3, code: 4294967294, x: 120, y: 90, pressure: 0.5, width: 960, height: 720, surfaceWidth: 960, surfaceHeight: 720 }]);
  await done(canceled);
  assert.equal(h.layoutReads, resizedReads);
  assert.deepEqual(display, [], "pointer acquisition and accepted input receipts leave the Worker-owned HUD alone");
  h.setNow(1302);
  pointer("pointerdown", { pointerId: 7, timeStamp: 1302 });
  const held = worker.last("play-step"), beforeStop = worker.messages("play-step").length;
  assert.equal(held.events[0].contact, 3n);
  h.click("stop"); await flush();
  assert.equal(surface.dataset.touchInput, undefined);
  assert.ok(h.releases.some(entry => entry.id === 7));
  pointer("pointermove", { pointerId: 7, timeStamp: 1303 });
  assert.equal(worker.messages("play-step").length, beforeStop, "cleanup releases capture without inventing a judged Cancel");
  await h.receive(finalScore(session.id));
  assert.equal(h.get("touch-input").disabled, false);
  assert.equal(h.get("touch-input").checked, false);
  assert.equal(h.get("position").value, preview.position);
  assert.match(h.get("status").textContent, /Hits 3.*Misses 1/);
  await h.close();
});

test("coalesced touch movement forwards original ordered samples from the parent anchor and keeps absent or empty fallback", async () => {
  const h = await harness({ touchSupported: true }); await h.preview();
  const session = await h.launch(), worker = h.workers[0], surface = h.get("canvas");
  h.window.devicePixelRatio = 1.5; h.resize(480, 360); await h.geometry({ playId: session.id }); h.setNow(1301);
  const pointer = (kind, fields = {}) => surface.emit(kind, { pointerType: "touch", pointerId: -2,
    isPrimary: true, offsetX: 120.25, offsetY: 180.5, clientX: 100, clientY: 200,
    pressure: 0.5, timeStamp: 1300, ...fields });
  const ack = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: request.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  let acquired = 0, predictions = 0;
  const noPredictions = () => { predictions++; assert.fail("predicted input cannot enter acquisition"); };
  const notMovement = () => { assert.fail("only Move may request a coalesced list"); };
  const display = watchPlayDisplay(h), reads = h.layoutReads, renders = worker.messages("play-render").length;
  pointer("pointerdown", { getCoalescedEvents: notMovement, getPredictedEvents: noPredictions });
  const down = worker.last("play-step");
  const children = [
    { pointerType: "touch", pointerId: -2, isPrimary: true, timeStamp: 1300.125, clientX: 99.5, clientY: 198.25, pressure: 0.125 },
    { pointerType: "touch", pointerId: -2, isPrimary: true, timeStamp: 1300.125, clientX: 102.75, clientY: 200.5, pressure: 0.875 },
    { pointerType: "touch", pointerId: -2, isPrimary: true, timeStamp: 1300.25, clientX: 105, clientY: 205, pressure: 1.25 },
  ];
  for (const child of children) {
    Object.defineProperty(child, "offsetX", { get() { assert.fail("undispatched child offset is not the canvas coordinate"); } });
    Object.defineProperty(child, "offsetY", { get() { return NaN; } });
  }
  pointer("pointermove", { timeStamp: 1300.5, pressure: 9, getPredictedEvents: noPredictions,
    getCoalescedEvents() { acquired++; assert.equal(this.target, surface); return children; } });
  assert.equal(acquired, 1); assert.equal(worker.last("play-step"), down);
  children[0].clientX = 10000; children[0].pressure = 0; children[0].timeStamp = 9999;
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.375 });
  assert.equal(h.layoutReads, reads);
  h.resize(960, 720); const resizedReads = h.layoutReads;
  await h.geometry({ playId: session.id });
  pointer("pointerup", { timeStamp: 1300.75, offsetX: -10.25, pressure: 0, getCoalescedEvents: notMovement });
  await ack(down);
  const batch = worker.last("play-step");
  assert.deepEqual(batch.events, [
    { kind: "touch", hostNs: 1300125000n, sequence: 2n, contact: 1n, phase: 1, code: 4294967294,
      x: 119.75, y: 178.75, pressure: 0.125, width: 480, height: 360, surfaceWidth: 720, surfaceHeight: 540 },
    { kind: "touch", hostNs: 1300125000n, sequence: 3n, contact: 1n, phase: 1, code: 4294967294,
      x: 123, y: 181, pressure: 0.875, width: 480, height: 360, surfaceWidth: 720, surfaceHeight: 540 },
    { kind: "touch", hostNs: 1300250000n, sequence: 4n, contact: 1n, phase: 1, code: 4294967294,
      x: 125.25, y: 185.5, pressure: 1.25, width: 480, height: 360, surfaceWidth: 720, surfaceHeight: 540 },
    { hostNs: 1300375000n, key: 2, down: true, sequence: 5n },
    { kind: "touch", hostNs: 1300750000n, sequence: 6n, contact: 1n, phase: 2, code: 4294967294,
      x: -10.25, y: 180.5, pressure: 0, width: 960, height: 720, surfaceWidth: 1440, surfaceHeight: 1080 },
  ]);
  assert.equal(h.releases.length, 1); assert.equal(h.layoutReads, resizedReads);
  await ack(batch);
  h.setNow(1302);
  pointer("pointerdown", { timeStamp: 1302 }); const held = worker.last("play-step");
  pointer("pointermove", { timeStamp: 1302.125, offsetX: 400, offsetY: -20, clientX: undefined, clientY: undefined });
  pointer("pointermove", { timeStamp: 1302.25, offsetX: 500, offsetY: 600, pressure: 0.625,
    clientX: NaN, clientY: undefined, isPrimary: undefined,
    getCoalescedEvents() { acquired++; return []; }, getPredictedEvents: noPredictions });
  pointer("pointermove", { timeStamp: 1302.375, offsetX: 550, offsetY: -25, pressure: 0.75,
    clientX: undefined, clientY: undefined, getCoalescedEvents: null });
  pointer("pointercancel", { timeStamp: 1302.5, getCoalescedEvents: notMovement });
  await ack(held);
  const fallback = worker.last("play-step");
  assert.deepEqual(fallback.events.map(event => [event.phase, event.x, event.y, event.pressure, event.hostNs, event.sequence, event.contact]), [
    [1, 400, -20, 0.5, 1302125000n, 8n, 2n],
    [1, 500, 600, 0.625, 1302250000n, 9n, 2n],
    [1, 550, -25, 0.75, 1302375000n, 10n, 2n],
    [3, 120.25, 180.5, 0.5, 1302500000n, 11n, 2n],
  ]);
  assert.equal(acquired, 2); assert.equal(predictions, 0); assert.equal(h.releases.length, 2);
  assert.deepEqual(display, []); assert.equal(h.layoutReads, resizedReads);
  assert.equal(worker.messages("play-render").length, renders); assert.equal(h.audio.polls, 0);
  await ack(fallback);
  h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
});

test("a malformed coalesced list refuses its whole prefix before publication or a second contact lifecycle", async () => {
  for (const fault of ["method", "throw", "not-array", "sparse", "257", "identity", "type", "primary", "missing-primary",
    "pressure", "client", "overflow", "parent-anchor", "parent-offset", "parent-time", "parent-primary", "descending", "raw-descending", "after-parent", "watermark"]) {
    const h = await harness({ touchSupported: true }); await h.preview();
    const session = await h.launch(), worker = h.workers[0], surface = h.get("canvas"); h.setNow(1301);
    const pointer = (kind, fields = {}) => surface.emit(kind, { pointerType: "touch", pointerId: 7, isPrimary: true,
      offsetX: 120.25, offsetY: 180.5, clientX: 100, clientY: 200, pressure: 0.5, timeStamp: 1300, ...fields });
    pointer("pointerdown"); const down = worker.last("play-step");
    await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: down.tickId,
      songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
    const first = { pointerType: "touch", pointerId: 7, isPrimary: true, timeStamp: 1300.125, clientX: 101, clientY: 201, pressure: 0.25 };
    const last = { ...first, timeStamp: 1300.25, clientX: 102, pressure: 0.75 };
    const parent = { timeStamp: 1300.5, getCoalescedEvents: () => [first, last] };
    switch (fault) {
      case "method": parent.getCoalescedEvents = 1; break;
      case "throw": parent.getCoalescedEvents = () => { throw new Error("coalesced acquisition refused"); }; break;
      case "not-array": parent.getCoalescedEvents = () => ({ 0: first, length: 1 }); break;
      case "sparse": parent.getCoalescedEvents = () => [first, , last]; break;
      case "257": parent.getCoalescedEvents = () => Array.from({ length: 257 }, () => ({ ...first })); break;
      case "identity": last.pointerId = 8; break;
      case "type": last.pointerType = "pen"; break;
      case "primary": last.isPrimary = false; break;
      case "missing-primary": delete last.isPrimary; break;
      case "pressure": last.pressure = NaN; break;
      case "client": last.clientX = Infinity; break;
      case "overflow": last.clientY = 1e40; break;
      case "parent-anchor": parent.clientX = undefined; break;
      case "parent-offset": parent.offsetY = NaN; break;
      case "parent-time": parent.timeStamp = NaN; break;
      case "parent-primary": parent.isPrimary = "true"; break;
      case "descending": last.timeStamp = 1300.0625; break;
      case "raw-descending": first.timeStamp = 1300.0000002; last.timeStamp = 1300.0000001; break;
      case "after-parent": last.timeStamp = 1300.75; break;
      case "watermark": first.timeStamp = Number(down.watermark) / 1000000 - 0.125; break;
    }
    const reads = h.layoutReads, before = worker.messages("play-step").length;
    pointer("pointermove", parent); await flush();
    assert.equal(worker.messages("play-step").length, before, fault);
    assert.equal(worker.last("play-stop").playId, session.id, fault);
    assert.equal(worker.last("play-stop").completed, false);
    assert.equal(h.captures.length, 1); assert.equal(h.releases.length, 1);
    assert.equal(h.releases[0].id, 7); assert.equal(h.layoutReads, reads);
    pointer("pointerup", { timeStamp: 1301.125 });
    pointer("lostpointercapture", { timeStamp: 1301.25 });
    assert.equal(worker.messages("play-step").length, before, "failure publishes neither a valid prefix nor a synthetic release");
    await h.receive(finalScore(session.id));
    assert.equal(h.audio.stopStarts, 1); assert.equal(surface.dataset.touchInput, undefined);
    await h.close();
  }
});

test("256 coalesced samples share the exact 1024 pending boundary with keyboard input and cannot admit a capacity prefix", async () => {
  for (const overflow of [false, true]) {
    const h = await harness({ touchSupported: true }); await h.preview();
    const session = await h.launch(), worker = h.workers[0], surface = h.get("canvas"); h.setNow(1301);
    const pointer = (kind, fields = {}) => surface.emit(kind, { pointerType: "touch", pointerId: 11, isPrimary: true,
      timeStamp: 1300, offsetX: 120.25, offsetY: 180.5, clientX: 100, clientY: 200, pressure: 0.5, ...fields });
    const ack = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: request.tickId,
      songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
    pointer("pointerdown"); const down = worker.last("play-step"), reads = h.layoutReads;
    for (let batch = 0; batch < 3; batch++) {
      const children = Array.from({ length: 256 }, (_, index) => ({ pointerType: "touch", pointerId: 11,
        isPrimary: true, timeStamp: 1300.125, clientX: 100 + index / 4, clientY: 200, pressure: index / 256 }));
      pointer("pointermove", { timeStamp: 1301, getCoalescedEvents: () => children });
    }
    for (let index = 0; index < (overflow ? 255 : 256); index++) {
      h.window.emit(index % 2 ? "keyup" : "keydown", { code: "KeyZ", repeat: false, timeStamp: 1300.5 });
    }
    assert.equal(worker.messages("play-step").length, 1);
    assert.equal(worker.messages("play-stop").length, 0);
    assert.equal(h.layoutReads, reads);
    if (overflow) {
      // 768 Moves plus 255 keyboard events leave one slot. Neither of these
      // two original samples may be accepted, and the existing queue is not truncated.
      const children = [1300.75, 1300.875].map(timeStamp => ({ pointerType: "touch", pointerId: 11,
        isPrimary: true, timeStamp, clientX: 101, clientY: 201, pressure: 0.25 }));
      pointer("pointermove", { timeStamp: 1301, getCoalescedEvents: () => children }); await flush();
      assert.equal(worker.messages("play-step").length, 1);
      assert.equal(worker.last("play-stop").completed, false);
      assert.equal(h.captures.length, 1); assert.equal(h.releases.length, 1);
      await ack(down);
      assert.equal(worker.messages("play-step").length, 1, "a late old ACK cannot publish the rejected acquisition or queued suffix");
      await h.receive(finalScore(session.id));
      assert.match(h.get("status").textContent, /capacity/i);
    } else {
      await ack(down);
      const admitted = [];
      for (let index = 0; index < 4; index++) {
        const request = worker.last("play-step");
        assert.equal(request.events.length, 256);
        assert.equal(request.events[0].sequence, [2n, 258n, 514n, 770n][index]);
        assert.equal(request.events.at(-1).sequence, [257n, 513n, 769n, 1025n][index]);
        assert.equal(request.watermark, index === 3 ? request.nowNs - 12000000n : null);
        admitted.push(...request.events);
        await ack(request);
      }
      assert.equal(admitted.length, 1024);
      assert.equal(admitted.filter(event => event.kind === "touch").length, 768);
      assert.ok(admitted.slice(0, 768).every(event => event.phase === 1 && event.contact === 1n && event.hostNs === 1300125000n));
      assert.deepEqual([admitted[0].x, admitted[255].x, admitted[255].pressure], [120.25, 184, 0.99609375]);
      assert.ok(admitted.slice(768).every(event => event.key === 2 && event.hostNs === 1300500000n));
      assert.equal(worker.messages("play-stop").length, 0);
      pointer("pointerup", { timeStamp: 1301.25 });
      const released = worker.last("play-step");
      assert.equal(released.events.length, 1); assert.equal(released.events[0].sequence, 1026n);
      assert.equal(released.events[0].contact, 1n); assert.equal(released.events[0].phase, 2);
      await ack(released);
      h.click("stop"); await flush(); await h.receive(finalScore(session.id));
    }
    assert.equal(h.audio.stopStarts, 1); await h.close();
  }
});

test("coalesced movement retains held contact through paging and lost capture while cancellation fences a reentrant old acquisition", async () => {
  const { h, session, worker, surface } = await pagedTouchSession();
  const pointer = (kind, fields = {}) => surface.emit(kind, { pointerType: "touch", pointerId: -2, isPrimary: true,
    timeStamp: 1300, offsetX: 120.25, offsetY: 180.5, clientX: 100, clientY: 200, pressure: 0.5, ...fields });
  const ack = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: request.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  pointer("pointerdown"); const down = worker.last("play-step"), original = down.events.find(event => event.kind === "touch");
  pointer("pointermove", { timeStamp: 1300.5, getCoalescedEvents: () => [
    { pointerType: "touch", pointerId: -2, isPrimary: true, timeStamp: 1300.125, clientX: 101, clientY: 202, pressure: 0.25 },
    { pointerType: "touch", pointerId: -2, isPrimary: true, timeStamp: 1300.25, clientX: 104, clientY: 203, pressure: 0.75 },
  ] });
  h.get("local-page").value = "1"; h.get("local-page").emit("change"); await flush();
  const captured = h.captures.length;
  pointer("pointerdown", { pointerId: 91, timeStamp: 1300.5 });
  assert.equal(h.captures.length, captured + 1); assert.equal(worker.messages("play-page").length, 0);
  await ack(down);
  const moves = worker.last("play-step"), touches = moves.events.filter(event => event.kind === "touch");
  assert.deepEqual(touches.map(event => [event.phase, event.hostNs, event.contact, event.x, event.y, event.pressure]), [
    [1, 1300125000n, original.contact, 121.25, 182.5, 0.25],
    [1, 1300250000n, original.contact, 124.25, 183.5, 0.75],
    [0, 1300500000n, original.contact + 1n, 120.25, 180.5, 0.5],
  ]);
  assert.ok(touches.every(event => event.page === 0), "pending choice retains acquisition page for every original sample");
  assert.equal(worker.messages("play-page").length, 0);
  await ack(moves); const page = worker.last("play-page"); assert.equal(page.page, 1);
  await h.reply(page, { kind: "local-page", page: 1, touchVisible: false });
  assert.equal(h.releases.length, 0, "page adoption preserves the actual held contact");
  h.resize(0, 0);
  pointer("lostpointercapture", { timeStamp: 1300.75, offsetX: NaN, offsetY: undefined, pressure: undefined,
    getCoalescedEvents() { assert.fail("lost capture remains a single original terminal sample"); } });
  const cancelled = worker.last("play-step"), terminal = cancelled.events.find(event => event.kind === "touch");
  assert.deepEqual([terminal.phase, terminal.contact, terminal.x, terminal.y, terminal.pressure], [3, original.contact, 124.25, 183.5, 0.75]);
  assert.deepEqual([terminal.width, terminal.height, terminal.surfaceWidth, terminal.surfaceHeight],
    [original.width, original.height, original.surfaceWidth, original.surfaceHeight]);
  await ack(cancelled);
  h.resize(960, 720); h.setNow(1301);
  pointer("pointerdown", { timeStamp: 1301 }); const held = worker.last("play-step");
  const before = worker.messages("play-step").length;
  pointer("pointermove", { timeStamp: 1301.5, getCoalescedEvents() {
    h.click("stop");
    return [{ pointerType: "touch", pointerId: -2, isPrimary: true, timeStamp: 1301.25,
      clientX: 105, clientY: 206, pressure: 0.875 }];
  } });
  await flush();
  assert.equal(worker.messages("play-step").length, before);
  assert.equal(worker.last("play-stop").playId, session.id);
  assert.equal(h.releases.filter(release => release.id === -2).length, 1);
  await ack(held); await h.receive(localFinal(session.start));
  await localCount(h, 1);
  const replacement = await h.launch(); h.setNow(1400);
  pointer("pointermove", { timeStamp: 1400, getCoalescedEvents() { assert.fail("an old unowned native pointer cannot acquire movement"); } });
  const previousPosts = worker.messages("play-step").length;
  await ack(held);
  assert.equal(worker.messages("play-step").length, previousPosts);
  pointer("pointerdown", { timeStamp: 1400 }); const fresh = worker.last("play-step").events.find(event => event.kind === "touch");
  assert.equal(fresh.phase, 0); assert.equal(fresh.contact, 1n);
  assert.equal(worker.last("play-step").playId, replacement.id);
  h.click("stop"); await flush(); await h.receive(finalScore(replacement.id)); await h.close();
});

test("touch capability and preparation refusals precede PCM while bounded capture failure and replay keep separate owners", async () => {
  for (const faults of [{}, { touchSupported: true, missingPointerCapture: true }, { touchSupported: true, missingPointerRelease: true }]) {
    const h = await harness(faults); await h.preview();
    h.get("touch-input").checked = true;
    h.click("play"); await flush();
    assert.equal(h.opens.length, 0);
    assert.equal(h.workers[0].messages("play-start").length, 0);
    assert.match(h.get("status").textContent, /Pointer Events.*pointer capture/);
    assert.equal(h.get("touch-input").disabled, false);
    assert.equal(h.get("touch-input").checked, true);
    // Recorded playback needs neither live pointer capabilities nor this draft.
    chooseRecording(h, [selectedRecording().file]);
    const replay = await h.launch(0, "replay");
    assert.equal(Object.hasOwn(replay.start, "inputMode"), false);
    h.get("canvas").emit("pointerdown", { pointerType: "touch", pointerId: 1, timeStamp: 1300 });
    assert.equal(h.workers[0].messages("play-step").length, 0);
    assert.equal(h.captures.length, 0);
    h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
    await h.close();
  }
  const h = await harness({ touchSupported: true }); await h.preview();
  const worker = h.workers[0];
  for (const inputMode of [undefined, "physical", "legacy", null]) {
    const start = await h.begin();
    assert.equal(start.inputMode, "physical-contact");
    await h.reply(start, { kind: "prepared", startNs: 0n, opponentCount: 0,
      title: "Wrong route", notes: 1, samples: 1, lanes: [0x11], inputMode }, false);
    assert.equal(worker.messages("play-samples-upload").length, 0);
    assert.equal(h.audio.samples.length, 0);
    assert.deepEqual(h.audio.finishArgs, []);
    assert.deepEqual(h.audio.arms, []);
    await h.receive(finalScore(start.playId));
    assert.equal(h.get("touch-input").checked, true);
  }
  const session = await h.launch(), surface = h.get("canvas");
  h.setNow(1300);
  const reads = h.layoutReads;
  for (let pointerId = 0; pointerId < 256; pointerId++) surface.emit("pointerdown", {
    pointerType: "touch", pointerId, timeStamp: 1300, offsetX: 100, offsetY: 200, pressure: 0,
  });
  assert.equal(h.captures.length, 256);
  assert.equal(worker.messages("play-step").length, 1, "one pending receipt retains the bounded queued suffix");
  surface.emit("pointerdown", { pointerType: "touch", pointerId: 256, timeStamp: 1300, offsetX: 100, offsetY: 200, pressure: 0 });
  await flush();
  assert.equal(h.captures.length, 256);
  assert.equal(h.releases.length, 256);
  assert.equal(h.layoutReads, reads);
  assert.equal(worker.last("play-stop").completed, false);
  await h.receive(finalScore(session.id));
  assert.match(h.get("status").textContent, /Touch contact capacity exceeded/);
  assert.equal(h.audio.stopStarts, 1);
  assert.equal(surface.dataset.touchInput, undefined);
  await h.close();
});

test("Window explicitly negotiates physical input before PCM and preserves native keyboard acquisition through setup and capture cleanup", async () => {
  const h = await harness();
  await h.preview();
  h.get("binding-11").value = "KeyA";
  h.get("record").checked = true;
  const start = await h.begin(), worker = h.workers[0];
  assert.equal(start.inputMode, "physical");
  assert.equal(start.recordReplay, true);
  assert.equal(h.opens[0].gesture, true);
  assert.equal(worker.messages("play-samples-upload").length, 0);
  assert.deepEqual(Array.from(start.keyPairs).slice(0, 4), [0x16, 1, 0x11, 19]);
  const initial = await h.prepared(start, 1);
  assert.equal(h.audio.samples.length, 0); assert.equal(h.audio.sampleAttachments, 1);
  assert.equal(initial.kind, "play-audio");
  assert.equal(h.audio.arms.length, 0, "the physical session still waits for initial core command admission");
  await h.reply(initial, null);
  assert.equal(worker.messages("play-commands").length, 0);
  assert.equal(worker.messages("play-ack").length, 0);
  await h.reply(worker.last("play-activate"), null);
  h.setNow(1300);
  h.window.emit("keydown", { code: "KeyA", repeat: false, timeStamp: 1300 });
  const down = worker.last("play-step");
  assert.deepEqual(down.events, [{ hostNs: 1300000000n, key: 19, down: true, sequence: 1n }]);
  h.window.emit("keyup", { code: "KeyA", repeat: false, timeStamp: 1300.125 });
  assert.equal(worker.last("play-step").tickId, down.tickId);
  const done = tick => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: tick.tickId,
    songNs: 50000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  await done(down);
  const up = worker.last("play-step");
  assert.ok(up.tickId > down.tickId);
  assert.deepEqual(up.events, [{ hostNs: 1300125000n, key: 19, down: false, sequence: 2n }]);
  assert.equal(up.watermark, up.nowNs - 12000000n);
  assert.equal(down.contextFrame, 62400n); assert.equal(up.contextFrame, 62400n);
  assert.equal(Object.hasOwn(down, "audioNs"), false); assert.equal(Object.hasOwn(up, "audioNs"), false);
  await done(up);
  assert.equal(worker.messages("play-stop").length, 0);
  h.click("stop"); await flush();
  assert.equal(worker.last("play-stop").completed, false);
  await h.receive(finalScore(start.playId, { hits: 1n, misses: 0n, combo: 1n,
    replay: Uint8Array.from([66, 75, 82, 1]), replayComplete: false, replayError: null }));
  assert.match(h.get("export").textContent, /prefix/);
  assert.match(h.get("status").textContent, /Hits 1.*Misses 0.*Combo 1/);
  await h.close();
});

test("missing or changed physical-route metadata refuses all sample acquisition while retry and recorded replay keep separate ownership", async () => {
  const h = await harness();
  await h.preview();
  const worker = h.workers[0];
  for (const inputMode of [undefined, null, "legacy", "Physical", 1]) {
    const start = await h.begin();
    assert.equal(start.inputMode, "physical");
    await h.reply(start, { kind: "prepared", title: "Unadmitted route", notes: 1, samples: 1,
      lanes: [0x11], opponentCount: 0, startNs: 0n, ...(inputMode === undefined ? {} : { inputMode }) }, false);
    assert.equal(worker.messages("play-samples-upload").length, 0);
    assert.equal(h.audio.samples.length, 0);
    assert.deepEqual(h.audio.finishArgs, []);
    assert.deepEqual(h.audio.arms, []);
    assert.equal(worker.last("play-stop").playId, start.playId);
    assert.equal(worker.last("play-stop").completed, false);
    await h.receive(finalScore(start.playId));
    assert.equal(h.get("status").dataset.error, "true");
    assert.equal(h.get("play").disabled, false);
  }
  const retry = await h.launch();
  assert.equal(retry.start.inputMode, "physical");
  h.click("stop"); await flush(); await h.receive(finalScore(retry.id));
  chooseRecording(h, [selectedRecording().file]);
  h.get("live-start").value = "bad live start";
  h.get("live-end").value = "bad live end";
  h.get("binding-11").value = "unknown live code";
  h.get("record").checked = true;
  const replay = await h.launch(0, "replay");
  assert.equal(replay.start.mode, "replay");
  for (const field of ["inputMode", "keyPairs", "timing", "startNs", "endNs", "recordReplay"]) {
    assert.equal(Object.hasOwn(replay.start, field), false);
  }
  assert.deepEqual(h.audio.finishArgs, [[]]);
  const steps = worker.messages("play-step").length;
  h.window.emit("keydown", { code: "KeyA", repeat: false, timeStamp: 1300 });
  h.window.emit("keyup", { code: "KeyA", repeat: false, timeStamp: 1300 });
  assert.equal(worker.messages("play-step").length, steps);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  assert.equal(h.get("binding-11").value, "unknown live code");
  await h.close();
});

test("user gesture opens real host boundary before awaits, then transfers sample authority and arms after setup", async () => {
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
  assert.equal(h.audio.samples.length, 0); assert.equal(h.audio.sampleAttachments, 1);
  assert.equal(worker.messages("play-sample").length, 0);
  assert.equal(worker.last("play-samples-upload").port, h.audio.samplePorts[0]);
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
  assert.equal(h.workers[0].messages("play-samples-upload").length, 0);
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
    await h.advance(9999);
    assert.equal(worker.terminations, 0, "audio cleanup failure cannot terminate the pending genuine capture");
    assert.equal(h.get("play").disabled, true);
    await h.advance(1);
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
      await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
        renderId: worker.last("play-render").renderId, completed: true });
      await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id,
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

test("pagehide joins the old capture and audio owners before replacing both Workers and transferred canvas", async () => {
  const stopGate = deferred();
  const h = await harness({ stopGate });
  await h.preview();
  const session = await h.launch();
  h.click("stop");
  await flush();
  const oldWorker = h.workers[0];
  const oldRenderer = h.renderers[0], oldCanvas = h.get("canvas");
  h.window.emit("pagehide");
  h.window.emit("pageshow", { persisted: true });
  await flush();
  assert.equal(oldWorker.terminations, 0);
  assert.equal(h.workers.length, 1);
  assert.equal(h.get("canvas"), oldCanvas);
  await h.receive(finalScore(session.id), oldWorker);
  assert.equal(oldWorker.terminations, 0, "capture cannot cancel the real pending output cleanup");
  stopGate.resolve();
  await flush();
  assert.equal(oldWorker.terminations, 1); assert.equal(oldRenderer.terminations, 1);
  assert.equal(h.workers.length, 2); assert.equal(h.renderers.length, 2);
  const freshCanvas = h.get("canvas"); assert.notEqual(freshCanvas, oldCanvas);
  await h.receive(finalScore(session.id), oldWorker);
  await h.geometry({ playId: session.id, width: 1, height: 1 }, oldWorker);
  assert.equal(h.get("canvas"), freshCanvas);
  assert.equal(h.get("title").textContent, "No chart prepared");
  assert.equal(h.get("position").value, "0");
  assert.doesNotMatch(h.get("status").textContent, /Hits 3|Playback stopped/);
  assert.equal(h.get("play").disabled, true);
  await h.receive({ kind: "ready" }); await h.receive({ kind: "ready" }, h.renderers.at(-1));
  assert.equal(h.timers.size, 0, "pagehide releases the stop deadline rather than waiting ten seconds");
  await h.close();
});

test("live progress acknowledgements leave Window display untouched while queued physical input and final completion still join", async () => {
  const stopping = deferred();
  const h = await harness({ stopGate: stopping });
  const preview = await h.preview();
  const session = await h.launch(), worker = h.workers[0];
  const writes = watchPlayDisplay(h);
  let lastTick = 0, lastRender = 0;
  const acknowledge = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: request.tickId,
    songNs: 604800000000001n + BigInt(request.tickId), hits: BigInt(request.tickId), misses: 2n, combo: 3n, preOriginInputs: 0 });
  h.setNow(1300);
  for (let index = 0; index < 8; index++) {
    await h.advance(125); // Cross the old HUD cadence on every round.
    const tick = worker.last("play-step"), render = worker.last("play-render");
    assert.ok(tick.tickId > lastTick);
    assert.ok(render.renderId > lastRender);
    lastTick = tick.tickId; lastRender = render.renderId;
    if (index === 0) {
      h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1425 });
      h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1425 });
      assert.equal(worker.last("play-step").tickId, tick.tickId, "one in-flight watermark retains the captured native events");
    }
    await acknowledge(tick);
    if (index === 0) {
      const captured = worker.last("play-step");
      assert.ok(captured.tickId > tick.tickId);
      assert.deepEqual(captured.events, [
        { hostNs: 1425000000n, key: 2, down: true, sequence: 1n },
        { hostNs: 1425000000n, key: 2, down: false, sequence: 2n },
      ]);
      assert.equal(captured.watermark, captured.nowNs - 12000000n);
      lastTick = captured.tickId;
      await acknowledge(captured);
    }
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: render.renderId, completed: false });
    assert.deepEqual(writes, [], "committed progress must not perform even redundant status, position or canvas-caption DOM writes");
    assert.equal(worker.messages("play-stop").length, 0);
  }
  await h.advance(8);
  const tick = worker.last("play-step"), render = worker.last("play-render");
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: render.renderId, completed: true });
  assert.equal(worker.messages("play-stop").length, 0, "completion cannot erase the pending actual input response");
  assert.deepEqual(writes, []);
  await acknowledge(tick);
  assert.equal(worker.messages("play-stop").length, 1);
  assert.equal(worker.last("play-stop").completed, true);
  assert.equal(h.audio.stopStarts, 1);
  await h.receive(finalScore(session.id, { songNs: 604800000000017n, hits: 17n, misses: 2n, combo: 3n }));
  assert.equal(h.get("play").disabled, true);
  stopping.resolve(); await flush();
  assert.match(h.get("status").textContent, /Song completed\..*Hits 17.*Misses 2.*Combo 3/);
  assert.equal(h.get("title").textContent, preview.title);
  assert.equal(h.get("details").textContent, preview.details);
  assert.equal(h.get("position").value, preview.position);
  assert.equal(h.get("play").disabled, false);
  assert.ok(writes.some(write => write.id === "status"));
  await h.close();
});

test("replay progress stays on the Worker HUD while correlated completion and malformed-response summaries retain cleanup ownership", async () => {
  for (const outcome of ["complete", "uncorrelated"]) {
    const stopping = deferred();
    const h = await harness({ stopGate: stopping });
    const preview = await h.preview();
    chooseRecording(h, [selectedRecording().file]);
    const session = await h.launch(0, "replay"), worker = h.workers[0];
    const writes = watchPlayDisplay(h);
    let lastRender = 0;
    h.setNow(1300);
    for (let index = 0; index < 8; index++) {
      await h.advance(125);
      const render = worker.last("play-render");
      assert.ok(render.renderId > lastRender);
      lastRender = render.renderId;
      await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: render.renderId, completed: false,
        songNs: 604800000000001n + BigInt(index), hits: 9007199254740993n + BigInt(index), misses: 4n, combo: 11n, preOriginInputs: 0 });
      assert.equal(worker.messages("play-step").length, 0);
      assert.equal(worker.messages("play-stop").length, 0);
      assert.deepEqual(writes, [], "replay score and original-song position stay out of continuous Window presentation");
    }
    await h.advance(8);
    const render = worker.last("play-render");
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id + 100, renderId: render.renderId, completed: true });
    assert.equal(worker.messages("play-stop").length, 0, "a different playback owner cannot finish the current recording");
    assert.deepEqual(writes, []);
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
      renderId: render.renderId + (outcome === "uncorrelated" ? 1 : 0), completed: true,
      songNs: 604800000000008n, hits: 9007199254741000n, misses: 4n, combo: 11n, preOriginInputs: 0 });
    assert.equal(worker.messages("play-stop").length, 1);
    assert.equal(worker.last("play-stop").completed, outcome === "complete");
    assert.equal(h.audio.stopStarts, 1);
    await h.receive(finalScore(session.id, { hits: 23n, misses: 4n, combo: 11n }));
    assert.equal(h.get("replay-play").disabled, true, "a Worker receipt is not the audio owner's release");
    stopping.resolve(); await flush();
    assert.match(h.get("status").textContent, outcome === "complete"
      ? /Recorded replay ended\..*Hits 23.*Misses 4.*Combo 11/
      : /Audio report response was not correlated\..*Hits 23.*Misses 4.*Combo 11/);
    assert.equal(h.get("status").dataset.error, outcome === "complete" ? "false" : "true");
    assert.equal(h.get("title").textContent, preview.title);
    assert.equal(h.get("details").textContent, preview.details);
    assert.equal(h.get("position").value, preview.position);
    assert.equal(h.get("replay-play").disabled, false);
    await h.close();
  }
});

test("input and render reports each have one in-flight request and watermarks cannot cross unsent input", async () => {
  const h = await harness();
  await h.preview();
  const session = await h.launch();
  const worker = h.workers[0];
  await h.advance(40);
  assert.equal(worker.messages("play-step").length, 1);
  assert.equal(worker.messages("play-render").length, 1);
  assert.equal(h.audio.polls, 0);
  h.setNow(1300);
  for (let index = 0; index < 600; index++) {
    h.window.emit(index % 2 === 0 ? "keydown" : "keyup", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  }
  assert.equal(worker.messages("play-step").length, 1);
  async function done(step) {
    await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: step.tickId,
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
  assert.equal(final.watermark, final.nowNs - 12000000n);
  const report = worker.last("play-render");
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: report.renderId, completed: false });
  await h.advance(8);
  assert.equal(h.audio.polls, 0);
  assert.equal(worker.messages("play-render").length, 2);
  assert.equal(worker.messages("play-step").length, 4, "the unacknowledged final input batch still fences another step");
  await h.close();
});

test("setup and active command rejection preserve the actual Worker failure without a Window command retry", async () => {
  for (const setup of [true, false]) {
    const h = await harness();
    await h.preview();
    const worker = h.workers[0];
    let playId;
    if (setup) {
      const start = await h.begin();
      playId = start.playId;
      const requested = await h.prepared(start);
      assert.equal(requested.kind, "play-audio");
      await h.receive({ kind: "play-reply", playId, rpcId: requested.rpcId, error: "original audio admission failure" });
    } else {
      const session = await h.launch();
      playId = session.id;
      await h.receive(finalScore(playId, { kind: "play-error", released: true,
        message: "original audio admission failure", hits: 2n }));
    }
    assert.equal(worker.messages("play-ack").length, 0);
    assert.equal(worker.messages("play-commands").length, 0);
    assert.equal(h.audio.commandsSeen.length, 0, "prefix acknowledgment belongs only to the actual Worker/core path");
    if (setup) {
      assert.ok(worker.last("play-stop"));
      await h.receive(finalScore(playId, { kind: "play-error", released: true, message: "secondary Worker rejection", hits: 2n }));
    }
    assert.match(h.get("status").textContent, /original audio admission failure/);
    assert.doesNotMatch(h.get("status").textContent, /secondary Worker rejection/);
    assert.match(h.get("status").textContent, /Hits 2/);
    await h.close();
  }
});

test("natural completion joins captured input and command admission before the normal release handshake", async () => {
  const stopGate = deferred();
  const h = await harness({ stopGate, outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
  const saved = await h.preview();
  const session = await h.launch();
  h.setNow(1300);
  await h.advance(8);
  const worker = h.workers[0];
  const firstTick = worker.last("play-step");
  const firstReport = worker.last("play-render");
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1308 });
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
    renderId: firstReport.renderId, completed: true });
  assert.equal(worker.messages("play-stop").length, 0, "captured input and its earlier watermark must join");
  h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1308 });
  const stepDone = (request, commandsPending = false) => h.receive({ kind: "play-step-done", pendingInputs: 0, playId: session.id,
    tickId: request.tickId, commandsPending, songNs: 58000000n, hits: 4n, misses: 1n, combo: 3n, preOriginInputs: 0 });
  await stepDone(firstTick);
  const captured = worker.last("play-step");
  assert.deepEqual(captured.events, [
    { hostNs: 1308000000n, key: 2, down: true, sequence: 1n },
    { hostNs: 1308000000n, key: 2, down: false, sequence: 2n },
  ]);
  await stepDone(captured, true);
  assert.equal(worker.messages("play-stop").length, 0);
  assert.equal(worker.messages("play-ack").length, 0, "direct audio work is owned by the Worker");
  assert.equal(worker.messages("play-stop").length, 0, "new input/audio invalidated the older completion receipt");
  await h.advance(8);
  const finalReport = worker.last("play-render");
  const finalTick = worker.last("play-step");
  assert.ok(finalReport.renderId > firstReport.renderId);
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
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

test("output observations retain original raw time without host polling, extrapolation or Window filtering", async () => {
  const h = await harness({ outputEvidence: { contextTime: 1.3, performanceTime: 1300 } });
  await h.preview();
  const session = await h.launch();
  h.setNow(1300);
  await h.advance(8);
  const worker = h.workers[0];
  assert.equal(h.audio.polls, 0);
  assert.equal(h.audio.outputReads, 1);
  assert.equal(worker.messages("play-render").length, 1);
  const first = worker.last("play-render");
  assert.deepEqual(first.timestamp, { contextTime: 1.3, performanceTime: 1300 });
  assert.equal(first.observedNowMs, 1300, "the original acquisition observation is not rewritten at receipt time");
  const outputIndex = h.traces.findIndex(row => row[0] === "output-timestamp");
  const requestIndex = h.traces.findIndex(row => row[0] === "post" && row[1] === "play-render");
  assert.ok(outputIndex >= 0 && outputIndex < requestIndex);
  assert.equal(Object.hasOwn(first, "report"), false);
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: first.renderId, completed: false });
  h.faults.outputEvidence = { contextTime: 1.29, performanceTime: 1308 };
  await h.advance(8);
  const regressed = worker.last("play-render");
  assert.deepEqual(regressed.timestamp, { contextTime: 1.29, performanceTime: 1308 });
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: regressed.renderId, completed: false });
  h.faults.outputEvidence = { contextTime: 1.3, performanceTime: 1316 };
  await h.advance(8);
  assert.deepEqual(worker.last("play-render").timestamp, { contextTime: 1.3, performanceTime: 1316 });
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
      assert.equal(report.timestamp, null);
      assert.equal(typeof report.observedNowMs, "number");
      await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id, renderId: report.renderId, completed: false });
      await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id,
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
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
      renderId: worker.last("play-render").renderId, completed });
    assert.equal(worker.messages("play-stop").length, 1);
    await h.receive(finalScore(session.id));
    assert.equal(h.get("status").dataset.error, "true");
    assert.match(h.get("status").textContent, /completion evidence was malformed/);
    await h.close();
  }
});

test("Window forwards coarse and regressing raw observations so the Worker owns the retained presentation frontier", async () => {
  const h = await harness({ outputEvidence: { contextTime: 1.5, performanceTime: 1500 } });
  await h.preview();
  const session = await h.launch();
  const worker = h.workers[0];
  h.setNow(1500);
  await h.advance(8);
  const initial = worker.last("play-render");
  assert.deepEqual(initial.timestamp, { contextTime: 1.5, performanceTime: 1500 });
  let previous = initial;
  for (const [contextTime, performanceTime] of [
    [1.5, 1504], [1.5009765625, 1500], [1.5009765625, 1502.125], [1.5, 1510],
    [1.501953125, 1501], [1.501953125, 1502.125], [1.501953125, 1510],
  ]) {
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
      renderId: previous.renderId, completed: false });
    h.faults.outputEvidence = { contextTime, performanceTime };
    await h.advance(8);
    const next = worker.last("play-render");
    assert.ok(next.renderId > previous.renderId);
    assert.deepEqual(next.timestamp, { contextTime, performanceTime });
    assert.equal(Object.hasOwn(next, "presentedNs"), false);
    assert.equal(Object.hasOwn(next, "presentedHostNs"), false);
    previous = next;
  }
  assert.equal(worker.messages("play-stop").length, 0);
  assert.equal(h.audio.stopStarts, 0);
  await h.close();
});

function chooseMultiplayer(h, host = true) {
  h.get("multiplayer").checked = true;
  h.get("multiplayer-mode").value = "peer";
  h.get("multiplayer-url").value = "https://example.test:4433/competition";
  h.get("multiplayer-role").value = host ? "host" : "join";
  h.get("multiplayer").emit("change");
}

const ROOM_URL = "https://example.test:4433/rooms/source_fixture";
const ROOM_PARTICIPANT = 18446744073709551615n;
function chooseRoom(h, url = ROOM_URL) {
  h.get("multiplayer").checked = true;
  h.get("multiplayer-mode").value = "room";
  h.get("multiplayer-url").value = url;
  h.get("multiplayer-mode").emit("change");
  h.get("multiplayer").emit("change");
}
function roomRoster(start, { count = 2, creator = true, phase = 0, ownReady = false } = {}) {
  const players = Uint32Array.from(Array.from(start.localPlanWords).filter((_, index) => index % 4 === 0));
  const members = Array.from({ length: count }, (_, index) => ({ participant: ROOM_PARTICIPANT - BigInt(index),
    players: index === 0 ? players : Uint32Array.of(0xffffffff - index, 1),
    prepared: phase === 2 || (index === 0 && ownReady) }));
  if (!creator) [members[0], members[1]] = [members[1], members[0]];
  return { kind: "snapshot", participant: ROOM_PARTICIPANT,
    snapshot: { phase, deadlineNs: phase === 2 ? null : 604800000000001n, members } };
}
async function roomEvent(h, start, event) {
  await h.receive({ kind: "play-room", playId: start.playId, event });
}
async function openRoomLobby(h, { completeOpen = true, samples = 0 } = {}) {
  chooseRoom(h);
  const start = await h.begin(), worker = h.workers.at(-1);
  await h.reply(await h.prepared(start, samples), null);
  const opening = worker.last("play-room-open");
  assert.ok(opening);
  if (completeOpen) await h.reply(opening, { kind: "room-opened" });
  return { start, worker, opening };
}
function roomStart(fields = {}) {
  return { kind: "start", targetHostNs: 1500000001n, songTargetHostNs: 1600000001n, uncertaintyNs: 0n, ...fields };
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

test("local saved targets freeze before audio acquisition and member comparisons appear only after joined cleanup", async () => {
  const opening = deferred(), stopping = deferred();
  const h = await harness({ touchSupported: true, openGate: opening, stopGate: stopping });
  await h.preview(); await localCount(h, 2);
  const own = selectedRecording(), other = selectedRecording();
  selectOpponent(h, own, { own: true, label: "Own prefix" });
  selectOpponent(h, other, { own: false, label: "<Other prefix>" });
  const assignTarget = (key, player) => {
    const field = h.get(`opponent-player-${key}`); field.value = String(player); field.emit("change");
  };
  assignTarget("file:1", 1); assignTarget("file:2", 2);
  h.click("local-discover"); await flush(); localAssign(h, 1, 1n); localAssign(h, 2, 2n);
  h.get("record").checked = true;
  h.click("play"); assert.equal(h.opens.length, 1); assert.equal(h.opens[0].gesture, true);
  assert.equal(h.get("opponent-player-file:1").disabled, true);
  assert.equal(h.get("opponent-player-file:2").disabled, true);
  assignTarget("file:1", 2);
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  assert.deepEqual(Array.from(start.opponents, row => [row.player, row.own, row.label]), [[1, true, "Own prefix"], [2, false, "<Other prefix>"]]);
  assert.equal(start.opponents[0].file, own.file); assert.equal(start.opponents[1].file, other.file);
  assert.equal(own.reads, 0); assert.equal(other.reads, 0);
  const handoff = await h.prepared(start, 1); await h.reply(handoff, null); await h.reply(worker.last("play-activate"), null);
  const rows = h.get("opponents-results"), writes = [], originalReplace = rows.replaceChildren.bind(rows);
  rows.replaceChildren = (...children) => { writes.push(children.length); return originalReplace(...children); };
  for (let index = 0; index < 8; index++) {
    await h.receive({ kind: "play-opponents", playId: start.playId, player: 1,
      opponents: comparison(start.playId, { kind: "own", label: "Own prefix", hits: 100n + BigInt(index) }).opponents, error: null });
  }
  assert.deepEqual(writes, []); assert.equal(rows.children.length, 0);
  await h.receive({ kind: "play-opponents", playId: start.playId, player: 2, opponents: null, error: "only second comparison stopped" });
  assert.match(h.get("opponents-status").textContent, /Player 2.*Other members continue/);
  assert.equal(h.get("stop").disabled, false); assert.equal(worker.messages("play-stop").length, 0);
  h.setNow(1300); h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  const input = worker.last("play-step"); assert.equal(input.events[0].down, true);
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: input.tickId,
    songNs: 1n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  h.click("stop"); await flush();
  const prefix = Uint8Array.from([66, 75, 82, 1]);
  const receipt = localFinal(start, {
    replays: [{ player: 1, replay: prefix, replayError: null, replayComplete: false },
      { player: 2, replay: null, replayError: "second recording export failed", replayComplete: false }],
    savedOpponents: { opponents: null, error: null, localOpponents: [
      { player: 1, opponents: comparison(start.playId, { kind: "own", label: "Own prefix", songNs: -9223372036854775808n,
        hits: 18446744073709551615n, combo: 0n, maxCombo: 18446744073709551615n }).opponents, error: null },
      { player: 2, opponents: null, error: "only second comparison stopped" },
    ] },
  });
  await h.receive(receipt); assert.deepEqual(writes, []); assert.equal(h.get("export").disabled, true);
  stopping.resolve(); await flush();
  assert.deepEqual(writes, [0, 1]); // Clear old rows, then publish one complete snapshot after cleanup.
  assert.equal(rows.children.length, 2);
  assert.match(rows.children[0].textContent, /Player 1.*Own prefix.*18446744073709551615/);
  assert.match(rows.children[1].textContent, /Player 2.*only second comparison stopped/);
  assert.match(h.get("local-results").children[1].textContent, /second recording export failed/);
  assert.equal(h.get("play").disabled, false); assert.equal(h.get("opponent-player-file:1").disabled, false);
  h.get("captured-replay").value = "1"; h.get("captured-replay").emit("change");
  assert.equal(h.get("export").disabled, false);
  h.click("export"); await flush();
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), prefix);
  assert.equal(h.downloads.at(-1).filename, `beatkernel-${start.playId}-player-1-prefix.bkr`);
  await h.receive(receipt); await h.receive({ kind: "play-opponents", playId: start.playId, player: 2, opponents: null, error: "late old failure" });
  assert.deepEqual(writes, [0, 1]); assert.match(rows.children[1].textContent, /only second comparison stopped/);
  assert.equal(own.reads, 0); assert.equal(other.reads, 0);
  await h.close();
});

test("retired targets and malformed member comparison receipts cannot be reassigned silently or change local captures", async () => {
  const retired = await harness({ touchSupported: true }); await retired.preview(); await localCount(retired, 3);
  selectOpponent(retired, selectedRecording(), { label: "retired target" });
  let target = retired.get("opponent-player-file:1"); target.value = "3"; target.emit("change");
  await localCount(retired, 2);
  target = retired.get("opponent-player-file:1"); assert.equal(target.value, "3");
  assert.ok(target.children.some(option => option.value === "3" && /Removed player/.test(option.textContent)));
  retired.click("local-discover"); await flush(); localAssign(retired, 1, 1n); localAssign(retired, 2, 2n);
  retired.click("play"); await flush(); assert.equal(retired.opens.length, 0);
  assert.equal(retired.workers[0].messages("play-start").length, 0);
  await localCount(retired, 3);
  assert.ok(retired.get("opponent-player-file:1").children.some(option => option.value === "4"));
  assert.equal(retired.get("opponent-player-file:1").value, "3", "a new roster row never inherits a retired comparison target");
  await retired.close();

  for (const failure of ["owned-row", "ownership-envelope"]) {
    const h = await harness({ touchSupported: true }); await h.preview(); await localCount(h, 2);
    selectOpponent(h, selectedRecording(), { own: true, label: "first record" });
    selectOpponent(h, selectedRecording(), { own: false, label: "second record" });
    for (const player of [1, 2]) {
      const field = h.get(`opponent-player-file:${player}`); field.value = String(player); field.emit("change");
    }
    h.click("local-discover"); await flush(); localAssign(h, 1, 1n); localAssign(h, 2, 2n); h.get("record").checked = true;
    const session = await h.launch();
    const groups = [
      { player: 1, opponents: comparison(session.id, { kind: "own", label: "first record" }).opponents, error: null },
      { player: 2, opponents: comparison(session.id, { label: "second record", hits: "not a counter" }).opponents, error: null },
    ];
    if (failure === "ownership-envelope") groups.reverse();
    h.click("stop"); await flush();
    const receipt = localFinal(session.start, {
      savedOpponents: { localOpponents: groups, opponents: null, error: null },
      replays: [1, 2].map(player => ({ player, replay: Uint8Array.from([66, 75, 82, player]), replayError: null, replayComplete: false })),
    });
    await h.receive(receipt);
    if (failure === "owned-row") {
      assert.equal(h.get("opponents-results").children.length, 2);
      assert.match(h.get("opponents-results").children[0].textContent, /Player 1.*first record.*Hits 7/);
      assert.match(h.get("opponents-results").children[1].textContent, /Player 2.*unavailable/i);
    } else {
      assert.equal(h.get("opponents-results").children.length, 0);
      assert.match(h.get("opponents-status").textContent, /ownership changed.*Local result unchanged/);
    }
    assert.equal(h.get("captured-replay").children.filter(option => option.value !== "").length, 2);
    assert.equal(h.get("local-results").children.length, 2); assert.equal(h.get("play").disabled, false);
    await localCount(h, 1);
    h.click("play"); await flush(); assert.equal(h.opens.length, 1, "solo refuses the still-targeted records before another audio owner");
    const replay = await h.launch(0, "replay");
    assert.equal(replay.start.opponents?.length ?? 0, 0); assert.equal(Object.hasOwn(replay.start, "localPlanWords"), false);
    const display = h.get("opponents-status").textContent;
    await h.receive(receipt); assert.equal(h.get("opponents-status").textContent, display);
    h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
    assert.equal(h.get("opponents-results").children.length, 0);
    await h.close();
  }
});

test("Window ignores periodic comparison counters and displays the exact final prefix only after both owners join", async () => {
  const stopping = deferred();
  const h = await harness({ stopGate: stopping }); const preview = await h.preview();
  selectOpponent(h, selectedRecording(), { own: false, label: "<img src=x>" });
  const session = await h.launch(), worker = h.workers[0];
  const rows = h.get("opponents-results"), status = h.get("opponents-status");
  const writes = [];
  for (const element of [rows, status]) {
    let value = element.textContent;
    Object.defineProperty(element, "textContent", { configurable: true, get: () => value,
      set(next) { writes.push([element.id, next]); value = next; } });
  }
  const replace = rows.replaceChildren.bind(rows);
  rows.replaceChildren = (...children) => { writes.push(["rows", children.length]); return replace(...children); };
  const maximum = 18446744073709551615n;
  for (let index = 0; index < 20; index++) {
    await h.receive(comparison(session.id, { hits: BigInt(index), combo: 0n, maxCombo: 0n }));
    await h.receive(comparison(session.id - 1, { hits: maximum }));
  }
  assert.deepEqual(writes, []); assert.equal(rows.children.length, 0);
  h.setNow(1300);
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1300 });
  const down = worker.last("play-step");
  h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1300.125 });
  const acknowledge = request => h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: request.tickId,
    songNs: 50000000n, hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0 });
  await acknowledge(down);
  const up = worker.last("play-step");
  assert.ok(up.tickId > down.tickId); assert.equal(up.events[0].down, false);
  await acknowledge(up);
  assert.deepEqual(writes, [], "input response correlation must not introduce a second DOM scoreboard");
  h.click("stop"); await flush();
  const final = finalScore(session.id, { savedOpponents: { error: null, opponents: comparison(session.id, {
    hits: maximum, misses: maximum, combo: maximum, maxCombo: maximum,
  }).opponents } });
  await h.receive(final);
  assert.equal(h.audio.stopStarts, 1); assert.equal(rows.children.length, 0);
  assert.deepEqual(writes, [], "a Worker result alone does not finish the outstanding audio cleanup");
  assert.equal(h.get("play").disabled, true);
  stopping.resolve(); await flush();
  assert.equal(rows.children.length, 1);
  assert.match(rows.children[0].textContent, /Other.*<img src=x>.*Hits 18446744073709551615.*Best 18446744073709551615.*recorded through -0\.000000001/);
  assert.equal(rows.children[0].children.length, 0, "labels remain text rather than markup");
  assert.match(status.textContent, /Final saved comparison prefixes/);
  assert.equal(h.get("title").textContent, preview.title);
  assert.equal(h.get("position").value, preview.position);
  assert.match(h.get("status").textContent, /Hits 3.*Misses 1/);
  const written = writes.length, retained = rows.children[0];
  await h.receive(final); await h.receive(comparison(session.id));
  assert.equal(writes.length, written); assert.equal(rows.children[0], retained);
  const replay = await h.launch(0, "replay");
  assert.equal(replay.start.opponents?.length ?? 0, 0);
  const replayDisplay = status.textContent;
  await h.receive(comparison(replay.id)); assert.equal(status.textContent, replayDisplay);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  assert.equal(rows.children.length, 0);
  await h.close();
});

test("unavailable or stale final comparisons cannot alter local recording results or a later page owner", async () => {
  for (const fault of ["missing", "malformed", "previously-failed"]) {
    const h = await harness(); await h.preview(); selectOpponent(h, selectedRecording());
    h.get("record").checked = true;
    const session = await h.launch();
    if (fault === "previously-failed") {
      await h.receive({ kind: "play-opponents", playId: session.id, opponents: null, error: "first comparison failure" });
      await h.receive({ kind: "play-opponents", playId: session.id, opponents: null, error: "replacement failure" });
      assert.match(h.get("opponents-status").textContent, /first comparison failure/);
    }
    h.click("stop"); await flush();
    const rows = comparison(session.id, fault === "malformed" ? { hits: "9007199254740993" } : {}).opponents;
    await h.receive(finalScore(session.id, { replay: Uint8Array.from([66, 75, 82, 255]), replayComplete: false, replayError: null,
      ...(fault === "missing" ? {} : { savedOpponents: { opponents: rows, error: null } }) }));
    assert.match(h.get("opponents-status").textContent, /Final saved comparisons unavailable.*Local result unchanged/);
    if (fault === "previously-failed") assert.match(h.get("opponents-status").textContent, /first comparison failure/);
    assert.equal(h.get("opponents-results").children.length, 0);
    assert.match(h.get("status").textContent, /Hits 3.*Misses 1/);
    assert.equal(h.get("export").disabled, false); assert.match(h.get("export").textContent, /prefix/);
    assert.doesNotMatch(h.get("status").textContent, /Replay export failed|Gameplay cleanup failed/);
    const finalStatus = h.get("opponents-status").textContent;
    await h.receive(finalScore(session.id, { savedOpponents: { opponents: comparison(session.id).opponents, error: null } }));
    assert.equal(h.get("opponents-status").textContent, finalStatus);
    await h.close();
  }
  const stopping = deferred(), h = await harness({ stopGate: stopping });
  await h.preview(); selectOpponent(h, selectedRecording());
  const prior = await h.launch(); h.click("stop"); await flush();
  await h.receive(finalScore(prior.id, { savedOpponents: { opponents: comparison(prior.id).opponents, error: null } }));
  h.window.emit("pagehide"); await flush();
  h.window.emit("pageshow", { persisted: true }); await flush();
  const resetStatus = h.get("opponents-status").textContent;
  stopping.resolve(); await flush();
  assert.equal(h.get("opponents-status").textContent, resetStatus);
  assert.equal(h.get("opponents-results").children.length, 0);
  await h.preview();
  delete h.faults.stopGate;
  const current = await h.launch(); assert.equal(current.start.opponents?.length ?? 0, 0);
  await h.receive(finalScore(prior.id, { savedOpponents: { opponents: comparison(prior.id).opponents, error: null } }));
  assert.equal(h.get("opponents-results").children.length, 0);
  assert.equal(h.get("stop").disabled, false);
  h.click("stop"); await flush(); await h.receive(finalScore(current.id));
  assert.equal(h.get("opponents-results").children.length, 0);
  await h.close();
});

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
  assert.equal(h.get("opponents-results").children.length, 0);
  assert.deepEqual({ title: h.get("title").textContent, details: h.get("details").textContent,
    status: h.get("status").textContent, network: h.get("multiplayer-status").textContent }, local);
  await h.receive(comparison(session.id, { hits: 8n, combo: 4n, maxCombo: 5n, recordedUntilNs: null }));
  assert.equal(h.get("opponents-results").children.length, 0, "normal counters have no Window display path");
  h.click("stop"); await flush();
  await h.receive(finalScore(session.id, { savedOpponents: { opponents: comparison(session.id).opponents, error: null } }));
  assert.match(h.get("opponents-results").children[0].textContent, /Other.*<img src=x>.*Hits 7.*recorded through -0\.000000001/);
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
    const beforeMessage = h.get("opponents-status").textContent;
    await h.receive(data);
    if (failure === "binding") assert.match(h.get("opponents-status").textContent, /stopped.*Local play continues/i);
    else assert.equal(h.get("opponents-status").textContent, beforeMessage, "unsolicited normal counters are ignored even when malformed");
    assert.equal(worker.messages("play-stop").length, 0);
    assert.equal(h.audio.stopStarts, 0);
    const comparisonFailure = h.get("opponents-status").textContent;
    await h.receive(comparison(session.id));
    assert.equal(h.get("opponents-status").textContent, comparisonFailure);
    h.setNow(1300); await h.advance(8);
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: session.id,
      renderId: worker.last("play-render").renderId, completed: true });
    await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: session.id, tickId: worker.last("play-step").tickId,
      songNs: 50000000n, hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0 });
    assert.equal(worker.last("play-stop").completed, true);
    await h.receive(finalScore(session.id, { replay: Uint8Array.from([66, 75, 82]), replayComplete: true, replayError: null,
      savedOpponents: failure === "binding" ? { opponents: null, error: "actual comparison failure" }
        : { opponents: comparison(session.id, { hits: "7" }).opponents, error: null } }));
    assert.match(h.get("status").textContent, /Song completed/);
    assert.match(h.get("export").textContent, /complete/);
    assert.equal(h.get("export").disabled, false);
    assert.match(h.get("opponents-status").textContent, /Final saved comparisons unavailable.*Local result unchanged/);
    const ended = h.get("opponents-status").textContent;
    await h.receive(comparison(session.id));
    assert.equal(h.get("opponents-status").textContent, ended);
    await h.close();
  }
});

test("actual Main snapshots explicit timing policy before audio open and replay ignores the live selectors", async () => {
  const opening = deferred(), h = await harness({ openGate: opening });
  await h.preview();
  h.get("judge-preset").value = BMS_TIMING_PRESET_ID;
  h.get("judge-precedence").value = "defexrank-first";
  h.get("judge-gauge").value = "hard";
  h.get("judge-offset").value = "-0.000037";
  h.click("play");
  assert.equal(h.opens.length, 1);
  for (const id of ["judge-preset", "judge-precedence", "judge-gauge"]) assert.equal(h.get(id).disabled, true);
  h.get("judge-preset").value = "";
  h.get("judge-precedence").value = "rank-first";
  h.get("judge-gauge").value = "beatkernel";
  opening.resolve(h.audio); await flush();
  const request = h.workers[0].last("play-start");
  assert.deepEqual(request.timingPolicy, { presetId: BMS_TIMING_PRESET_ID, rankPrecedence: "defexrank-first", gauge: "hard" });
  assert.equal(request.timing.offsetNs, -37n);
  await h.reply(request, { kind: "prepared", title: "Selected", artist: "Fixture", notes: 1, samples: 0,
    lanes: [0x11], startNs: 0n, opponentCount: 0, inputMode: "physical" });
  await h.close();

  const replay = await harness(); await replay.preview();
  chooseRecording(replay, [selectedRecording().file]);
  replay.get("judge-preset").value = "unknown";
  replay.get("judge-precedence").value = "unknown";
  replay.get("judge-gauge").value = "unknown";
  const recorded = await replay.begin("replay");
  assert.equal(Object.hasOwn(recorded, "timingPolicy"), false);
  assert.equal(Object.hasOwn(recorded, "timing"), false);
  await replay.close();
});

test("actual Main keeps baseline requests unchanged and refuses an unconfigured gauge before audio opens", async () => {
  const legacy = await harness(); await legacy.preview();
  const request = await legacy.begin();
  assert.equal(Object.hasOwn(request, "timingPolicy"), false);
  await legacy.close();
  const invalid = await harness(); await invalid.preview();
  invalid.get("judge-gauge").value = "groove";
  invalid.click("play"); await flush();
  assert.equal(invalid.opens.length, 0);
  assert.equal(invalid.workers[0].messages("play-start").length, 0);
  assert.match(invalid.get("status").textContent, /explicit numerical timing preset/);
  await invalid.close();
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

test("finite live controls capture one pre-gesture section and join input, output and recording before publishing Section completed", async () => {
  const html = await readFile(new URL("index.html", import.meta.url), "utf8");
  assert.match(html, /<input\b(?=[^>]*\bid="live-end")(?=[^>]*\btype="text")(?=[^>]*\bvalue="")(?=[^>]*\bmaxlength="20")[^>]*>/);
  assert.match(html, /original song positions with a short preroll/);
  assert.match(html, /Leave the end blank[^<]*Replay uses its recorded section/);
  const opening = deferred(), stopping = deferred();
  const h = await harness({ openGate: opening, stopGate: stopping, actualRate: 44100,
    outputEvidence: { contextTime: 1.4, performanceTime: 1400 } });
  await h.preview();
  const startField = h.get("live-start"), endField = h.get("live-end"), worker = h.workers[0];
  assert.equal(endField.value, "");
  startField.value = "1"; endField.value = "1.000000001";
  h.get("output-rate").value = "96000";
  h.get("record").checked = true;
  h.click("play");
  assert.equal(h.opens.length, 1);
  assert.equal(h.opens[0].gesture, true);
  assert.equal(h.opens[0].options.contextOptions.sampleRate, 96000);
  assert.ok(startField.disabled && endField.disabled);
  startField.value = "2"; endField.value = "3";
  opening.resolve(h.audio); await flush();
  const start = worker.last("play-start");
  assert.equal(start.startNs, 1000000000n);
  assert.equal(start.endNs, 1000000001n);
  assert.equal(start.rate, 44100);
  assert.equal(start.recordReplay, true);
  let actualEnd = 1000000001n, actualFrame = 4411n;
  const reads = { end: 0, frame: 0 };
  worker.emit("message", { data: { kind: "play-reply", playId: start.playId, rpcId: start.rpcId, result: {
    kind: "prepared", inputMode: "physical", title: "Finite live section", notes: 1, samples: 1, lanes: [0x11], opponentCount: 0, startNs: 1000000000n,
    get endNs() { assert.equal(++reads.end, 1); return actualEnd; },
    get endFrame() { assert.equal(++reads.frame, 1); return actualFrame; },
  } } });
  await flush();
  assert.deepEqual(reads, { end: 1, frame: 1 });
  actualEnd = undefined; actualFrame = undefined;
  await h.admitSamples(worker.last("play-samples-upload"), 1);
  await h.reply(worker.last("play-samples-upload"), { kind: "samples-uploaded", count: 1, bytes: 8 });
  assert.deepEqual(h.audio.finishArgs, [[4411n]], "the endpoint uses actual output rate without a Window sample relay");
  await h.reply(worker.last("play-audio"), null);
  assert.equal(worker.last("play-activate").startFrame, 55125n);
  assert.equal(worker.last("play-activate").hostNs, 1250000000n);
  await h.reply(worker.last("play-activate"), null);
  assert.match(h.get("details").textContent, /start 1 s · end 1\.000000001 s/);
  assert.ok(startField.disabled && endField.disabled);
  h.setNow(1400); await h.advance(8);
  const firstTick = worker.last("play-step"), firstReport = worker.last("play-render");
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1408 });
  h.window.emit("keyup", { code: "KeyZ", repeat: false, timeStamp: 1408 });
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: start.playId, renderId: firstReport.renderId, completed: true });
  assert.equal(worker.messages("play-stop").length, 0);
  const stepDone = (request, commandsPending = false) => h.receive({ kind: "play-step-done", pendingInputs: 0, playId: start.playId, tickId: request.tickId,
    commandsPending, songNs: 1000000001n, hits: 4n, misses: 1n, combo: 3n, preOriginInputs: 0 });
  await stepDone(firstTick);
  const captured = worker.last("play-step");
  assert.deepEqual(captured.events, [
    { hostNs: 1408000000n, key: 2, down: true, sequence: 1n },
    { hostNs: 1408000000n, key: 2, down: false, sequence: 2n },
  ]);
  await stepDone(captured, true);
  assert.equal(worker.messages("play-stop").length, 0);
  assert.equal(worker.messages("play-ack").length, 0);
  assert.equal(worker.messages("play-stop").length, 0, "new input and commands invalidate the earlier completion receipt");
  await h.advance(8);
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: start.playId, renderId: worker.last("play-render").renderId, completed: true });
  assert.equal(worker.messages("play-stop").length, 0, "the final actual input watermark must still join");
  await stepDone(worker.last("play-step"));
  assert.equal(worker.last("play-stop").completed, true);
  await h.receive(finalScore(start.playId, { songNs: 1000000001n, hits: 4n, combo: 3n,
    replay: Uint8Array.from([1, 2, 3]), replayComplete: true, replayError: null }));
  assert.equal(h.get("export").disabled, true);
  assert.equal(endField.disabled, true);
  stopping.resolve(); await flush();
  assert.match(h.get("status").textContent, /Section completed\..*Hits 4/);
  assert.equal(h.get("export").disabled, false);
  assert.match(h.get("export").textContent, /complete/);
  assert.ok(!startField.disabled && !endField.disabled);
  assert.equal(endField.value, "3");
  assert.deepEqual(reads, { end: 1, frame: 1 });
  await h.close();
});

test("invalid or mismatched live ends preserve drafts for retry and manual prefixes while replay uses its recorded section", async () => {
  const h = await harness();
  await h.preview();
  const startField = h.get("live-start"), endField = h.get("live-end"), worker = h.workers[0];
  startField.value = "1";
  for (const end of [" ", "1", "0.999999999", "-1", "2.0000000001", "9223372034.854775808"]) {
    endField.value = end;
    h.click("play"); await flush();
    assert.equal(h.opens.length, 0);
    assert.equal(worker.messages("play-start").length, 0);
    assert.equal(endField.value, end);
    assert.equal(endField.disabled, false);
    assert.equal(h.get("status").dataset.error, "true");
  }
  endField.value = "2";
  for (const endpoint of [{}, { endNs: 2000000001n, endFrame: 52801n }, { endNs: 2000000000n, endFrame: 52801n }]) {
    const start = await h.begin();
    await h.reply(start, { kind: "prepared", title: "Wrong finite metadata", samples: 1, notes: 1,
      lanes: [0x11], opponentCount: 0, startNs: 1000000000n, ...endpoint });
    assert.equal(worker.messages("play-samples-upload").length, 0);
    assert.deepEqual(h.audio.finishArgs, []);
    assert.deepEqual(h.audio.arms, []);
    assert.equal(worker.last("play-stop").completed, false);
    await h.receive(finalScore(start.playId));
    assert.equal(endField.value, "2");
    assert.equal(endField.disabled, false);
  }
  h.get("record").checked = true;
  const start = await h.begin();
  assert.equal(start.endNs, 2000000000n);
  await h.reply(start, { kind: "prepared", title: "Retried finite section", samples: 0, notes: 1,
    lanes: [0x11], opponentCount: 0, startNs: 1000000000n, endNs: 2000000000n, endFrame: 52800n });
  await h.admitSamples(worker.last("play-samples-upload"), 0);
  await h.reply(worker.last("play-samples-upload"), { kind: "samples-uploaded", count: 0, bytes: 0 });
  assert.deepEqual(h.audio.finishArgs, [[52800n]]);
  await h.reply(worker.last("play-audio"), null);
  await h.reply(worker.last("play-activate"), null);
  h.click("stop"); await flush();
  assert.equal(worker.last("play-stop").completed, false);
  await h.receive(finalScore(start.playId, { replay: Uint8Array.from([4, 5]), replayComplete: false, replayError: null }));
  assert.match(h.get("export").textContent, /prefix/);
  assert.doesNotMatch(h.get("status").textContent, /Section completed/);
  assert.equal(endField.value, "2");
  const file = selectedRecording();
  chooseRecording(h, [file.file]);
  startField.value = "bad live start"; endField.value = "bad live end";
  h.get("judge-early").value = "bad live judge";
  const replay = await h.begin("replay");
  assert.equal(Object.hasOwn(replay, "startNs"), false);
  assert.equal(Object.hasOwn(replay, "endNs"), false);
  assert.equal(Object.hasOwn(replay, "timing"), false);
  assert.equal(h.opens.at(-1).gesture, true);
  assert.equal(file.reads, 0);
  await h.reply(replay, { kind: "prepared", mode: "replay", title: "Recorded finite section", samples: 0, notes: 1,
    lanes: [0x11], opponentCount: 0, startNs: 9000000000n, endNs: 9000000001n, endFrame: 4801n });
  await h.admitSamples(worker.last("play-samples-upload"), 0);
  await h.reply(worker.last("play-samples-upload"), { kind: "samples-uploaded", count: 0, bytes: 0 });
  assert.deepEqual(h.audio.finishArgs, [[4801n]]);
  await h.reply(worker.last("play-audio"), null);
  await h.reply(worker.last("play-activate"), null);
  assert.match(h.get("details").textContent, /start 9 s · recorded end 9\.000000001 s/);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.playId));
  assert.equal(endField.value, "bad live end");
  assert.equal(endField.disabled, false);
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
  assert.equal(h.opens[0].options.pcmLimits.maxSamples, 7940);
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
    assert.equal(worker.messages("play-samples-upload").length, 0);
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

test("keyboard acquisition snapshots native fields once and ignored or reentrant keys consume no shared input sequence", async () => {
  const h = await harness({ pointerSupported: true }); await h.preview(); enablePointers(h);
  const session = await h.launch(), worker = h.workers[0]; h.setNow(1301);
  const display = watchPlayDisplay(h), layout = h.layoutReads;
  const ignoredTimestamp = () => { assert.fail("ignored keyboard input must not acquire a timestamp"); };
  const unbound = observedKeyboard({ code: "F24" }, {
    repeat() { assert.fail("unbound code has no repeat acquisition"); }, timeStamp: ignoredTimestamp,
    preventDefault() { assert.fail("unbound code retains browser behavior"); },
  });
  dispatchKeyboard(h, "keydown", unbound.event);
  assert.deepEqual(unbound.reads, { code: 1, repeat: 0, timeStamp: 0, preventDefault: 0 });
  const repeat = observedKeyboard({ repeat: true }, { timeStamp: ignoredTimestamp });
  const unmatched = observedKeyboard({}, { timeStamp: ignoredTimestamp });
  dispatchKeyboard(h, "keydown", repeat.event); dispatchKeyboard(h, "keyup", unmatched.event);
  assert.equal(repeat.reads.repeat, 1); assert.equal(unmatched.reads.repeat, 1);
  assert.equal(worker.messages("play-step").length, 0);
  const down = observedKeyboard({}, { repeat() {
    down.fields.code = "KeyX"; // Changing the native object cannot redirect the acquired code.
    dispatchKeyboard(h, "keydown", { get code() { assert.fail("same-owner synchronous key reentry is ignored before reading fields"); } });
  }, timeStamp() { down.fields.timeStamp = 9999; } });
  dispatchKeyboard(h, "keydown", down.event);
  assert.deepEqual(down.reads, { code: 1, repeat: 1, timeStamp: 1, preventDefault: 1 }); assert.equal(down.calls, 1);
  const first = worker.last("play-step");
  assert.deepEqual(first.events, [{ hostNs: 1300125000n, key: 2, down: true, sequence: 1n }]);
  const duplicate = observedKeyboard({}, { timeStamp: ignoredTimestamp });
  dispatchKeyboard(h, "keydown", duplicate.event);
  h.get("canvas").emit("pointerdown", nativePointer({ timeStamp: 1300.25 }));
  const up = observedKeyboard({ timeStamp: 1300.5 }, { call() {
    dispatchKeyboard(h, "keyup", { get code() { assert.fail("preventDefault cannot recursively release the same pressed key"); } });
  } });
  dispatchKeyboard(h, "keyup", up.event);
  dispatchKeyboard(h, "keyup", observedKeyboard({}, { timeStamp: ignoredTimestamp }).event);
  assert.deepEqual(up.reads, { code: 1, repeat: 1, timeStamp: 1, preventDefault: 1 });
  assert.equal(worker.messages("play-step").length, 1);
  await pointerStepDone(h, first);
  const mixed = worker.last("play-step");
  assert.deepEqual(mixed.events.map(row => [row.kind ?? "keyboard", row.sequence, row.hostNs]), [
    ["pointer", 2n, 1300250000n], ["pointer-button", 3n, 1300250000n], ["keyboard", 4n, 1300500000n],
  ]);
  assert.deepEqual(mixed.events[2], { hostNs: 1300500000n, key: 2, down: false, sequence: 4n });
  await pointerStepDone(h, mixed);
  dispatchKeyboard(h, "keydown", observedKeyboard({ timeStamp: 1300.75 }).event);
  const again = worker.last("play-step");
  assert.deepEqual(again.events, [{ hostNs: 1300750000n, key: 2, down: true, sequence: 5n }]);
  await pointerStepDone(h, again);
  assert.equal(h.layoutReads, layout); assert.deepEqual(display, []);
  h.click("stop"); await flush(); await h.receive(finalScore(session.id)); await h.close();
});

test("keyboard getter and preventDefault retirement discards the old event and cannot poison a replacement pressed state", async () => {
  for (const boundary of ["code", "preventDefault", "call", "repeat", "timeStamp"]) {
    const h = await harness(); await h.preview(); const old = await h.launch(), worker = h.workers[0]; h.setNow(1301);
    const probe = observedKeyboard({}, { [boundary]() { h.click("stop"); } });
    dispatchKeyboard(h, "keydown", probe.event); await flush();
    assert.equal(worker.messages("play-step").length, 0, `${boundary} cancellation admits no old input`);
    assert.equal(worker.messages("play-stop").length, 1); assert.equal(worker.last("play-stop").playId, old.id);
    assert.equal(probe.reads.code, 1);
    if (boundary === "code") assert.equal(probe.reads.preventDefault, 0);
    if (boundary === "preventDefault") assert.equal(probe.calls, 0, "a getter that retires the owner cannot authorize its returned callback");
    if (boundary !== "timeStamp") assert.equal(probe.reads.timeStamp, 0);
    await h.receive(finalScore(old.id));
    const fresh = await h.launch();
    const unmatched = observedKeyboard({}, { timeStamp() { assert.fail("old cancelled Down did not become a replacement press"); } });
    dispatchKeyboard(h, "keyup", unmatched.event);
    dispatchKeyboard(h, "keydown", observedKeyboard({ timeStamp: 1301 }).event);
    const first = worker.last("play-step");
    assert.equal(first.playId, fresh.id);
    assert.deepEqual(first.events, [{ hostNs: 1301000000n, key: 2, down: true, sequence: 1n }]);
    await pointerStepDone(h, first);
    h.click("stop"); await flush(); await h.receive(finalScore(fresh.id)); await h.close();
  }
  for (const boundary of ["code", "call"]) {
    const h = await harness(); await h.preview(); const old = await h.launch(), oldWorker = h.workers[0];
    let formatted = 0;
    const retiredError = { get message() { formatted++; return "obsolete native failure"; } };
    const probe = observedKeyboard({}, { [boundary]() {
      h.window.emit("pagehide"); h.window.emit("pageshow", { persisted: true }); throw retiredError;
    } });
    dispatchKeyboard(h, "keydown", probe.event); await flush();
    assert.equal(formatted, 0, "a retired native exception cannot invoke diagnostics against replacement ownership");
    assert.equal(oldWorker.messages("play-step").length, 0); assert.equal(oldWorker.terminations, 0);
    assert.equal(h.workers.length, 1);
    await h.receive(finalScore(old.id), oldWorker);
    assert.equal(oldWorker.terminations, 1);
    assert.equal(h.workers.length, 2); await h.preview(); const fresh = await h.launch(), current = h.workers[1];
    await h.receive(finalScore(old.id), oldWorker);
    assert.equal(current.messages("play-stop").length, 0);
    dispatchKeyboard(h, "keydown", observedKeyboard().event);
    assert.deepEqual(current.last("play-step").events, [{ hostNs: 1300125000n, key: 2, down: true, sequence: 1n }]);
    h.click("stop"); await flush(); await h.receive(finalScore(fresh.id)); await h.close();
  }
});

test("keyboard mode and source gates avoid gameplay fields while malformed current acquisition stops without a partial event", async () => {
  const denied = () => { assert.fail("ineligible keyboard event must not read gameplay or prevention fields"); };
  for (const mode of ["preparing", "replay", "unassigned"]) {
    const h = await harness({ pointerSupported: true }); await h.preview();
    if (mode === "replay") chooseRecording(h, [selectedRecording().file]);
    if (mode === "unassigned") {
      enablePointers(h); await localCount(h, 2); h.click("local-discover"); await flush();
      const choices = h.get("local-source-1").children;
      localAssign(h, 1, BigInt(choices.find(row => /mouse/i.test(row.textContent)).value));
      localAssign(h, 2, BigInt(choices.find(row => /pen/i.test(row.textContent)).value));
    }
    const start = mode === "preparing" ? await h.begin() : (await h.launch(0, mode === "replay" ? "replay" : "live")).start;
    const worker = h.workers[0];
    for (const type of ["keydown", "keyup"]) {
      const event = observedKeyboard({}, { repeat: denied, timeStamp: denied, preventDefault: denied });
      dispatchKeyboard(h, type, event.event);
      assert.deepEqual(event.reads, { code: 1, repeat: 0, timeStamp: 0, preventDefault: 0 });
    }
    assert.equal(worker.messages("play-step").length, 0);
    const escape = observedKeyboard({ code: "Escape" }, { repeat: denied, timeStamp: denied });
    dispatchKeyboard(h, "keydown", escape.event); await flush();
    assert.equal(escape.calls, 1); assert.equal(escape.reads.code, 1); assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(mode === "unassigned" ? localFinal(start) : finalScore(start.playId)); await h.close();
  }
  for (const failure of ["code", "repeat", "timeStamp", "preventDefault", "call", "negative", "nonfinite", "behind"]) {
    const h = await harness(); await h.preview(); const session = await h.launch(), worker = h.workers[0]; h.setNow(1301);
    let closedPrefix;
    if (failure === "behind") {
      dispatchKeyboard(h, "keydown", observedKeyboard({ timeStamp: 1301 }).event);
      const accepted = worker.last("play-step"); closedPrefix = accepted.watermark;
      await pointerStepDone(h, accepted);
    }
    const before = worker.messages("play-step").length;
    const fields = { timeStamp: failure === "negative" ? -1 : failure === "nonfinite" ? NaN
      : failure === "behind" ? Number(closedPrefix) / 1000000 - 0.125 : 1300.125 };
    const effects = ["code", "repeat", "timeStamp", "preventDefault", "call"].includes(failure)
      ? { [failure]() { throw new Error(`native ${failure} refused`); } } : {};
    dispatchKeyboard(h, failure === "behind" ? "keyup" : "keydown", observedKeyboard(fields, effects).event); await flush();
    assert.equal(worker.messages("play-step").length, before); assert.equal(worker.messages("play-stop").length, 1);
    assert.equal(worker.last("play-stop").playId, session.id);
    await h.receive(finalScore(session.id)); await h.close();
  }
});

test("one pre-audio binding snapshot supplies Worker pairs, displayed keys and physical Down Up events", async () => {
  const opening = deferred();
  const h = await harness({ openGate: opening });
  await h.preview();
  const select = h.get("binding-11");
  assert.equal(select.value, "KeyZ");
  select.value = "KeyA"; select.emit("change");
  h.click("play");
  assert.equal(h.opens.length, 1);
  assert.equal(h.opens[0].gesture, true);
  assert.equal(select.disabled, true);
  assert.equal(h.get("bindings-reset").disabled, true);
  select.value = "KeyB"; // A script can mutate a disabled element; the owner cannot.
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  const index = Array.from(start.keyPairs).findIndex((value, index) => index % 2 === 0 && value === 0x11);
  assert.equal(start.keyPairs[index + 1], 19);
  await h.reply(await h.prepared(start), null);
  await h.reply(worker.last("play-activate"), null);
  assert.match(h.get("keys").textContent, /11: KeyA/);
  assert.doesNotMatch(h.get("keys").textContent, /KeyB|KeyZ/);
  h.setNow(1300);
  const before = worker.messages("play-step").length;
  assert.equal(h.window.emit("keydown", { code: "KeyZ", key: "a", repeat: false, timeStamp: 1300 }).defaultPrevented, false);
  assert.equal(h.window.emit("keydown", { code: "KeyB", repeat: false, timeStamp: 1300 }).defaultPrevented, false);
  assert.equal(worker.messages("play-step").length, before);
  assert.equal(h.window.emit("keydown", { code: "KeyA", key: "z", repeat: false, timeStamp: 1300 }).defaultPrevented, true);
  const down = worker.last("play-step");
  assert.deepEqual(down.events, [{ hostNs: 1300000000n, key: 19, down: true, sequence: 1n }]);
  h.window.emit("keydown", { code: "KeyA", repeat: true, timeStamp: 1300 });
  h.window.emit("keyup", { code: "KeyA", key: "z", timeStamp: 1300 });
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: down.tickId,
    songNs: 50000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  const up = worker.last("play-step");
  assert.deepEqual(up.events, [{ hostNs: 1300000000n, key: 19, down: false, sequence: 2n }]);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.equal(select.disabled, false);
  assert.equal(select.value, "KeyB");
  delete h.faults.openGate;
  const fresh = await h.launch();
  assert.equal(fresh.start.keyPairs[index + 1], 20);
  assert.match(h.get("keys").textContent, /11: KeyB/);
  assert.equal(h.get("binding-11"), select, "retained controls are not rebuilt for another session");
  await h.close();
});

test("retained binding drafts and reset obey busy ownership and invalid drafts can be corrected and retried", async () => {
  const loading = deferred();
  const h = await harness({ recordsList: [savedRecord()], recordsLoadGate: loading });
  await h.preview();
  const first = h.get("binding-11"), second = h.get("binding-12"), reset = h.get("bindings-reset");
  assert.equal(first.children[0].value, "");
  assert.ok(first.children.some(option => option.value === "KeyA"));
  assert.ok(first.children.every(option => option.value !== "Escape"));
  first.value = "KeyA"; first.emit("change");
  h.click("bindings-reset");
  assert.equal(first.value, "KeyZ");
  first.value = "KeyA"; first.emit("change");
  h.click("records-refresh"); await flush(); h.click("records-use"); await flush();
  assert.equal(first.disabled, true);
  assert.equal(second.disabled, true);
  assert.equal(reset.disabled, true);
  h.click("bindings-reset");
  assert.equal(first.value, "KeyA");
  loading.resolve(); await flush();
  assert.equal(first.disabled, false);
  first.value = "KeyS"; first.emit("change");
  h.click("play"); await flush();
  assert.equal(h.opens.length, 0);
  assert.equal(h.workers[0].messages("play-start").length, 0);
  assert.equal(first.value, "KeyS");
  assert.equal(second.value, "KeyS");
  assert.equal(h.get("status").dataset.error, "true");
  second.value = ""; second.emit("change");
  const failed = await h.begin();
  assert.equal(h.opens[0].gesture, true);
  assert.equal(Array.from(failed.keyPairs).filter((_, index) => index % 2 === 0).includes(0x12), false);
  assert.equal(reset.disabled, true);
  await h.receive({ kind: "play-reply", playId: failed.playId, rpcId: failed.rpcId, error: "actual preparation refused" });
  await h.receive(finalScore(failed.playId));
  assert.equal(first.value, "KeyS");
  assert.equal(second.value, "");
  assert.equal(reset.disabled, false);
  const retry = await h.launch();
  assert.match(h.get("keys").textContent, /11: KeyS/);
  h.click("stop"); await flush(); await h.receive(finalScore(retry.id));
  h.click("bindings-reset");
  assert.equal(first.value, "KeyZ");
  assert.equal(second.value, "KeyS");
  assert.equal(h.get("binding-11"), first);
  await h.close();
});

test("missing actual lane coverage refuses setup and replay ignores invalid live binding drafts", async () => {
  const h = await harness();
  await h.preview();
  const first = h.get("binding-11");
  first.value = ""; first.emit("change");
  const start = await h.begin();
  const worker = h.workers[0];
  await h.reply(start, { kind: "prepared", title: "Actual lane", notes: 1, samples: 1,
    lanes: [0x11], opponentCount: 0, startNs: 0n });
  assert.equal(worker.messages("play-samples-upload").length, 0);
  assert.deepEqual(h.audio.arms, []);
  assert.equal(worker.last("play-stop").playId, start.playId);
  await h.receive(finalScore(start.playId));
  assert.equal(first.value, "");
  const recording = selectedRecording();
  chooseRecording(h, [recording.file]);
  first.value = "KeyS"; first.emit("change"); // Duplicate the next lane intentionally.
  const replay = await h.launch(0, "replay");
  assert.equal(Object.hasOwn(replay.start, "keyPairs"), false);
  assert.equal(recording.reads, 0);
  assert.match(h.get("keys").textContent, /Recorded input playback/);
  assert.equal(first.disabled, true);
  assert.equal(h.get("bindings-reset").disabled, true);
  h.setNow(1300);
  const steps = worker.messages("play-step").length;
  h.window.emit("keydown", { code: "KeyS", repeat: false, timeStamp: 1300 });
  h.window.emit("keyup", { code: "KeyS", timeStamp: 1300 });
  assert.equal(worker.messages("play-step").length, steps);
  assert.equal(h.window.emit("keydown", { code: "Escape", repeat: false, timeStamp: 1300 }).defaultPrevented, true);
  await flush(); await h.receive(finalScore(replay.id));
  assert.equal(first.value, "KeyS");
  assert.equal(first.disabled, false);
  await h.close();
});

test("audio capacity drafts snapshot before the live gesture await and retain exact setup and active batch acknowledgements", async () => {
  const opening = deferred(), stopping = deferred();
  const h = await harness({ openGate: opening, stopGate: stopping });
  await h.preview();
  const ids = ["audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands"];
  const fields = ids.map(h.get);
  assert.deepEqual(fields.map(field => field.value), ["4096", "4096", "4096", "4096", "4096"]);
  ["3", "7", "11", "257", "2"].forEach((value, index) => { fields[index].value = value; });
  h.click("play");
  assert.equal(h.opens.length, 1);
  assert.equal(h.opens[0].gesture, true);
  const captured = h.opens[0].options.audioLimits;
  assert.deepEqual(structuredClone(captured), { queueCapacity: 3, maxVoices: 7, pendingCapacity: 11, maxFrames: 257, maxCommandsPerRender: 2 });
  assert.ok(Object.isFrozen(captured));
  assert.ok(fields.every(field => field.disabled));
  fields[0].value = "65536";
  fields[3].value = "128";
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  assert.equal(start.commandBatchLimit, 3);
  assert.equal(captured.queueCapacity, 3);
  const attachment = await h.prepared(start);
  assert.equal(attachment.kind, "play-audio"); assert.equal(attachment.queueCapacity, 3);
  assert.equal(h.audio.arms.length, 0, "actual initial command draining remains a setup barrier");
  await h.reply(attachment, null);
  await h.reply(worker.last("play-activate"), null);
  assert.equal(h.audio.attachments, 1); assert.deepEqual(h.audio.commandsSeen, []);
  assert.equal(worker.messages("play-commands").length, 0); assert.equal(worker.messages("play-ack").length, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.ok(fields.every(field => field.disabled), "the audio cleanup owner still holds the draft controls");
  stopping.resolve(); await flush();
  assert.ok(fields.every(field => !field.disabled));
  assert.deepEqual(fields.map(field => field.value), ["65536", "7", "11", "128", "2"]);
  assert.deepEqual(ids.map(h.get), fields, "controls remain retained across the entire owner lifecycle");
  delete h.faults.openGate;
  const nextPlay = await h.launch();
  assert.equal(nextPlay.start.commandBatchLimit, 256, "larger allocation preserves the bounded Worker transfer size");
  assert.equal(h.opens[1].options.audioLimits.queueCapacity, 65536);
  assert.equal(h.opens[1].options.audioLimits.maxFrames, 128);
  assert.notEqual(h.opens[1].options.audioLimits, captured);
  await h.close();
});

test("both modes refuse invalid capacity drafts before audio and a refused replay output can retry the same captured budgets", async () => {
  const opening = deferred();
  const h = await harness({ openGate: opening, replayStart: 123456789n });
  await h.preview();
  chooseRecording(h, [selectedRecording().file]);
  const ids = ["audio-queue", "audio-voices", "audio-pending", "audio-frames", "audio-commands"];
  const fields = ids.map(h.get);
  for (const mode of ["play", "replay-play"]) {
    for (const [index, invalid] of [[0, "65537"], [1, "4097"], [2, "0"], [3, "128.0"], [4, "+256"]]) {
      fields[index].value = invalid;
      h.click(mode); await flush();
      assert.equal(h.opens.length, 0);
      assert.equal(h.workers[0].messages("play-start").length, 0);
      assert.equal(h.get("status").dataset.error, "true");
      assert.equal(fields[index].value, invalid);
      assert.ok(fields.every(field => !field.disabled));
      fields[index].value = "4096";
    }
  }
  ["1", "2", "3", "128", "1"].forEach((value, index) => { fields[index].value = value; });
  h.get("judge-early").value = "invalid live judge";
  h.get("live-start").value = "invalid live section";
  h.get("binding-11").value = "KeyS";
  h.click("replay-play");
  assert.equal(h.opens[0].gesture, true);
  assert.ok(fields.every(field => field.disabled));
  opening.reject(new Error("chosen audio capacities were refused")); await flush();
  assert.equal(h.workers[0].messages("play-start").length, 0);
  assert.match(h.get("status").textContent, /chosen audio capacities were refused/);
  assert.deepEqual(fields.map(field => field.value), ["1", "2", "3", "128", "1"]);
  assert.ok(fields.every(field => !field.disabled));
  const retry = deferred(); h.faults.openGate = retry;
  h.click("replay-play");
  const captured = h.opens[1].options.audioLimits;
  assert.deepEqual(structuredClone(captured), { queueCapacity: 1, maxVoices: 2, pendingCapacity: 3, maxFrames: 128, maxCommandsPerRender: 1 });
  assert.ok(Object.isFrozen(captured));
  assert.equal(h.opens[1].gesture, true);
  fields[0].value = "2";
  retry.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  assert.equal(start.mode, "replay");
  assert.equal(start.commandBatchLimit, 1);
  for (const field of ["timing", "startNs", "keyPairs"]) assert.equal(Object.hasOwn(start, field), false);
  await h.reply(await h.prepared(start), null);
  await h.reply(worker.last("play-activate"), null);
  assert.equal(h.audio.arms.length, 1);
  assert.match(h.get("details").textContent, /start 0\.123456789 s/);
  assert.equal(worker.messages("play-step").length, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.equal(fields[0].value, "2");
  assert.equal(fields[0].disabled, false);
  await h.close();
});

test("output preferences are captured inside the live gesture while busy controls retain drafts and actual rate drives preparation", async () => {
  const opening = deferred(), loading = deferred(), stopping = deferred();
  const h = await harness({ openGate: opening, recordsList: [savedRecord()], recordsLoadGate: loading, stopGate: stopping });
  await h.preview();
  const latency = h.get("output-latency"), ms = h.get("output-latency-ms"), rate = h.get("output-rate");
  assert.deepEqual([latency.value, ms.value, rate.value], ["interactive", "10", ""]);
  assert.equal(ms.disabled, true);
  latency.value = "custom"; latency.emit("change");
  assert.equal(ms.disabled, false);
  ms.value = "10.125001"; rate.value = "96000";
  h.click("records-refresh"); await flush(); h.click("records-use"); await flush();
  assert.ok([latency, ms, rate].every(field => field.disabled));
  assert.equal(h.opens.length, 0);
  loading.resolve(); await flush();
  assert.ok([latency, ms, rate].every(field => !field.disabled));
  h.click("play");
  assert.equal(h.opens.length, 1);
  assert.equal(h.opens[0].gesture, true);
  const captured = h.opens[0].options.contextOptions;
  assert.deepEqual(structuredClone(captured), { latencyHint: 0.010125001, sampleRate: 96000 });
  assert.ok(Object.isFrozen(captured));
  assert.ok([latency, ms, rate].every(field => field.disabled));
  ms.value = "90000"; rate.value = "22050";
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  assert.equal(start.rate, 48000, "request preferences cannot replace actual output format");
  assert.deepEqual(structuredClone(captured), { latencyHint: 0.010125001, sampleRate: 96000 });
  await h.reply(await h.prepared(start, 1), null);
  await h.reply(worker.last("play-activate"), null);
  assert.equal(h.audio.samples.length, 0, "asset samples remain in the Worker-to-Worklet lane");
  h.click("stop"); await flush(); await h.receive(finalScore(start.playId));
  assert.ok([latency, ms, rate].every(field => field.disabled));
  stopping.resolve(); await flush();
  assert.deepEqual([latency.value, ms.value, rate.value], ["custom", "90000", "22050"]);
  assert.ok([latency, ms, rate].every(field => !field.disabled));
  ms.value = "2.5";
  delete h.faults.openGate;
  const next = await h.launch();
  assert.deepEqual(structuredClone(h.opens[1].options.contextOptions), { latencyHint: 0.0025, sampleRate: 22050 });
  assert.equal(h.opens[1].gesture, true);
  assert.equal(next.start.rate, 48000);
  await h.close();
});

test("both playback modes refuse invalid output drafts before opening and replay retry preserves recorded gameplay settings", async () => {
  const opening = deferred();
  const h = await harness({ openGate: opening, replayStart: 123456789n });
  await h.preview();
  chooseRecording(h, [selectedRecording().file]);
  const latency = h.get("output-latency"), ms = h.get("output-latency-ms"), rate = h.get("output-rate");
  for (const mode of ["play", "replay-play"]) {
    for (const [category, milliseconds, requested] of [["custom", "-0", "48000"], ["custom", "60000.000001", ""],
      ["balanced", "ignored", "0"], ["interactive", "ignored", "48e3"], ["playback", "ignored", "4294967296"]]) {
      latency.value = category; latency.emit("change"); ms.value = milliseconds; rate.value = requested;
      h.click(mode); await flush();
      assert.equal(h.opens.length, 0);
      assert.equal(h.workers[0].messages("play-start").length, 0);
      assert.equal(h.get("status").dataset.error, "true");
      assert.deepEqual([latency.value, ms.value, rate.value], [category, milliseconds, requested]);
      assert.equal(latency.disabled, false);
      assert.equal(ms.disabled, category !== "custom");
    }
  }
  latency.value = "balanced"; latency.emit("change"); ms.value = "invalid inactive custom value"; rate.value = "96000";
  h.get("live-start").value = "invalid live start";
  h.get("judge-early").value = "invalid live judge";
  h.get("binding-11").value = "KeyS"; // Invalid live duplicate is irrelevant to replay.
  h.click("replay-play");
  assert.equal(h.opens[0].gesture, true);
  assert.deepEqual(structuredClone(h.opens[0].options.contextOptions), { latencyHint: "balanced", sampleRate: 96000 });
  opening.reject(new Error("requested output context was refused")); await flush();
  assert.equal(h.workers[0].messages("play-start").length, 0);
  assert.match(h.get("status").textContent, /requested output context was refused/);
  assert.deepEqual([latency.value, ms.value, rate.value], ["balanced", "invalid inactive custom value", "96000"]);
  assert.equal(latency.disabled, false);
  assert.equal(ms.disabled, true);
  delete h.faults.openGate;
  const replay = await h.launch(0, "replay");
  assert.equal(h.opens[1].gesture, true);
  assert.deepEqual(structuredClone(h.opens[1].options.contextOptions), { latencyHint: "balanced", sampleRate: 96000 });
  assert.equal(replay.start.rate, 48000);
  for (const field of ["timing", "startNs", "keyPairs"]) assert.equal(Object.hasOwn(replay.start, field), false);
  assert.ok([latency, ms, rate].every(field => field.disabled));
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  assert.equal(h.get("live-start").value, "invalid live start");
  assert.equal(h.get("judge-early").value, "invalid live judge");
  assert.equal(rate.value, "96000");
  assert.equal(latency.disabled, false);
  assert.equal(ms.disabled, true);
  await h.close();
});

test("multiplayer readiness follows real audio setup and one committed grid arms both game and output", async () => {
  const h = await harness();
  await h.preview();
  assert.equal(h.get("multiplayer").checked, false);
  chooseMultiplayer(h);
  const start = await h.begin();
  const worker = h.workers[0];
  assert.equal(h.opens[0].gesture, true);
  assert.deepEqual(start.multiplayer, { url: "https://example.test:4433/competition", host: true,
    windowOriginNs: 9000000000n });
  assert.deepEqual(Array.from(start.localPlanWords), [1, 0, 0, 0]);
  assert.equal(h.get("multiplayer").disabled, true);
  const commands = await h.prepared(start, 1);
  assert.equal(h.audio.finishes, 1);
  assert.equal(worker.messages("play-network-ready").length, 0);
  assert.equal(worker.messages("play-network-ready").length, 0, "pending actual PCM command write is not readiness");
  assert.equal(commands.kind, "play-audio");
  await h.reply(commands, null);
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
  const peerBefore = h.get("multiplayer-status").textContent;
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "progress",
    songNs: -1n, hits: 18446744073709551615n, misses: 0n, combo: 7n, maxCombo: 99n } });
  assert.equal(h.get("multiplayer-status").textContent, peerBefore, "live peer counters stay in the Worker HUD");
  assert.deepEqual({ title: h.get("title").textContent, details: h.get("details").textContent,
    status: h.get("status").textContent }, before);
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "progress",
    songNs: 0n, hits: "4", misses: 0n, combo: 0n, maxCombo: 0n } });
  assert.equal(h.get("multiplayer-status").textContent, peerBefore, "Window has no periodic counter parser or rendering path");
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "disconnected", error: "peer left" } });
  assert.match(h.get("multiplayer-status").textContent, /peer left.*local play continues/i);
  assert.equal(h.audio.stopStarts, 0);
  assert.equal(worker.messages("play-stop").length, 0);
  await h.advance(8);
  assert.ok(worker.messages("play-step").length > 0);
  h.click("stop"); await flush();
  await h.receive(localFinal(start, { multiplayer: { finalWritten: true, finalAcknowledged: false, error: "peer ACK timed out",
    peers: [localPeer(1, 91, { status: "disconnected" })] } }));
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

async function launchPeerSession(h) {
  chooseMultiplayer(h, false);
  const start = await h.begin();
  await activatePeerSession(h, start);
  return start;
}

async function activatePeerSession(h, start, sampleCount = 0) {
  const worker = h.workers.at(-1);
  await h.reply(await h.prepared(start, sampleCount), null);
  await h.reply(worker.last("play-network-ready"), { kind: "multiplayer-start", targetHostNs: 1500000000n,
    songTargetHostNs: 1600000000n, uncertaintyNs: 0n });
  await h.reply(worker.last("play-activate"), null);
}

async function discoverLocalPeers(h, count = 2) {
  await localCount(h, count); h.click("local-discover"); await flush();
  localAssign(h, 1, 1n); localAssign(h, 2, 2n);
  if (count === 3) {
    const source = h.get("local-source-3").children.find(option => option.textContent.startsWith("Gamepad "));
    assert.ok(source); localAssign(h, 3, BigInt(source.value));
  }
}

async function launchLocalPeers(h, count = 2) {
  await discoverLocalPeers(h, count);
  return launchPeerSession(h);
}

function localPeer(player, remotePlayer, overrides = {}) {
  return { player, remotePlayer, status: "stopped", progress: null, final: false, error: null, ...overrides };
}

test("automatic one-player networking retains every admitted input owner and native sample without source discovery", async () => {
  const opening = deferred(), standard = nativeGamepad(0), ignored = nativeGamepad(1, { mapping: "" });
  const h = await harness({ openGate: opening, touchSupported: true, gamepads: [standard, ignored],
    hidSupported: true, hidAdmittedSources: [5n] });
  await h.preview(); const profile = selectedControllerProfile(); chooseControllerProfile(h, profile.file); chooseMultiplayer(h);
  assert.equal(h.get("local-count").value, "1"); assert.equal(h.get("local-discover").disabled, true);
  h.click("play");
  assert.equal(h.opens.length, 1); assert.equal(h.opens[0].gesture, true); assert.equal(h.gamepadReads, 1);
  assert.ok(h.traces.findIndex(row => row[0] === "gamepad-poll") < h.traces.findIndex(row => row[0] === "open"));
  assert.equal(h.hid.requests.length, 0);
  h.get("multiplayer").checked = false; h.get("local-count").value = "2"; h.get("local-count").emit("change");
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  assert.deepEqual(Array.from(start.localPlanWords), [1, 0, 0, 0]); assert.equal(start.localPage, 0);
  assert.equal(start.multiplayer.host, true); assert.equal(start.multiplayer.windowOriginNs, 9000000000n);
  assert.equal(start.inputMode, "physical-contact"); assert.equal(start.hidProfileFile, profile.file);
  assert.deepEqual(start.gamepadDevices.map(device => device.source), [3n, 4n]);
  assert.deepEqual(start.hidDevices.map(device => device.source), [5n, 6n]);
  assert.equal(h.hid.gets, 1); assert.ok(h.hidDevices.every(device => device.opens === 1));
  await activatePeerSession(h, start, 1);
  assert.equal(h.audio.samples.length, 0); assert.equal(h.audio.sampleAttachments, 1); assert.equal(h.audio.attachments, 1);
  assert.equal(h.get("local-page").disabled, true);
  h.get("local-page").value = "0"; h.get("local-page").emit("change"); await flush();
  assert.equal(worker.messages("play-page").length, 0, "automatic scope does not expose a page transition");
  const native = h.hidDevices[0], display = watchPlayDisplay(h), layout = h.layoutReads;
  h.setNow(1700.125); standard.timestamp = 1700.0625;
  standard.buttons[0] = { value: 0.12345678901234566, pressed: true, touched: false };
  const backing = Uint8Array.from([99, 7, 255, 88]);
  h.faults.onGamepadPoll = () => {
    delete h.faults.onGamepadPoll;
    native.emit("inputreport", { device: native, reportId: 7, timeStamp: 1700.125, data: new DataView(backing.buffer, 1, 2) });
  };
  await h.advance(8);
  const first = worker.last("play-step");
  assert.deepEqual(first.events.map(event => [event.kind, event.source]), [["hid", 5n], ["gamepad", 3n]]);
  assert.deepEqual(first.events.map(event => event.hostNs), [1700125000n, 1700062500n]);
  assert.equal(first.events[1].buttons[0].value, 0.12345678901234566);
  backing.fill(0); assert.deepEqual(Array.from(first.events[0].data), [7, 255]);
  h.get("canvas").emit("pointerdown", { pointerType: "touch", pointerId: -2, timeStamp: 1700.25,
    offsetX: 120.25, offsetY: 180.5, pressure: 0.375 });
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1700.5 });
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: first.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  const second = worker.last("play-step");
  assert.equal(second.events[0].kind, "touch"); assert.equal(second.events[0].hostNs, 1700250000n);
  assert.equal(second.events[0].code, 0xfffffffe); assert.equal(second.events[1].key, 2);
  assert.equal(second.events[1].hostNs, 1700500000n); assert.equal(second.events.length, 2);
  assert.ok(first.events[0].sequence < first.events[1].sequence);
  assert.ok(first.events[1].sequence < second.events[0].sequence && second.events[0].sequence < second.events[1].sequence);
  const reads = h.gamepadReads;
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: second.tickId,
    songNs: 1n, hits: 0n, misses: 0n, combo: 0n, preOriginInputs: 0 });
  assert.equal(h.gamepadReads, reads, "peer touch, key and ACK callbacks never poll Gamepads");
  await h.advance(8);
  const third = worker.last("play-step");
  assert.equal(third.events.length, 1); assert.equal(third.events[0].source, 3n);
  assert.ok(second.events[1].sequence < third.events[0].sequence);
  assert.deepEqual(display, []); assert.equal(h.layoutReads, layout); assert.equal(profile.reads, 0);
  h.click("stop"); await flush(); await h.receive(localFinal(start, { multiplayer: { finalWritten: true,
    finalAcknowledged: true, error: null, peers: [localPeer(1, 77)] } }));
  assert.ok(h.hidDevices.every(device => device.closes === 1)); assert.equal(h.releases.length, 1);
  assert.equal(h.window.listeners.get("gamepaddisconnected")?.size ?? 0, 0); await h.close();
});

test("automatic cohort source receipts preserve standard eligibility and custom owned subsets without guessing unsupported controls", async () => {
  for (const kind of ["unavailable", "empty", "unsupported", "custom"]) {
    const pads = kind === "empty" ? [] : kind === "unsupported"
      ? [nativeGamepad(0, { mapping: "" }), nativeGamepad(1, { buttons: nativeGamepad().buttons.slice(0, 8) })]
      : [nativeGamepad(0, { mapping: "" }), nativeGamepad(1, { mapping: "" })];
    const h = await harness(kind === "unavailable" ? {} : { gamepads: pads,
      ...(kind === "custom" ? { gamepadAdmittedSources: [4n] } : {}) });
    await h.preview(); chooseMultiplayer(h, false);
    const profile = kind === "custom" ? selectedGamepadProfile() : null;
    if (profile) chooseGamepadProfile(h, profile.file);
    const start = await h.begin();
    assert.deepEqual(Array.from(start.localPlanWords), [1, 0, 0, 0]);
    assert.equal(Object.hasOwn(start, "gamepadDevices"), kind !== "unavailable");
    await activatePeerSession(h, start);
    h.setNow(1700); await h.advance(8);
    const worker = h.workers[0], tick = worker.last("play-step");
    assert.deepEqual(tick.events.filter(event => event.kind === "gamepad").map(event => event.source), kind === "custom" ? [4n] : []);
    if (profile) {
      assert.equal(start.gamepadProfileFile, profile.file); assert.equal(profile.reads, 0);
      pads[0].connected = false; h.window.emit("gamepaddisconnected", { gamepad: pads[0] }); await flush();
      assert.equal(worker.messages("play-stop").length, 0, "unmatched custom devices remain outside the automatic member's admitted inputs");
    }
    h.click("stop"); await flush(); await h.receive(localFinal(start, { multiplayer: { finalWritten: false,
      finalAcknowledged: false, error: null, peers: [localPeer(1, null)] } }));
    const reads = h.gamepadReads; chooseRecording(h, [selectedRecording().file]);
    const replay = await h.launch(0, "replay");
    assert.equal(replay.start.localPlanWords, undefined); assert.equal(replay.start.multiplayer, undefined);
    assert.equal(replay.start.gamepadDevices, undefined); assert.equal(h.gamepadReads, reads);
    h.click("stop"); await flush(); await h.receive(finalScore(replay.id)); await h.close();
  }
});

test("automatic network saved comparisons target the actual sole member without mutating retained solo selections", async () => {
  const h = await harness(); await h.preview();
  const own = selectedRecording(), other = selectedRecording();
  selectOpponent(h, own, { own: true, label: "Own prefix" });
  selectOpponent(h, other, { own: false, label: "Other prefix" });
  assert.equal(h.get("opponent-player-file:1").value, ""); assert.equal(h.get("opponent-player-file:2").value, "");
  const start = await launchPeerSession(h);
  assert.deepEqual(Array.from(start.opponents, row => [row.player, row.own, row.label]), [[1, true, "Own prefix"], [1, false, "Other prefix"]]);
  assert.equal(start.opponents[0].file, own.file); assert.equal(start.opponents[1].file, other.file);
  assert.equal(own.reads, 0); assert.equal(other.reads, 0);
  h.click("stop"); await flush();
  const opponents = [comparison(start.playId, { kind: "own", label: "Own prefix", hits: 7n, misses: 0n }).opponents[0],
    comparison(start.playId, { kind: "other", label: "Other prefix", hits: 9n, misses: 0n }).opponents[0]];
  await h.receive(localFinal(start, { savedOpponents: { opponents: null, error: null,
    localOpponents: [{ player: 1, opponents, error: null }] }, multiplayer: { finalWritten: true,
    finalAcknowledged: true, error: null, peers: [localPeer(1, 99)] } }));
  assert.match(content(h.get("opponents-results")), /Player 1.*Own prefix.*Player 1.*Other prefix/);
  assert.equal(h.get("opponent-player-file:1").value, ""); assert.equal(h.get("opponent-player-file:2").value, "");
  h.get("multiplayer").checked = false;
  const solo = await h.launch();
  assert.equal(solo.start.localPlanWords, undefined);
  assert.ok(solo.start.opponents.every(row => !Object.hasOwn(row, "player")), "session mapping never persists into the user's solo selection");
  h.click("stop"); await flush(); await h.receive(finalScore(solo.id, { savedOpponents: { opponents, error: null } }));
  chooseRecording(h, [selectedRecording().file]); h.get("multiplayer").checked = true;
  const replay = await h.launch(0, "replay");
  assert.equal(replay.start.opponents, undefined); assert.equal(replay.start.localPlanWords, undefined);
  assert.equal(replay.start.multiplayer, undefined); assert.equal(own.reads, 0); assert.equal(other.reads, 0);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id)); await h.close();
});

test("one automatic member records and saves its actual cohort row while an independently sized peer contributes only its assigned prefix", async () => {
  const stopping = deferred(), h = await harness({ stopGate: stopping }); await h.preview(); h.get("record").checked = true;
  const start = await launchPeerSession(h), worker = h.workers[0];
  assert.deepEqual(Array.from(start.localPlanWords), [1, 0, 0, 0]); assert.equal(start.recordReplay, true);
  h.setNow(1700); await h.advance(8);
  const tick = worker.last("play-step"), render = worker.last("play-render");
  await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: start.playId, renderId: render.renderId,
    completed: true, commandsPending: false, observedTick: tick.tickId });
  assert.equal(worker.messages("play-stop").length, 0);
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: tick.tickId,
    songNs: 2350000000n, hits: 9007199254740993n, misses: 1n, combo: 2n, preOriginInputs: 0 });
  assert.equal(worker.last("play-stop").completed, true);
  const bytes = Uint8Array.from([66, 75, 82, 1, 255]);
  const receipt = localFinal(start, { hits: 777n, misses: 777n, combo: 777n,
    localScores: [{ player: 1, songNs: 2350000000n, hits: 9007199254740993n, misses: 1n, combo: 2n, maxCombo: 3n }],
    replays: [{ player: 1, replay: bytes, replayError: null, replayComplete: true }],
    multiplayer: { finalWritten: true, finalAcknowledged: true, error: null,
      peers: [localPeer(1, 0xffffffff, { final: true, progress: { songNs: 604800000000001n,
        hits: 18446744073709551615n, misses: 0n, combo: 4n, maxCombo: 5n } })] } });
  await h.receive(receipt);
  assert.equal(h.get("captured-replay").disabled, true); assert.equal(h.get("export").disabled, true);
  stopping.resolve(); await flush();
  assert.match(content(h.get("local-results")), /Player 1.*9007199254740993.*Complete recording/);
  assert.doesNotMatch(content(h.get("local-results")), /777|18446744073709551615/);
  assert.match(h.get("multiplayer-status").textContent, /Player 1.*remote Player 4294967295.*604800\.000000001.*18446744073709551615/);
  assert.equal(h.get("export").disabled, true, "the one-member cohort still selects the actual recording explicitly");
  h.get("captured-replay").value = "1"; h.get("captured-replay").emit("change");
  h.click("export"); await flush();
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), bytes);
  assert.equal(h.downloads.at(-1).filename, `beatkernel-${start.playId}-player-1-complete.bkr`);
  h.click("records-save"); await flush();
  const saved = h.recordCalls.find(call => call.method === "save");
  assert.deepEqual(saved.value.bytes, bytes); assert.equal(saved.value.complete, true);
  assert.equal(saved.value.hits, 9007199254740993n); assert.equal(saved.value.misses, 1n); assert.equal(saved.value.combo, 2n);
  await h.close();
});

test("automatic cohort metadata refusal and failed prefixes release for retry without discovery or stale member results", async () => {
  for (const changed of [{ localPlayers: undefined }, { localPlayers: [2] }, { localPage: 1 },
    { recordLimits: { bytes: 67108863, records: 1000000 } }]) {
    const h = await harness(); await h.preview(); h.get("record").checked = true; chooseMultiplayer(h);
    const start = await h.begin(), worker = h.workers[0];
    await h.reply(start, { kind: "prepared", title: "Contradictory automatic metadata", samples: 1, lanes: [0x11],
      startNs: 0n, opponentCount: 0, localPlayers: [1], localPage: 0,
      recordLimits: { bytes: 67108864, records: 1000000 }, ...changed });
    assert.equal(worker.messages("play-samples-upload").length, 0); assert.equal(worker.messages("play-network-ready").length, 0);
    assert.deepEqual(h.audio.arms, []); assert.equal(worker.last("play-stop").playId, start.playId);
    await h.receive(localFinal(start)); assert.equal(h.get("play").disabled, false);
    const retry = await launchPeerSession(h); assert.deepEqual(Array.from(retry.localPlanWords), [1, 0, 0, 0]);
    h.click("stop"); await flush(); await h.receive(localFinal(retry)); await h.close();
  }
  const h = await harness(); await h.preview(); h.get("record").checked = true;
  const failed = await launchPeerSession(h), prefix = Uint8Array.from([66, 75, 82, 9]);
  const failure = localFinal(failed, { kind: "play-error", message: "actual member input failure", released: true,
    replays: [{ player: 1, replay: prefix, replayError: null, replayComplete: false }],
    multiplayer: { finalWritten: false, finalAcknowledged: false, error: "group disconnected", peers: [localPeer(1, null, { status: "disconnected" })] } });
  await h.receive(failure); assert.match(h.get("status").textContent, /actual member input failure/);
  h.get("captured-replay").value = "1"; h.get("captured-replay").emit("change"); h.click("export"); await flush();
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), prefix);
  assert.equal(h.downloads.at(-1).filename, `beatkernel-${failed.playId}-player-1-prefix.bkr`);
  const retry = await launchPeerSession(h), before = h.get("multiplayer-status").textContent;
  await h.receive(failure); assert.equal(h.get("multiplayer-status").textContent, before);
  assert.equal(h.get("stop").disabled, false); assert.equal(h.get("local-discover").disabled, true);
  h.click("stop"); await flush(); await h.receive(localFinal(retry)); await h.close();
});

test("discovered local sources and network settings freeze together before audio and share one committed start", async () => {
  const opening = deferred(), pad = nativeGamepad();
  const h = await harness({ touchSupported: true, gamepads: [pad], openGate: opening });
  const preview = await h.preview();
  await localCount(h, 3); await localCount(h, 2); await localCount(h, 3);
  h.click("local-discover"); await flush();
  const source = BigInt(h.get("local-source-4").children.find(option => option.textContent.startsWith("Gamepad ")).value);
  localAssign(h, 1, 1n); localAssign(h, 2, 2n); localAssign(h, 4, source);
  chooseMultiplayer(h);
  const retainedDisconnect = [...h.window.listeners.get("gamepaddisconnected")][0];
  h.click("play");
  assert.equal(h.opens.length, 1); assert.equal(h.opens[0].gesture, true);
  for (const id of ["local-count", "local-discover", "local-release", "local-source-1", "local-source-2", "local-source-4",
    "multiplayer", "multiplayer-url", "multiplayer-role"]) assert.equal(h.get(id).disabled, true);
  h.get("local-count").value = "2"; h.get("local-count").emit("change");
  localAssign(h, 4, 1n);
  h.get("multiplayer-url").value = "https://later.example/rooms/changed";
  h.get("multiplayer-role").value = "join";
  opening.resolve(h.audio); await flush();
  const worker = h.workers[0], start = worker.last("play-start");
  assert.deepEqual(Array.from(start.localPlanWords), [1, 1, 1, 0, 2, 1, 2, 0, 4, 1, Number(source), 0]);
  assert.deepEqual(start.multiplayer, { url: "https://example.test:4433/competition", host: true, windowOriginNs: 9000000000n });
  assert.equal(start.localPage, 0); assert.equal(start.inputMode, "physical-contact");
  assert.equal([...h.window.listeners.get("gamepaddisconnected")][0], retainedDisconnect);
  const handoff = await h.prepared(start, 1);
  assert.equal(h.audio.samples.length, 0); assert.equal(h.audio.sampleAttachments, 1); assert.equal(h.audio.finishes, 1);
  assert.equal(worker.messages("play-network-ready").length, 0); assert.deepEqual(h.audio.arms, []);
  await h.reply(handoff, null);
  const ready = worker.last("play-network-ready"); assert.ok(ready);
  assert.equal(h.audio.attachments, 1); assert.equal(worker.messages("play-audio").length, 1);
  await h.reply(ready, { kind: "multiplayer-start", targetHostNs: 1500000001n,
    songTargetHostNs: 1600000001n, uncertaintyNs: 0n });
  const activation = worker.last("play-activate");
  assert.deepEqual(h.audio.arms, [72001n]); assert.equal(activation.startFrame, 72001n);
  assert.equal(activation.targetHostNs, 1500000001n); assert.equal(activation.hostNs, 1500020833n);
  await h.reply(activation, null);
  h.setNow(1700);
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1700 });
  const tick = worker.last("play-step");
  assert.ok(tick.events.some(event => event.key === 2 && event.down === true && event.hostNs === 1700000000n));
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: tick.tickId,
    songNs: 100000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  assert.equal(worker.messages("play-commands").length, 0); assert.equal(worker.messages("play-ack").length, 0);
  assert.equal(h.audio.polls, 0);
  h.click("stop"); await flush();
  await h.receive(localFinal(start, { multiplayer: { finalWritten: true, finalAcknowledged: true, error: null,
    peers: [localPeer(1, 91), localPeer(2, 92), localPeer(4, null)] } }));
  assert.equal(h.audio.stopStarts, 1); assert.equal(h.window.listeners.get("gamepaddisconnected")?.size ?? 0, 0);
  assert.equal(h.get("position").value, preview.position); assert.equal(h.get("play").disabled, false);
  await h.close();
});

test("local network launch still requires owned distinct sources and exact prepared membership before any PCM", async () => {
  for (const invalid of ["undiscovered", "unassigned", "duplicate", "unowned", "role"]) {
    const h = await harness({ touchSupported: true }); await h.preview(); await localCount(h, 2);
    chooseMultiplayer(h);
    if (invalid !== "undiscovered") {
      h.click("local-discover"); await flush(); localAssign(h, 1, 1n);
      if (invalid === "duplicate") localAssign(h, 2, 1n);
      if (invalid === "unowned") localAssign(h, 2, 18446744073709551615n);
      if (invalid === "role") { localAssign(h, 2, 2n); h.get("multiplayer-role").value = "spectator"; }
    }
    h.click("play"); await flush();
    assert.equal(h.opens.length, 0, invalid); assert.equal(h.workers[0].messages("play-start").length, 0, invalid);
    await h.close();
  }
  const h = await harness({ touchSupported: true }); await h.preview(); await discoverLocalPeers(h);
  chooseMultiplayer(h, false);
  const start = await h.begin(), worker = h.workers[0];
  await h.reply(start, { kind: "prepared", title: "Wrong group", samples: 1, lanes: [0x11],
    startNs: 0n, opponentCount: 0, localPlayers: [2, 1], localPage: 0 });
  assert.equal(worker.messages("play-samples-upload").length, 0); assert.equal(h.audio.samples.length, 0);
  assert.equal(worker.messages("play-network-ready").length, 0); assert.deepEqual(h.audio.arms, []);
  assert.equal(worker.last("play-stop").playId, start.playId);
  await h.receive(localFinal(start)); assert.match(h.get("status").textContent, /roster|page/);
  await h.close();
});

test("local peer notifications never render periodic group counters and display failure is correlated once per admitted member", async () => {
  const h = await harness({ touchSupported: true }); await h.preview();
  const start = await launchLocalPeers(h), worker = h.workers[0];
  const field = h.get("multiplayer-status"), writes = [], display = watchPlayDisplay(h);
  let text = field.textContent;
  Object.defineProperty(field, "textContent", { configurable: true, get: () => text, set(value) { writes.push(value); text = value; } });
  const localRows = [...h.get("local-results").children], sources = [...h.get("local-sources").children];
  for (let index = 0; index < 18; index++) {
    await h.receive({ kind: "play-multiplayer", playId: start.playId, event: {
      kind: ["roster", "group-progress", "group-final-progress"][index % 3],
      players: new Uint32Array([91, 92]), sequence: BigInt(index), words: new Uint32Array([0xffffffff]),
    } });
  }
  for (const player of [undefined, null, 0, 3, "1", 0x100000000]) {
    await h.receive({ kind: "play-multiplayer", playId: start.playId,
      event: { kind: "peer-display-unavailable", player, error: "unowned display notice" } });
  }
  assert.deepEqual(writes, []); assert.deepEqual(display, []);
  assert.deepEqual(h.get("local-results").children, localRows); assert.deepEqual(h.get("local-sources").children, sources);
  for (const player of [1, 1, 2, 2, 1]) {
    await h.receive({ kind: "play-multiplayer", playId: start.playId,
      event: { kind: "peer-display-unavailable", player, error: `member ${player} presentation failed` } });
  }
  assert.equal(writes.length, 2); assert.match(writes[0], /Player 1 peer.*member 1 presentation failed/);
  assert.match(writes[1], /Player 2 peer.*member 2 presentation failed/);
  assert.equal(worker.messages("play-stop").length, 0); assert.equal(h.audio.stopStarts, 0);
  h.setNow(1700); await h.advance(8);
  assert.ok(worker.last("play-step")); assert.ok(worker.last("play-render"));
  assert.deepEqual(display, []);
  h.click("stop"); await flush();
  await h.receive(localFinal(start, { multiplayer: { finalWritten: false, finalAcknowledged: false, error: null,
    peers: [localPeer(1, 91, { error: "member 1 presentation failed" }), localPeer(2, 92)] } }));
  await h.close();
});

test("joined local network results retain separate full-width peer prefixes and independent member replay exports", async () => {
  const stopping = deferred(), h = await harness({ touchSupported: true, gamepads: [nativeGamepad()], stopGate: stopping });
  const preview = await h.preview(); h.get("record").checked = true;
  const start = await launchLocalPeers(h, 3), worker = h.workers[0];
  const field = h.get("multiplayer-status"), writes = [];
  let text = field.textContent;
  Object.defineProperty(field, "textContent", { configurable: true, get: () => text, set(value) { writes.push(value); text = value; } });
  const first = Uint8Array.from([66, 75, 82, 1]), third = Uint8Array.from([66, 75, 82, 3]);
  const peers = [
    localPeer(1, 0xffffffff, { status: "disconnected", final: true, progress: {
      songNs: -1n, hits: 18446744073709551615n, misses: 0n, combo: 7n, maxCombo: 99n } }),
    localPeer(2, 7, { progress: { songNs: 604800000000001n, hits: 9007199254740993n, misses: 2n, combo: 3n, maxCombo: 4n },
      error: "member 2 canvas unavailable" }),
    localPeer(3, null),
  ];
  h.setNow(1700); await h.advance(8);
  const tick = worker.last("play-step"), render = worker.last("play-render");
  await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: start.playId, renderId: render.renderId,
    completed: true, commandsPending: false, observedTick: tick.tickId });
  assert.equal(worker.messages("play-stop").length, 0, "even a group final report cannot skip input acknowledgement");
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: tick.tickId,
    songNs: 2350000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  assert.equal(worker.last("play-stop").completed, true);
  const final = localFinal(start, { replays: [
    { player: 1, replay: first, replayError: null, replayComplete: true },
    { player: 2, replay: null, replayError: "member 2 export refused", replayComplete: false },
    { player: 3, replay: third, replayError: null, replayComplete: true },
  ], multiplayer: { finalWritten: true, finalAcknowledged: false, error: "group ACK deadline", peers } });
  await h.receive(final);
  assert.deepEqual(writes, []); assert.equal(h.get("play").disabled, true);
  assert.equal(h.get("captured-replay").disabled, true);
  stopping.resolve(); await flush();
  assert.equal(writes.length, 1); assert.match(text, /written.*ACK unavailable.*group ACK deadline/);
  const one = text.slice(text.indexOf("Player 1 ·"), text.indexOf("Player 2 ·"));
  const two = text.slice(text.indexOf("Player 2 ·"), text.indexOf("Player 3 ·"));
  const three = text.slice(text.indexOf("Player 3 ·"));
  assert.match(one, /remote Player 4294967295.*final prefix.*disconnected.*-0\.000000001.*18446744073709551615/);
  assert.doesNotMatch(one, /9007199254740993|member 2/);
  assert.match(two, /remote Player 7.*604800\.000000001.*9007199254740993.*member 2 canvas unavailable/);
  assert.doesNotMatch(two, /18446744073709551615/);
  assert.match(three, /unassigned remote player.*stopped.*no received score prefix/); assert.doesNotMatch(three, /Hits|final prefix/);
  const local = content(h.get("local-results"));
  assert.match(local, /member 2 export refused/); assert.doesNotMatch(local, /18446744073709551615|9007199254740993/);
  assert.equal(h.get("title").textContent, preview.title); assert.equal(h.get("position").value, preview.position);
  assert.equal(h.get("play").disabled, false); assert.equal(h.get("export").disabled, true);
  h.get("captured-replay").value = "3"; h.get("captured-replay").emit("change"); h.click("export"); await flush();
  assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), third);
  assert.equal(h.downloads.at(-1).filename, `beatkernel-${start.playId}-player-3-complete.bkr`);
  await h.receive(final); assert.equal(writes.length, 1);
  await h.close();
});

test("malformed local peer envelopes reject the entire comparison without scalar fallback or loss of local replay evidence", async () => {
  const valid = () => [localPeer(1, 91, { final: true, progress: {
    songNs: 604800000000001n, hits: 9007199254740993n, misses: 1n, combo: 2n, maxCombo: 3n } }), localPeer(2, null)];
  const cases = [
    () => undefined, () => ({}), rows => rows.slice(0, 1), rows => rows.reverse(),
    rows => [rows[0], { ...rows[1], player: 1 }], rows => [rows[0], { ...rows[1], player: 3 }],
    rows => [rows[0], { ...rows[1], player: "2" }], rows => [rows[0], ,],
    rows => [rows[0], { ...rows[1], remotePlayer: 0 }], rows => [rows[0], { ...rows[1], remotePlayer: "92" }],
    rows => [rows[0], { ...rows[1], progress: rows[0].progress }], rows => [rows[0], { ...rows[1], final: true }],
    rows => [rows[0], { ...localPeer(2, 92), final: 1 }],
    rows => [rows[0], { ...localPeer(2, 92), status: "revived" }],
    rows => [rows[0], { ...localPeer(2, 92), error: "" }],
    rows => [rows[0], localPeer(2, 92, { progress: { ...rows[0].progress, hits: "9007199254740993" } })],
    rows => [rows[0], localPeer(2, 92, { progress: { ...rows[0].progress, combo: 4n, maxCombo: 3n } })],
    rows => [rows[0], localPeer(2, 92, { progress: { ...rows[0].progress, hits: 18446744073709551615n, misses: 1n } })],
  ];
  for (const malformed of cases) {
    const h = await harness({ touchSupported: true }); await h.preview(); h.get("record").checked = true;
    const start = await launchLocalPeers(h), first = Uint8Array.from([66, 75, 82, 1]), second = Uint8Array.from([66, 75, 82, 2]);
    h.click("stop"); await flush();
    await h.receive(localFinal(start, { replays: [
      { player: 1, replay: first, replayError: null, replayComplete: false },
      { player: 2, replay: second, replayError: null, replayComplete: false },
    ], multiplayer: { finalWritten: true, finalAcknowledged: true, error: null, peers: malformed(valid()),
      peer: { ...localPeer(1, 91), progress: { songNs: 0n, hits: 777777n, misses: 0n, combo: 0n, maxCombo: 0n } } } }));
    const text = h.get("multiplayer-status").textContent;
    assert.match(text, /acknowledged.*Final local peer summaries unavailable or malformed.*Local result unchanged/);
    assert.doesNotMatch(text, /9007199254740993|777777|remote Player 91/);
    assert.equal(h.get("play").disabled, false);
    assert.doesNotMatch(h.get("status").textContent, /Replay export failed|cleanup failed/);
    assert.doesNotMatch(content(h.get("local-results")), /unavailable|malformed/);
    assert.deepEqual(h.get("captured-replay").children.filter(option => option.value !== "").map(option => option.value), ["1", "2"]);
    h.get("captured-replay").value = "2"; h.get("captured-replay").emit("change"); h.click("export"); await flush();
    assert.deepEqual(new Uint8Array(await h.urls.at(-1).blob.arrayBuffer()), second);
    await h.close();
  }
});

test("cancelled group readiness and late final receipts cannot arm or overwrite a new page, automatic member or replay", async () => {
  const cancelled = await harness({ touchSupported: true }); await cancelled.preview(); await discoverLocalPeers(cancelled);
  chooseMultiplayer(cancelled);
  const start = await cancelled.begin(), firstWorker = cancelled.workers[0];
  await cancelled.reply(await cancelled.prepared(start), null);
  const ready = firstWorker.last("play-network-ready");
  cancelled.window.emit("keydown", { code: "Escape", repeat: false, timeStamp: 1000 }); await flush();
  await cancelled.reply(ready, { kind: "multiplayer-start", targetHostNs: 1500000000n,
    songTargetHostNs: 1600000000n, uncertaintyNs: 0n });
  assert.deepEqual(cancelled.audio.arms, []); assert.equal(firstWorker.messages("play-activate").length, 0);
  await cancelled.receive(localFinal(start, { multiplayer: { finalWritten: false, finalAcknowledged: false,
    error: "cancelled", peers: [localPeer(1, null), localPeer(2, null)] } }));
  assert.equal(cancelled.get("play").disabled, false); await cancelled.close();

  const stopping = deferred(), h = await harness({ touchSupported: true, stopGate: stopping });
  await h.preview(); const prior = await launchLocalPeers(h);
  h.click("stop"); await flush();
  const receipt = localFinal(prior, { multiplayer: { finalWritten: true, finalAcknowledged: true, error: null,
    peers: [localPeer(1, 91, { progress: { songNs: 0n, hits: 777777n, misses: 0n, combo: 0n, maxCombo: 0n } }), localPeer(2, null)] } });
  await h.receive(receipt);
  h.window.emit("pagehide"); await flush(); h.window.emit("pageshow", { persisted: true }); await flush();
  const freshStatus = h.get("multiplayer-status").textContent, freshResults = content(h.get("local-results"));
  stopping.resolve(); await flush();
  assert.equal(h.get("multiplayer-status").textContent, freshStatus); assert.equal(content(h.get("local-results")), freshResults);
  delete h.faults.stopGate; await h.preview(); await localCount(h, 1);
  const automatic = await launchPeerSession(h); assert.deepEqual(Array.from(automatic.localPlanWords), [1, 0, 0, 0]);
  const automaticStatus = h.get("multiplayer-status").textContent;
  await h.receive(receipt); assert.equal(h.get("multiplayer-status").textContent, automaticStatus);
  h.click("stop"); await flush();
  await h.receive(localFinal(automatic, { multiplayer: { finalWritten: true, finalAcknowledged: true, error: null,
    peers: [localPeer(1, 91, { progress: { songNs: 0n, hits: 9n, misses: 0n, combo: 1n, maxCombo: 2n } })] } }));
  assert.match(h.get("multiplayer-status").textContent, /Hits 9/); assert.doesNotMatch(h.get("multiplayer-status").textContent, /777777/);
  await localCount(h, 2); chooseRecording(h, [selectedRecording().file]);
  h.get("multiplayer-url").value = "invalid live draft";
  const replay = await h.launch(0, "replay");
  assert.equal(replay.start.localPlanWords, undefined); assert.equal(replay.start.multiplayer, undefined);
  const replayStatus = h.get("multiplayer-status").textContent;
  await h.receive(receipt); await h.receive({ kind: "play-multiplayer", playId: replay.id,
    event: { kind: "peer-display-unavailable", player: 1, error: "no replay network owner" } });
  assert.equal(h.get("multiplayer-status").textContent, replayStatus);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id)); await h.close();
});

test("Window leaves all periodic and final peer counters to the Worker and shows one actual prefix after joined cleanup", async () => {
  const stopping = deferred(), h = await harness({ stopGate: stopping });
  const preview = await h.preview(), start = await launchPeerSession(h);
  const field = h.get("multiplayer-status"), writes = [];
  let currentText = field.textContent;
  Object.defineProperty(field, "textContent", { configurable: true, get: () => currentText,
    set(value) { writes.push(value); currentText = value; } });
  const peer = { songNs: -1n, hits: 18446744073709551615n, misses: 0n, combo: 7n, maxCombo: 99n };
  for (let index = 0; index < 20; index++) {
    await h.receive({ kind: "play-multiplayer", playId: start.playId,
      event: { kind: index % 2 ? "progress" : "final-progress", ...peer } });
  }
  await h.receive({ kind: "play-multiplayer", playId: start.playId, event: { kind: "progress", hits: "bad" } });
  assert.deepEqual(writes, []); assert.equal(h.audio.stopStarts, 0);
  h.click("stop"); await flush();
  const final = localFinal(start, { multiplayer: { finalWritten: true, finalAcknowledged: false,
    error: "actual ACK timeout", peers: [localPeer(1, 91, { status: "disconnected", progress: peer, final: true })] } });
  await h.receive(final);
  assert.deepEqual(writes, []); assert.equal(h.get("play").disabled, true);
  stopping.resolve(); await flush();
  assert.match(field.textContent, /18446744073709551615/); assert.match(field.textContent, /-0\.000000001/);
  assert.match(field.textContent, /ACK.*unavailable.*actual ACK timeout/i);
  assert.match(field.textContent, /reported/i); assert.match(field.textContent, /final/i);
  assert.equal(h.get("title").textContent, preview.title); assert.equal(h.get("position").value, preview.position);
  assert.match(content(h.get("local-results")), /Player 1.*Hits 1.*Misses 0/);
  const count = writes.length;
  await h.receive(final); await h.receive({ kind: "play-multiplayer", playId: start.playId,
    event: { kind: "final-progress", ...peer, songNs: 604800000000001n } });
  assert.equal(writes.length, count);
  h.get("multiplayer").checked = false;
  const solo = await h.launch(); assert.equal(solo.start.multiplayer, undefined);
  const soloText = field.textContent;
  await h.receive(final); assert.equal(field.textContent, soloText);
  h.click("stop"); await flush(); await h.receive(finalScore(solo.id));
  chooseRecording(h, [selectedRecording().file]); h.get("multiplayer").checked = true;
  const replay = await h.launch(0, "replay"); assert.equal(replay.start.multiplayer, undefined);
  const replayText = field.textContent;
  await h.receive({ kind: "play-multiplayer", playId: replay.id, event: { kind: "progress", ...peer } });
  assert.equal(field.textContent, replayText);
  h.click("stop"); await flush(); await h.receive(finalScore(replay.id));
  await h.close();
});

test("peer display errors and malformed or absent final prefixes never change local capture or a later page", async () => {
  const valid = { songNs: 604800000000001n, hits: 9007199254740993n, misses: 2n, combo: 3n, maxCombo: 4n };
  const cases = [
    { status: "stopped", progress: valid, final: true, error: "actual HUD unavailable" },
    { status: "stopped", progress: null, final: false, error: null },
    { status: "stopped", progress: { ...valid, hits: "9007199254740993" }, final: true, error: null },
    { status: "stopped", progress: { ...valid, hits: 18446744073709551615n, misses: 1n }, final: true, error: null },
    { status: "revived", progress: valid, final: true, error: null },
  ];
  for (const [index, peer] of cases.entries()) {
    const h = await harness(); await h.preview(); h.get("record").checked = true;
    const start = await launchPeerSession(h);
    if (index === 0) {
      await h.receive({ kind: "play-multiplayer", playId: start.playId,
        event: { kind: "peer-display-unavailable", player: 1, error: "actual HUD unavailable" } });
      assert.match(h.get("multiplayer-status").textContent, /actual HUD unavailable/);
      assert.equal(h.audio.stopStarts, 0); assert.equal(h.workers[0].messages("play-stop").length, 0);
    }
    h.click("stop"); await flush();
    await h.receive(localFinal(start, { replays: [{ player: 1, replay: Uint8Array.from([66, 75, 82]), replayComplete: false, replayError: null }],
      multiplayer: { finalWritten: true, finalAcknowledged: true, error: null, peers: [localPeer(1, 91, peer)] } }));
    assert.match(content(h.get("local-results")), /Player 1.*Hits 1.*Misses 0/);
    h.get("captured-replay").value = "1"; h.get("captured-replay").emit("change");
    assert.equal(h.get("export").disabled, false); assert.match(h.get("export").textContent, /prefix/);
    assert.doesNotMatch(h.get("status").textContent, /Replay export failed|Gameplay cleanup failed/);
    const display = h.get("multiplayer-status").textContent;
    assert.match(display, /acknowledged/i);
    if (index === 0) { assert.match(display, /9007199254740993/); assert.match(display, /actual HUD unavailable/); }
    else if (index === 1) assert.doesNotMatch(display, /Hits\s+0|9007199254740993/);
    else { assert.match(display, /malformed|unavailable/i); assert.doesNotMatch(display, /Hits\s+9007199254740993/); }
    await h.close();
  }
  const stopping = deferred(), h = await harness({ stopGate: stopping });
  await h.preview(); const prior = await launchPeerSession(h);
  h.click("stop"); await flush();
  const receipt = localFinal(prior, { multiplayer: { finalWritten: true, finalAcknowledged: true, error: null,
    peers: [localPeer(1, 91, { progress: valid, final: true })] } });
  await h.receive(receipt);
  h.window.emit("pagehide"); await flush(); h.window.emit("pageshow", { persisted: true }); await flush();
  const resetText = h.get("multiplayer-status").textContent;
  stopping.resolve(); await flush(); assert.equal(h.get("multiplayer-status").textContent, resetText);
  await h.preview(); h.get("multiplayer").checked = false; delete h.faults.stopGate;
  const current = await h.launch(), text = h.get("multiplayer-status").textContent;
  await h.receive(receipt); assert.equal(h.get("multiplayer-status").textContent, text);
  assert.equal(h.get("stop").disabled, false);
  h.click("stop"); await flush(); await h.receive(finalScore(current.id)); await h.close();
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
    await h.receive(localFinal(start, { multiplayer: { finalWritten: false, finalAcknowledged: false, error: "setup refused",
      peers: [localPeer(1, null)] } }));
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
  await h.receive(localFinal(start, { multiplayer: { finalWritten: false, finalAcknowledged: false, error: "cancelled",
    peers: [localPeer(1, null)] } }));
  assert.equal(h.get("play").disabled, false);
  await h.close();
});

test("explicit room launch freezes the real local roster and Window origin before gesture audio and opens only after its direct ACK", async () => {
  for (const local of [false, true]) {
    const opening = deferred();
    const h = await harness({ openGate: opening, touchSupported: local, ...(local ? { gamepads: [nativeGamepad()] } : {}) });
    await h.preview();
    assert.equal(h.get("multiplayer-mode").value, "peer");
    if (local) await discoverLocalPeers(h, 3);
    chooseRoom(h);
    h.get("multiplayer-role").value = "ignored-room-role";
    h.click("play");
    assert.equal(h.opens.length, 1); assert.equal(h.opens[0].gesture, true);
    const worker = h.workers[0];
    assert.equal(worker.messages("play-room-open").length, 0);
    for (const id of ["multiplayer", "multiplayer-mode", "multiplayer-url", "local-count"])
      assert.equal(h.get(id).disabled, true);
    h.get("multiplayer-mode").value = "peer"; h.get("multiplayer-mode").emit("change");
    h.get("multiplayer-url").value = "https://later.example/rooms/replaced";
    h.setTimeOrigin(12000);
    opening.resolve(h.audio); await flush();
    const start = worker.last("play-start");
    assert.equal(start.multiplayer, undefined, "room admission does not construct the bilateral owner");
    assert.equal(start.localPlanWords.length, local ? 12 : 4);
    assert.deepEqual(Array.from(start.localPlanWords).filter((_, index) => index % 4 === 0), local ? [1, 2, 3] : [1]);
    if (!local) assert.deepEqual(Array.from(start.localPlanWords), [1, 0, 0, 0]);
    const audioReady = await h.prepared(start, 1);
    assert.equal(h.audio.finishes, 1); assert.equal(h.audio.samples.length, 0); assert.equal(h.audio.sampleAttachments, 1);
    assert.equal(worker.messages("play-room-open").length, 0); assert.deepEqual(h.audio.arms, []);
    await h.reply(audioReady, null);
    const room = worker.last("play-room-open");
    assert.deepEqual({ url: room.url, windowOriginNs: room.windowOriginNs }, { url: ROOM_URL, windowOriginNs: 9000000000n });
    assert.equal(worker.messages("play-network-ready").length, 0);
    assert.equal(worker.messages("play-room-open").length, 1);
    assert.equal(h.audio.attachments, 1); assert.equal(worker.messages("play-audio").length, 1);
    assert.equal(worker.messages("play-activate").length, 0);
    h.click("stop"); await flush();
    await h.reply(room, { kind: "room-opened" });
    assert.deepEqual(h.audio.arms, []);
    await h.receive(localFinal(start));
    assert.equal(h.audio.stopStarts, 1); assert.equal(worker.messages("play-stop").length, 1);
    await h.close();
  }
  const replay = await harness(); await replay.preview();
  chooseRecording(replay, [selectedRecording().file]); chooseRoom(replay, "invalid room draft");
  const playing = await replay.launch(0, "replay");
  assert.equal(playing.start.multiplayer, undefined); assert.equal(playing.start.localPlanWords, undefined);
  assert.equal(replay.workers[0].messages("play-room-open").length, 0);
  assert.equal(replay.get("room-seal").disabled, true); assert.equal(replay.get("room-ready").disabled, true);
  replay.click("stop"); await flush(); await replay.receive(finalScore(playing.id)); await replay.close();
});

test("room lobby controls use the admitted creator and own frozen readiness while sharing one pending RPC slot", async () => {
  for (const creator of [true, false]) {
    const h = await harness(); await h.preview();
    const { start, worker, opening } = await openRoomLobby(h, { completeOpen: false });
    await roomEvent(h, start, roomRoster(start, { creator, count: creator ? 64 : 3 }));
    for (const id of ["room-seal", "room-ready", "room-leave"]) assert.equal(h.get(id).disabled, true);
    h.click("room-seal"); h.click("room-ready"); h.click("room-leave");
    assert.equal(worker.messages("play-room-seal").length, 0); assert.equal(worker.messages("play-room-ready").length, 0);
    assert.equal(worker.messages("play-room-leave").length, 0);
    await h.reply(opening, { kind: "room-opened" });
    assert.equal(h.get("room-seal").disabled, !creator);
    assert.equal(h.get("room-ready").disabled, true); assert.equal(h.get("room-leave").disabled, false);
    if (creator) {
      h.click("room-seal"); await flush();
      const seal = worker.last("play-room-seal"); assert.ok(seal);
      for (const id of ["room-seal", "room-ready", "room-leave"]) assert.equal(h.get(id).disabled, true);
      h.click("room-seal"); h.click("room-ready"); h.click("room-leave");
      assert.equal(worker.messages("play-room-seal").length, 1);
      assert.equal(worker.messages("play-room-ready").length, 0); assert.equal(worker.messages("play-room-leave").length, 0);
      await h.reply(seal, { kind: "room-requested", operation: "seal" });
      assert.equal(h.get("room-seal").disabled, true, "queue success cannot issue Seal twice against the same snapshot");
    }
    await roomEvent(h, start, roomRoster(start, { creator, phase: 1, count: creator ? 64 : 3 }));
    assert.equal(h.get("room-seal").disabled, true); assert.equal(h.get("room-ready").disabled, false);
    h.click("room-ready"); await flush();
    const ready = worker.last("play-room-ready"); assert.ok(ready);
    h.click("room-ready"); h.click("room-leave");
    assert.equal(worker.messages("play-room-ready").length, 1); assert.equal(worker.messages("play-room-leave").length, 0);
    await h.reply(ready, { kind: "room-requested", operation: "ready" });
    assert.deepEqual(h.audio.arms, []); assert.equal(worker.messages("play-activate").length, 0);
    assert.equal(h.get("room-ready").disabled, true);
    await roomEvent(h, start, roomRoster(start, { creator, phase: 1, ownReady: true, count: creator ? 64 : 3 }));
    assert.equal(h.get("room-ready").disabled, true); assert.equal(h.get("room-leave").disabled, false);
    await roomEvent(h, start, roomRoster(start, { creator, phase: 2, count: creator ? 64 : 3 }));
    assert.deepEqual(h.audio.arms, [], "all-ready metadata is still not a committed schedule");
    assert.equal(worker.messages("play-step").length, 0); assert.equal(worker.messages("play-render").length, 0);
    h.click("stop"); await flush(); await h.receive(localFinal(start)); await h.close();
  }
});

test("an early committed room event survives the open reply and activates one exact output grid without Window gameplay HUD writes", async () => {
  const h = await harness({ touchSupported: true }); await h.preview(); await discoverLocalPeers(h);
  const { start, worker, opening } = await openRoomLobby(h, { completeOpen: false, samples: 1 });
  await roomEvent(h, start, roomRoster(start, { phase: 2 }));
  const schedule = roomStart();
  await roomEvent(h, start, schedule);
  assert.deepEqual(h.audio.arms, [], "an unresolved room-open RPC still owns the shared setup slot");
  assert.equal(worker.messages("play-activate").length, 0);
  for (const id of ["room-seal", "room-ready", "room-leave"]) assert.equal(h.get(id).disabled, true);
  schedule.targetHostNs = 0n;
  await h.reply(opening, { kind: "room-opened" });
  assert.deepEqual(h.audio.arms, [72001n]);
  const activation = worker.last("play-activate");
  assert.equal(activation.targetHostNs, 1500000001n); assert.equal(activation.startFrame, 72001n);
  assert.equal(activation.hostNs, 1500020833n);
  await h.reply(activation, null);
  assert.equal(h.get("stop").disabled, false);
  const display = watchPlayDisplay(h), lobbyText = h.get("multiplayer-status").textContent;
  const layout = h.layoutReads;
  h.setNow(1700);
  h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1699.125 });
  const input = worker.last("play-step");
  assert.ok(input.events.some(event => event.key === 2 && event.hostNs === 1699125000n));
  await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: input.tickId,
    songNs: 200000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
  await h.advance(8);
  const render = worker.last("play-render"); assert.ok(render);
  await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: start.playId, renderId: render.renderId,
    songNs: 200000000n, hits: 1n, misses: 0n, combo: 1n, completed: false });
  assert.deepEqual(display, []); assert.equal(h.layoutReads, layout);
  assert.equal(h.get("multiplayer-status").textContent, lobbyText);
  assert.equal(worker.messages("play-activate").length, 1); assert.equal(h.audio.arms.length, 1);
  assert.equal(worker.messages("play-network-ready").length, 0); assert.equal(h.audio.polls, 0);
  assert.equal(worker.messages("play-commands").length, 0); assert.equal(worker.messages("play-ack").length, 0);
  h.click("stop"); await flush(); await h.receive(localFinal(start));
  assert.equal(h.audio.stopStarts, 1); await h.close();

  const pending = await harness(); await pending.preview();
  const queued = await openRoomLobby(pending);
  await roomEvent(pending, queued.start, roomRoster(queued.start, { phase: 1 }));
  pending.click("room-ready"); await flush();
  const ready = queued.worker.last("play-room-ready"); assert.ok(ready);
  await roomEvent(pending, queued.start, roomRoster(queued.start, { phase: 2 }));
  await roomEvent(pending, queued.start, roomStart());
  assert.deepEqual(pending.audio.arms, []);
  assert.equal(queued.worker.messages("play-activate").length, 0, "the Ready RPC still owns the single setup slot");
  await pending.reply(ready, { kind: "room-requested", operation: "ready" });
  assert.deepEqual(pending.audio.arms, [72001n]);
  await pending.reply(queued.worker.last("play-activate"), null);
  pending.click("stop"); await flush(); await pending.receive(localFinal(queued.start)); await pending.close();
});

test("room score controls page the Worker HUD through one correlated choice while live score receipts leave Window presentation untouched", async () => {
  const markup = await readFile(new URL("./index.html", import.meta.url), "utf8");
  for (const id of ["room-score-prev", "room-score-next"])
    assert.match(markup, new RegExp(`<button[^>]*id="${id}"[^>]*type="button"[^>]*hidden[^>]*disabled`));
  assert.match(markup, /id="room-score-page"[^>]*role="status"[^>]*hidden/);
  for (const count of [4, 64]) {
    const stopping = deferred(), h = await harness({ touchSupported: true, stopGate: stopping });
    await h.preview();
    if (count === 4) await discoverLocalPeers(h);
    const { start, worker, opening } = await openRoomLobby(h, { completeOpen: false });
    const roster = roomRoster(start, { phase: 2, count });
    if (count === 64) for (const member of roster.snapshot.members)
      if (member.participant !== ROOM_PARTICIPANT) member.players = Uint32Array.from({ length: 64 }, (_, index) => 0xffffffff - index);
    const pages = count === 64 ? 1008 : 2;
    await roomEvent(h, start, roster);
    await roomEvent(h, start, { kind: "score-pages", page: 0, pages });
    assert.equal(h.get("room-score-page").textContent, `Room scores 1 / ${pages}`);
    assert.equal(h.get("room-score-page").children.length, 0, "page count is bounded text, not thousands of DOM rows/options");
    assert.equal(h.get("room-score-prev").disabled, true); assert.equal(h.get("room-score-next").disabled, true);
    h.click("room-score-next"); assert.equal(worker.messages("play-room-page").length, 0);
    await h.reply(opening, { kind: "room-opened" });
    assert.equal(h.get("room-score-next").disabled, false);
    h.click("room-score-next"); await flush();
    const request = worker.last("play-room-page");
    assert.equal(request.page, 1); assert.equal(request.playId, start.playId);
    assert.equal(h.get("room-score-prev").disabled, true); assert.equal(h.get("room-score-next").disabled, true);
    h.click("room-score-prev"); h.click("room-score-next");
    assert.equal(worker.messages("play-room-page").length, 1);
    assert.equal(h.get("room-score-page").textContent, `Room scores 1 / ${pages}`, "queued request is not the correlated page receipt");
    await h.reply(request, { kind: "room-page", page: 1, pages });
    assert.equal(h.get("room-score-page").textContent, `Room scores 2 / ${pages}`);
    assert.equal(h.get("room-score-next").disabled, pages === 2);
    h.click("room-score-prev"); await flush();
    await h.reply(worker.last("play-room-page"), { kind: "room-page", page: 0, pages });
    await roomEvent(h, start, roomStart()); await h.reply(worker.last("play-activate"), null);
    const writes = watchPlayDisplay(h), pageWrites = [], field = h.get("room-score-page");
    let pageText = field.textContent;
    Object.defineProperty(field, "textContent", { configurable: true, get: () => pageText,
      set(value) { pageWrites.push(value); pageText = value; } });
    const layout = h.layoutReads, status = h.get("multiplayer-status").textContent;
    h.setNow(1700);
    for (let index = 0; index < 4; index++) {
      await h.advance(8);
      const tick = worker.last("play-step"), render = worker.last("play-render");
      await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: tick.tickId,
        songNs: BigInt(index), hits: 18446744073709551615n, misses: 0n, combo: 1n, preOriginInputs: 0 });
      await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: start.playId, renderId: render.renderId,
        songNs: BigInt(index), hits: 18446744073709551615n, misses: 0n, combo: 1n, completed: false });
    }
    assert.deepEqual(writes, []); assert.deepEqual(pageWrites, []);
    assert.equal(h.layoutReads, layout); assert.equal(h.get("multiplayer-status").textContent, status);
    await roomEvent(h, start, { kind: "closed", error: "retained disconnected scores" });
    assert.equal(worker.messages("play-stop").length, 0);
    h.click("room-score-next"); await flush();
    assert.equal(worker.last("play-room-page").page, 1, "retained disconnected pages remain inspectable");
    const pendingPage = worker.last("play-room-page");
    h.click("stop"); await flush();
    for (const id of ["room-score-prev", "room-score-next"]) {
      assert.equal(h.get(id).hidden, true); assert.equal(h.get(id).disabled, true);
    }
    const closingText = pageText, requests = worker.messages("play-room-page").length;
    await h.reply(pendingPage, { kind: "room-page", page: 1, pages });
    assert.equal(pageText, closingText);
    h.click("room-score-next"); assert.equal(worker.messages("play-room-page").length, requests);
    await h.receive(localFinal(start)); stopping.resolve(); await flush();
    await roomEvent(h, start, { kind: "score-pages", page: 0, pages });
    assert.equal(h.get("room-score-page").hidden, true); assert.equal(h.audio.stopStarts, 1);
    await h.close();
  }

  for (const fault of ["pages", "notice", "response", "refusal"]) {
    const h = await harness(); await h.preview();
    const { start, worker } = await openRoomLobby(h);
    await roomEvent(h, start, roomRoster(start, { phase: 2, count: 4 }));
    await roomEvent(h, start, { kind: "score-pages", page: 0, pages: 2 });
    await roomEvent(h, start, roomStart()); await h.reply(worker.last("play-activate"), null);
    if (fault === "pages") await roomEvent(h, start, { kind: "score-pages", page: 0, pages: 1009 });
    else if (fault === "notice") await roomEvent(h, start, { kind: "display-unavailable", error: null });
    else {
      h.click("room-score-next"); await flush(); const request = worker.last("play-room-page");
      if (fault === "response") await h.reply(request, { kind: "room-page", page: 0, pages: 2 });
      else await h.receive({ kind: "play-reply", playId: start.playId, rpcId: request.rpcId, error: "display page refused" });
    }
    assert.match(h.get("room-score-page").textContent, /unavailable/i);
    assert.match(h.get("multiplayer-status").textContent, /Local play continues/);
    assert.equal(h.get("room-score-prev").disabled, true); assert.equal(h.get("room-score-next").disabled, true);
    assert.equal(worker.messages("play-stop").length, 0); assert.equal(h.audio.stopStarts, 0);
    const choices = worker.messages("play-room-page").length;
    await roomEvent(h, start, { kind: "display-unavailable", error: "already disabled display" });
    h.click("room-score-next"); assert.equal(worker.messages("play-room-page").length, choices);
    h.click("stop"); await flush(); await h.receive(localFinal(start)); await h.close();
  }
});

test("a valid playing room closure preserves Window input and output while final status distinguishes queued, written and acknowledged prefixes", async () => {
  for (const [written, acknowledged, expected] of [
    [false, false, /queued · full write unconfirmed/],
    [true, false, /written · aggregate ACK unavailable/],
    [true, true, /written and acknowledged by the room relay/],
  ]) {
    const h = await harness(); await h.preview(); h.get("record").checked = true;
    const { start, worker } = await openRoomLobby(h);
    await roomEvent(h, start, roomRoster(start, { phase: 2 }));
    await roomEvent(h, start, roomStart());
    await h.reply(worker.last("play-activate"), null);
    assert.match(h.get("multiplayer-status").textContent, /reported scores appear below the playfields/);
    const display = watchPlayDisplay(h), layout = h.layoutReads;
    await roomEvent(h, start, { kind: "closed", error: "room stream lost after activation" });
    const disconnected = h.get("multiplayer-status").textContent;
    assert.match(disconnected, /room stream lost after activation.*local play continues/);
    assert.equal(worker.messages("play-stop").length, 0); assert.equal(h.audio.stopStarts, 0);
    assert.equal(h.get("stop").disabled, false); assert.equal(h.get("record").disabled, true);
    for (const id of ["room-seal", "room-ready", "room-leave"]) assert.equal(h.get(id).disabled, true);
    await roomEvent(h, start, { kind: "closed", error: "duplicate late closure" });
    assert.equal(h.get("multiplayer-status").textContent, disconnected);
    h.setNow(1700);
    h.window.emit("keydown", { code: "KeyZ", repeat: false, timeStamp: 1699.125 });
    const input = worker.last("play-step");
    assert.ok(input.events.some(event => event.key === 2 && event.hostNs === 1699125000n));
    await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: input.tickId,
      songNs: 200000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
    await h.advance(8);
    const render = worker.last("play-render"); assert.ok(render);
    assert.equal(Object.hasOwn(render, "report"), false);
    await h.receive({ kind: "play-render-done", commandsPending: false, observedTick: h.workers[0].last("play-step")?.tickId ?? 0, pendingInputs: 0, playId: start.playId, renderId: render.renderId,
      songNs: 200000000n, hits: 1n, misses: 0n, combo: 1n, completed: false });
    assert.deepEqual(display, []); assert.equal(h.layoutReads, layout);
    assert.equal(h.get("multiplayer-status").textContent, disconnected);
    assert.equal(h.audio.polls, 0); assert.equal(worker.messages("play-stop").length, 0);
    h.click("stop"); await flush();
    const player = start.localPlanWords[0];
    await h.receive(localFinal(start, {
      room: { participant: ROOM_PARTICIPANT, finalQueued: true, finalWritten: written,
        finalAcknowledged: acknowledged, localComplete: acknowledged, finalDrain: "cancelled",
        error: "room stream lost after activation", peers: [] },
      replays: [{ player, replay: Uint8Array.of(66, 75, 82, 1), replayError: null, replayComplete: false }],
    }));
    assert.equal(h.audio.stopStarts, 1); assert.equal(worker.messages("play-stop").length, 1);
    assert.match(h.get("multiplayer-status").textContent, expected);
    assert.match(h.get("multiplayer-status").textContent, /Coordinated room drain cancelled/);
    assert.match(content(h.get("local-results")), /Recorded prefix/);
    assert.equal(h.get("captured-replay").children.some(option => option.value === String(player)), true);
    await h.close();
  }
  for (const error of ["", 7]) {
    const h = await harness(); await h.preview(); const { start, worker } = await openRoomLobby(h);
    await roomEvent(h, start, roomRoster(start, { phase: 2 }));
    await roomEvent(h, start, roomStart()); await h.reply(worker.last("play-activate"), null);
    await roomEvent(h, start, { kind: "closed", error });
    assert.equal(worker.messages("play-stop").length, 1, "malformed initial closure metadata remains fatal during play");
    assert.equal(h.audio.stopStarts, 1);
    await h.receive(localFinal(start)); await h.close();
  }
});

test("joined room drain outcomes remain distinct while the natural cleanup wait preserves local recordings and Window ownership", async () => {
  for (const [finalDrain, expected] of [
    ["complete", /Coordinated room drain completed/],
    ["failed", /Coordinated room drain failed; the local result is retained/],
    ["cancelled", /Coordinated room drain cancelled/],
  ]) {
    const stopping = deferred(), h = await harness({ stopGate: stopping });
    await h.preview(); h.get("record").checked = true;
    const { start, worker } = await openRoomLobby(h);
    await roomEvent(h, start, roomRoster(start, { phase: 2 }));
    await roomEvent(h, start, roomStart()); await h.reply(worker.last("play-activate"), null);
    const field = h.get("multiplayer-status"), writes = [];
    let text = field.textContent;
    Object.defineProperty(field, "textContent", { configurable: true, get: () => text,
      set(value) { writes.push(value); text = value; } });
    const display = watchPlayDisplay(h), layout = h.layoutReads;
    h.setNow(1700); await h.advance(8);
    const tick = worker.last("play-step"), render = worker.last("play-render");
    await h.receive({ kind: "play-render-done", pendingInputs: 0, playId: start.playId, renderId: render.renderId,
      completed: true, commandsPending: false, observedTick: tick.tickId });
    assert.equal(worker.messages("play-stop").length, 0);
    assert.deepEqual(display, []); assert.equal(h.layoutReads, layout); assert.deepEqual(writes, []);
    await h.receive({ kind: "play-step-done", commandsPending: false, pendingInputs: 0, playId: start.playId, tickId: tick.tickId,
      songNs: 2350000000n, hits: 1n, misses: 0n, combo: 1n, preOriginInputs: 0 });
    assert.equal(worker.last("play-stop").completed, true); assert.equal(h.audio.stopStarts, 1);
    assert.deepEqual(display.filter(write => write.id !== "status"), []);
    assert.equal(h.layoutReads, layout); assert.deepEqual(writes, []);
    const inputCount = worker.messages("play-step").length, renderCount = worker.messages("play-render").length;
    await h.advance(10001);
    assert.equal(worker.terminations, 0, "the host watchdog leaves room for the independent ten-second drain deadline");
    assert.equal(h.get("play").disabled, true); assert.equal(worker.messages("play-stop").length, 1);
    assert.equal(worker.messages("play-step").length, inputCount);
    assert.equal(worker.messages("play-render").length, renderCount);
    const player = start.localPlanWords[0];
    const result = localFinal(start, {
      room: { participant: ROOM_PARTICIPANT, finalQueued: true, finalWritten: true,
        finalAcknowledged: true, localComplete: true, finalDrain,
        error: finalDrain === "failed" ? "actual coordinated drain timeout" : null, peers: [] },
      replays: [{ player, replay: Uint8Array.of(66, 75, 82, 1), replayError: null, replayComplete: true }],
    });
    await h.receive(result);
    assert.deepEqual(writes, [], "the audio owner has not finished cleanup");
    assert.equal(h.get("captured-replay").disabled, true);
    stopping.resolve(); await flush();
    assert.equal(writes.length, 1); assert.match(text, expected);
    assert.match(text, /written and acknowledged by the room relay/);
    assert.match(content(h.get("local-results")), /Complete/);
    h.get("captured-replay").value = String(player); h.get("captured-replay").emit("change");
    assert.equal(h.get("export").disabled, false); assert.match(h.get("export").textContent, /complete/i);
    assert.equal(h.get("play").disabled, false);
    const once = writes.length;
    await h.receive(result);
    await roomEvent(h, start, { kind: "closed", error: "late prior drain closure" });
    assert.equal(writes.length, once); assert.equal(worker.messages("play-room-leave").length, 0);
    assert.equal(h.audio.polls, 0); assert.equal(worker.messages("play-commands").length, 0);
    await h.close();
  }
});

async function stoppedRoomResults(h, { count = 4, metadata = { page: 1, pages: 2, failed: false }, beforeTerminal = null } = {}) {
  await h.preview(); h.get("record").checked = true;
  const { start, worker } = await openRoomLobby(h);
  const roster = roomRoster(start, { phase: 2, count });
  if (count === 64) for (const member of roster.snapshot.members) if (member.participant !== ROOM_PARTICIPANT)
    member.players = Uint32Array.from({ length: 64 }, (_, index) => 0xffffffff - index);
  await roomEvent(h, start, roster);
  await roomEvent(h, start, { kind: "score-pages", page: 0, pages: count === 64 ? 1008 : 2 });
  await roomEvent(h, start, roomStart()); await h.reply(worker.last("play-activate"), null);
  h.click("stop"); await flush();
  if (beforeTerminal) await beforeTerminal(start, worker);
  const player = start.localPlanWords[0];
  const terminal = localFinal(start, {
    roomResults: metadata,
    room: { participant: ROOM_PARTICIPANT, finalQueued: true, finalWritten: false,
      finalAcknowledged: false, localComplete: false, finalDrain: "cancelled", error: null, peers: [] },
    replays: [{ player, replay: Uint8Array.of(66, 75, 82, 1), replayError: null, replayComplete: false }],
  });
  await h.receive(terminal);
  return { start, worker, terminal, player };
}

test("joined room Results use bounded metadata and acknowledged Worker pages without recreating lobby or Window score work", async () => {
  for (const count of [4, 64]) {
    const stopping = deferred(), h = await harness({ stopGate: stopping });
    const pages = count === 64 ? 1008 : 2;
    const { start, worker, terminal, player } = await stoppedRoomResults(h, {
      count, metadata: { page: pages - 1, pages, failed: false },
    });
    assert.equal(h.get("room-score-page").hidden, true, "Worker final data cannot bypass joined audio cleanup");
    assert.equal(h.get("captured-replay").disabled, true);
    stopping.resolve(); await flush();
    assert.equal(h.get("room-score-page").textContent, `Room results ${pages} / ${pages}`);
    assert.equal(h.get("room-score-page").children.length, 0);
    assert.equal(h.get("room-score-prev").disabled, false); assert.equal(h.get("room-score-next").disabled, true);
    for (const id of ["room-seal", "room-ready", "room-leave"]) {
      assert.equal(h.get(id).hidden, true); assert.equal(h.get(id).disabled, true);
    }
    const status = h.get("multiplayer-status").textContent, local = content(h.get("local-results"));
    assert.match(status, /full write unconfirmed.*Coordinated room drain cancelled/);
    h.get("captured-replay").value = String(player); h.get("captured-replay").emit("change");
    assert.equal(h.get("export").disabled, false);
    const display = watchPlayDisplay(h), layout = h.layoutReads;
    const inputs = worker.messages("play-step").length, renders = worker.messages("play-render").length;
    h.click("room-score-prev"); await flush();
    const request = worker.last("play-room-page");
    assert.equal(request.playId, start.playId); assert.equal(request.page, pages - 2);
    assert.ok(request.rpcId > worker.last("play-activate").rpcId);
    assert.equal(h.get("room-score-page").textContent, `Room results ${pages} / ${pages}`);
    assert.equal(h.get("room-score-prev").disabled, true); assert.equal(h.get("room-score-next").disabled, true);
    const choices = worker.messages("play-room-page").length;
    h.click("room-score-prev"); h.click("room-score-next");
    assert.equal(worker.messages("play-room-page").length, choices);
    await h.receive({ kind: "play-reply", playId: start.playId, rpcId: request.rpcId - 1,
      result: { kind: "room-page", page: pages - 2, pages } });
    assert.equal(h.get("room-score-page").textContent, `Room results ${pages} / ${pages}`);
    await h.reply(request, { kind: "room-page", page: pages - 2, pages });
    assert.equal(h.get("room-score-page").textContent, `Room results ${pages - 1} / ${pages}`);
    await h.advance(100);
    await roomEvent(h, start, { kind: "score-pages", page: 0, pages });
    await h.receive(terminal);
    assert.equal(h.get("room-score-page").textContent, `Room results ${pages - 1} / ${pages}`);
    assert.equal(h.get("multiplayer-status").textContent, status); assert.equal(content(h.get("local-results")), local);
    assert.equal(worker.messages("play-step").length, inputs); assert.equal(worker.messages("play-render").length, renders);
    assert.equal(h.layoutReads, layout); assert.deepEqual(display, []);
    assert.equal(worker.messages("play-room-leave").length, 0); assert.equal(h.audio.stopStarts, 1);
    h.click("room-score-next"); await flush();
    const pending = worker.last("play-room-page");
    const next = await h.begin();
    assert.notEqual(next.playId, start.playId);
    assert.equal(h.get("room-score-page").hidden, true);
    const freshStatus = h.get("multiplayer-status").textContent;
    await h.reply(pending, { kind: "room-page", page: pages - 1, pages });
    await h.receive({ kind: "play-room-results", playId: start.playId, page: 0, pages, failed: true, error: "obsolete renderer" });
    assert.equal(h.get("multiplayer-status").textContent, freshStatus);
    assert.equal(h.get("room-score-page").hidden, true);
    h.click("stop"); await flush(); await h.receive(localFinal(next)); await h.close();
  }
});

test("room Results metadata, page and renderer failures affect only retained presentation and never invalidate a local recording", async () => {
  for (const fault of ["metadata", "failed", "closing-notice", "notice", "reply", "refusal", "timeout", "seek"]) {
    const h = await harness();
    const metadata = fault === "metadata" ? { page: 1, pages: 1009, failed: false }
      : { page: 1, pages: 2, failed: fault === "failed" };
    const { start, worker, player } = await stoppedRoomResults(h, {
      metadata,
      beforeTerminal: fault === "closing-notice" ? async start => h.receive({
        kind: "play-room-results", playId: start.playId, page: 1, pages: 2, failed: true, error: "late joined draw failed",
      }) : null,
    });
    const local = content(h.get("local-results"));
    h.get("captured-replay").value = String(player); h.get("captured-replay").emit("change");
    assert.equal(h.get("export").disabled, false);
    if (["notice", "reply", "refusal", "timeout", "seek"].includes(fault)) {
      h.click("room-score-prev"); await flush(); const request = worker.last("play-room-page");
      assert.ok(request);
      if (fault === "notice") await h.receive({ kind: "play-room-results", playId: start.playId,
        page: 1, pages: 2, failed: true, error: "renderer surface lost" });
      if (fault === "reply") await h.reply(request, { kind: "room-page", page: 1, pages: 2 });
      if (fault === "refusal") await h.receive({ kind: "play-reply", playId: start.playId,
        rpcId: request.rpcId, error: "archive projection refused" });
      if (fault === "timeout") await h.advance(10001);
      if (fault === "seek") {
        h.get("position").value = "0"; h.get("seek-form").emit("submit"); await flush();
        await h.reply(request, { kind: "room-page", page: 0, pages: 2 });
        assert.equal(h.get("room-score-page").hidden, true);
      }
    }
    if (fault !== "seek") {
      assert.match(h.get("room-score-page").textContent, /unavailable/i);
      assert.equal(h.get("room-score-prev").disabled, true); assert.equal(h.get("room-score-next").disabled, true);
    }
    assert.equal(content(h.get("local-results")), local);
    assert.equal(h.get("export").disabled, false); assert.equal(h.get("play").disabled, false);
    assert.equal(worker.messages("play-stop").length, 1); assert.equal(h.audio.stopStarts, 1);
    assert.equal(worker.terminations, 0); assert.equal(worker.messages("play-room-leave").length, 0);
    assert.match(h.get("multiplayer-status").textContent, /Coordinated room drain cancelled/);
    h.click("export"); await flush();
    assert.equal(h.downloads.length, 1); assert.match(h.downloads[0].filename, /prefix\.bkr$/);
    await h.close();
  }
});

test("room URL, bounded roster and committed schedule refusals preserve cleanup and never reinterpret queue success as start", async () => {
  for (const url of ["http://example.test/rooms/a", "https://example.test/competition",
    "https://example.test/rooms/a?extra=1", "https://example.test/rooms/a#fragment",
    "https://example.test/rooms/" + "a".repeat(1025)]) {
    const h = await harness(); await h.preview(); chooseRoom(h, url);
    h.click("play"); await flush();
    assert.equal(h.opens.length, 0); assert.equal(h.workers[0].messages("play-start").length, 0);
    assert.equal(h.get("play").disabled, false); await h.close();
  }
  const malformed = [
    event => { event.participant = 0n; },
    event => { event.snapshot.members[1].participant = event.snapshot.members[0].participant; },
    event => { event.snapshot.members[1].players = Uint32Array.of(1, 1); },
    event => { event.snapshot.members[0].players = Uint32Array.of(99); },
    event => { event.snapshot.phase = 2; event.snapshot.deadlineNs = null; },
    event => { event.snapshot.deadlineNs = -1n; },
  ];
  for (const mutate of malformed) {
    const h = await harness(); await h.preview(); const { start, worker } = await openRoomLobby(h);
    const event = roomRoster(start); mutate(event);
    await roomEvent(h, start, event);
    assert.equal(worker.messages("play-stop").length, 1); assert.deepEqual(h.audio.arms, []);
    assert.equal(worker.messages("play-activate").length, 0);
    await h.receive(localFinal(start)); assert.equal(h.audio.stopStarts, 1); await h.close();
  }
  for (const schedule of [roomStart({ targetHostNs: 1500000000 }),
    roomStart({ songTargetHostNs: 1500000001n }), roomStart({ uncertaintyNs: 100000001n }),
    roomStart({ targetHostNs: 1000000000n, songTargetHostNs: 1100000000n })]) {
    const h = await harness(); await h.preview(); const { start, worker } = await openRoomLobby(h);
    await roomEvent(h, start, roomRoster(start, { phase: 2 }));
    await roomEvent(h, start, schedule);
    assert.deepEqual(h.audio.arms, []); assert.equal(worker.messages("play-activate").length, 0);
    assert.equal(worker.messages("play-stop").length, 1);
    await h.receive(localFinal(start)); await h.close();
  }
  const refused = await harness(); await refused.preview();
  const lobby = await openRoomLobby(refused);
  await roomEvent(refused, lobby.start, roomRoster(lobby.start));
  refused.click("room-seal"); await flush();
  const seal = lobby.worker.last("play-room-seal");
  await refused.receive({ kind: "play-reply", playId: lobby.start.playId, rpcId: seal.rpcId, error: "common state refused" });
  assert.equal(lobby.worker.messages("play-stop").length, 0); assert.equal(refused.get("room-seal").disabled, false);
  refused.click("room-seal"); await flush();
  await refused.reply(lobby.worker.last("play-room-seal"), { kind: "room-requested", operation: "ready" });
  assert.equal(lobby.worker.messages("play-stop").length, 1);
  await refused.receive(localFinal(lobby.start)); await refused.close();

  const duplicate = await harness(); await duplicate.preview();
  const first = await openRoomLobby(duplicate);
  await roomEvent(duplicate, first.start, roomRoster(first.start, { phase: 2 }));
  await roomEvent(duplicate, first.start, roomStart());
  await duplicate.reply(first.worker.last("play-activate"), null);
  await roomEvent(duplicate, first.start, roomStart());
  assert.equal(first.worker.messages("play-activate").length, 1); assert.equal(duplicate.audio.arms.length, 1);
  assert.equal(first.worker.messages("play-stop").length, 1);
  await duplicate.receive(localFinal(first.start)); await duplicate.close();
});

test("room Leave, closure, deadline and cancellation settle the schedule wait and join one current owner", async () => {
  for (const rejected of [false, true]) {
    const stopped = deferred(), h = await harness({ stopGate: stopped }); await h.preview();
    const { start, worker } = await openRoomLobby(h);
    await roomEvent(h, start, roomRoster(start));
    h.click("room-leave"); await flush();
    const leaving = worker.last("play-room-leave"); assert.ok(leaving);
    for (const id of ["room-seal", "room-ready", "room-leave"]) assert.equal(h.get(id).disabled, true);
    await roomEvent(h, start, { kind: "closed", error: "Room leave was written." });
    assert.equal(worker.messages("play-stop").length, 0, "intentional closure is owned by the pending Leave RPC");
    if (rejected) await h.receive({ kind: "play-reply", playId: start.playId, rpcId: leaving.rpcId, error: "leave write refused" });
    else await h.reply(leaving, { kind: "room-left", leaveWritten: true });
    assert.equal(worker.messages("play-stop").length, 1); assert.equal(h.audio.stopStarts, 1);
    await h.receive(localFinal(start));
    assert.equal(h.get("play").disabled, true);
    stopped.resolve(); await flush();
    assert.equal(h.get("play").disabled, false); assert.deepEqual(h.audio.arms, []);
    await h.close();
  }
  const h = await harness(); await h.preview(); const prior = await openRoomLobby(h);
  await roomEvent(h, prior.start, roomRoster(prior.start));
  await roomEvent(h, prior.start, { kind: "closed", error: "actual room stream failed" });
  assert.equal(prior.worker.messages("play-stop").length, 1);
  await h.receive(localFinal(prior.start));
  const next = await openRoomLobby(h), before = h.get("multiplayer-status").textContent;
  for (const event of [roomStart(), roomRoster(prior.start), { kind: "closed", error: "late old stream" }])
    await roomEvent(h, prior.start, event);
  assert.equal(h.get("multiplayer-status").textContent, before); assert.deepEqual(h.audio.arms, []);
  const escape = h.window.emit("keydown", { code: "Escape", repeat: false, timeStamp: 1000 });
  assert.equal(escape.defaultPrevented, true); await flush();
  await roomEvent(h, next.start, roomStart());
  assert.deepEqual(h.audio.arms, []);
  await h.receive(localFinal(next.start));
  assert.equal(h.audio.stopStarts, 1); await h.close();

  const deadline = await harness(); await deadline.preview(); const waiting = await openRoomLobby(deadline);
  await roomEvent(deadline, waiting.start, roomRoster(waiting.start, { phase: 2 }));
  await deadline.advance(60000);
  assert.equal(waiting.worker.messages("play-stop").length, 1); assert.deepEqual(deadline.audio.arms, []);
  await deadline.receive(localFinal(waiting.start)); await deadline.close();
});

async function selectedHistoricalGrades(h, pages = 3) {
  await h.preview(); h.click("records-refresh"); await flush();
  h.click("records-use"); await flush();
  const worker = h.workers.at(-1), request = worker.last("historical-record-present");
  assert.ok(request, "actual completed-record load requests Worker historical presentation");
  await h.receive({ kind: "historical-record-result", id: request.id, available: true, error: null, gradePage: 0, gradePages: pages });
  return { worker, request };
}
function storedHistoricalFields(id = 41) {
  const row = savedRecord({ id });
  return { recordsList: [row, savedRecord({ id: 42, name: "replacement.bkr" })],
    recordsLoaded: { metadata: row, bytes: Uint8Array.from([66,75,82,0]), completedArchive: Uint8Array.from([66,75,82,69,83,85,76,84]), archivePlayer: 4294967295 } };
}

async function completedRecordHost(h) {
  await h.preview(); const session = await h.launch(), worker = h.workers[0];
  h.setNow(1700); await h.advance(8);
  const tick = worker.last("play-step"), render = worker.last("play-render");
  await h.receive({ kind: "play-render-done", playId: session.id, renderId: render.renderId,
    completed: true, commandsPending: false, observedTick: tick.tickId, pendingInputs: 0 });
  await h.receive({ kind: "play-step-done", playId: session.id, tickId: tick.tickId,
    commandsPending: false, pendingInputs: 0, songNs: 2350000000n, hits: 3n, misses: 1n, combo: 2n, preOriginInputs: 0 });
  assert.equal(worker.last("play-stop").completed, true);
  const metadata = { proof: true, players: [1], page: 0, pages: 2,
    comparisons: false, hasComparisons: false, failed: false };
  await h.receive(finalScore(session.id, { completedResults: metadata }));
  const present = worker.last("play-results-present"); assert.ok(present);
  await h.reply(present, { kind: "completed-results", completedResults: metadata });
  return { worker, metadata, session };
}

test("record continuity: matching historical commit retires pending Results RPC and late ACK cannot revive it", async () => {
  const h = await harness(storedHistoricalFields()); const { worker, metadata } = await completedRecordHost(h);
  h.window.emit("keydown", { code: "PageDown", repeat: false }); await flush();
  const oldPage = worker.last("play-results-page"); assert.ok(oldPage);
  h.click("records-refresh"); await flush(); h.click("records-use"); await flush();
  const request = worker.last("historical-record-present"); assert.ok(request);
  const resultsPages = worker.messages("play-results-page").length;
  const accepts = worker.messages("historical-record-accept").length;
  await h.receive({ kind: "historical-record-result", id: request.id - 1, available: true,
    error: null, gradePage: 0, gradePages: 3 });
  assert.equal(h.get("historical-grade-page").textContent, "");
  assert.equal(worker.messages("historical-record-accept").length, accepts);
  const originalPost = worker.postMessage.bind(worker), captionsAtAccept = [];
  worker.postMessage = (message, transfer) => {
    if (message.kind === "historical-record-accept") captionsAtAccept.push(h.get("historical-grade-page").textContent);
    return originalPost(message, transfer);
  };
  await h.receive({ kind: "historical-record-result", id: request.id, available: true,
    error: null, gradePage: 0, gradePages: 3 });
  const current = statusSnapshot(h);
  assert.equal(worker.last("historical-record-accept").id, request.id);
  assert.deepEqual(captionsAtAccept, [""], "accept reaches Worker before new historical controls replace prior Results");
  await h.reply(oldPage, { kind: "completed-results", completedResults: { ...metadata, page: 1 } });
  await h.receive({ kind: "play-completed-results", playId: oldPage.playId,
    completedResults: metadata, error: "obsolete Results error" });
  h.window.emit("keydown", { code: "PageDown", repeat: false }); await flush();
  assert.equal(worker.messages("play-results-page").length, resultsPages);
  assert.deepEqual(statusSnapshot(h), current);
  assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 1 / 3");
  assert.equal(h.get("historical-grade-next").disabled, false);
  await h.close();
});

test("record continuity: failed historical response and timeout retain Results navigation", async () => {
  for (const failure of ["refused", "metadata", "timeout", "read"]) {
    const h = await harness(storedHistoricalFields()); const { worker } = await completedRecordHost(h);
    h.click("records-refresh"); await flush();
    if (failure === "read") h.faults.recordsLoadError = new Error("storage read refused");
    h.click("records-use"); await flush(); const request = worker.last("historical-record-present");
    if (failure === "timeout") await h.advance(10000);
    else if (failure === "refused") await h.receive({ kind: "historical-record-result", id: request.id,
      available: false, error: "archive association refused", gradePage: null, gradePages: 0 });
    else if (failure === "metadata") await h.receive({ kind: "historical-record-result", id: request.id,
      available: true, error: null, gradePage: 0, gradePages: 0 });
    h.window.emit("keydown", { code: "PageDown", repeat: false }); await flush();
    const page = worker.last("play-results-page"); assert.ok(page, `${failure} preserves original Results keyboard controls`);
    assert.equal(page.page, 1); assert.equal(h.get("historical-grade-page").textContent, "");
    if (failure === "timeout") {
      const before = worker.messages("historical-record-accept").length, previous = statusSnapshot(h);
      await h.receive({ kind: "historical-record-result", id: request.id,
        available: true, error: null, gradePage: 0, gradePages: 3 });
      assert.equal(worker.messages("historical-record-accept").length, before);
      assert.deepEqual(statusSnapshot(h), previous);
    }
    await h.close();
  }
});

test("record continuity: failed replacement and timeout retain prior historical controls and canvas", async () => {
  for (const failure of ["refused", "timeout", "read"]) {
    const h = await harness(storedHistoricalFields()); const { worker, request: prior } = await selectedHistoricalGrades(h);
    const clears = worker.messages("historical-record-clear").length;
    if (failure === "read") h.faults.recordsLoadError = new Error("storage read refused");
    h.click("records-use"); await flush();
    assert.equal(worker.messages("historical-record-clear").length, clears, "candidate acquisition keeps accepted display");
    assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 1 / 3");
    assert.equal(h.get("canvas").hidden, false);
    const candidate = worker.last("historical-record-present");
    if (failure === "timeout") await h.advance(10000);
    if (failure === "refused") await h.receive({ kind: "historical-record-result", id: candidate.id,
      available: false, error: "candidate refused", gradePage: null, gradePages: 0 });
    assert.equal(h.get("historical-grade-next").disabled, false);
    assert.equal(h.get("canvas").hidden, false);
    h.click("historical-grade-next"); await flush();
    assert.equal(worker.last("historical-record-page").id, prior.id);
    if (failure === "timeout") {
      const clear = worker.last("historical-record-clear"); assert.equal(clear.cancelId, candidate.id);
      const status = statusSnapshot(h);
      const accepts = worker.messages("historical-record-accept").length;
      await h.receive({ kind: "historical-record-result", id: candidate.id, available: true,
        error: null, gradePage: 0, gradePages: 2 });
      assert.deepEqual(statusSnapshot(h), status);
      assert.equal(worker.messages("historical-record-accept").length, accepts);
      assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 1 / 3");
    }
    await h.close();
  }
});
test("historical page controls survive completed library operation and only matching ACK changes confirmed caption", async () => {
  const h = await harness(storedHistoricalFields()); const { worker, request } = await selectedHistoricalGrades(h);
  assert.equal(h.get("historical-grade-next").hidden, false); assert.equal(h.get("historical-grade-next").disabled, false);
  assert.equal(h.get("historical-grade-prev").disabled, true); assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 1 / 3");
  h.click("historical-grade-next"); await flush(); const page = worker.last("historical-record-page");
  assert.equal(page.id, request.id); assert.equal(page.page, 1);
  assert.equal(h.get("historical-grade-next").disabled, true); assert.equal(h.get("historical-grade-prev").disabled, true);
  h.click("historical-grade-next"); await flush(); assert.equal(worker.messages("historical-record-page").length, 1);
  for (const stale of [{ id: page.id + 1, rpcId: page.rpcId }, { id: page.id, rpcId: page.rpcId - 1 }]) {
    await h.receive({ kind: "historical-record-page-result", ...stale, gradePage: 1, gradePages: 3, error: null });
    assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 1 / 3"); assert.equal(h.get("historical-grade-next").disabled, true);
  }
  await h.receive({ kind: "historical-record-page-result", id: page.id, rpcId: page.rpcId, gradePage: 1, gradePages: 3, error: null });
  assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 2 / 3"); assert.equal(h.get("historical-grade-prev").disabled, false);
  h.click("historical-grade-next"); await flush(); const refused = worker.last("historical-record-page");
  await h.receive({ kind: "historical-record-page-result", id: refused.id, rpcId: refused.rpcId, gradePage: null, gradePages: 0, error: "cold page allocation refused" });
  assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 2 / 3"); assert.equal(h.get("historical-grade-next").disabled, false);
  assert.equal(h.opens.length, 0); await h.close();
});
test("page timeout post refusal and malformed matching receipt clear uncertain display but preserve selected replay", async () => {
  for (const failure of ["timeout", "post", "malformed"]) {
    const h = await harness(storedHistoricalFields()); const { worker } = await selectedHistoricalGrades(h);
    const replayName = h.get("replay-name").textContent;
    if (failure === "post") worker.failKind = "historical-record-page";
    h.click("historical-grade-next"); await flush();
    if (failure === "timeout") await h.advance(10000);
    if (failure === "malformed") {
      const request = worker.last("historical-record-page");
      await h.receive({ kind: "historical-record-page-result", id: request.id, rpcId: request.rpcId, gradePage: 2, gradePages: 3, error: null });
    }
    assert.equal(h.get("historical-grade-next").hidden, true); assert.equal(h.get("historical-grade-page").textContent, "");
    assert.ok(worker.last("historical-record-clear")); assert.equal(h.get("replay-name").textContent, replayName);
    if (failure === "post") {
      assert.match(h.get("status").textContent, /unavailable/i);
      assert.match(h.get("status").textContent, /replay.*remain/i);
    }
    assert.equal(h.get("replay-play").disabled, false); assert.equal(h.opens.length, 0); await h.close();
  }
});
test("replacement record protects its page controls from old RPC ACK and already queued old deadline", async () => {
  const h = await harness(storedHistoricalFields()); const { worker } = await selectedHistoricalGrades(h);
  const priorTimers = new Set(h.timers.keys());
  h.click("historical-grade-next"); await flush(); const old = worker.last("historical-record-page");
  const queued = [...h.timers.entries()].find(([id]) => !priorTimers.has(id))?.[1];
  assert.ok(queued);
  h.get("records").value = "42"; h.get("records").emit("change"); await flush();
  h.faults.recordsLoaded = storedHistoricalFields(42).recordsLoaded;
  h.click("records-use"); await flush(); const replacement = worker.last("historical-record-present");
  assert.notEqual(replacement.id, old.id);
  await h.receive({ kind: "historical-record-result", id: replacement.id, available: true, error: null, gradePage: 0, gradePages: 2 });
  const clears = worker.messages("historical-record-clear").length;
  queued.callback(); await flush();
  await h.receive({ kind: "historical-record-page-result", id: old.id, rpcId: old.rpcId, gradePage: 1, gradePages: 3, error: null });
  assert.equal(worker.messages("historical-record-clear").length, clears);
  assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 1 / 2"); assert.equal(h.get("historical-grade-next").disabled, false);
  await h.close();
});
test("saved-record selection change aborts unabortable library read before stale historical or replay replacement", async () => {
  const gate = deferred(); const h = await harness({ ...storedHistoricalFields(), recordsLoadGate: gate }); await h.preview();
  const original = selectedRecording(); chooseRecording(h, [original.file]);
  const name = h.get("replay-name").textContent;
  h.click("records-refresh"); await flush(); h.click("records-use"); await flush();
  assert.equal(h.recordCalls.filter(call => call.method === "load").length, 1);
  h.get("records").value = "42"; h.get("records").emit("change"); await flush();
  gate.resolve(); await flush();
  assert.equal(h.workers.at(-1).messages("historical-record-present").length, 0);
  assert.equal(h.get("replay-name").textContent, name); assert.equal(original.reads, 0);
  assert.equal(h.get("historical-grade-next").hidden, true); assert.equal(h.opens.length, 0); await h.close();
  const disposed = await harness(storedHistoricalFields()); const selected = await selectedHistoricalGrades(disposed);
  disposed.click("historical-grade-next"); await flush(); const pending = selected.worker.last("historical-record-page");
  await disposed.close();
  await disposed.receive({ kind: "historical-record-page-result", id: pending.id, rpcId: pending.rpcId, gradePage: 1, gradePages: 3, error: null }, selected.worker);
  assert.equal(disposed.get("historical-grade-next").hidden, true); assert.equal(disposed.timers.size, 0);
});

test("main stored-record detail caption admits 1033 pages while matching changed-count receipt clears display only", async () => {
  const h = await harness(storedHistoricalFields()); const { worker, request } = await selectedHistoricalGrades(h, 1033);
  const replay = h.get("replay-name").textContent;
  assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 1 / 1033");
  assert.equal(h.get("historical-grade-next").disabled, false);
  h.click("historical-grade-next"); await flush(); const page = worker.last("historical-record-page");
  assert.equal(page.id, request.id); assert.equal(page.page, 1);
  await h.receive({ kind: "historical-record-page-result", id: page.id, rpcId: page.rpcId, gradePage: 1, gradePages: 1033, error: null });
  assert.equal(h.get("historical-grade-page").textContent, "Stored record details page 2 / 1033");
  h.click("historical-grade-next"); await flush(); const malformed = worker.last("historical-record-page");
  await h.receive({ kind: "historical-record-page-result", id: malformed.id, rpcId: malformed.rpcId, gradePage: 2, gradePages: 1034, error: null });
  assert.equal(h.get("historical-grade-next").hidden, true); assert.equal(h.get("replay-name").textContent, replay);
  assert.equal(h.get("replay-play").disabled, false); assert.equal(h.opens.length, 0); await h.close();
});

// These exercise the actual Window module's status wiring. The controlled
// Worker is deliberately not proof of a native GPU presentation.
function presentationPacket(h, kind, fields = {}) {
  return { kind, selectedId: h.workers.at(-1).last("select").id,
    generation: 7n, content: 11n, ...fields };
}
function statusSnapshot(h) {
  return { text: h.get("status").textContent, error: h.get("status").dataset.error };
}
test("suspended menus cannot own idle roster actions after live or replay playback", async () => {
  for (const mode of ["live", "replay"]) {
    const h = await harness({ touchSupported: true }); await h.preview();
    h.click("menu-open"); await flush();
    const old = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 3, fields: ["0", ""], selected: 0 };
    await h.receive(old);
    if (mode === "replay") chooseRecording(h, [selectedRecording().file]);
    const session = await h.launch(0, mode);
    assert.equal(h.get("menu-editor").hidden, true);
    h.click("stop"); await flush(); await h.receive(finalScore(session.id));
    await h.receive(old);
    const actions = h.workers[0].messages("menu-roster-count").length;
    await localCount(h, 2);
    assert.equal(h.workers[0].messages("menu-roster-count").length, actions, "idle roster never addresses suspended menu");
    assert.ok(h.get("local-source-2"));
    assert.equal(h.get("local-discover").disabled, false);
    h.click("local-discover"); await flush(); localAssign(h, 1, 1n); localAssign(h, 2, 2n);
    h.click("menu-open"); await flush();
    assert.deepEqual(Array.from(h.workers[0].last("menu-open").roster.players), [1, 2]);
    await h.receive(old);
    assert.equal(h.get("menu-editor").hidden, true, "old generation cannot resurrect during fresh open");
    await h.receive({ ...old, menuGeneration: 78n });
    await localCount(h, 3);
    assert.equal(h.workers[0].last("menu-roster-count").menuGeneration, 78n);
    await h.close();
  }
});

test("audio opening failure before play-start leaves current business menu usable", async () => {
  const opening = deferred();
  const h = await harness({ openGate: opening }); await h.preview();
  h.click("menu-open"); await flush();
  await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 3, fields: ["0", ""], selected: 0 });
  h.click("play"); await flush();
  assert.equal(h.workers[0].last("play-start"), undefined);
  opening.reject(new Error("actual audio opening refused")); await flush();
  await localCount(h, 2);
  assert.equal(h.workers[0].last("menu-roster-count").menuGeneration, 77n);
  await h.close();
});

test("dispatched play refusal and natural completion do not revive retired menu state or errors", async () => {
  for (const refused of [true, false]) {
    const h = await harness(); await h.preview(); h.click("menu-open"); await flush();
    const old = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
      route: 3, fields: ["0", ""], selected: 0 };
    await h.receive(old);
    let id;
    if (refused) {
      const start = await h.begin(); id = start.playId;
      await h.receive({ kind: "play-reply", playId: id, rpcId: start.rpcId, error: "actual preparation refused" });
    } else {
      const session = await h.launch(); id = session.id;
      await h.receive({ kind: "play-render-done", playId: id, renderId: 1,
        completed: true, songNs: 2350000000n, hits: 3n, misses: 1n, combo: 2n });
    }
    await h.receive(finalScore(id));
    const ended = statusSnapshot(h);
    await h.receive(old);
    await h.receive({ ...old, kind: "menu-error", message: "late retired menu refusal" });
    assert.deepEqual(statusSnapshot(h), ended);
    await localCount(h, 2);
    assert.equal(h.workers[0].messages("menu-roster-count").length, 0);
    assert.ok(h.get("local-source-2"));
    h.click("menu-open"); await flush();
    await h.receive({ ...old, kind: "menu-error", message: "retired error after new open" });
    assert.doesNotMatch(h.get("status").textContent, /retired error/);
    await h.receive({ ...old, menuGeneration: 78n });
    await localCount(h, 3);
    assert.equal(h.workers[0].last("menu-roster-count").menuGeneration, 78n);
    await h.close();
  }
});

test("replay setup carries the actual Window origin before audio or visual progress", async () => {
  const h = await harness(); await h.preview();
  chooseRecording(h, [selectedRecording().file]);
  const start = await h.begin("replay");
  assert.equal(start.mode, "replay");
  assert.equal(typeof start.windowOriginNs, "bigint");
  assert.equal(start.windowOriginNs, 9000000000n);
  assert.equal(h.audio.arms.length, 0);
  await h.close();
});

test("presentation feedback: preview wait restores status only on matching drawn and idle packets do not write", async () => {
  const h = await harness(); await h.preview(); const baseline = statusSnapshot(h);
  const writes = watchPlayDisplay(h);
  await h.receive(presentationPacket(h, "drawn"));
  assert.equal(writes.length, 0);
  await h.receive(presentationPacket(h, "render-wait"));
  assert.match(h.get("status").textContent, /graphics surface.*not ready/i);
  assert.equal(h.get("status").dataset.error, "true");
  await h.receive(presentationPacket(h, "render-wait"));
  assert.equal(writes.filter(write => write.id === "status").length, 1);
  for (const fields of [{ generation: 6n }, { content: 12n }, { selectedId: 999 }, { playId: 1 }]) {
    await h.receive(presentationPacket(h, "drawn", fields));
    assert.match(h.get("status").textContent, /graphics surface.*not ready/i);
  }
  await h.receive(presentationPacket(h, "drawn")); assert.deepEqual(statusSnapshot(h), baseline);
  assert.equal(h.get("canvas").hidden, false);
  const count = writes.length; await h.receive(presentationPacket(h, "drawn")); assert.equal(writes.length, count);
  await h.close();
});
test("presentation feedback: newer preview messages and late application errors survive presentation", async () => {
  for (const error of [false, true]) {
    const h = await harness(); await h.preview();
    await h.receive(presentationPacket(h, "render-wait"));
    h.get("position").value = "3.125"; h.get("seek-form").emit("submit");
    const seek = h.workers[0].last("seek");
    await h.receive(error ? { kind: "seek-error", id: seek.id, selectedId: seek.selectedId, message: "Seek operation failed" }
      : { kind: "position", id: seek.id, selectedId: seek.selectedId, ns: "3125000000" });
    const ordinary = statusSnapshot(h);
    assert.equal(ordinary.error, String(error));
    assert.match(ordinary.text, error ? /Seek operation failed/ : /Preview position: 3.125/);
    await h.receive(presentationPacket(h, "drawn")); assert.deepEqual(statusSnapshot(h), ordinary);
    await h.receive(presentationPacket(h, "render-wait", { generation: 8n }));
    await h.receive(presentationPacket(h, "drawn", { generation: 8n }));
    assert.deepEqual(statusSnapshot(h), ordinary, "a preceding error is restored with its error bit");
    await h.close();
  }
});
test("presentation feedback: live replay and local play restore their own ordinary feedback and retain controls", async () => {
  for (const mode of ["live", "replay", "local"]) {
    const h = await harness({ touchSupported: mode === "local" }); await h.preview();
    if (mode === "replay") chooseRecording(h, [selectedRecording().file]);
    if (mode === "local") {
      await localCount(h, 2); h.click("local-discover"); await flush();
      localAssign(h, 1, 1n); localAssign(h, 2, 2n);
    }
    const session = await h.launch(0, mode === "replay" ? "replay" : "live");
    if (mode === "local") assert.equal(session.start.localPlanWords.length, 8);
    const baseline = statusSnapshot(h);
    await h.receive(presentationPacket(h, "render-wait", { playId: session.id }));
    assert.match(h.get("status").textContent, /graphics surface.*not ready/i);
    for (const fields of [{ playId: session.id + 1 }, { playId: undefined }, { generation: 6n }, { content: 2n }]) {
      await h.receive(presentationPacket(h, "drawn", fields));
      assert.match(h.get("status").textContent, /graphics surface.*not ready/i);
    }
    await h.receive(presentationPacket(h, "drawn", { playId: session.id }));
    assert.deepEqual(statusSnapshot(h), baseline);
    assert.equal(h.get("stop").disabled, false); assert.equal(h.get("play").disabled, true);
    h.click("stop"); await flush();
    await h.receive(mode === "local" ? localFinal(session.start) : finalScore(session.id));
    await h.close();
  }
});
test("presentation feedback: invalid or inactive play packets never create a wait overlay", async () => {
  const h = await harness(); await h.preview(); const baseline = statusSnapshot(h);
  for (const fields of [{ playId: 1 }, { selectedId: 0 }, { generation: undefined }, { generation: 0n },
    { generation: 1 }, { content: undefined }, { content: 0n }, { content: "11" }]) {
    await h.receive(presentationPacket(h, "render-wait", fields));
    await h.receive(presentationPacket(h, "drawn", fields));
    assert.deepEqual(statusSnapshot(h), baseline);
  }
  const session = await h.launch(); const playing = statusSnapshot(h);
  for (const fields of [{ playId: session.id + 1 }, { playId: undefined }]) {
    await h.receive(presentationPacket(h, "render-wait", fields)); assert.deepEqual(statusSnapshot(h), playing);
  }
  await h.close();
});
test("presentation feedback: preparing chart fences previous waits and presentations", async () => {
  const h = await harness(); await h.preview(); const old = presentationPacket(h, "drawn");
  await h.receive({ ...old, kind: "render-wait" });
  h.get("prepare-form").emit("submit"); await flush(); const preparing = statusSnapshot(h);
  assert.match(preparing.text, /Preparing chart/); assert.equal(h.get("canvas").hidden, true);
  await h.receive(old); await h.receive({ ...old, kind: "render-wait" });
  assert.deepEqual(statusSnapshot(h), preparing); assert.equal(h.get("canvas").hidden, true);
  await h.close();
});
test("presentation feedback: stop and new play cannot revive old play status", async () => {
  const h = await harness(); await h.preview(); const first = await h.launch();
  const old = presentationPacket(h, "drawn", { playId: first.id });
  await h.receive({ ...old, kind: "render-wait" });
  h.click("stop"); await flush();
  const closing = statusSnapshot(h);
  await h.receive(old); await h.receive({ ...old, kind: "render-wait" }); assert.deepEqual(statusSnapshot(h), closing);
  await h.receive(finalScore(first.id)); const completed = statusSnapshot(h);
  await h.receive(old); await h.receive({ ...old, kind: "render-wait" }); assert.deepEqual(statusSnapshot(h), completed);
  const second = await h.launch(); const playing = statusSnapshot(h);
  await h.receive(old); await h.receive({ ...old, kind: "render-wait" }); assert.deepEqual(statusSnapshot(h), playing);
  await h.receive(presentationPacket(h, "render-wait", { playId: second.id, generation: 9n }));
  await h.receive(presentationPacket(h, "drawn", { playId: second.id, generation: 9n }));
  assert.deepEqual(statusSnapshot(h), playing); await h.close();
});
test("presentation feedback: shutdown and replaced Worker cannot alter the replacement owner", async () => {
  const h = await harness({ holdDispose: true }); await h.preview();
  const oldWorker = h.workers[0], old = presentationPacket(h, "drawn");
  await h.receive({ ...old, kind: "render-wait" });
  h.window.emit("pagehide"); await flush(); const shuttingDown = statusSnapshot(h);
  await h.receive(old, oldWorker); await h.receive({ ...old, kind: "render-wait" }, oldWorker);
  assert.deepEqual(statusSnapshot(h), shuttingDown);
  h.window.emit("pageshow", { persisted: true });
  await h.receive({ kind: "disposed" }, oldWorker); await h.receive({ kind: "disposed" }, h.renderers[0]);
  await flush(); h.faults.holdDispose = false; await h.preview();
  const replacement = statusSnapshot(h), replacementCanvas = h.get("canvas");
  await h.receive(old, oldWorker); await h.receive({ ...old, kind: "render-wait" }, oldWorker);
  assert.deepEqual(statusSnapshot(h), replacement); assert.equal(h.get("canvas"), replacementCanvas);
  await h.close();
});
test("presentation feedback: menu Back and history remain ordinary feedback owners", async () => {
  const h = await harness(storedHistoricalFields()); await selectedHistoricalGrades(h);
  const baseline = statusSnapshot(h);
  await h.receive(presentationPacket(h, "render-wait"));
  await h.receive(presentationPacket(h, "drawn")); assert.deepEqual(statusSnapshot(h), baseline);
  h.click("menu-open"); await flush();
  const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 3, fields: ["0", ""], selected: 0 };
  await h.receive(state); h.click("menu-back"); await flush();
  assert.equal(h.workers[0].last("menu-action").control, 72n);
  const afterBack = statusSnapshot(h);
  await h.receive(presentationPacket(h, "render-wait", { generation: 9n }));
  await h.receive(presentationPacket(h, "drawn", { generation: 9n }));
  assert.deepEqual(statusSnapshot(h), afterBack); assert.equal(h.opens.length, 0); await h.close();
});
test("presentation feedback: matching preroll presentation restores preparing text without claiming audio activation", async () => {
  const h = await harness(); await h.preview(); const start = await h.begin();
  assert.ok(start); const baseline = statusSnapshot(h); assert.match(baseline.text, /Preparing playable/);
  await h.receive(presentationPacket(h, "render-wait", { playId: start.playId }));
  assert.match(h.get("status").textContent, /graphics surface.*not ready/i);
  await h.receive(presentationPacket(h, "drawn", { playId: start.playId }));
  assert.deepEqual(statusSnapshot(h), baseline); assert.equal(h.audio.arms.length, 0);
  assert.equal(h.workers[0].messages("play-activate").length, 0);
  await h.close();
});

test("presentation feedback: accepted menu navigation cancels old waits and fences queued screen evidence", async () => {
  const h = await harness(); await h.preview();
  const baseline = statusSnapshot(h);
  await h.receive(presentationPacket(h, "render-wait"));
  assert.match(statusSnapshot(h).text, /graphics surface.*not ready/i);
  h.click("menu-open"); await flush();
  const state = { kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 3, fields: ["0", ""], selected: 0 };
  await h.receive(state);
  assert.deepEqual(statusSnapshot(h), baseline, "retired preview wait is not guidance for the menu");
  const old = { mode: "menu", generation: 9n, content: 11n,
    menuGeneration: 77n, screen: 3n, revision: 5n };
  await h.receive(presentationPacket(h, "render-wait", old));
  assert.match(statusSnapshot(h).text, /graphics surface.*not ready/i);
  const current = { ...old, screen: 4n, revision: 6n };
  await h.receive({ ...state, screen: 4n, revision: 6n });
  assert.deepEqual(statusSnapshot(h), baseline);
  for (const kind of ["render-wait", "drawn"]) {
    await h.receive(presentationPacket(h, kind, old));
    assert.deepEqual(statusSnapshot(h), baseline, "previous menu screen cannot publish feedback");
  }
  await h.receive(presentationPacket(h, "render-wait", current));
  await h.receive(presentationPacket(h, "drawn", old));
  assert.match(statusSnapshot(h).text, /graphics surface.*not ready/i);
  await h.receive(presentationPacket(h, "drawn", current));
  assert.deepEqual(statusSnapshot(h), baseline);
  await h.close();
});

test("presentation feedback: first-run menu waiting and recovery require no selected chart", async () => {
  const h = await harness();
  await h.receive({ kind: "ready" });
  await h.receive({ kind: "ready" }, h.renderers.at(-1));
  const baseline = statusSnapshot(h);
  assert.equal(h.workers[0].last("select"), undefined);
  h.click("menu-open"); await flush();
  assert.ok(h.workers[0].last("menu-open"), "initialized first-run owner admits menu navigation");
  await h.receive({ kind: "menu-state", menuGeneration: 77n, screen: 3n, revision: 5n,
    route: 1, fields: [], selected: 0 });
  const menu = { selectedId: 0, mode: "menu", generation: 7n, content: 11n,
    menuGeneration: 77n, screen: 3n, revision: 5n };
  await h.receive({ ...menu, kind: "render-wait", mode: "preview" });
  assert.deepEqual(statusSnapshot(h), baseline);
  await h.receive({ ...menu, kind: "render-wait" });
  assert.match(statusSnapshot(h).text, /graphics surface.*not ready/i);
  await h.receive({ ...menu, kind: "drawn" });
  assert.deepEqual(statusSnapshot(h), baseline);
  assert.equal(h.get("canvas").hidden, false);
  assert.equal(h.opens.length, 0);
  assert.equal(h.workers[0].last("select"), undefined);
  await h.close();
});

test("presentation feedback: stored history can wait and recover before preparing a chart", async () => {
  const h = await harness(storedHistoricalFields());
  await h.receive({ kind: "ready" });
  await h.receive({ kind: "ready" }, h.renderers.at(-1));
  assert.equal(h.workers[0].last("select"), undefined);
  h.click("records-refresh"); await flush(); h.click("records-use"); await flush();
  const request = h.workers[0].last("historical-record-present");
  assert.ok(request, "history presentation does not require a prepared preview");
  await h.receive({ kind: "historical-record-result", id: request.id, available: true, error: null, gradePage: 0, gradePages: 1 });
  const baseline = statusSnapshot(h);
  const history = { selectedId: 0, mode: "history", generation: 7n, content: 11n };
  await h.receive({ ...history, kind: "render-wait" });
  assert.match(statusSnapshot(h).text, /graphics surface.*not ready/i);
  await h.receive({ ...history, kind: "drawn" });
  assert.deepEqual(statusSnapshot(h), baseline);
  assert.equal(h.get("canvas").hidden, false);
  assert.equal(h.workers[0].last("select"), undefined);
  assert.equal(h.opens.length, 0);
  await h.close();
});

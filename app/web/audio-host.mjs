// DOM-side ownership only. Gameplay and audio presentation evidence remain in
// the shared runtime and the actual Worklet reports, not UI timers.
import { readAudioFailureDiagnostics } from "./audio-failure.mjs";
import { CLOCK_VERSION, CLOCK_ID, CLOCK_PERIODS, CLOCK_NAMES, clockGeometry } from "./native-audio-clock.mjs";
import { markerTime } from "./worklet-native-frame-probe.mjs";
const OWNER = Symbol("AudioHost owner");
const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const COMMAND_FIELDS = ["kind", "voice", "sample", "at", "gain", "value", "denominator"];

export class AudioHostError extends Error {
  constructor(code, message, details = {}) {
    super(message, details.cause === undefined ? undefined : { cause: details.cause });
    this.name = "AudioHostError";
    this.code = code;
    this.operation = details.operation ?? null;
    this.generation = details.generation ?? null;
    this.sequence = details.sequence ?? null;
    this.status = details.status ?? null;
    this.admitted = details.admitted ?? null;
    this.remoteError = details.remoteError?.slice(0, 4096) ?? null;
    this.diagnostics = details.diagnostics ?? null;
  }
}

function integer(value, minimum, maximum) {
  return Number.isSafeInteger(value) && value >= minimum && value <= maximum;
}

function unsigned(value) {
  return typeof value === "bigint" && value >= 0n && value <= U64_MAX;
}

function signed(value) {
  return typeof value === "bigint" && value >= I64_MIN && value <= I64_MAX;
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  // A terminal event may arrive before the next await attaches its handler.
  promise.catch(() => {});
  return { promise, resolve, reject };
}

function contextConfiguration(value) {
  if (value === undefined) value = {};
  if (value === null || typeof value !== "object" || Array.isArray(value)
    || Reflect.ownKeys(value).some(key => key !== "latencyHint" && key !== "sampleRate")) {
    throw new AudioHostError("validation", "Invalid AudioContext output options.", { operation: "open" });
  }
  const { latencyHint: requestedLatency, sampleRate } = value;
  const latencyHint = requestedLatency === undefined ? "interactive" : requestedLatency;
  if ((!["interactive", "balanced", "playback"].includes(latencyHint)
      && !(typeof latencyHint === "number" && Number.isFinite(latencyHint) && latencyHint >= 0 && latencyHint <= 60))
    || (sampleRate !== undefined && !integer(sampleRate, 1, 0xffffffff))) {
    throw new AudioHostError("validation", "Output latency must be a known category or 0 to 60 seconds; requested rate must be a positive unsigned 32-bit integer.", { operation: "open" });
  }
  return Object.freeze(sampleRate === undefined ? { latencyHint } : { latencyHint, sampleRate });
}

function configuration(options) {
  const pcm = options?.pcmLimits;
  const audio = options?.audioLimits;
  const timeoutMs = options?.timeoutMs;
  const config = {
    module: options?.module,
    generation: options?.generation,
    channels: options?.channels,
    contextOptions: contextConfiguration(options?.contextOptions),
    pcmLimits: {
      maxAssetBytes: pcm?.maxAssetBytes,
      maxTotalBytes: pcm?.maxTotalBytes,
      maxSamples: pcm?.maxSamples,
    },
    audioLimits: {
      queueCapacity: audio?.queueCapacity,
      maxVoices: audio?.maxVoices,
      pendingCapacity: audio?.pendingCapacity,
      maxFrames: audio?.maxFrames,
      maxCommandsPerRender: audio?.maxCommandsPerRender,
    },
    timeoutMs: timeoutMs === undefined ? 5000 : timeoutMs,
    signal: options?.signal,
  };
  const p = config.pcmLimits;
  const a = config.audioLimits;
  const signal = config.signal;
  if (!(config.module instanceof WebAssembly.Module)
    || !integer(config.generation, 1, Number.MAX_SAFE_INTEGER)
    || !integer(config.channels, 1, 32)
    || !integer(p.maxAssetBytes, 1, 2147483644)
    || !integer(p.maxTotalBytes, p.maxAssetBytes, 2147483644)
    || !integer(p.maxSamples, 1, 65536)
    || !integer(a.queueCapacity, 1, 65536) || !integer(a.maxVoices, 1, 4096)
    || !integer(a.pendingCapacity, 1, 65536) || !integer(a.maxFrames, 1, 1048576)
    || !integer(a.maxCommandsPerRender, 1, 65536)
    || !integer(config.timeoutMs, 1, 60000)
    || (signal !== undefined && (signal === null || typeof signal.aborted !== "boolean"
      || typeof signal.addEventListener !== "function" || typeof signal.removeEventListener !== "function"))) {
    throw new AudioHostError("validation", "Invalid bounded AudioHost options.", { operation: "open" });
  }
  return config;
}

function validReport(report) {
  return report !== null && typeof report === "object"
    && report.words instanceof Uint32Array && report.words.length === 58
    && report.words.buffer instanceof ArrayBuffer && report.words.buffer.resizable !== true
    && report.words.buffer.byteLength === 232 && report.words.byteOffset === 0
    && report.words.byteLength === 232
    && report.words[0] <= 1 && report.words[1] === 0
    && typeof report.available === "boolean" && report.available === (report.words[0] === 1);
}

export class AudioHost {
  #config;
  #context = null;
  #node = null;
  #sampleRate = null;
  #state = "opening";
  #sequence = 0;
  #pending = null;
  #cancelled = null;
  #failure = null;
  #stopPromise = null;
  #opening = deferred();
  #ready = deferred();
  #readyReceived = false;
  #clockProbe = deferred();
  #clockReady = deferred();
  #clockReadyReceived = false;
  #clockFrames = null;
  #clockAcquiredMs = null;
  #clockDeliveryMs = null;
  #clockGeometry = null;
  #clockRevision = 0;
  #clockCommitted = false;
  #clockPending = null;
  #clockCancelled = null;
  #setupTimer = null;
  #abort = null;
  #sampleIds = new Set();
  #pcmBytes = 0;
  #samplesTransferred = false;
  #commandsTransferred = false;

  constructor(token, config) {
    if (token !== OWNER) throw new AudioHostError("state", "Use AudioHost.open().");
    this.#config = config;
  }

  get sampleRate() { return this.#sampleRate; }
  get channels() { return this.#config.channels; }
  get generation() { return this.#config.generation; }
  get state() { return this.#state; }
  // Estimated control-time frame for selecting a future arm target. This is
  // not a callback, presentation or acoustic timestamp; the Worklet remains
  // authoritative and rejects an arm command that arrives too late.
  get currentFrame() {
    if (this.#failure) throw this.#failure;
    if (this.#context === null || !["setup", "allocated", "armed"].includes(this.#state)) {
      throw this.#error("state", "currentFrame", "Audio context time is unavailable in this state.");
    }
    const seconds = this.#context.currentTime;
    if (typeof seconds !== "number" || !Number.isFinite(seconds) || seconds < 0) {
      throw this.#error("state", "currentFrame", "Audio context time is not finite and nonnegative.");
    }
    const frame = Math.floor(seconds * this.sampleRate);
    if (!Number.isSafeInteger(frame) || frame < 0) {
      throw this.#error("state", "currentFrame", "Audio context time exceeds the safe frame range.");
    }
    return BigInt(frame);
  }

  controlClock() {
    this.currentFrame;
    const beforeMs = performance.now();
    const contextTime = this.#context.currentTime;
    const afterMs = performance.now();
    if (![beforeMs, contextTime, afterMs].every(value => typeof value === "number" && Number.isFinite(value) && value >= 0)
      || afterMs < beforeMs) throw this.#error("state", "clock", "Audio control clock observation is invalid.");
    return { beforeMs, contextTime, afterMs, sampleRate: this.sampleRate };
  }

  outputTimestamp() {
    this.currentFrame;
    if (typeof this.#context.getOutputTimestamp !== "function") {
      throw this.#error("unsupported", "presentation", "Audio output timestamps are unavailable.");
    }
    const timestamp = this.#context.getOutputTimestamp();
    if (![timestamp?.contextTime, timestamp?.performanceTime].every(value => typeof value === "number" && Number.isFinite(value) && value >= 0)) {
      throw this.#error("state", "presentation", "Audio output timestamp evidence is malformed.");
    }
    if (timestamp.contextTime === 0 || timestamp.performanceTime === 0) {
      throw this.#error("unavailable", "presentation", "Audio output timestamp evidence is not available yet.");
    }
    if (timestamp.contextTime > this.#context.currentTime) {
      throw this.#error("state", "presentation", "Audio output position exceeds its context render clock.");
    }
    return { contextTime: timestamp.contextTime, performanceTime: timestamp.performanceTime };
  }

  static async open(options) {
    const config = configuration(options);
    if (config.signal?.aborted) {
      throw new AudioHostError("aborted", "Audio setup was cancelled.", {
        operation: "open", generation: config.generation,
      });
    }
    if (typeof globalThis.AudioContext !== "function" || typeof globalThis.AudioWorkletNode !== "function") {
      throw new AudioHostError("unsupported", "AudioContext and AudioWorkletNode are required.", {
        operation: "open", generation: config.generation,
      });
    }
    const host = new AudioHost(OWNER, config);
    try {
      // One deadline covers resume, module acquisition and processor readiness.
      host.#setupTimer = setTimeout(() => host.#fail(host.#error("timeout", "open", "Audio setup timed out.")), config.timeoutMs);
      if (config.signal !== undefined) {
        host.#abort = () => host.#fail(host.#error("aborted", "open", "Audio setup was cancelled."));
        config.signal.addEventListener("abort", host.#abort, { once: true });
      }
      if (config.signal?.aborted) throw host.#error("aborted", "open", "Audio setup was cancelled.");
      const context = host.#context = new AudioContext(config.contextOptions);
      host.#context.onstatechange = () => {
        if (!host.#stopPromise && ["setup", "allocated", "armed"].includes(host.#state)
          && host.#context.state !== "running") {
          host.#fail(host.#error("state", "context", "AudioContext stopped running."));
        }
      };
      host.#sampleRate = host.#context.sampleRate;
      if (!integer(host.sampleRate, 1, 0xffffffff) || !host.#context.audioWorklet) {
        throw host.#error("unsupported", "open", "The context has no usable AudioWorklet format.");
      }
      // Invoke synchronously in the initiating gesture, before the first await.
      const resumed = Promise.resolve(host.#context.resume()).catch(cause => {
        throw host.#error("transport", "resume", "AudioContext resume failed.", { cause });
      });
      resumed.catch(error => {
        if (!host.#stopPromise) host.#fail(error);
      });
      const loaded = Promise.resolve(host.#context.audioWorklet.addModule(new URL("./audio-worklet.js", import.meta.url)))
        .catch(cause => { throw host.#error("transport", "open", "AudioWorklet module loading failed.", { cause }); });
      await Promise.race([loaded, host.#opening.promise]);
      host.#openingOwner(context, null);
      const node = host.#node = new AudioWorkletNode(context, "beatkernel-audio", {
        numberOfInputs: 0,
        numberOfOutputs: 1,
        outputChannelCount: [config.channels],
        channelCount: config.channels,
        channelCountMode: "explicit",
        channelInterpretation: "discrete",
        processorOptions: {
          module: config.module, generation: config.generation, channels: config.channels,
          pcmLimits: config.pcmLimits, audioLimits: config.audioLimits,
        },
      });
      host.#clockAcquiredMs = performance.now();
      host.#node.port.onmessage = event => host.#message(event.data);
      host.#node.port.onmessageerror = () => host.#fail(host.#error("message", "receive", "AudioWorklet message could not be decoded."));
      host.#node.onprocessorerror = () => host.#fail(host.#error("processor", "render", "AudioWorklet processor failed."));
      host.#node.port.start();
      host.#node.connect(host.#context.destination);
      await Promise.race([Promise.all([resumed, host.#ready.promise, host.#clockProbe.promise]), host.#opening.promise]);
      host.#openingOwner(context, node, "running");
      await host.#openingTransition("suspend", "running", "suspended");
      host.#openingOwner(context, node, "suspended");
      await Promise.race([host.#startClock(), host.#opening.promise]);
      host.#openingOwner(context, node, "suspended");
      await host.#openingTransition("resume", "suspended", "running");
      host.#openingOwner(context, node, "running");
      await Promise.race([host.#clockReady.promise, host.#opening.promise]);
      host.#openingOwner(context, node, "running");
      host.#clearSetup();
      host.#opening = null;
      host.#ready = null;
      host.#clockProbe = null;
      host.#clockReady = null;
      host.#state = "setup";
      return host;
    } catch (cause) {
      const error = cause instanceof AudioHostError ? cause
        : host.#error("transport", "open", "Audio setup failed.", { cause });
      host.#fail(error);
      try { await host.stop(); }
      catch (cleanupError) { host.#failure.cleanupError = cleanupError; }
      throw host.#failure;
    }
  }

  #error(code, operation, message, details = {}) {
    return new AudioHostError(code, message, { operation, generation: this.generation, ...details });
  }

  #openingOwner(context, node, expectedState = undefined) {
    const current = () => !this.#failure && !this.#stopPromise && this.#state === "opening"
      && this.#context === context && this.#node === node;
    if (!current()) throw this.#failure ?? this.#error("closed", "open", "Audio opening owner was stopped.");
    if (expectedState !== undefined) {
      const actualState = context.state;
      if (!current()) throw this.#failure ?? this.#error("closed", "open", "Audio opening owner was stopped.");
      if (actualState !== expectedState) {
        throw this.#error("state", "open", "AudioContext did not enter its required opening state.");
      }
    }
  }

  async #openingTransition(operation, before, after) {
    const context = this.#context;
    const node = this.#node;
    this.#openingOwner(context, node, before);
    const transition = context[operation];
    this.#openingOwner(context, node, before);
    if (typeof transition !== "function") {
      throw this.#error("unsupported", operation, "Native AudioContext setup transition is unavailable.");
    }
    let result;
    try {
      result = Promise.resolve(transition.call(context)).catch(cause => {
        this.#openingOwner(context, node);
        throw this.#error("transport", operation, "Native AudioContext setup transition failed.", { cause });
      });
    } catch (cause) {
      this.#openingOwner(context, node);
      throw this.#error("transport", operation, "Native AudioContext setup transition failed.", { cause });
    }
    await Promise.race([result, this.#opening.promise]);
    this.#openingOwner(context, node, after);
  }

  #clearSetup() {
    if (this.#setupTimer !== null) clearTimeout(this.#setupTimer);
    this.#setupTimer = null;
    if (this.#abort !== null) this.#config.signal.removeEventListener("abort", this.#abort);
    this.#abort = null;
  }

  #measureClockDelivery() {
    if (!this.#readyReceived || this.#clockFrames === null || this.#clockDeliveryMs !== null) return;
    const receivedMs = performance.now();
    if (typeof receivedMs !== "number" || !Number.isFinite(receivedMs)
      || typeof this.#clockAcquiredMs !== "number" || !Number.isFinite(this.#clockAcquiredMs)
      || receivedMs < this.#clockAcquiredMs) {
      throw this.#error("state", "clock", "Native clock delivery observation is invalid.");
    }
    this.#clockDeliveryMs = receivedMs - this.#clockAcquiredMs;
  }

  #clockFuture(boundaries) {
    if (this.#failure) throw this.#failure;
    if (this.#stopPromise || this.#context === null || this.#node === null) {
      throw this.#error("closed", "clock", "Native clock owner was stopped.");
    }
    const context = this.#context;
    const node = this.#node;
    if (this.#state === "opening") this.#openingOwner(context, node, "suspended");
    const seconds = this.#context.currentTime;
    // A native getter/API can synchronously retire the owner; never continue
    // scheduling after that retirement.
    if (this.#failure) throw this.#failure;
    if (this.#stopPromise || this.#context === null || this.#node === null) {
      throw this.#error("closed", "clock", "Native clock owner was stopped.");
    }
    if (this.#state === "opening") this.#openingOwner(context, node, "suspended");
    if (typeof seconds !== "number" || !Number.isFinite(seconds) || seconds < 0) {
      throw this.#error("state", "clock", "Native clock scheduling time is invalid.");
    }
    for (const frame of boundaries) {
      const time = markerTime(frame, this.sampleRate);
      if (time === null || seconds >= time) {
        throw this.#error("state", "clock", "Native clock scheduling missed its original deadline.");
      }
    }
  }

  #clockParameter(name, ramp) {
    const parameter = this.#node.parameters?.get(name);
    if (!parameter || typeof parameter.setValueAtTime !== "function"
      || (ramp && typeof parameter.linearRampToValueAtTime !== "function")) {
      throw this.#error("unsupported", "clock", "Native clock AudioParams are unavailable.");
    }
    return parameter;
  }

  #scheduleClock(parameter, methodName, value, frame, boundaries) {
    const method = parameter[methodName];
    const time = markerTime(frame, this.sampleRate);
    if (typeof method !== "function" || time === null) {
      throw this.#error("unsupported", "clock", "Native clock event is not representable.");
    }
    this.#clockFuture(boundaries);
    method.call(parameter, value, time);
    this.#clockFuture(boundaries);
  }

  async #startClock() {
    const context = this.#context;
    const node = this.#node;
    this.#openingOwner(context, node, "suspended");
    const quantum = this.#clockFrames;
    if (!integer(quantum, 2, this.#config.audioLimits.maxFrames)
      || this.#clockDeliveryMs === null) {
      throw this.#error("unsupported", "clock", "Initial native clock needs an interior quantum edge.");
    }
    const contextSeconds = this.#context.currentTime;
    if (this.#failure) throw this.#failure;
    if (this.#stopPromise) throw this.#error("closed", "clock", "Native clock owner was stopped.");
    if (typeof contextSeconds !== "number" || !Number.isFinite(contextSeconds) || contextSeconds < 0) {
      throw this.#error("state", "clock", "Native clock scheduling time is invalid.");
    }
    const frame = Math.ceil(contextSeconds * this.sampleRate);
    const lead = Math.max(4 * quantum, Math.ceil(2 * this.#clockDeliveryMs * this.sampleRate / 1000));
    const origin = Math.ceil((frame + lead) / quantum) * quantum + Math.floor(quantum / 2);
    const geometry = clockGeometry(origin, this.#config.audioLimits.maxFrames, this.sampleRate);
    if (!integer(frame, 0, Number.MAX_SAFE_INTEGER) || !integer(lead, 1, Number.MAX_SAFE_INTEGER)
      || !integer(frame + lead, 1, Number.MAX_SAFE_INTEGER) || geometry === null) {
      throw this.#error("state", "clock", "Native clock startup exceeds the representable frame range.");
    }
    this.#clockGeometry = geometry;
    await this.#clockRequest("clock-reserve", {
      revision: 0, origin, h0: geometry.h0, h1: geometry.h1, h2: geometry.h2,
    });
    this.#openingOwner(context, node, "suspended");
    const boundaries = [origin];
    this.#clockFuture(boundaries);
    const marker = this.#clockParameter(CLOCK_NAMES[0], false);
    const lanes = CLOCK_NAMES.slice(1).map(name => this.#clockParameter(name, true));
    // Fresh defaults and a single marker event are the origin provenance.
    this.#scheduleClock(marker, "setValueAtTime", 1, origin, boundaries);
    for (let lane = 0; lane < CLOCK_PERIODS.length; lane += 1) {
      const parameter = lanes[lane];
      const period = CLOCK_PERIODS[lane];
      this.#scheduleClock(parameter, "setValueAtTime", 1, origin, boundaries);
      for (let cycle = 1; cycle <= geometry.periods; cycle += 1) {
        const end = origin + cycle * period;
        this.#scheduleClock(parameter, "linearRampToValueAtTime", period + 1, end, boundaries);
        this.#scheduleClock(parameter, "setValueAtTime", 1, end, boundaries);
      }
    }
    this.#clockFuture(boundaries);
    await this.#clockRequest("clock-commit", {
      revision: 0, origin, h0: geometry.h0, h1: geometry.h1, h2: geometry.h2, futureEligible: true,
    }, () => { this.#clockCommitted = true; });
    this.#openingOwner(context, node, "suspended");
  }

  #clockRequest(operation, payload, commit = null) {
    const result = deferred();
    if (this.#failure || this.#stopPromise || this.#clockPending !== null) {
      result.reject(this.#failure ?? this.#error("state", "clock", "Native clock request owner is unavailable."));
      return result.promise;
    }
    const pending = { operation, revision: payload.revision, commit, ...result, timer: null };
    this.#clockPending = pending;
    pending.timer = setTimeout(() => {
      if (this.#clockPending !== pending) return;
      this.#fail(this.#error("timeout", operation, "Native clock request timed out."));
    }, this.#config.timeoutMs);
    try {
      this.#node.port.postMessage({ kind: operation, generation: this.generation,
        clockVersion: CLOCK_VERSION, clockId: CLOCK_ID, ...payload });
    } catch (cause) {
      this.#fail(this.#error("transport", operation, "Native clock request could not be sent.", { cause }));
    }
    return result.promise;
  }

  #refillClock(message) {
    const old = this.#clockGeometry;
    // Reserve the one refill transaction before native API/property calls.
    this.#clockCommitted = false;
    const horizons = [old.h0, old.h1, old.h2];
    const next = [message.to0, message.to1, message.to2];
    // The existing reset at each OLD horizon begins the appended ramp.
    // Scheduling another reset there would rewrite the original program.
    this.#clockFuture(horizons);
    const lanes = CLOCK_NAMES.slice(1).map(name => this.#clockParameter(name, true));
    for (let lane = 0; lane < CLOCK_PERIODS.length; lane += 1) {
      this.#scheduleClock(lanes[lane], "linearRampToValueAtTime", CLOCK_PERIODS[lane] + 1, next[lane], horizons);
      this.#scheduleClock(lanes[lane], "setValueAtTime", 1, next[lane], horizons);
    }
    this.#clockFuture(horizons);
    const geometry = { ...old, h0: next[0], h1: next[1], h2: next[2], periods: old.periods + 1 };
    this.#clockRequest("clock-commit", {
      revision: message.revision, origin: old.origin,
      h0: next[0], h1: next[1], h2: next[2], futureEligible: true,
    }, () => {
      this.#clockGeometry = geometry;
      this.#clockRevision = message.revision;
      this.#clockCommitted = true;
    }).catch(error => {
      // stop() already owns cancellation and its fresh ACK; rejecting that
      // cleanup again would retire the ACK path before it releases the owner.
      if (!this.#stopPromise) this.#fail(error);
    });
  }

  #gate(operation, phases) {
    if (this.#failure) throw this.#failure;
    if (this.#stopPromise) throw this.#error("closed", operation, "Audio host is closing or closed.");
    if (!phases.includes(this.#state)) throw this.#error("state", operation, "Audio operation is not allowed in this state.");
    if (this.#pending) throw this.#error("busy", operation, "An audio operation is already pending.");
    // Reserve the final representable sequence for explicit cleanup.
    if (this.#sequence >= Number.MAX_SAFE_INTEGER - 1) throw this.#error("state", operation, "Audio control sequence exhausted; stop the owner.");
  }

  async sample(input) {
    this.#gate("sample", ["setup"]);
    if (this.#samplesTransferred) throw this.#error("state", "sample", "Sample authority belongs to the transferred port.");
    const id = input?.id;
    const rate = input?.rate;
    const channels = input?.channels;
    const pcm = input?.pcm;
    const buffer = pcm?.buffer;
    const bytes = pcm?.byteLength;
    const offset = pcm?.byteOffset;
    const length = pcm?.length;
    const bufferBytes = buffer?.byteLength;
    const resizable = buffer?.resizable;
    this.#gate("sample", ["setup"]);
    if (this.#samplesTransferred) throw this.#error("state", "sample", "Sample authority belongs to the transferred port.");
    const limits = this.#config.pcmLimits;
    if (!unsigned(id) || !integer(rate, 1, 0xffffffff) || channels !== this.channels
      || !(pcm instanceof Float32Array) || !(buffer instanceof ArrayBuffer)
      || resizable === true || offset !== 0 || bytes !== bufferBytes
      || length % channels !== 0 || bytes > limits.maxAssetBytes
      || bytes > limits.maxTotalBytes - this.#pcmBytes
      || this.#sampleIds.size >= limits.maxSamples || this.#sampleIds.has(id)) {
      throw this.#error("validation", "sample", "Invalid sample or nonexclusive PCM backing buffer.");
    }
    try {
      // A detached empty view otherwise has the same sizes as a valid empty
      // sample. Constructing a zero-length view detects detachment without a copy.
      new Float32Array(buffer, 0, 0);
    } catch (cause) {
      throw this.#error("validation", "sample", "PCM backing buffer is detached.", { cause });
    }
    // Worklet validates finite values before insert_sample. Bad PCM causes a
    // remote error and fences this owner; the transferred buffer cannot be retried.
    this.#gate("sample", ["setup"]);
    if (this.#samplesTransferred) throw this.#error("state", "sample", "Sample authority belongs to the transferred port.");
    return this.#request("sample", { id, rate, channels, pcm }, 0, [buffer], () => {
      this.#sampleIds.add(id);
      this.#pcmBytes += bytes;
    });
  }

  async openSamplePort() {
    this.#gate("attach-samples", ["setup"]);
    if (this.#samplesTransferred || this.#sampleIds.size !== 0) {
      throw this.#error("state", "attach-samples", "Sample handoff requires empty setup and an untransferred owner.");
    }
    if (typeof globalThis.MessageChannel !== "function") {
      throw this.#error("unsupported", "attach-samples", "MessageChannel is required for direct sample ownership.");
    }
    let port1;
    let port2;
    try {
      const channel = new MessageChannel();
      port1 = channel.port1;
      port2 = channel.port2;
      if (port1 === port2 || ![port1, port2].every(port => port
        && ["postMessage", "start", "close"].every(name => typeof port[name] === "function"))) {
        throw this.#error("unsupported", "attach-samples", "MessageChannel did not provide two usable endpoints.");
      }
      this.#gate("attach-samples", ["setup"]);
      if (this.#samplesTransferred || this.#sampleIds.size !== 0) {
        throw this.#error("state", "attach-samples", "Sample ownership changed during channel allocation.");
      }
    } catch (cause) {
      try { port1?.close(); } catch {}
      try { port2?.close(); } catch {}
      throw cause instanceof AudioHostError ? cause : this.#error("transport", "attach-samples", "Sample channel allocation failed.", { cause });
    }
    // Ambiguous transfer cannot restore host upload authority.
    this.#samplesTransferred = true;
    try {
      await this.#request("attach-samples", { port: port2 }, 0, [port2]);
      if (this.#failure) throw this.#failure;
      if (this.#stopPromise) throw this.#error("closed", "attach-samples", "Audio owner stopped during sample handoff.");
      return Object.freeze({ port: port1, generation: this.generation, channels: this.channels,
        pcmLimits: Object.freeze({ ...this.#config.pcmLimits }), timeoutMs: this.#config.timeoutMs });
    } catch (error) {
      try { port1.close(); } catch {}
      try { port2.close(); } catch {}
      throw error;
    }
  }

  async finish(endFrame = undefined) {
    this.#gate("finish", ["setup"]);
    if (endFrame !== undefined && !unsigned(endFrame)) throw this.#error("validation", "finish", "Playback end frame must be a u64 BigInt.");
    return this.#request("finish", endFrame === undefined ? {} : { endFrame }, 0, [], () => { this.#state = "allocated"; });
  }

  async arm(frame) {
    this.#gate("arm", ["allocated"]);
    if (!unsigned(frame)) throw this.#error("validation", "arm", "Arm frame must be a u64 BigInt.");
    return this.#request("arm", { frame }, 0, [], () => { this.#state = "armed"; });
  }

  async openCommandPort() {
    this.#gate("attach-commands", ["allocated"]);
    if (this.#commandsTransferred) throw this.#error("state", "attach-commands", "Command authority has already been transferred.");
    if (typeof globalThis.MessageChannel !== "function") {
      throw this.#error("unsupported", "attach-commands", "MessageChannel is required for direct command ownership.");
    }
    let channel;
    try {
      channel = new MessageChannel();
      if (channel.port1 === channel.port2 || ![channel.port1, channel.port2].every(port => port
        && ["postMessage", "start", "close"].every(name => typeof port[name] === "function"))) {
        throw this.#error("unsupported", "attach-commands", "MessageChannel did not provide two usable endpoints.");
      }
    } catch (cause) {
      try { channel?.port1?.close(); } catch {}
      try { channel?.port2?.close(); } catch {}
      throw cause instanceof AudioHostError ? cause : this.#error("transport", "attach-commands", "Command channel allocation failed.", { cause });
    }
    // A failed or timed-out transfer cannot prove that the processor did not
    // adopt its endpoint. There is no fallback to the host command producer.
    this.#commandsTransferred = true;
    try {
      await this.#request("attach-commands", { port: channel.port2 }, 0, [channel.port2]);
      if (this.#failure) throw this.#failure;
      if (this.#stopPromise) throw this.#error("closed", "attach-commands", "Audio owner stopped during command handoff.");
      return Object.freeze({ port: channel.port1, generation: this.generation,
        queueCapacity: this.#config.audioLimits.queueCapacity, timeoutMs: this.#config.timeoutMs });
    } catch (error) {
      try { channel.port1.close(); } catch {}
      try { channel.port2.close(); } catch {}
      throw error;
    }
  }

  async commands(commands) {
    this.#gate("commands", ["allocated", "armed"]);
    if (this.#commandsTransferred) throw this.#error("state", "commands", "Command authority belongs to the transferred port.");
    const count = Array.isArray(commands) ? commands.length : 0;
    if (!integer(count, 1, this.#config.audioLimits.queueCapacity)) {
      throw this.#error("validation", "commands", "Command batch must be nonempty and bounded.");
    }
    const snapshot = [];
    for (let index = 0; index < count; index++) {
      const input = commands[index];
      if (input === null || typeof input !== "object" || Reflect.ownKeys(input).length !== COMMAND_FIELDS.length
        || !COMMAND_FIELDS.every(field => Object.hasOwn(input, field))) {
        throw this.#error("validation", "commands", "Command records require exactly seven fields.");
      }
      const command = { kind: input.kind, voice: input.voice, sample: input.sample, at: input.at,
        gain: input.gain, value: input.value, denominator: input.denominator };
      if (!integer(command.kind, 0, 3) || !unsigned(command.voice) || !unsigned(command.sample)
        || !signed(command.at) || typeof command.gain !== "number" || !Number.isFinite(command.gain)
        || !Number.isFinite(Math.fround(command.gain)) || !signed(command.value) || !unsigned(command.denominator)) {
        throw this.#error("validation", "commands", "Command fields exceed the numeric protocol bounds.");
      }
      snapshot.push(command);
    }
    return this.#request("commands", { commands: snapshot }, snapshot.length);
  }

  async poll() {
    this.#gate("poll", ["setup", "allocated", "armed"]);
    const reply = await this.#request("poll");
    return reply.report;
  }

  #request(operation, payload = {}, count = 0, transfer = [], commit = null) {
    const operationResult = deferred();
    const sequence = ++this.#sequence;
    if (!Number.isSafeInteger(sequence)) {
      operationResult.reject(this.#error("state", operation, "Audio control sequence exhausted."));
      return operationResult.promise;
    }
    const pending = { operation, sequence, count, commit, ...operationResult, timer: null };
    this.#pending = pending;
    pending.timer = setTimeout(() => {
      if (this.#pending !== pending) return;
      this.#fail(this.#error("timeout", operation, "Audio operation timed out.", { sequence }));
    }, this.#config.timeoutMs);
    try {
      this.#node.port.postMessage({ kind: operation, generation: this.generation, sequence, ...payload }, transfer);
    } catch (cause) {
      this.#fail(this.#error("transport", operation, "Audio control message could not be sent.", { sequence, cause }));
    }
    return operationResult.promise;
  }

  #ackValid(message, pending) {
    if (message.sequence !== pending.sequence || message.operation !== pending.operation
      || !integer(message.status, 0, 0xffffffff) || !integer(message.admitted, 0, pending.count)
      || !(message.error === null || typeof message.error === "string")
      || (message.status === 0 && message.error !== null)) return false;
    if (pending.operation === "commands" && message.status === 0 && message.admitted !== pending.count) return false;
    return pending.operation === "poll" && message.status === 0
      ? validReport(message.report) : message.report === null;
  }

  #clockMessage(message, kind, ownerCurrent, ownerMalformed) {
    const pending = this.#clockPending;
    const cancelled = this.#clockCancelled;
    const geometry = this.#clockGeometry;
    const revision = this.#clockRevision;
    const committed = this.#clockCommitted;
    const readyReceived = this.#clockReadyReceived;
    const frames = this.#clockFrames;
    const current = () => ownerCurrent() && this.#clockPending === pending
      && this.#clockCancelled === cancelled && this.#clockGeometry === geometry
      && this.#clockRevision === revision && this.#clockCommitted === committed
      && this.#clockReadyReceived === readyReceived && this.#clockFrames === frames;
    const malformed = cause => { if (current()) ownerMalformed(cause); };
    let fields;
    try {
      const clockVersion = message.clockVersion;
      const clockId = message.clockId;
      if (!current()) return;
      if (clockVersion !== CLOCK_VERSION || clockId !== CLOCK_ID) { malformed(); return; }
      // stop has revoked clock authority. Progress already queued by the
      // Worklet cannot be correlated against the cancelled active geometry;
      // only exact clock ACKs remain relevant to cleanup.
      if (this.#stopPromise && (kind === "clock-probe"
        || kind === "clock-ready" || kind === "clock-refill")) return;
      if (kind === "clock-probe") {
        fields = { frames: message.frames };
      } else if (kind === "clock-ready") {
        fields = { revision: message.revision, origin: message.origin };
      } else if (kind === "clock-refill") {
        fields = { revision: message.revision, from0: message.from0, from1: message.from1,
          from2: message.from2, to0: message.to0, to1: message.to1, to2: message.to2,
          frames: message.frames, endLow: message.endLow, endHigh: message.endHigh };
      } else if (kind === "clock-reserved" || kind === "clock-committed") {
        fields = { revision: message.revision, status: message.status, error: message.error };
      } else {
        malformed();
        return;
      }
    } catch (cause) { malformed(cause); return; }
    if (!current()) return;
    if (kind === "clock-probe") {
      if (!integer(fields.frames, 1, this.#config.audioLimits.maxFrames)
        || frames !== null || this.#clockProbe === null) { malformed(); return; }
      if (this.#stopPromise) return;
      if (this.#state !== "opening") { malformed(); return; }
      this.#clockFrames = fields.frames;
      try { this.#measureClockDelivery(); }
      catch (error) { this.#fail(error); return; }
      this.#clockProbe.resolve();
      return;
    }
    if (kind === "clock-ready") {
      if (fields.revision !== 0 || geometry === null || fields.origin !== geometry.origin
        || !committed || readyReceived) { malformed(); return; }
      if (this.#stopPromise) return;
      if (this.#state !== "opening" || this.#clockReady === null) { malformed(); return; }
      this.#clockReadyReceived = true;
      this.#clockReady.resolve();
      return;
    }
    if (kind === "clock-refill") {
      if (!integer(fields.endLow, 0, 0xffffffff) || !integer(fields.endHigh, 0, 0x1fffff)) {
        malformed();
        return;
      }
      const end = fields.endLow + fields.endHigh * 4294967296;
      if (!readyReceived || !committed || geometry === null || pending !== null
        || !integer(fields.revision, 1, Number.MAX_SAFE_INTEGER) || fields.revision !== revision + 1
        || fields.from0 !== geometry.h0 || fields.from1 !== geometry.h1 || fields.from2 !== geometry.h2
        || !integer(fields.to0, 1, Number.MAX_SAFE_INTEGER) || fields.to0 !== geometry.h0 + CLOCK_PERIODS[0]
        || !integer(fields.to1, 1, Number.MAX_SAFE_INTEGER) || fields.to1 !== geometry.h1 + CLOCK_PERIODS[1]
        || !integer(fields.to2, 1, Number.MAX_SAFE_INTEGER) || fields.to2 !== geometry.h2 + CLOCK_PERIODS[2]
        || markerTime(fields.to0, this.sampleRate) === null
        || markerTime(fields.to1, this.sampleRate) === null
        || markerTime(fields.to2, this.sampleRate) === null
        || !integer(fields.frames, 1, this.#config.audioLimits.maxFrames)
        || !integer(end, fields.frames, Math.min(geometry.h0, geometry.h1, geometry.h2))
        || end < geometry.origin || Math.min(geometry.h0, geometry.h1, geometry.h2) - end > geometry.lowWater) {
        malformed();
        return;
      }
      if (this.#stopPromise) return;
      try { this.#refillClock(fields); }
      catch (cause) {
        this.#fail(cause instanceof AudioHostError ? cause
          : this.#error("transport", "clock", "Native clock refill scheduling failed.", { cause }));
      }
      return;
    }
    const ackValid = request => request !== null
      && kind === (request.operation === "clock-reserve" ? "clock-reserved" : "clock-committed")
      && fields.revision === request.revision && integer(fields.status, 0, 0xffffffff)
      && (fields.error === null || (typeof fields.error === "string" && fields.error.length <= 4096))
      && (fields.status !== 0 || fields.error === null);
    if (ackValid(cancelled)) {
      this.#clockCancelled = null;
      return;
    }
    if (!ackValid(pending)) { malformed(); return; }
    clearTimeout(pending.timer);
    this.#clockPending = null;
    if (fields.status !== 0) {
      const error = this.#error("remote", pending.operation, "AudioWorklet rejected its native clock transaction.", {
        status: fields.status, remoteError: fields.error,
      });
      pending.reject(error);
      this.#fail(error);
      return;
    }
    if (pending.commit) pending.commit();
    pending.resolve(fields);
  }

  #message(message) {
    if (this.#node === null) return;
    const pending = this.#pending;
    const cancelled = this.#cancelled;
    const stopPromise = this.#stopPromise;
    const state = this.#state;
    const failure = this.#failure;
    const current = () => this.#node !== null && this.#pending === pending
      && this.#cancelled === cancelled && this.#stopPromise === stopPromise
      && this.#state === state && this.#failure === failure;
    const malformed = cause => {
      if (current()) this.#fail(this.#error("protocol", "receive", "Malformed or uncorrelated AudioWorklet response.", { cause }));
    };
    if (message === null || typeof message !== "object") { malformed(); return; }
    let fields;
    try {
      const generation = message.generation;
      if (integer(generation, 1, Number.MAX_SAFE_INTEGER) && generation !== this.generation) return;
      fields = { generation, kind: message.kind, operation: message.operation, sequence: message.sequence,
        status: message.status, admitted: message.admitted, error: message.error, report: message.report,
        sampleRate: message.sampleRate, channels: message.channels };
    } catch (cause) { malformed(cause); return; }
    if (!current()) return;
    if (fields.generation !== this.generation) { malformed(); return; }
    if (typeof fields.kind === "string" && fields.kind.startsWith("clock-")) {
      this.#clockMessage(message, fields.kind, current, malformed);
      return;
    }
    if (fields.kind === "terminal") {
      if (!integer(fields.status, 1, 0xffffffff)) { malformed(); return; }
      let diagnostics;
      try { diagnostics = readAudioFailureDiagnostics(message); }
      catch (cause) { malformed(cause); return; }
      if (!current()) return;
      // The fresh stop ACK owns cleanup after a rejected control; the
      // original error retains its status, diagnostics and admitted prefix.
      const error = this.#error("processor", "render", `AudioWorklet reported a terminal failure (status ${fields.status}).`,
        { status: fields.status, diagnostics });
      if (this.#stopPromise) {
        this.#failure ??= error;
        this.#state = "failed";
        return;
      }
      this.#fail(error);
      return;
    }
    if (fields.kind === "ready") {
      if (fields.sampleRate !== this.sampleRate || fields.channels !== this.channels) { malformed(); return; }
      if (this.#stopPromise) return;
      if (this.#state !== "opening" || this.#ready === null || this.#readyReceived) { malformed(); return; }
      this.#readyReceived = true;
      try { this.#measureClockDelivery(); }
      catch (error) { this.#fail(error); return; }
      this.#ready.resolve();
      return;
    }
    if (fields.kind !== "ack") { malformed(); return; }
    let cancelledAck;
    let valid;
    try {
      cancelledAck = cancelled !== null && this.#ackValid(fields, cancelled);
      valid = cancelledAck || (pending !== null && this.#ackValid(fields, pending));
    } catch (cause) { malformed(cause); return; }
    if (!current()) return;
    if (cancelledAck) {
      this.#cancelled = null;
      return;
    }
    if (!valid) { malformed(); return; }
    let diagnostics = null;
    try {
      const supplied = message.diagnostics;
      if (supplied !== undefined) {
        if (fields.status === 0) throw new TypeError("unexpected audio failure diagnostics");
        diagnostics = readAudioFailureDiagnostics(supplied);
        if (diagnostics === null) throw new TypeError("missing audio failure diagnostic version");
      }
    } catch (cause) { malformed(cause); return; }
    if (!current()) return;
    clearTimeout(pending.timer);
    this.#pending = null;
    if (fields.status !== 0) {
      const error = this.#error("remote", pending.operation, `AudioWorklet rejected the operation (status ${fields.status})${fields.error ? `: ${fields.error.slice(0, 4096)}` : "."}`, {
        sequence: pending.sequence, status: fields.status, admitted: fields.admitted,
        remoteError: fields.error, diagnostics,
      });
      pending.reject(error);
      this.#fail(error);
      return;
    }
    if (pending.commit) pending.commit();
    pending.resolve(fields);
  }

  #fail(error) {
    // An opening await may rethrow the exact failure that already started
    // stop. Join its cleanup rather than rejecting its still-pending ACK.
    if (this.#stopPromise && error === this.#failure) return;
    this.#failure ??= error;
    this.#state = "failed";
    this.#opening?.reject(this.#failure);
    this.#clockReady?.reject(this.#failure);
    this.#clockProbe?.reject(this.#failure);
    // During cleanup, a broken ACK/message path cannot wait forever for stop.
    if (this.#pending?.operation === "stop") {
      const pending = this.#pending;
      this.#pending = null;
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.stop();
  }

  stop() {
    if (this.#stopPromise) return this.#stopPromise;
    const stopped = deferred();
    this.#stopPromise = stopped.promise;
    this.#clearSetup();
    const cancellation = this.#failure ?? this.#error("closed", "stop", "Audio owner was stopped.");
    this.#opening?.reject(cancellation);
    this.#clockReady?.reject(cancellation);
    this.#clockProbe?.reject(cancellation);
    if (this.#clockPending) {
      const pending = this.#clockPending;
      this.#clockCancelled = { operation: pending.operation, revision: pending.revision };
      this.#clockPending = null;
      clearTimeout(pending.timer);
      pending.reject(cancellation);
    }
    if (this.#pending) {
      const pending = this.#pending;
      this.#cancelled = { operation: pending.operation, sequence: pending.sequence, count: pending.count };
      this.#pending = null;
      clearTimeout(pending.timer);
      pending.reject(cancellation);
    }
    if (!this.#failure) this.#state = "closed";
    this.#cleanup().then(stopped.resolve, stopped.reject);
    return this.#stopPromise;
  }

  async #cleanup() {
    let failure = null;
    if (this.#node !== null) {
      // Remove audible output immediately; keep only the control path alive
      // until the Worklet has freed its owner and acknowledged stop.
      try { this.#node.disconnect(); } catch (cause) {
        failure ??= this.#error("transport", "stop", "Audio node disconnect failed.", { cause });
      }
      try { await this.#request("stop"); } catch (error) { failure ??= error; }
      this.#node.onprocessorerror = null;
      this.#node.port.onmessage = null;
      this.#node.port.onmessageerror = null;
      try { this.#node.port.close(); } catch (cause) {
        failure ??= this.#error("transport", "stop", "Audio message port close failed.", { cause });
      }
      this.#node = null;
    }
    if (this.#context !== null) {
      this.#context.onstatechange = null;
      let timer = null;
      try {
        const timeout = new Promise((_, reject) => {
          timer = setTimeout(() => reject(this.#error("timeout", "close", "AudioContext close timed out.")), this.#config.timeoutMs);
        });
        await Promise.race([this.#context.close(), timeout]);
      } catch (cause) {
        failure ??= cause instanceof AudioHostError ? cause
          : this.#error("transport", "close", "AudioContext close failed.", { cause });
      } finally {
        if (timer !== null) clearTimeout(timer);
        this.#context = null;
      }
    }
    this.#cancelled = null;
    this.#clockCancelled = null;
    this.#clockGeometry = null;
    this.#clockReady = null;
    this.#clockProbe = null;
    this.#ready = null;
    this.#opening = null;
    this.#sampleIds.clear();
    if (failure) {
      this.#failure ??= failure;
      this.#state = "failed";
      throw failure;
    }
  }
}

import "./worklet-encoding.mjs";
import { initSync, BrowserAudio } from "./audio-pkg/beatkernel_bms_runtime.js";
import { CLOCK_VERSION, CLOCK_ID, CLOCK_PERIODS, CLOCK_NAMES, clockGeometry, validClockLane } from "./native-audio-clock.mjs";
import { markerTime, nativePhase } from "./worklet-native-frame-probe.mjs";
import {
  AUDIO_FAILURE_DIAGNOSTIC_VERSION, AUDIO_FAILURE_ORIGIN_CONTROL,
  AUDIO_FAILURE_ORIGIN_ARM, AUDIO_FAILURE_ORIGIN_PROCESS,
} from "./audio-failure.mjs";

// The generated glue owns module-global WASM state. A second live processor
// must never initialize or replace it. Only an acknowledged stop releases it.
let liveOwner = null;

const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const WORD = 4294967296;
const REPORT_FIELDS = 29;
const INVALID = 100;
const SEQUENCE = 101;
const GENERATION = 102;
const STATE = 103;
const LAYOUT = 104;
const MEMORY = 105;
const EXCEPTION = 106;
const FRAME = 107;
const COMMAND_FIELDS = ["kind", "voice", "sample", "at", "gain", "value", "denominator"];

function integer(value, min, max) {
  return Number.isSafeInteger(value) && value >= min && value <= max;
}

function zeroLane(values, frames) {
  if (!(values instanceof Float32Array) || (values.length !== 1 && values.length !== frames)) return false;
  for (let index = 0; index < values.length; index++) if (values[index] !== 0) return false;
  return true;
}

function oneMarker(values, frames) {
  if (!(values instanceof Float32Array) || (values.length !== 1 && values.length !== frames)) return false;
  for (let index = 0; index < values.length; index++) if (values[index] !== 1) return false;
  return true;
}

function unsigned(value) {
  return typeof value === "bigint" && value >= 0n && value <= U64_MAX;
}

function signed(value) {
  return typeof value === "bigint" && value >= I64_MIN && value <= I64_MAX;
}

function validCommand(command) {
  return command !== null && typeof command === "object"
    && Reflect.ownKeys(command).length === COMMAND_FIELDS.length
    && COMMAND_FIELDS.every(field => Object.hasOwn(command, field))
    && integer(command.kind, 0, 3) && unsigned(command.voice) && unsigned(command.sample)
    && signed(command.at) && typeof command.gain === "number" && Number.isFinite(command.gain)
    && Number.isFinite(Math.fround(command.gain)) && signed(command.value) && unsigned(command.denominator);
}

function validOptions(options) {
  const pcm = options?.pcmLimits;
  const audio = options?.audioLimits;
  return options?.module instanceof WebAssembly.Module
    && integer(options.generation, 1, Number.MAX_SAFE_INTEGER)
    && integer(sampleRate, 1, 0xffffffff) && integer(options.channels, 1, 32)
    && integer(pcm?.maxAssetBytes, 1, 2147483644)
    && integer(pcm?.maxTotalBytes, pcm.maxAssetBytes, 2147483644)
    && integer(pcm?.maxSamples, 1, 65536)
    && integer(audio?.queueCapacity, 1, 65536) && integer(audio?.maxVoices, 1, 4096)
    && integer(audio?.pendingCapacity, 1, 65536) && integer(audio?.maxFrames, 1, 1048576)
    && integer(audio?.maxCommandsPerRender, 1, 65536);
}

// Controls use generation and consecutive sequence numbers beginning at 1.
// sample: {id: bigint, rate, channels, pcm: Float32Array}; finish: {endFrame?: bigint}; arm: {frame: bigint};
// attach-samples: {port}; its independent setup sequence accepts sample and
// end-samples: {count, bytes}. Only an accepted end permits host finish.
// commands: {commands: [{kind, voice, sample, at, gain, value, denominator}]}; poll; stop.
// Command IDs/denominator are u64 BigInt; at/value are i64 BigInt. No fields default.
// ACK admitted counts only this batch's exact successful queue prefix. A failed
// batch fences playback; neither its successful prefix nor game operations retry.
class BeatKernelAudioProcessor extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    return [
      { name: CLOCK_NAMES[0], defaultValue: 0, minValue: 0, maxValue: 1, automationRate: "a-rate" },
      { name: CLOCK_NAMES[1], defaultValue: 0, minValue: 0, maxValue: CLOCK_PERIODS[0] + 1, automationRate: "a-rate" },
      { name: CLOCK_NAMES[2], defaultValue: 0, minValue: 0, maxValue: CLOCK_PERIODS[1] + 1, automationRate: "a-rate" },
      { name: CLOCK_NAMES[3], defaultValue: 0, minValue: 0, maxValue: CLOCK_PERIODS[2] + 1, automationRate: "a-rate" },
    ];
  }

  constructor(options) {
    super();
    if (liveOwner !== null) throw new Error("An audio processor already owns this worklet module.");
    const config = options?.processorOptions;
    if (!validOptions(config)) throw new Error("Invalid bounded audio processor options.");
    this.generation = config.generation;
    this.sequence = 0;
    this.commandPort = null;
    this.commandSequence = 0;
    this.samplePort = null;
    this.sampleSequence = 0;
    this.samplesTransferred = false;
    this.samplesEnded = false;
    this.channels = config.channels;
    this.maxFrames = config.audioLimits.maxFrames;
    this.maxBatch = config.audioLimits.queueCapacity;
    this.pcmLimits = config.pcmLimits;
    this.sampleIds = new Set();
    this.pcmBytes = 0;
    this.owner = null;
    this.phase = 0; // 0 setup, 1 allocated, 2 armed, 3 stopped.
    this.failed = false;
    this.diagnosed = false;
    this.terminalReady = false;
    this.terminalPosted = false;
    this.memory = null;
    this.buffer = null;
    this.output = null;
    this.clockStage = 0; // 0 virgin, 1 reserved, 2 committed, 3 first certificate, 4 ready.
    this.clockOrigin = 0;
    this.clockH0 = 0;
    this.clockH1 = 0;
    this.clockH2 = 0;
    this.clockLowWater = 0;
    this.clockRevision = 0;
    this.clockPendingRevision = 0;
    this.clockEnd = 0;
    this.clockProbePosted = false;
    this.pendingArm = null;
    this.pendingStartLow = 0;
    this.pendingStartHigh = 0;
    // These messages are allocated before any real-time callback.
    this.clockProbe = { kind: "clock-probe", generation: this.generation,
      clockVersion: CLOCK_VERSION, clockId: CLOCK_ID, frames: 0 };
    this.clockReserved = { kind: "clock-reserved", generation: this.generation,
      clockVersion: CLOCK_VERSION, clockId: CLOCK_ID, revision: 0, status: 0, error: null };
    this.clockCommitted = { kind: "clock-committed", generation: this.generation,
      clockVersion: CLOCK_VERSION, clockId: CLOCK_ID, revision: 0, status: 0, error: null };
    this.clockReady = { kind: "clock-ready", generation: this.generation,
      clockVersion: CLOCK_VERSION, clockId: CLOCK_ID, revision: 0, origin: 0 };
    this.clockRefill = { kind: "clock-refill", generation: this.generation,
      clockVersion: CLOCK_VERSION, clockId: CLOCK_ID, revision: 0,
      from0: 0, from1: 0, from2: 0, to0: 0, to1: 0, to2: 0,
      frames: 0, endLow: 0, endHigh: 0 };
    // Preallocated because even terminal process paths must not build objects.
    this.terminal = {
      kind: "terminal", generation: this.generation, status: 0,
      diagnosticVersion: AUDIO_FAILURE_DIAGNOSTIC_VERSION,
      origin: AUDIO_FAILURE_ORIGIN_CONTROL, ownerPhase: 0,
      currentFramePresent: 0, currentFrame: 0,
      blockFramesPresent: 0, blockFrames: 0,
      expectedFramePresent: 0, expectedFrameLow: 0, expectedFrameHigh: 0,
      startFramePresent: 0, startFrameLow: 0, startFrameHigh: 0,
      successfulArmFramePresent: 0, successfulArmFrame: 0,
    };
    liveOwner = this;
    try {
      initSync({ module: config.module });
      const pcm = config.pcmLimits;
      const audio = config.audioLimits;
      this.owner = new BrowserAudio(sampleRate, config.channels,
        pcm.maxAssetBytes, pcm.maxTotalBytes, pcm.maxSamples,
        audio.queueCapacity, audio.maxVoices, audio.pendingCapacity,
        audio.maxFrames, audio.maxCommandsPerRender);
      if (this.owner.status() !== 0) throw new Error("Audio processor allocation or configuration failed.");
      if (this.owner.report_fields !== REPORT_FIELDS) throw new Error("Incompatible audio processor report format.");
      if (!["native_clock_start", "native_clock_extend", "native_frame", "native_clock_ready", "arm_words"]
        .every(name => typeof this.owner[name] === "function")) {
        throw new Error("Incompatible native audio clock bindings.");
      }
      this.port.onmessage = event => this.control(event.data);
      this.port.postMessage({ kind: "ready", generation: this.generation, sampleRate, channels: this.channels });
    } catch (error) {
      try {
        if (this.owner !== null) this.owner.free();
      } finally {
        this.owner = null;
        liveOwner = null;
      }
      throw error;
    }
  }

  captureFailure(status, origin, frame, framePresent, frames, framesPresent) {
    this.failed = true;
    if (!this.diagnosed) {
      this.diagnosed = true;
      const terminal = this.terminal;
      terminal.status = status;
      terminal.origin = origin;
      terminal.ownerPhase = this.phase;
      terminal.currentFrame = frame;
      terminal.currentFramePresent = framePresent;
      terminal.blockFrames = frames;
      terminal.blockFramesPresent = framesPresent;
      // A presence flag is committed only after both native words succeed.
      // A getter failure preserves the original status and remaining absence.
      try {
        if (this.owner !== null && this.owner.report_word(23, false) === 1) {
          const low = this.owner.report_word(24, false);
          const high = this.owner.report_word(24, true);
          if (integer(low, 0, 0xffffffff) && integer(high, 0, 0xffffffff)) {
            terminal.expectedFrameLow = low;
            terminal.expectedFrameHigh = high;
            terminal.expectedFramePresent = 1;
          }
        }
        if (this.owner !== null && this.owner.report_word(25, false) === 1) {
          const low = this.owner.report_word(26, false);
          const high = this.owner.report_word(26, true);
          if (integer(low, 0, 0xffffffff) && integer(high, 0, 0xffffffff)) {
            terminal.startFrameLow = low;
            terminal.startFrameHigh = high;
            terminal.startFramePresent = 1;
          }
        }
      } catch {}
      this.terminalReady = true;
    }
  }

  fence(status, origin = AUDIO_FAILURE_ORIGIN_CONTROL, frame = 0, framePresent = 0, frames = 0, framesPresent = 0) {
    this.captureFailure(status, origin, frame, framePresent, frames, framesPresent);
    this.cancelArm(status, "fenced");
    this.clockPendingRevision = 0;
    if (this.terminalReady && !this.terminalPosted) {
      this.terminalPosted = true;
      const commandPort = this.commandPort;
      const samplePort = this.samplePort;
      try { this.port.postMessage(this.terminal); } catch {}
      try { commandPort?.postMessage(this.terminal); } catch {}
      try { samplePort?.postMessage(this.terminal); } catch {}
    }
  }

  cancelArm(status, error) {
    const pending = this.pendingArm;
    if (pending === null) return true;
    this.pendingArm = null;
    pending.status = status;
    pending.error = error;
    if (status !== 0 && this.diagnosed) pending.diagnostics = this.terminal;
    try { this.port.postMessage(pending); return true; } catch { return false; }
  }

  clockReply(reply, revision, status, error) {
    reply.revision = integer(revision, 0, Number.MAX_SAFE_INTEGER) ? revision : 0;
    reply.status = status;
    reply.error = error;
    this.port.postMessage(reply);
  }

  rejectClock(message, status, error) {
    this.captureFailure(status, AUDIO_FAILURE_ORIGIN_CONTROL, 0, 0, 0, 0);
    const reply = message?.kind === "clock-reserve" ? this.clockReserved : this.clockCommitted;
    try { this.clockReply(reply, message?.revision, status, error); } catch {}
    this.fence(status);
  }

  clockControl(message) {
    try {
      const generation = message.generation;
      if (integer(generation, 1, Number.MAX_SAFE_INTEGER) && generation !== this.generation) return;
      const kind = message.kind;
      const version = message.clockVersion;
      const id = message.clockId;
      const revision = message.revision;
      const origin = message.origin;
      const h0 = message.h0;
      const h1 = message.h1;
      const h2 = message.h2;
      const futureEligible = message.futureEligible;
      if (this.failed || this.phase === 3) return;
      if (generation !== this.generation || version !== CLOCK_VERSION || id !== CLOCK_ID
        || !integer(revision, 0, Number.MAX_SAFE_INTEGER)
        || !integer(origin, 1, Number.MAX_SAFE_INTEGER)
        || !integer(h0, origin + 1, Number.MAX_SAFE_INTEGER)
        || !integer(h1, origin + 1, Number.MAX_SAFE_INTEGER)
        || !integer(h2, origin + 1, Number.MAX_SAFE_INTEGER)) {
        this.rejectClock(message, INVALID, "clock-identity-or-geometry");
        return;
      }
      if (kind === "clock-reserve") {
        const geometry = clockGeometry(origin, this.maxFrames, sampleRate);
        if (this.clockStage !== 0 || this.phase !== 0 || revision !== 0 || geometry === null
          || h0 !== geometry.h0 || h1 !== geometry.h1 || h2 !== geometry.h2) {
          this.rejectClock(message, STATE, "clock-reserve");
          return;
        }
        this.clockOrigin = origin;
        this.clockH0 = h0;
        this.clockH1 = h1;
        this.clockH2 = h2;
        this.clockLowWater = geometry.lowWater;
        this.clockStage = 1;
        this.clockReady.origin = origin;
        this.clockReply(this.clockReserved, revision, 0, null);
        return;
      }
      if (kind !== "clock-commit" || futureEligible !== true || origin !== this.clockOrigin) {
        this.rejectClock(message, INVALID, "clock-commit");
        return;
      }
      let status;
      if (this.clockStage === 1 && revision === 0 && this.phase === 0
        && h0 === this.clockH0 && h1 === this.clockH1 && h2 === this.clockH2) {
        status = this.owner.native_clock_start(origin % WORD, Math.floor(origin / WORD),
          h0 % WORD, Math.floor(h0 / WORD), h1 % WORD, Math.floor(h1 / WORD),
          h2 % WORD, Math.floor(h2 / WORD));
        if (status === 0) this.clockStage = 2;
      } else if (this.clockStage === 4 && this.clockPendingRevision !== 0
        && revision === this.clockPendingRevision && revision === this.clockRevision + 1
        && h0 === this.clockRefill.to0 && h1 === this.clockRefill.to1 && h2 === this.clockRefill.to2
        && this.clockEnd < Math.min(this.clockH0, this.clockH1, this.clockH2)
        && markerTime(h0, sampleRate) !== null && markerTime(h1, sampleRate) !== null
        && markerTime(h2, sampleRate) !== null) {
        status = this.owner.native_clock_extend(h0 % WORD, Math.floor(h0 / WORD),
          h1 % WORD, Math.floor(h1 / WORD), h2 % WORD, Math.floor(h2 / WORD));
        if (status === 0) {
          this.clockH0 = h0;
          this.clockH1 = h1;
          this.clockH2 = h2;
          this.clockRevision = revision;
          this.clockPendingRevision = 0;
        }
      } else {
        this.rejectClock(message, STATE, "clock-commit-state");
        return;
      }
      if (!integer(status, 0, 0xffffffff) || status !== 0) {
        this.rejectClock(message, integer(status, 1, 0xffffffff) ? status : EXCEPTION, "clock-audio");
        return;
      }
      this.clockReply(this.clockCommitted, revision, 0, null);
    } catch {
      if (!this.failed && this.phase !== 3) this.rejectClock(message, EXCEPTION, "clock-exception");
    }
  }

  ack(message, status, admitted = 0, error = null, report = null, port = this.port) {
    const acknowledgement = { kind: "ack", generation: this.generation,
      sequence: Number.isSafeInteger(message?.sequence) ? message.sequence : null,
      operation: typeof message?.kind === "string" ? message.kind : null,
      status, admitted, error, report };
    if (status !== 0) acknowledgement.diagnostics = this.terminal;
    port.postMessage(acknowledgement);
  }

  reject(message, status, error, admitted = 0, port = this.port,
    origin = AUDIO_FAILURE_ORIGIN_CONTROL, frame = 0, framePresent = 0) {
    // Preserve ACK admission ordering while latching before any port callback.
    this.captureFailure(status, origin, frame, framePresent, 0, 0);
    // A refused transfer still owns its received endpoint, not a live producer.
    if ((message?.kind === "attach-commands" || message?.kind === "attach-samples")
      && message.port !== this.commandPort && message.port !== this.samplePort && message.port !== this.port) {
      try { message.port?.close(); } catch {}
    }
    try { this.ack(message, status, admitted, error, null, port); } catch {}
    this.fence(status, origin, frame, framePresent);
  }

  closeCommandPort() {
    const port = this.commandPort;
    if (port === null) return;
    this.commandPort = null;
    this.closeTransferredPort(port, true);
  }

  closeSamplePort(notify = true) {
    const port = this.samplePort;
    if (port === null) return;
    this.samplePort = null;
    this.closeTransferredPort(port, notify);
  }

  closeTransferredPort(port, notify) {
    let failure = null;
    if (notify) {
      try { port.postMessage({ kind: "closed", generation: this.generation }); } catch (error) { failure = { error }; }
    }
    try { port.onmessage = null; } catch (error) { failure ??= { error }; }
    try { port.onmessageerror = null; } catch (error) { failure ??= { error }; }
    try { port.close(); } catch (error) { failure ??= { error }; }
    if (failure) throw failure.error;
  }

  stop(message) {
    this.phase = 3;
    this.cancelArm(STATE, "stopped");
    this.clockPendingRevision = 0;
    // Control handlers, never process(), own deallocation. Stop is also the
    // cleanup route after a malformed command or a terminal render failure.
    this.output = null;
    this.buffer = null;
    this.memory = null;
    this.sampleIds.clear();
    let failure = null;
    try {
      if (this.owner !== null) {
        this.owner.free();
        this.owner = null;
      }
    } catch (error) {
      failure = { error };
    }
    try { this.closeSamplePort(); } catch (error) { failure ??= { error }; }
    try { this.closeCommandPort(); } catch (error) { failure ??= { error }; }
    if (this.owner === null && liveOwner === this) liveOwner = null;
    if (failure) {
      this.fence(EXCEPTION);
      throw failure.error;
    }
    this.ack(message, 0);
    this.port.onmessage = null;
    this.port.close();
  }

  commands(message, port) {
    let admitted = 0;
    try {
      const commands = message.commands;
      if ((this.phase !== 1 && this.phase !== 2) || !Array.isArray(commands)
        || commands.length === 0 || commands.length > this.maxBatch) {
        this.reject(message, INVALID, "commands", 0, port);
        return;
      }
      // Both producers use this whole-batch preflight and exact admission loop.
      for (const command of commands) {
        if (!validCommand(command)) {
          this.reject(message, INVALID, "command", 0, port);
          return;
        }
      }
      for (const command of commands) {
        const status = this.owner.enqueue(command.kind, command.voice, command.sample,
          command.at, command.gain, command.value, command.denominator);
        if (status !== 0) {
          this.reject(message, status, "admission", admitted, port);
          return;
        }
        admitted++;
      }
      this.ack(message, 0, admitted, null, null, port);
    } catch {
      this.reject(message, EXCEPTION, "exception", admitted, port);
    }
  }

  poll(message, port) {
    // Each pair is low/high u32 for the binding's documented report index.
    // No report is synthesized before the first actual Mixer render.
    const words = new Uint32Array(REPORT_FIELDS * 2);
    for (let index = 0; index < REPORT_FIELDS; index++) {
      words[index * 2] = this.owner.report_word(index, false);
      words[index * 2 + 1] = this.owner.report_word(index, true);
    }
    this.ack(message, 0, 0, null, { available: words[0] !== 0, words }, port);
  }

  commandControl(message) {
    const port = this.commandPort;
    if (port === null) return;
    try {
      if (message === null || typeof message !== "object"
        || (message.kind !== "commands" && message.kind !== "poll")) {
        this.reject(message, INVALID, "command-operation", 0, port);
        return;
      }
      if (message.generation !== this.generation) {
        this.reject(message, GENERATION, "generation", 0, port);
        return;
      }
      if (!integer(message.sequence, 1, Number.MAX_SAFE_INTEGER) || message.sequence !== this.commandSequence + 1) {
        this.reject(message, SEQUENCE, "sequence", 0, port);
        return;
      }
      this.commandSequence = message.sequence;
      if (this.failed || this.phase === 3) {
        this.reject(message, STATE, "fenced", 0, port);
        return;
      }
      if (message.kind === "commands") this.commands(message, port);
      else this.poll(message, port);
    } catch {
      this.reject(message, EXCEPTION, "exception", 0, port);
    }
  }

  sample(message, port = this.port) {
    try {
      const pcm = message.pcm;
      if (this.phase !== 0) {
        this.reject(message, STATE, "sample-state", 0, port);
        return;
      }
      if (!unsigned(message.id) || !integer(message.rate, 1, 0xffffffff)
        || message.channels !== this.channels || !(pcm instanceof Float32Array)
        || !(pcm.buffer instanceof ArrayBuffer) || pcm.buffer.resizable === true
        || pcm.byteOffset !== 0 || pcm.byteLength !== pcm.buffer.byteLength
        || pcm.length % this.channels !== 0 || pcm.byteLength > this.pcmLimits.maxAssetBytes
        || pcm.byteLength > this.pcmLimits.maxTotalBytes - this.pcmBytes
        || this.sampleIds.size >= this.pcmLimits.maxSamples || this.sampleIds.has(message.id)) {
        this.reject(message, INVALID, "sample", 0, port);
        return;
      }
      try { new Float32Array(pcm.buffer, 0, 0); } catch {
        this.reject(message, INVALID, "sample", 0, port);
        return;
      }
      // Both producers use this finite preflight before a binding allocation.
      for (let index = 0; index < pcm.length; index++) {
        if (!Number.isFinite(pcm[index])) {
          this.reject(message, INVALID, "sample-pcm", 0, port);
          return;
        }
      }
      const status = this.owner.insert_sample(message.id, message.rate, message.channels, pcm);
      if (status !== 0) {
        this.reject(message, status, "audio", 0, port);
        return;
      }
      this.sampleIds.add(message.id);
      this.pcmBytes += pcm.byteLength;
      this.ack(message, 0, 0, null, null, port);
    } catch {
      this.reject(message, EXCEPTION, "exception", 0, port);
    }
  }

  sampleControl(message, port) {
    // A saved handler from an ended/stopped endpoint has no current authority.
    if (port !== this.samplePort) return;
    try {
      if (!message || typeof message !== "object"
        || (message.kind !== "sample" && message.kind !== "end-samples")) {
        this.reject(message, INVALID, "sample-operation", 0, port);
        return;
      }
      if (message.generation !== this.generation) {
        this.reject(message, GENERATION, "generation", 0, port);
        return;
      }
      if (!integer(message.sequence, 1, Number.MAX_SAFE_INTEGER) || message.sequence !== this.sampleSequence + 1) {
        this.reject(message, SEQUENCE, "sequence", 0, port);
        return;
      }
      this.sampleSequence = message.sequence;
      if (this.failed || this.phase !== 0 || this.samplesEnded) {
        this.reject(message, STATE, "fenced", 0, port);
        return;
      }
      if (message.kind === "sample") {
        this.sample(message, port);
        return;
      }
      if (!integer(message.count, 0, this.pcmLimits.maxSamples)
        || !integer(message.bytes, 0, this.pcmLimits.maxTotalBytes)
        || message.count !== this.sampleIds.size || message.bytes !== this.pcmBytes) {
        this.reject(message, INVALID, "sample-end", 0, port);
        return;
      }
      this.ack(message, 0, 0, null, null, port);
      this.closeSamplePort(false);
      this.samplesEnded = true;
    } catch {
      this.reject(message, EXCEPTION, "exception", 0, port);
    }
  }

  control(message) {
    let admitted = 0;
    let origin = AUDIO_FAILURE_ORIGIN_CONTROL;
    let frame = 0;
    let framePresent = 0;
    try {
      if (message === null || typeof message !== "object" || typeof message.kind !== "string") {
        this.reject(message, INVALID, "message");
        return;
      }
      if (message.kind.startsWith("clock-")) {
        this.clockControl(message);
        return;
      }
      if (message.kind === "arm") origin = AUDIO_FAILURE_ORIGIN_ARM;
      if (message.generation !== this.generation) {
        this.reject(message, GENERATION, "generation", 0, this.port, origin, frame, framePresent);
        return;
      }
      // A fenced owner still accepts explicit cleanup. Its sequence must be
      // fresh; it need not fill a gap that was itself the cause of fencing.
      if (!integer(message.sequence, 1, Number.MAX_SAFE_INTEGER)
        || (this.failed && message.kind === "stop" ? message.sequence <= this.sequence : message.sequence !== this.sequence + 1)) {
        this.reject(message, SEQUENCE, "sequence", 0, this.port, origin, frame, framePresent);
        return;
      }
      this.sequence = message.sequence;
      if (message.kind === "stop") {
        this.stop(message);
        return;
      }
      if (this.failed || this.phase === 3) {
        this.reject(message, STATE, "fenced", 0, this.port, origin, frame, framePresent);
        return;
      }
      let status = 0;
      if (message.kind === "sample") {
        if (this.samplesTransferred) {
          this.reject(message, STATE, "transferred-sample-owner");
          return;
        }
        this.sample(message);
        return;
      } else if (message.kind === "finish") {
        if (this.phase !== 0 || (this.samplesTransferred && !this.samplesEnded)) {
          this.reject(message, STATE, "finish-state");
          return;
        }
        const endFrame = message.endFrame;
        if (endFrame !== undefined && !unsigned(endFrame)) {
          this.reject(message, INVALID, "finish-end");
          return;
        }
        status = endFrame === undefined ? this.owner.finish() : this.owner.finish_at(endFrame);
        if (status === 0) {
          this.memory = BrowserAudio.memory();
          this.buffer = this.memory.buffer;
          const pointer = this.owner.output_ptr();
          const length = this.owner.output_len();
          if (this.owner.channels() !== this.channels || this.owner.max_frames() !== this.maxFrames
            || !integer(pointer, 0, 0xffffffff) || pointer % 4 !== 0
            || length !== this.maxFrames * this.channels || pointer + length * 4 > this.buffer.byteLength) {
            this.reject(message, MEMORY, "output-storage");
            return;
          }
          this.output = new Float32Array(this.buffer, pointer, length);
          this.phase = 1;
        }
      } else if (message.kind === "attach-samples") {
        const port = message.port;
        if (this.phase !== 0 || this.samplesTransferred || this.sampleIds.size !== 0
          || !port || port === this.port || port === this.commandPort
          || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")) {
          this.reject(message, STATE, "sample-port");
          return;
        }
        this.samplesTransferred = true;
        this.samplePort = port;
        port.onmessage = event => this.sampleControl(event.data, port);
        port.onmessageerror = () => { if (this.samplePort === port) this.fence(INVALID); };
        port.start();
      } else if (message.kind === "attach-commands") {
        const port = message.port;
        if (this.phase !== 1 || this.commandPort !== null || !port || port === this.port
          || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")) {
          this.reject(message, STATE, "command-port");
          return;
        }
        this.commandPort = port;
        port.onmessage = event => this.commandControl(event.data);
        port.onmessageerror = () => this.fence(INVALID);
        port.start();
      } else if (message.kind === "arm") {
        if (this.phase !== 1 || this.pendingArm !== null || !unsigned(message.frame)) {
          this.reject(message, INVALID, "arm", 0, this.port, origin, frame, framePresent);
          return;
        }
        this.pendingStartLow = Number(message.frame & 4294967295n);
        this.pendingStartHigh = Number(message.frame >> 32n);
        this.pendingArm = { kind: "ack", generation: this.generation, sequence: message.sequence,
          operation: "arm", status: 0, admitted: 0, error: null, report: null, diagnostics: undefined };
        return;
      } else if (message.kind === "commands") {
        if (this.commandPort !== null) {
          this.reject(message, STATE, "transferred-command-owner");
          return;
        }
        this.commands(message, this.port);
        return;
      } else if (message.kind === "poll") {
        this.poll(message, this.port);
        return;
      } else {
        this.reject(message, INVALID, "operation");
        return;
      }
      if (status !== 0) this.reject(message, status, "audio", 0, this.port, origin, frame, framePresent);
      else this.ack(message, 0);
    } catch {
      this.reject(message, EXCEPTION, "exception", admitted, this.port, origin, frame, framePresent);
    }
  }

  silence(outputs) {
    for (let bus = 0; bus < outputs.length; bus++) {
      for (let channel = 0; channel < outputs[bus].length; channel++) outputs[bus][channel].fill(0);
    }
  }

  process(_inputs, outputs, parameters) {
    // Original native arrays and scalar WASM calls only: no callback-owned
    // objects, BigInts, views, formatted strings or owner destruction.
    this.silence(outputs);
    if (this.failed || this.phase === 3) return false;
    let frame = 0;
    let framePresent = 0;
    let frames = 0;
    let framesPresent = 0;
    let failureOrigin = AUDIO_FAILURE_ORIGIN_PROCESS;
    try {
      if (outputs.length > 0 && outputs[0].length > 0) {
        const actualFrames = outputs[0][0].length;
        if (integer(actualFrames, 0, 0xffffffff)) {
          frames = actualFrames;
          framesPresent = 1;
        }
      }
      if (outputs.length !== 1 || outputs[0].length !== this.channels
        || framesPresent !== 1 || frames > this.maxFrames) {
        this.fence(LAYOUT, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      for (let channel = 0; channel < this.channels; channel++) {
        if (outputs[0][channel].length !== frames) {
          this.fence(LAYOUT, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
          return false;
        }
      }
      if (this.phase !== 0 && (this.memory.buffer !== this.buffer
        || this.output.byteLength !== this.maxFrames * this.channels * 4)) {
        this.fence(MEMORY, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      if (frames === 0) return true;
      const marker = parameters?.[CLOCK_NAMES[0]];
      const lane0 = parameters?.[CLOCK_NAMES[1]];
      const lane1 = parameters?.[CLOCK_NAMES[2]];
      const lane2 = parameters?.[CLOCK_NAMES[3]];
      let firstIndex = 0;
      if (this.clockStage < 3) {
        if (zeroLane(marker, frames) && zeroLane(lane0, frames)
          && zeroLane(lane1, frames) && zeroLane(lane2, frames)) {
          if (!this.clockProbePosted) {
            this.clockProbePosted = true;
            this.clockProbe.frames = frames;
            this.port.postMessage(this.clockProbe);
          }
          return !this.failed && this.phase !== 3;
        }
        if (this.clockStage !== 2 || !(marker instanceof Float32Array) || marker.length !== frames) {
          this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
          return false;
        }
        while (firstIndex < frames && marker[firstIndex] === 0) firstIndex++;
        if (firstIndex === 0 || firstIndex === frames) {
          this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
          return false;
        }
        for (let index = firstIndex; index < frames; index++) {
          if (marker[index] !== 1) {
            this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
            return false;
          }
        }
      } else if (!oneMarker(marker, frames)) {
        this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      if (!(lane0 instanceof Float32Array) || lane0.length !== frames
        || !(lane1 instanceof Float32Array) || lane1.length !== frames
        || !(lane2 instanceof Float32Array) || lane2.length !== frames) {
        this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      const first0 = nativePhase(lane0[firstIndex], CLOCK_PERIODS[0]);
      const first1 = nativePhase(lane1[firstIndex], CLOCK_PERIODS[1]);
      const first2 = nativePhase(lane2[firstIndex], CLOCK_PERIODS[2]);
      const last0 = nativePhase(lane0[frames - 1], CLOCK_PERIODS[0]);
      const last1 = nativePhase(lane1[frames - 1], CLOCK_PERIODS[1]);
      const last2 = nativePhase(lane2[frames - 1], CLOCK_PERIODS[2]);
      if (first0 === null || first1 === null || first2 === null
        || last0 === null || last1 === null || last2 === null
        || (firstIndex !== 0 && (first0 !== 0 || first1 !== 0 || first2 !== 0))) {
        this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      const candidate = this.owner.native_frame(first0, first1, first2,
        last0, last1, last2, firstIndex, frames);
      if (!integer(candidate, 0, Number.MAX_SAFE_INTEGER) || !Number.isSafeInteger(candidate + frames)) {
        const status = this.owner.status();
        this.fence(integer(status, 1, 0xffffffff) ? status : FRAME,
          AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      if (firstIndex !== 0 && candidate + firstIndex !== this.clockOrigin) {
        this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      if (!validClockLane(lane0, frames, candidate, this.clockOrigin, this.clockH0,
        CLOCK_PERIODS[0], sampleRate, firstIndex)
        || !validClockLane(lane1, frames, candidate, this.clockOrigin, this.clockH1,
          CLOCK_PERIODS[1], sampleRate, firstIndex)
        || !validClockLane(lane2, frames, candidate, this.clockOrigin, this.clockH2,
          CLOCK_PERIODS[2], sampleRate, firstIndex)) {
        this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
        return false;
      }
      frame = candidate;
      framePresent = 1;
      this.clockEnd = frame + frames;
      if (this.clockStage === 2) {
        this.clockStage = 3;
        return true; // First partial callback never renders or commits an arm.
      }
      if (this.clockStage === 3) {
        if (this.owner.native_clock_ready() !== true) {
          this.fence(STATE, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
          return false;
        }
        this.clockStage = 4;
        this.port.postMessage(this.clockReady);
        if (this.failed || this.phase === 3) return false;
      }
      if (this.pendingArm !== null) {
        failureOrigin = AUDIO_FAILURE_ORIGIN_ARM;
        const status = this.owner.arm_words(this.pendingStartLow, this.pendingStartHigh,
          frame % WORD, Math.floor(frame / WORD));
        if (status !== 0) {
          this.captureFailure(integer(status, 1, 0xffffffff) ? status : EXCEPTION,
            AUDIO_FAILURE_ORIGIN_ARM, frame, framePresent, frames, framesPresent);
          this.cancelArm(this.terminal.status, "audio");
          this.fence(this.terminal.status, AUDIO_FAILURE_ORIGIN_ARM, frame, framePresent, frames, framesPresent);
          return false;
        }
        this.phase = 2;
        this.terminal.successfulArmFrame = frame;
        this.terminal.successfulArmFramePresent = 1;
        if (!this.cancelArm(0, null)) {
          this.fence(EXCEPTION, AUDIO_FAILURE_ORIGIN_ARM, frame, framePresent, frames, framesPresent);
          return false;
        }
        if (this.failed || this.phase === 3) return false;
        failureOrigin = AUDIO_FAILURE_ORIGIN_PROCESS;
      }
      if (this.phase !== 0) {
        const status = this.owner.render(frame % WORD, Math.floor(frame / WORD), frames);
        if (status !== 0) {
          this.fence(status, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
          return false;
        }
        if (this.memory.buffer !== this.buffer) {
          this.fence(MEMORY, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
          return false;
        }
      }
      if (this.clockPendingRevision === 0
        && Math.min(this.clockH0, this.clockH1, this.clockH2) - this.clockEnd <= this.clockLowWater) {
        const nextRevision = this.clockRevision + 1;
        const to0 = this.clockH0 + CLOCK_PERIODS[0];
        const to1 = this.clockH1 + CLOCK_PERIODS[1];
        const to2 = this.clockH2 + CLOCK_PERIODS[2];
        if (!integer(nextRevision, 1, Number.MAX_SAFE_INTEGER)
          || !integer(to0, 1, Number.MAX_SAFE_INTEGER) || !integer(to1, 1, Number.MAX_SAFE_INTEGER)
          || !integer(to2, 1, Number.MAX_SAFE_INTEGER) || markerTime(to0, sampleRate) === null
          || markerTime(to1, sampleRate) === null || markerTime(to2, sampleRate) === null) {
          this.fence(FRAME, AUDIO_FAILURE_ORIGIN_PROCESS, frame, framePresent, frames, framesPresent);
          return false;
        }
        this.clockPendingRevision = nextRevision;
        const refill = this.clockRefill;
        refill.revision = nextRevision;
        refill.from0 = this.clockH0;
        refill.from1 = this.clockH1;
        refill.from2 = this.clockH2;
        refill.to0 = to0;
        refill.to1 = to1;
        refill.to2 = to2;
        refill.frames = frames;
        refill.endLow = this.clockEnd % WORD;
        refill.endHigh = Math.floor(this.clockEnd / WORD);
        this.port.postMessage(refill);
        if (this.failed || this.phase === 3) return false;
      }
      if (this.phase !== 0) {
        for (let channel = 0; channel < this.channels; channel++) {
          for (let index = 0; index < frames; index++) {
            outputs[0][channel][index] = this.output[index * this.channels + channel];
          }
        }
      }
      return true;
    } catch {
      this.silence(outputs);
      this.captureFailure(EXCEPTION, failureOrigin, frame, framePresent, frames, framesPresent);
      this.cancelArm(EXCEPTION, "exception");
      this.fence(EXCEPTION, failureOrigin, frame, framePresent, frames, framesPresent);
      return false;
    }
  }
}

registerProcessor("beatkernel-audio", BeatKernelAudioProcessor);

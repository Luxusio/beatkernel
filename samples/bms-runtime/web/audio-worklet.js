import "./worklet-encoding.mjs";
import { initSync, BrowserAudio } from "./audio-pkg/beatkernel_bms_runtime.js";

// The generated glue owns module-global WASM state. A second live processor
// must never initialize or replace it. Only an acknowledged stop releases it.
let liveOwner = null;

const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const WORD = 4294967296;
const REPORT_FIELDS = 28;
const INVALID = 100;
const SEQUENCE = 101;
const GENERATION = 102;
const STATE = 103;
const LAYOUT = 104;
const MEMORY = 105;
const EXCEPTION = 106;
const FRAME = 107;

function integer(value, min, max) {
  return Number.isSafeInteger(value) && value >= min && value <= max;
}

function unsigned(value) {
  return typeof value === "bigint" && value >= 0n && value <= U64_MAX;
}

function signed(value) {
  return typeof value === "bigint" && value >= I64_MIN && value <= I64_MAX;
}

function validCommand(command) {
  return command !== null && typeof command === "object"
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
// sample: {id: bigint, rate, channels, pcm: Float32Array}; finish; arm: {frame: bigint};
// commands: {commands: [{kind, voice, sample, at, gain, value, denominator}]}; poll; stop.
// Command IDs/denominator are u64 BigInt; at/value are i64 BigInt. No fields default.
// ACK admitted counts only this batch's exact successful queue prefix. A failed
// batch fences playback; neither its successful prefix nor game operations retry.
class BeatKernelAudioProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    if (liveOwner !== null) throw new Error("An audio processor already owns this worklet module.");
    const config = options?.processorOptions;
    if (!validOptions(config)) throw new Error("Invalid bounded audio processor options.");
    this.generation = config.generation;
    this.sequence = 0;
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
    this.memory = null;
    this.buffer = null;
    this.output = null;
    // Preallocated because even terminal process paths must not build objects.
    this.terminal = { kind: "terminal", generation: this.generation, status: 0 };
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

  fence(status) {
    this.failed = true;
    if (!this.diagnosed) {
      this.diagnosed = true;
      this.terminal.status = status;
      this.port.postMessage(this.terminal);
    }
  }

  ack(message, status, admitted = 0, error = null, report = null) {
    this.port.postMessage({ kind: "ack", generation: this.generation,
      sequence: Number.isSafeInteger(message?.sequence) ? message.sequence : null,
      operation: typeof message?.kind === "string" ? message.kind : null,
      status, admitted, error, report });
  }

  reject(message, status, error, admitted = 0) {
    this.ack(message, status, admitted, error);
    this.fence(status);
  }

  stop(message) {
    this.phase = 3;
    // Control handlers, never process(), own deallocation. Stop is also the
    // cleanup route after a malformed command or a terminal render failure.
    this.output = null;
    this.buffer = null;
    this.memory = null;
    this.sampleIds.clear();
    if (this.owner !== null) {
      this.owner.free();
      this.owner = null;
    }
    if (liveOwner === this) liveOwner = null;
    this.ack(message, 0);
    this.port.onmessage = null;
    this.port.close();
  }

  control(message) {
    let admitted = 0;
    try {
      if (message === null || typeof message !== "object" || typeof message.kind !== "string") {
        this.reject(message, INVALID, "message");
        return;
      }
      if (message.generation !== this.generation) {
        this.reject(message, GENERATION, "generation");
        return;
      }
      // A fenced owner still accepts explicit cleanup. Its sequence must be
      // fresh; it need not fill a gap that was itself the cause of fencing.
      if (!integer(message.sequence, 1, Number.MAX_SAFE_INTEGER)
        || (this.failed && message.kind === "stop" ? message.sequence <= this.sequence : message.sequence !== this.sequence + 1)) {
        this.reject(message, SEQUENCE, "sequence");
        return;
      }
      this.sequence = message.sequence;
      if (message.kind === "stop") {
        this.stop(message);
        return;
      }
      if (this.failed || this.phase === 3) {
        this.reject(message, STATE, "fenced");
        return;
      }
      let status = 0;
      if (message.kind === "sample") {
        const pcm = message.pcm;
        if (this.phase !== 0) {
          this.reject(message, STATE, "sample-state");
          return;
        }
        if (!unsigned(message.id) || !integer(message.rate, 1, 0xffffffff)
          || message.channels !== this.channels || !(pcm instanceof Float32Array)
          || pcm.length % this.channels !== 0 || pcm.byteLength > this.pcmLimits.maxAssetBytes
          || pcm.byteLength > this.pcmLimits.maxTotalBytes - this.pcmBytes
          || this.sampleIds.size >= this.pcmLimits.maxSamples || this.sampleIds.has(message.id)) {
          this.reject(message, INVALID, "sample");
          return;
        }
        // Avoid a binding allocation for malformed PCM. Rust also validates it.
        for (let index = 0; index < pcm.length; index++) {
          if (!Number.isFinite(pcm[index])) {
            this.reject(message, INVALID, "sample-pcm");
            return;
          }
        }
        status = this.owner.insert_sample(message.id, message.rate, message.channels, pcm);
        if (status === 0) {
          this.sampleIds.add(message.id);
          this.pcmBytes += pcm.byteLength;
        }
      } else if (message.kind === "finish") {
        if (this.phase !== 0) {
          this.reject(message, STATE, "finish-state");
          return;
        }
        status = this.owner.finish();
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
      } else if (message.kind === "arm") {
        if (this.phase !== 1 || !unsigned(message.frame) || !integer(currentFrame, 0, Number.MAX_SAFE_INTEGER)) {
          this.reject(message, INVALID, "arm");
          return;
        }
        status = this.owner.arm(message.frame, BigInt(currentFrame));
        if (status === 0) this.phase = 2;
      } else if (message.kind === "commands") {
        const commands = message.commands;
        if ((this.phase !== 1 && this.phase !== 2) || !Array.isArray(commands) || commands.length === 0 || commands.length > this.maxBatch) {
          this.reject(message, INVALID, "commands");
          return;
        }
        // Complete structural preflight precedes any admission, preserving the
        // meaning of admitted-prefix ACKs even for malformed later records.
        for (const command of commands) {
          if (!validCommand(command)) {
            this.reject(message, INVALID, "command");
            return;
          }
        }
        for (const command of commands) {
          status = this.owner.enqueue(command.kind, command.voice, command.sample,
            command.at, command.gain, command.value, command.denominator);
          if (status !== 0) {
            this.reject(message, status, "admission", admitted);
            return;
          }
          admitted++;
        }
        this.ack(message, 0, admitted);
        return;
      } else if (message.kind === "poll") {
        // Each pair is low/high u32 for the binding's documented report index.
        // No report is synthesized before the first actual Mixer render.
        const words = new Uint32Array(REPORT_FIELDS * 2);
        for (let index = 0; index < REPORT_FIELDS; index++) {
          words[index * 2] = this.owner.report_word(index, false);
          words[index * 2 + 1] = this.owner.report_word(index, true);
        }
        this.ack(message, 0, 0, null, { available: words[0] !== 0, words });
        return;
      } else {
        this.reject(message, INVALID, "operation");
        return;
      }
      if (status !== 0) this.reject(message, status, "audio");
      else this.ack(message, 0);
    } catch {
      this.reject(message, EXCEPTION, "exception", admitted);
    }
  }

  silence(outputs) {
    for (let bus = 0; bus < outputs.length; bus++) {
      for (let channel = 0; channel < outputs[bus].length; channel++) outputs[bus][channel].fill(0);
    }
  }

  process(_inputs, outputs) {
    // No allocation, BigInt conversion, string formatting, view creation or
    // owner destruction here. Browser messaging/GC still has no hard deadline.
    this.silence(outputs);
    if (this.failed || this.phase === 3) return false;
    try {
      if (outputs.length !== 1 || outputs[0].length !== this.channels) {
        this.fence(LAYOUT);
        return false;
      }
      const frames = outputs[0][0].length;
      if (frames > this.maxFrames) {
        this.fence(LAYOUT);
        return false;
      }
      for (let channel = 0; channel < this.channels; channel++) {
        if (outputs[0][channel].length !== frames) {
          this.fence(LAYOUT);
          return false;
        }
      }
      if (this.phase === 0) return true;
      if (this.memory.buffer !== this.buffer || this.output.byteLength !== this.maxFrames * this.channels * 4) {
        this.fence(MEMORY);
        return false;
      }
      if (!integer(currentFrame, 0, Number.MAX_SAFE_INTEGER) || !Number.isSafeInteger(currentFrame + frames)) {
        this.fence(FRAME);
        return false;
      }
      const status = this.owner.render(currentFrame % WORD, Math.floor(currentFrame / WORD), frames);
      if (status !== 0) {
        this.fence(status);
        return false;
      }
      if (this.memory.buffer !== this.buffer) {
        this.fence(MEMORY);
        return false;
      }
      for (let channel = 0; channel < this.channels; channel++) {
        for (let frame = 0; frame < frames; frame++) outputs[0][channel][frame] = this.output[frame * this.channels + channel];
      }
      return true;
    } catch {
      this.silence(outputs);
      this.fence(EXCEPTION);
      return false;
    }
  }
}

registerProcessor("beatkernel-audio", BeatKernelAudioProcessor);

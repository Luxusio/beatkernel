// One Worker-owned command producer. Host stop alone proves processor cleanup.
import { readAudioFailureDiagnostics } from "./audio-failure.mjs";
const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const FIELDS = ["kind", "voice", "sample", "at", "gain", "value", "denominator"];
const integer = (value, min, max) => Number.isSafeInteger(value) && value >= min && value <= max;
const unsigned = value => typeof value === "bigint" && value >= 0n && value <= U64_MAX;
const signed = value => typeof value === "bigint" && value >= I64_MIN && value <= I64_MAX;

function validReport(report) {
  const words = report?.words;
  if (typeof report?.available !== "boolean" || !(words instanceof Uint32Array)
    || !(words.buffer instanceof ArrayBuffer) || words.buffer.resizable === true
    || words.buffer.byteLength !== 224 || words.byteOffset !== 0
    || words.byteLength !== 224 || words.length !== 56) return false;
  // A detached or invalid backing must never become output evidence.
  try { new Uint32Array(words.buffer, 0, 56); } catch { return false; }
  return (words[0] === 0 || words[0] === 1) && words[1] === 0
    && report.available === (words[0] === 1);
}

function failure(code, message, generation = null, details = {}) {
  const error = new Error(message, details.cause === undefined ? undefined : { cause: details.cause });
  Object.assign(error, { code, generation, sequence: details.sequence ?? null,
    status: details.status ?? null, admitted: details.admitted ?? null,
    diagnostics: details.diagnostics ?? null });
  return error;
}

export class AudioCommandClient {
  #port;
  #generation;
  #capacity;
  #timeout;
  #sequence = 0;
  #pending = null;
  #state = "ready";
  #failure = null;

  constructor(options = {}) {
    const { port, generation, queueCapacity, timeoutMs } = options ?? {};
    if (!port || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")
      || !integer(generation, 1, Number.MAX_SAFE_INTEGER) || !integer(queueCapacity, 1, 65536)
      || !integer(timeoutMs, 1, 60000)) {
      throw failure("validation", "Invalid bounded command port configuration.");
    }
    this.#port = port;
    this.#generation = generation;
    this.#capacity = queueCapacity;
    this.#timeout = timeoutMs;
    try {
      port.onmessage = event => this.#message(event.data);
      port.onmessageerror = () => this.#dispose(this.#error("message", "Command response could not be decoded."));
      port.start();
    } catch (cause) {
      const error = this.#error("transport", "Command port could not start.", { cause });
      this.#dispose(error);
      throw error;
    }
  }

  get state() { return this.#state; }
  get failure() { return this.#failure; }

  #error(code, message, details = {}) {
    return failure(code, message, this.#generation, details);
  }

  #available() {
    if (this.#failure) throw this.#failure;
    if (this.#pending) throw this.#error("busy", "An audio port operation is already pending.");
    if (this.#sequence === Number.MAX_SAFE_INTEGER) {
      const error = this.#error("state", "Command sequence exhausted.");
      this.#dispose(error);
      throw error;
    }
  }

  async commands(commands) {
    this.#available();
    const count = Array.isArray(commands) ? commands.length : 0;
    if (!integer(count, 1, this.#capacity)) {
      throw this.#error("validation", "Command batch must be nonempty and bounded.");
    }
    const snapshot = [];
    for (let index = 0; index < count; index++) {
      const input = commands[index];
      if (!input || typeof input !== "object" || Reflect.ownKeys(input).length !== FIELDS.length
        || !FIELDS.every(field => Object.hasOwn(input, field))) {
        throw this.#error("validation", "Command records require exactly seven fields.");
      }
      const command = { kind: input.kind, voice: input.voice, sample: input.sample, at: input.at,
        gain: input.gain, value: input.value, denominator: input.denominator };
      if (!integer(command.kind, 0, 3) || !unsigned(command.voice) || !unsigned(command.sample)
        || !signed(command.at) || typeof command.gain !== "number" || !Number.isFinite(command.gain)
        || !Number.isFinite(Math.fround(command.gain)) || !signed(command.value) || !unsigned(command.denominator)) {
        throw this.#error("validation", "Command fields exceed the numeric protocol bounds.");
      }
      snapshot.push(command);
    }
    return this.#request("commands", snapshot.length, { commands: snapshot });
  }

  async poll() {
    return this.#request("poll", 0);
  }

  #request(operation, count, fields = {}) {
    // Reading caller fields may run getters. Never revive an owner they closed
    // or replace an operation admitted reentrantly during that snapshot.
    this.#available();
    let resolve;
    let reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    promise.catch(() => {});
    const sequence = ++this.#sequence;
    const pending = { sequence, operation, count, resolve, reject, timer: null };
    this.#pending = pending;
    pending.timer = setTimeout(() => {
      if (this.#pending === pending) this.#dispose(this.#error("timeout", "Audio port acknowledgement timed out.", { sequence }));
    }, this.#timeout);
    try {
      this.#port.postMessage({ kind: operation, generation: this.#generation, sequence, ...fields });
    } catch (cause) {
      this.#dispose(this.#error("transport", "Audio port operation could not be sent.", { sequence, cause }));
    }
    return promise;
  }

  #message(message) {
    if (this.#state !== "ready") return;
    const pending = this.#pending;
    const current = () => this.#state === "ready" && this.#pending === pending;
    const malformed = cause => {
      if (current()) this.#dispose(this.#error("protocol", "Malformed or uncorrelated command response.", { cause }));
    };
    if (!message || typeof message !== "object") { malformed(); return; }
    let fields;
    try {
      const generation = message.generation;
      if (integer(generation, 1, Number.MAX_SAFE_INTEGER) && generation !== this.#generation) return;
      fields = { generation, kind: message.kind, operation: message.operation, sequence: message.sequence,
        status: message.status, admitted: message.admitted, error: message.error, report: message.report };
    } catch (cause) { malformed(cause); return; }
    if (!current()) return;
    if (fields.generation !== this.#generation) { malformed(); return; }
    if (fields.kind === "closed") {
      this.#dispose(this.#error("closed", "The audio host stopped the command owner."), "closed");
      return;
    }
    if (fields.kind === "terminal") {
      if (!integer(fields.status, 1, 0xffffffff)) { malformed(); return; }
      let diagnostics;
      try { diagnostics = readAudioFailureDiagnostics(message); }
      catch (cause) { malformed(cause); return; }
      if (!current()) return;
      this.#dispose(this.#error("processor", `Audio processor reported a terminal failure (status ${fields.status}).`,
        { status: fields.status, diagnostics }));
      return;
    }
    let valid;
    try {
      valid = fields.kind === "ack" && pending !== null && fields.operation === pending.operation
        && fields.sequence === pending.sequence && integer(fields.status, 0, 0xffffffff)
        && integer(fields.admitted, 0, pending.count)
        && (fields.error === null || (typeof fields.error === "string" && fields.error.length <= 4096))
        && (fields.status !== 0 || (fields.admitted === pending.count && fields.error === null))
        && (pending.operation === "poll" && fields.status === 0
          ? validReport(fields.report) : fields.report === null);
    } catch (cause) { malformed(cause); return; }
    if (!current()) return;
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
    if (fields.status !== 0) {
      this.#dispose(this.#error("remote", `Audio processor rejected ${pending.operation} (status ${fields.status})${fields.error ? `: ${fields.error}` : "."}`,
        { sequence: pending.sequence, status: fields.status, admitted: fields.admitted, diagnostics }));
      return;
    }
    this.#pending = null;
    clearTimeout(pending.timer);
    pending.resolve(pending.operation === "poll" ? fields.report : fields);
  }

  #dispose(error, state = "failed") {
    if (this.#state !== "ready") return;
    this.#failure = error;
    this.#state = state;
    const pending = this.#pending;
    this.#pending = null;
    if (pending) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    try { this.#port.onmessage = null; } catch {}
    try { this.#port.onmessageerror = null; } catch {}
    try { this.#port.close(); } catch {}
  }

  close() {
    this.#dispose(this.#error("closed", "Command client was closed."), "closed");
  }
}

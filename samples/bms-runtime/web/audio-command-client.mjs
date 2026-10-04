// One Worker-owned command producer. Host stop alone proves processor cleanup.
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
  Object.assign(error, { code, generation, sequence: null, status: null, admitted: null }, details);
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
    if (message !== null && typeof message === "object"
      && integer(message.generation, 1, Number.MAX_SAFE_INTEGER) && message.generation !== this.#generation) return;
    const malformed = () => this.#dispose(this.#error("protocol", "Malformed or uncorrelated command response."));
    if (!message || typeof message !== "object" || message.generation !== this.#generation) { malformed(); return; }
    if (message.kind === "closed") {
      this.#dispose(this.#error("closed", "The audio host stopped the command owner."), "closed");
      return;
    }
    if (message.kind === "terminal") {
      if (!integer(message.status, 1, 0xffffffff)) { malformed(); return; }
      this.#dispose(this.#error("processor", "Audio processor reported a terminal failure.", { status: message.status }));
      return;
    }
    const pending = this.#pending;
    if (message.kind !== "ack" || pending === null || message.operation !== pending.operation
      || message.sequence !== pending.sequence || !integer(message.status, 0, 0xffffffff)
      || !integer(message.admitted, 0, pending.count)
      || !(message.error === null || (typeof message.error === "string" && message.error.length <= 4096))
      || (message.status === 0 && (message.admitted !== pending.count || message.error !== null))
      || (pending.operation === "poll" && message.status === 0
        ? !validReport(message.report) : message.report !== null)) {
      malformed();
      return;
    }
    if (message.status !== 0) {
      this.#dispose(this.#error("remote", `Audio processor rejected ${pending.operation}${message.error ? `: ${message.error}` : "."}`,
        { sequence: pending.sequence, status: message.status, admitted: message.admitted }));
      return;
    }
    this.#pending = null;
    clearTimeout(pending.timer);
    pending.resolve(pending.operation === "poll" ? message.report : message);
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

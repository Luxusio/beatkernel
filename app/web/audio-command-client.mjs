// One Worker-owned command producer. Host stop alone proves processor cleanup.
import { readAudioFailureDiagnostics } from "./audio-failure.mjs";
const U64_MAX = 18446744073709551615n;
const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;
const FIELDS = ["kind", "voice", "sample", "at", "gain", "value", "denominator"];
const integer = (value, min, max) => Number.isSafeInteger(value) && value >= min && value <= max;
const unsigned = value => typeof value === "bigint" && value >= 0n && value <= U64_MAX;
const signed = value => typeof value === "bigint" && value >= I64_MIN && value <= I64_MAX;
const RETAINED_ACK_FIELDS = ["kind", "version", "generation", "sequence", "operation", "programId",
  "status", "admitted", "error", "terminal", "report", "retained"];
const exactFields = (value, fields) => value !== null && typeof value === "object"
  && Reflect.ownKeys(value).length === fields.length && fields.every(field => Object.hasOwn(value, field));
const word = (words, index) => BigInt(words[index * 2]) | (BigInt(words[index * 2 + 1]) << 32n);

function snapshotWords(words, count = undefined, buffers = null) {
  if (!(words instanceof Uint32Array)) throw new TypeError("Expected retained u32 words.");
  const buffer = words.buffer, length = words.length, offset = words.byteOffset, bytes = words.byteLength;
  if (!(buffer instanceof ArrayBuffer) || buffer.resizable === true || offset !== 0
    || !integer(length, 0, 0xffffffff) || bytes !== length * 4 || buffer.byteLength !== bytes
    || (count !== undefined && length !== count)) throw new TypeError("Invalid retained word extent.");
  if (buffers?.has(buffer)) throw new TypeError("Retained word buffers must be independent.");
  buffers?.add(buffer);
  return new Uint32Array(new Uint32Array(buffer, 0, length));
}

function retainedPollSnapshot(report, retained, programId) {
  if (!exactFields(report, ["available", "words"]) || !exactFields(retained, ["metadata", "history", "event"])) {
    throw new TypeError("Invalid retained poll fields.");
  }
  const available = report.available, reportWords = report.words;
  const metadataWords = retained.metadata, historyWords = retained.history, eventWords = retained.event;
  const buffers = new Set();
  const snapshot = { available, words: snapshotWords(reportWords, 58, buffers) };
  if (!validReport(snapshot)) throw new TypeError("Invalid retained output report.");
  const metadata = snapshotWords(metadataWords, 12, buffers), history = snapshotWords(historyWords, undefined, buffers);
  const event = eventWords === null ? null : snapshotWords(eventWords, 80, buffers);
  if (history.length < 20 || word(history, 0) !== 1n || word(history, 1) !== programId
    || word(history, 9) > 0xffffffffn || history.length !== 20 + Number(word(history, 9)) * 10
    || (event !== null && (word(event, 0) !== 1n || word(event, 2) !== programId))) {
    throw new TypeError("Invalid retained snapshot identity or extent.");
  }
  return { report: snapshot, retained: { metadata, history, event } };
}

function validReport(report) {
  const words = report?.words;
  if (typeof report?.available !== "boolean" || !(words instanceof Uint32Array)
    || !(words.buffer instanceof ArrayBuffer) || words.buffer.resizable === true
    || words.buffer.byteLength !== 232 || words.byteOffset !== 0
    || words.byteLength !== 232 || words.length !== 58) return false;
  // A detached or invalid backing must never become output evidence.
  try { new Uint32Array(words.buffer, 0, 58); } catch { return false; }
  return (words[0] === 0 || words[0] === 1) && words[1] === 0
    && report.available === (words[0] === 1);
}

function failure(code, message, generation = null, details = {}) {
  const error = new Error(message, details.cause === undefined ? undefined : { cause: details.cause });
  Object.assign(error, { code, generation, sequence: details.sequence ?? null,
    status: details.status ?? null, admitted: details.admitted ?? null,
    diagnostics: details.diagnostics ?? null });
  if (details.terminal !== undefined) error.terminal = details.terminal;
  return error;
}

export class AudioCommandClient {
  #port;
  #generation;
  #capacity;
  #timeout;
  #programId;
  #sequence = 0;
  #pending = null;
  #state = "ready";
  #failure = null;

  constructor(options = {}) {
    const { port, generation, queueCapacity, timeoutMs, programId } = options ?? {};
    if (!port || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")
      || !integer(generation, 1, Number.MAX_SAFE_INTEGER) || !integer(queueCapacity, 1, 65536)
      || !integer(timeoutMs, 1, 60000) || (programId !== undefined && (!unsigned(programId) || programId === 0n))) {
      throw failure("validation", "Invalid bounded command port configuration.");
    }
    this.#port = port;
    this.#generation = generation;
    this.#capacity = queueCapacity;
    this.#timeout = timeoutMs;
    this.#programId = programId;
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

  async retainedRate(request) {
    return this.#retainedRequest("retained-rate", request, ["id", "attempt", "revision", "preparation"]);
  }

  async retainedRegion(request) {
    return this.#retainedRequest("retained-region", request, ["id", "attempt", "revision", "start", "end", "repeat"]);
  }

  async retainedDisableLoop(request) {
    return this.#retainedRequest("retained-disable-loop", request, ["id", "attempt", "revision"]);
  }

  async retainedRestartOriginal(request) {
    return this.#retainedRequest("retained-restart-original", request, ["id", "attempt", "revision", "preparation"]);
  }

  async retainedPause(request) {
    return this.#retainedRequest("retained-pause", request, ["id", "attempt", "paused"]);
  }

  async retainedPoll() {
    return this.#retainedRequest("retained-poll", {}, []);
  }

  async retainedEventAck(serial) {
    return this.#retainedRequest("retained-event-ack", { serial }, ["serial"]);
  }

  async retainedHistoryRetire(pin) {
    return this.#retainedRequest("retained-history-retire", { pin }, ["pin"]);
  }

  #retainedRequest(operation, input, names) {
    this.#available();
    if (this.#programId === undefined) throw this.#error("state", "Retained controls require the actual Program identity.");
    if (!exactFields(input, names)) throw this.#error("validation", "Invalid retained request fields.");
    const fields = {};
    for (const name of names) fields[name] = input[name];
    this.#available();
    for (const name of names) {
      const value = fields[name];
      if (name === "preparation") {
        try { fields[name] = snapshotWords(value, 20); }
        catch (cause) { throw this.#error("validation", "Invalid retained preparation words.", { cause }); }
      } else if (name === "start" || name === "end" ? !signed(value)
        : name === "repeat" || name === "paused" ? typeof value !== "boolean" : !unsigned(value)) {
        throw this.#error("validation", "Retained request exceeds numeric protocol bounds.");
      }
    }
    return this.#request(operation, 0, { version: 1, programId: this.#programId, ...fields });
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
    const pending = { sequence, operation, count, retained: operation.startsWith("retained-"), resolve, reject, timer: null };
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
      if (fields.kind === "ack" && pending?.retained) Object.assign(fields, { version: message.version, programId: message.programId,
        terminal: message.terminal, retained: message.retained });
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
    let valid, poll = null;
    try {
      valid = fields.kind === "ack" && pending !== null && fields.operation === pending.operation
        && fields.sequence === pending.sequence && integer(fields.status, 0, 0xffffffff)
        && integer(fields.admitted, 0, pending.count)
        && (fields.error === null || (typeof fields.error === "string" && fields.error.length <= 4096))
        && (fields.status !== 0 || (fields.admitted === pending.count && fields.error === null))
        && (pending.operation === "poll" && fields.status === 0 ? validReport(fields.report)
          : (pending.operation === "retained-poll" && fields.status === 0) || fields.report === null);
      if (valid && pending.retained) {
        const diagnostics = Object.hasOwn(message, "diagnostics");
        valid = exactFields(message, diagnostics ? [...RETAINED_ACK_FIELDS, "diagnostics"] : RETAINED_ACK_FIELDS)
          && fields.version === 1 && fields.programId === this.#programId && fields.admitted === 0
          && typeof fields.terminal === "boolean" && (fields.status !== 0 || fields.terminal === false)
          && (fields.status === 0 || typeof fields.error === "string")
          && (!diagnostics || fields.terminal === true);
        if (valid && pending.operation === "retained-poll" && fields.status === 0) {
          poll = retainedPollSnapshot(fields.report, fields.retained, this.#programId);
        } else if (valid) valid = fields.report === null && fields.retained === null;
      }
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
      const error = this.#error("remote", `Audio processor rejected ${pending.operation} (status ${fields.status})${fields.error ? `: ${fields.error}` : "."}`,
        { sequence: pending.sequence, status: fields.status, admitted: fields.admitted, diagnostics,
          ...(pending.retained ? { terminal: fields.terminal } : {}) });
      if (pending.retained && fields.terminal === false) {
        this.#pending = null;
        clearTimeout(pending.timer);
        pending.reject(error);
      } else this.#dispose(error);
      return;
    }
    this.#pending = null;
    clearTimeout(pending.timer);
    pending.resolve(poll ?? (pending.operation === "poll" ? fields.report : fields));
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

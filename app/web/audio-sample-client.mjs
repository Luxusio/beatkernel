// One setup sample producer. Ending this port does not free the audio owner.
const U64_MAX = 18446744073709551615n;
const integer = (value, min, max) => Number.isSafeInteger(value) && value >= min && value <= max;
const unsigned = value => typeof value === "bigint" && value >= 0n && value <= U64_MAX;

function failure(code, message, generation = null, details = {}) {
  const error = new Error(message, details.cause === undefined ? undefined : { cause: details.cause });
  Object.assign(error, { code, generation, sequence: null, status: null, admitted: null }, details);
  return error;
}

export class AudioSampleClient {
  #port;
  #generation;
  #channels;
  #limits;
  #timeout;
  #sequence = 0;
  #pending = null;
  #state = "ready";
  #failure = null;
  #sampleIds = new Set();
  #bytes = 0;

  constructor(options = {}) {
    const { port, generation, channels, pcmLimits, timeoutMs } = options ?? {};
    const limits = { maxAssetBytes: pcmLimits?.maxAssetBytes,
      maxTotalBytes: pcmLimits?.maxTotalBytes, maxSamples: pcmLimits?.maxSamples };
    if (!port || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")
      || !integer(generation, 1, Number.MAX_SAFE_INTEGER) || !integer(channels, 1, 32)
      || !integer(limits.maxAssetBytes, 1, 2147483644)
      || !integer(limits.maxTotalBytes, limits.maxAssetBytes, 2147483644)
      || !integer(limits.maxSamples, 1, 65536) || !integer(timeoutMs, 1, 60000)) {
      throw failure("validation", "Invalid bounded sample port configuration.");
    }
    this.#port = port;
    this.#generation = generation;
    this.#channels = channels;
    this.#limits = limits;
    this.#timeout = timeoutMs;
    try {
      port.onmessage = event => this.#message(event.data);
      if (this.#failure) throw this.#failure;
      port.onmessageerror = () => this.#dispose(this.#error("message", "Sample response could not be decoded."));
      if (this.#failure) throw this.#failure;
      port.start();
      if (this.#failure) throw this.#failure;
    } catch (cause) {
      const error = this.#failure ?? this.#error("transport", "Sample port could not start.", { cause });
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
    if (this.#state !== "ready") throw this.#error("state", "Sample transfer has already ended.");
    if (this.#pending) throw this.#error("busy", "A sample port operation is already pending.");
    if (this.#sequence === Number.MAX_SAFE_INTEGER) {
      const error = this.#error("state", "Sample sequence exhausted.");
      this.#dispose(error);
      throw error;
    }
  }

  async sample(input) {
    this.#available();
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
    // A caller getter may close the owner or admit another operation.
    this.#available();
    const limits = this.#limits;
    if (!unsigned(id) || !integer(rate, 1, 0xffffffff) || channels !== this.#channels
      || !(pcm instanceof Float32Array) || !(buffer instanceof ArrayBuffer)
      || resizable === true || offset !== 0 || bytes !== bufferBytes
      || !integer(length, 0, 536870911) || bytes !== length * 4 || length % channels !== 0
      || bytes > limits.maxAssetBytes || bytes > limits.maxTotalBytes - this.#bytes
      || this.#sampleIds.size >= limits.maxSamples || this.#sampleIds.has(id)) {
      throw this.#error("validation", "Invalid sample or nonexclusive PCM backing buffer.");
    }
    try { new Float32Array(buffer, 0, 0); } catch (cause) {
      throw this.#error("validation", "PCM backing buffer is detached.", { cause });
    }
    // The Worklet owns finite-value validation; transfer the original buffer.
    return this.#request("sample", { id, rate, channels, pcm }, [buffer], () => {
      this.#sampleIds.add(id);
      this.#bytes += bytes;
    });
  }

  async end() {
    this.#available();
    const totals = Object.freeze({ count: this.#sampleIds.size, bytes: this.#bytes });
    return this.#request("end-samples", totals, [], () => {
      this.#state = "ended";
      this.#closePort();
      return totals;
    });
  }

  #request(operation, fields, transfer, commit) {
    this.#available();
    let resolve;
    let reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    promise.catch(() => {});
    const sequence = ++this.#sequence;
    const pending = { sequence, operation, commit, resolve, reject, timer: null };
    this.#pending = pending;
    pending.timer = setTimeout(() => {
      if (this.#pending === pending) this.#dispose(this.#error("timeout", "Sample acknowledgement timed out.", { sequence }));
    }, this.#timeout);
    try {
      this.#port.postMessage({ kind: operation, generation: this.#generation, sequence, ...fields }, transfer);
    } catch (cause) {
      this.#dispose(this.#error("transport", "Sample operation could not be sent.", { sequence, cause }));
    }
    return promise;
  }

  #message(message) {
    if (this.#state !== "ready") return;
    const malformed = () => this.#dispose(this.#error("protocol", "Malformed or uncorrelated sample response."));
    if (!message || typeof message !== "object") { malformed(); return; }
    const pending = this.#pending;
    let fields;
    try {
      const generation = message.generation;
      if (integer(generation, 1, Number.MAX_SAFE_INTEGER) && generation !== this.#generation) return;
      fields = { generation, kind: message.kind, operation: message.operation, sequence: message.sequence,
        status: message.status, admitted: message.admitted, error: message.error, report: message.report };
    } catch { malformed(); return; }
    // Read response fields only once. Getters cannot revive a closed owner or
    // settle a different operation admitted while the response was inspected.
    if (this.#state !== "ready" || this.#pending !== pending) return;
    if (fields.generation !== this.#generation) { malformed(); return; }
    if (fields.kind === "closed") {
      this.#dispose(this.#error("closed", "The audio host stopped the sample owner."), "closed");
      return;
    }
    if (fields.kind === "terminal") {
      if (!integer(fields.status, 1, 0xffffffff)) { malformed(); return; }
      this.#dispose(this.#error("processor", "Audio processor reported a terminal failure.", { status: fields.status }));
      return;
    }
    if (fields.kind !== "ack" || pending === null || fields.operation !== pending.operation
      || fields.sequence !== pending.sequence || !integer(fields.status, 0, 0xffffffff)
      || fields.admitted !== 0 || fields.report !== null
      || !(fields.error === null || (typeof fields.error === "string" && fields.error.length <= 4096))
      || (fields.status === 0 && fields.error !== null)) {
      malformed();
      return;
    }
    if (fields.status !== 0) {
      this.#dispose(this.#error("remote", `Audio processor rejected ${pending.operation}${fields.error ? `: ${fields.error}` : "."}`,
        { sequence: pending.sequence, status: fields.status, admitted: fields.admitted }));
      return;
    }
    this.#pending = null;
    clearTimeout(pending.timer);
    const result = pending.commit();
    pending.resolve(result ?? fields);
  }

  #closePort() {
    try { this.#port.onmessage = null; } catch {}
    try { this.#port.onmessageerror = null; } catch {}
    try { this.#port.close(); } catch {}
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
    this.#closePort();
  }

  close() {
    this.#dispose(this.#error("closed", "Sample client was closed."), "closed");
  }
}

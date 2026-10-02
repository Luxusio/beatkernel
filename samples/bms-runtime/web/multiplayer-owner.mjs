import { WebTransportChannel } from "./multiplayer-transport.mjs";

const OWNER = Symbol("BrowserMultiplayerOwner");
const I64_MAX = 9223372036854775807n;
const I64_MIN = -9223372036854775808n;
const U64_MAX = 18446744073709551615n;
const MAX_FRAME = 65547;
const SESSION_METHODS = ["request_ready", "needed_bytes", "receive_bytes", "next_write", "send_progress",
  "written", "poll_event", "setup_complete", "preparation_pending", "start_committed", "close", "free"];

export class BrowserMultiplayerOwnerError extends Error {
  constructor(code, operation, message, cause) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "BrowserMultiplayerOwnerError";
    this.code = code;
    this.operation = operation;
  }
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  promise.catch(() => {});
  return { promise, resolve, reject };
}

function integer(value, min, max) {
  return Number.isSafeInteger(value) && value >= min && value <= max;
}

function quietCall(owner, method) {
  try { Promise.resolve(owner?.[method]()).catch(() => {}); } catch {}
}

function configuration(options) {
  const { session, now, channelFactory = (url, config) => WebTransportChannel.open(url, config),
    signal, setupTimeoutMs = 10000, tickMs = 5, onEvent, onClose } = options ?? {};
  if (typeof globalThis.AbortController !== "function"
    || !session || SESSION_METHODS.some(name => typeof session[name] !== "function")
    || typeof now !== "function" || typeof channelFactory !== "function"
    || !integer(setupTimeoutMs, 1, 60000) || !integer(tickMs, 1, 1000)
    || (onEvent !== undefined && typeof onEvent !== "function")
    || (onClose !== undefined && typeof onClose !== "function")
    || (signal !== undefined && (signal === null || typeof signal.aborted !== "boolean"
      || typeof signal.addEventListener !== "function" || typeof signal.removeEventListener !== "function"))) {
    throw new BrowserMultiplayerOwnerError("validation", "open", "Invalid multiplayer owner configuration.");
  }
  return { session, now, channelFactory, signal, setupTimeoutMs, tickMs, onEvent, onClose };
}

export class BrowserMultiplayerOwner {
  #config;
  #session;
  #channel = null;
  #abortController = new AbortController();
  #abort = null;
  #failure = null;
  #closedGate = deferred();
  #origin = null;
  #lastNow = null;
  #setupTimer = null;
  #tick = null;
  #submission = null;
  #finalWaiter = null;
  #finalAcknowledged = false;
  #writeCompletion = null;

  constructor(token, config) {
    if (token !== OWNER) throw new TypeError("Use BrowserMultiplayerOwner.open().");
    this.#config = config;
    this.#session = config.session;
  }

  get origin() { return this.#origin; }
  get closed() { return this.#failure !== null; }

  static async open(url, options) {
    const config = configuration(options);
    const owner = new BrowserMultiplayerOwner(OWNER, config);
    try {
      owner.#origin = owner.#clock();
      owner.#lastNow = owner.#origin;
      owner.#setupTimer = setTimeout(() => owner.#fail(new BrowserMultiplayerOwnerError(
        "timeout", "setup", "Multiplayer connection/preparation timed out.")), config.setupTimeoutMs);
      if (config.signal !== undefined) {
        owner.#abort = () => owner.#fail(new BrowserMultiplayerOwnerError("aborted", "abort", "Multiplayer was cancelled."));
        config.signal.addEventListener("abort", owner.#abort, { once: true });
        if (config.signal.aborted) owner.#abort();
      }
      owner.#ensureOpen();
      const opening = Promise.resolve(config.channelFactory(url, {
        signal: owner.#abortController.signal, setupTimeoutMs: config.setupTimeoutMs,
      }));
      opening.then(channel => { if (owner.closed) quietCall(channel, "close"); }, () => {});
      const channel = await owner.#await(opening);
      if (owner.closed) { quietCall(channel, "close"); owner.#ensureOpen(); }
      owner.#channel = channel;
      if (!channel || ["readPrefix", "write", "close"].some(name => typeof channel[name] !== "function")) {
        throw new BrowserMultiplayerOwnerError("transport", "open", "Multiplayer channel is malformed.");
      }
      owner.#drainEvents();
      owner.#ensureOpen();
      owner.#readLoop().catch(cause => owner.#fatal(cause, "transport", "read"));
      owner.#writeLoop().catch(cause => owner.#fatal(cause, "core", "write"));
      owner.#ensureOpen();
      return owner;
    } catch (cause) { throw owner.#fatal(cause, "transport", "open"); }
  }

  #ensureOpen() {
    if (this.#failure) throw this.#failure;
  }

  #clock() {
    let value;
    try { value = this.#config.now(); }
    catch (cause) { throw new BrowserMultiplayerOwnerError("clock", "clock", "Multiplayer clock acquisition failed.", cause); }
    if (typeof value !== "bigint" || value < 0n) {
      throw new BrowserMultiplayerOwnerError("clock", "clock", "Multiplayer clock must return nonnegative bigint nanoseconds.");
    }
    return value;
  }

  elapsed() {
    this.#ensureOpen();
    try {
      const now = this.#clock();
      const elapsed = now - this.#origin;
      if (now < this.#lastNow || elapsed < 0n || elapsed > I64_MAX) {
        throw new BrowserMultiplayerOwnerError("clock", "clock", "Multiplayer elapsed clock regressed or overflowed.");
      }
      this.#lastNow = now;
      return elapsed;
    } catch (cause) { throw this.#fatal(cause, "clock", "clock"); }
  }

  #core(operation, callback) {
    this.#ensureOpen();
    try { return callback(this.#session); }
    catch (cause) { throw this.#fatal(cause, "core", operation); }
  }

  #fatal(cause, code, operation) {
    const error = cause instanceof BrowserMultiplayerOwnerError ? cause
      : new BrowserMultiplayerOwnerError(code, operation, `Multiplayer ${operation} failed.`, cause);
    return this.#fail(error);
  }

  #fail(error) {
    if (this.#failure) return this.#failure;
    this.#failure = error;
    clearTimeout(this.#setupTimer);
    this.#setupTimer = null;
    if (this.#abort !== null) {
      try { this.#config.signal.removeEventListener("abort", this.#abort); } catch {}
      this.#abort = null;
    }
    this.#closedGate.reject(error);
    this.#submission?.reject(error);
    this.#submission = null;
    this.#finalWaiter?.reject(error);
    this.#finalWaiter = null;
    this.#wake();
    try { this.#abortController.abort(); } catch {}
    quietCall(this.#channel, "close");
    this.#channel = null;
    const session = this.#session;
    this.#session = null;
    // Free once even if explicit close throws. Every async continuation checks
    // the fence before its next call through generated WASM glue.
    quietCall(session, "close");
    quietCall(session, "free");
    try { Promise.resolve(this.#config.onClose?.(error)).catch(() => {}); } catch {}
    return error;
  }

  #await(promise) {
    return Promise.race([promise, this.#closedGate.promise]);
  }

  #wake() {
    if (this.#tick === null) return;
    const tick = this.#tick;
    this.#tick = null;
    clearTimeout(tick.timer);
    tick.resolve();
  }

  #waitTick() {
    if (this.closed) return Promise.resolve();
    return new Promise(resolve => {
      this.#tick = { resolve, timer: setTimeout(() => this.#wake(), this.#config.tickMs) };
    });
  }

  #drainEvents() {
    for (let count = 0; count <= 8 && !this.closed; count++) {
      const event = this.#core("poll_event", session => session.poll_event());
      if (event === null || event === undefined) break;
      if (count === 8 || typeof event !== "object" || typeof event.kind !== "string") {
        throw this.#fail(new BrowserMultiplayerOwnerError("core", "poll_event", "Multiplayer event boundary is malformed or exceeds eight events."));
      }
      // A callback may close this owner. Retain proven peer acknowledgement
      // before invoking it; cleanup cannot revoke an already observed receipt.
      if (event.kind === "final-acknowledged") {
        this.#finalAcknowledged = true;
        this.#finalWaiter?.resolve();
        this.#finalWaiter = null;
      }
      try {
        const result = this.#config.onEvent?.(event);
        if (result && typeof result.then === "function") {
          Promise.resolve(result).catch(() => {});
          throw new Error("Multiplayer event callbacks must be synchronous.");
        }
      } catch (cause) { throw this.#fatal(cause, "callback", "event"); }
    }
    if (!this.closed && !this.#core("preparation_pending", session => session.preparation_pending())) {
      clearTimeout(this.#setupTimer);
      this.#setupTimer = null;
    }
  }

  async #readLoop() {
    while (!this.closed) {
      const needed = this.#core("needed_bytes", session => session.needed_bytes());
      if (!integer(needed, 1, MAX_FRAME)) {
        throw new BrowserMultiplayerOwnerError("core", "read", "Invalid Rust decoder prefix size.");
      }
      let prefix;
      try { prefix = await this.#await(this.#channel.readPrefix(needed)); }
      catch (cause) {
        // A fulfilled local write must credit the Rust owner and drain its ACK
        // event before a simultaneous EOF is allowed to dispose that owner.
        if (cause?.code === "closed" && this.#writeCompletion !== null && !this.closed) {
          try { await this.#await(this.#writeCompletion); } catch {}
        }
        throw cause;
      }
      this.#ensureOpen();
      if (!(prefix instanceof Uint8Array) || !(prefix.buffer instanceof ArrayBuffer)
        || !integer(prefix.byteLength, 1, needed)) {
        throw new BrowserMultiplayerOwnerError("transport", "read", "Channel returned an invalid bounded prefix.");
      }
      const consumed = this.#core("receive_bytes", session => session.receive_bytes(prefix, this.elapsed()));
      if (consumed !== prefix.byteLength) {
        throw new BrowserMultiplayerOwnerError("core", "read", "Rust decoder did not consume its exact requested prefix.");
      }
      this.#drainEvents();
      this.#wake();
    }
  }

  #takeWrite(write) {
    try {
      const kind = write.kind;
      const id = write.frame_id;
      if (![0, 1, 2].includes(kind) || typeof id !== "bigint"
        || id < 0n || id > U64_MAX || (kind === 1 ? id === 0n : id !== 0n)) {
        throw new Error("Invalid Rust write admission.");
      }
      if (kind !== 1) return { kind };
      const bytes = write.take_bytes();
      if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
        || bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength
        || !integer(bytes.byteLength, 1, MAX_FRAME)) throw new Error("Invalid Rust frame bytes.");
      return { kind, id, bytes };
    } finally { write.free(); }
  }

  async #transmit(frame, submission) {
    try { await this.#await(this.#channel.write(frame.bytes)); }
    catch (cause) { throw this.#fatal(cause, "transport", "write"); }
    this.#ensureOpen();
    this.#core("written", session => session.written(frame.id, this.elapsed()));
    if (submission !== null) {
      this.#submission = null;
      submission.resolve();
    }
    this.#drainEvents();
    this.#ensureOpen();
  }

  async #writeLoop() {
    while (!this.closed) {
      let frame = this.#takeWrite(this.#core("next_write", session => session.next_write(this.elapsed())));
      let submission = null;
      if (frame.kind === 2 && this.#submission !== null) {
        submission = this.#submission;
        const { songNs, hits, misses, combo, maxCombo } = submission.progress;
        frame = this.#takeWrite(this.#core("send_progress", session => session.send_progress(
          songNs, hits, misses, combo, maxCombo, submission.finalPrefix, this.elapsed())));
        if (frame.kind !== 1) throw new Error("Rust application admission did not return a frame.");
      }
      if (frame.kind === 1) {
        const completion = this.#transmit(frame, submission);
        this.#writeCompletion = completion;
        try { await completion; }
        finally { if (this.#writeCompletion === completion) this.#writeCompletion = null; }
      } else {
        await this.#waitTick();
      }
    }
  }

  request_ready() {
    this.#core("request_ready", session => session.request_ready());
    this.#drainEvents();
    this.#wake();
  }

  async submit(progress, finalPrefix = false) {
    this.#ensureOpen();
    if (this.#submission !== null) throw new BrowserMultiplayerOwnerError("busy", "submit", "A multiplayer submission is already pending.");
    const snapshot = { songNs: progress?.songNs, hits: progress?.hits, misses: progress?.misses,
      combo: progress?.combo, maxCombo: progress?.maxCombo };
    if (typeof finalPrefix !== "boolean" || typeof snapshot.songNs !== "bigint"
      || snapshot.songNs < I64_MIN || snapshot.songNs > I64_MAX
      || [snapshot.hits, snapshot.misses, snapshot.combo, snapshot.maxCombo]
        .some(value => typeof value !== "bigint" || value < 0n || value > U64_MAX)) {
      throw new BrowserMultiplayerOwnerError("validation", "submit", "Progress fields must fit the exact Rust scalar ABI.");
    }
    // Only scalar ABI bounds are checked here. Rust owns cumulative progression,
    // readiness, final sequencing and every protocol admission decision.
    const pending = deferred();
    this.#submission = { ...pending, progress: snapshot, finalPrefix };
    this.#wake();
    return pending.promise;
  }

  async wait_final_ack() {
    this.#ensureOpen();
    if (this.#finalAcknowledged) return;
    if (this.#finalWaiter !== null) throw new BrowserMultiplayerOwnerError("busy", "final", "A final acknowledgement waiter already exists.");
    this.#finalWaiter = deferred();
    return this.#finalWaiter.promise;
  }

  close() {
    this.#fail(new BrowserMultiplayerOwnerError("closed", "close", "Multiplayer owner was closed."));
  }
}

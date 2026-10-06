import { WebTransportChannel } from "./multiplayer-transport.mjs";

const TOKEN = Symbol("BrowserRoomOwner");
const MAX_FRAME = 65808;
const MAX_CHUNK = 1024 * 1024;
const U64_MAX = 18446744073709551615n;
const I64_MAX = 9223372036854775807n;
export const ROOM_SESSION_METHODS = Object.freeze(["request_seal", "request_ready", "request_leave", "needed_bytes",
  "frame_pending", "receive_bytes", "next_write", "written", "participant_id",
  "revision", "has_snapshot", "leave_written", "snapshot", "take_start", "publish_progress",
  "publication_due", "publish_progress_at", "take_peer_progress", "local_final_written", "local_final_acknowledged", "peer_final_ack_written",
  "progress_complete", "request_drain", "drain_complete", "begin_drain", "drain_wait_step", "drain_requested", "begin_setup", "setup_wait_step", "configure_frame_wait", "frame_wait_step", "close", "free"]);

export class BrowserRoomOwnerError extends Error {
  constructor(code, operation, message, cause) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "BrowserRoomOwnerError";
    this.code = code;
    this.operation = operation;
  }
}

function gate() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  promise.catch(() => {});
  return { promise, resolve, reject };
}
function integer(value, min, max) { return Number.isSafeInteger(value) && value >= min && value <= max; }
function u64(value, min = 0n) { return typeof value === "bigint" && value >= min && value <= U64_MAX; }
function quiet(owner, method) { try { Promise.resolve(owner?.[method]()).catch(() => {}); } catch {} }
function configuration(url, options) {
  const { session, now, signal, setupTimeoutMs = 10000, ioTimeoutMs = 10000,
    channelFactory = (address, config) => WebTransportChannel.open(address, config),
    onSnapshot, onStart, onProgress, onReceipts, onClose } = options ?? {};
  if (typeof AbortController !== "function" || !session || ROOM_SESSION_METHODS.some(name => typeof session[name] !== "function")
    || typeof now !== "function" || typeof channelFactory !== "function" || !integer(setupTimeoutMs, 1, 60000) || !integer(ioTimeoutMs, 1, 60000)
    || (onSnapshot !== undefined && typeof onSnapshot !== "function") || (onClose !== undefined && typeof onClose !== "function")
    || (onStart !== undefined && typeof onStart !== "function")
    || (onProgress !== undefined && typeof onProgress !== "function")
    || (onReceipts !== undefined && typeof onReceipts !== "function")
    || (signal !== undefined && (!signal || typeof signal.aborted !== "boolean"
      || typeof signal.addEventListener !== "function" || typeof signal.removeEventListener !== "function"))) {
    throw new BrowserRoomOwnerError("validation", "open", "Invalid room owner configuration.");
  }
  if (typeof url !== "string" || url.length === 0 || url.length > 4096) {
    throw new BrowserRoomOwnerError("validation", "open", "Room requires a canonical HTTPS room URL.");
  }
  let address;
  try { address = new URL(url); } catch {}
  if (!address || address.href !== url
    || address.protocol !== "https:" || !address.hostname || address.port === "0"
    || address.username || address.password || address.search || address.hash
    || !/^\/rooms\/[A-Za-z0-9_-]{1,1024}$/.test(address.pathname)) {
    throw new BrowserRoomOwnerError("validation", "open", "Room requires a canonical HTTPS room URL.");
  }
  return { session, now, signal, setupTimeoutMs, ioTimeoutMs, channelFactory, onSnapshot, onStart, onProgress, onReceipts, onClose };
}

function progressWords(value, players = null) {
  if (!(value instanceof Uint32Array) || !(value.buffer instanceof ArrayBuffer)
    || value.buffer.resizable === true || !integer(value.length, 11, 704) || value.length % 11 !== 0
    || value.byteLength !== value.length * 4 || value.buffer.byteLength > 2816) {
    throw new Error("Room progress requires 1..64 bounded eleven-word rows.");
  }
  const view = new Uint32Array(value.buffer, value.byteOffset, value.length);
  if (players !== null && (view.length !== players.length * 11
    || players.some((player, index) => view[index * 11] !== player))) {
    throw new Error("Room progress must preserve the prepared local roster.");
  }
  const copy = new Uint32Array(view.length);
  copy.set(view);
  return copy;
}

function validSnapshot(snapshot, participant) {
  if (!snapshot || typeof snapshot !== "object" || !integer(snapshot.phase, 0, 2)
    || !Array.isArray(snapshot.members) || !integer(snapshot.members.length, 1, 64)
    || !u64(participant, 1n)) return false;
  if (snapshot.phase === 2 ? snapshot.deadlineNs !== null
    : typeof snapshot.deadlineNs !== "bigint" || snapshot.deadlineNs < 0n || snapshot.deadlineNs > I64_MAX) return false;
  const hosts = new Set();
  let prepared = 0;
  for (const member of snapshot.members) {
    if (!member || !u64(member.participant, 1n) || hosts.has(member.participant) || typeof member.prepared !== "boolean"
      || !(member.players instanceof Uint32Array) || !(member.players.buffer instanceof ArrayBuffer)
      || member.players.buffer.resizable === true || !integer(member.players.length, 1, 64)
      || member.players.byteLength !== member.players.length * 4 || member.players.buffer.byteLength > 256) return false;
    try { new Uint32Array(member.players.buffer, member.players.byteOffset, member.players.length); } catch { return false; }
    const players = new Set();
    for (const player of member.players) { if (player === 0 || players.has(player)) return false; players.add(player); }
    hosts.add(member.participant);
    if (member.prepared) prepared++;
  }
  return hosts.has(participant) && (snapshot.phase === 0 ? prepared === 0
    : snapshot.members.length >= 2 && (snapshot.phase === 1 ? prepared < snapshot.members.length : prepared === snapshot.members.length));
}

// Worker-owned lifecycle component. Rust retains all room semantics; this
// adapter moves bounded bytes, retains actual observation times and publishes
// validated metadata, schedules and accepted progress from the common owner.
export class BrowserRoomOwner {
  #config;
  #session;
  #channel = null;
  #controller = new AbortController();
  #failure = null;
  #closedGate = gate();
  #wakeGate = null;
  #leaveGate = null;
  #completionGate = null;
  #drainGate = null;
  #drainSettled = false;
  #drainTimer = null;
  #drainRequested = false;
  #setupTimer = null;
  #setupComplete = false;
  #frameTimer = null;
  #prepared = false;
  #start = null;
  #origin = null;
  #lastNow = null;
  #abort = null;
  #loops = [];
  #operations = new Set();
  #closing = null;
  #channelClosing = null;
  #cleanupError = null;
  #revision = 0n;
  #participant = 0n;
  #snapshot = null;
  #roster = null;
  #receipts = Object.freeze({ localFinalWritten: false, localFinalAcknowledged: false, complete: false, drainComplete: false });

  constructor(token, config) {
    if (token !== TOKEN) throw new TypeError("Use BrowserRoomOwner.open().");
    this.#config = config;
    this.#session = config.session;
  }
  get closed() { return this.#failure !== null; }
  get participant() { return this.#participant; }
  get snapshot() { return this.#snapshot; }
  get origin() { return this.#origin; }
  get receipts() { return this.#receipts; }

  static async open(url, options) {
    const config = configuration(url, options);
    const owner = new BrowserRoomOwner(TOKEN, config);
    try {
      owner.#origin = owner.#clock();
      owner.#lastNow = owner.#origin;
      if (config.signal !== undefined) {
        owner.#abort = () => owner.#fail(new BrowserRoomOwnerError("aborted", "abort", "Room was cancelled."));
        config.signal.addEventListener("abort", owner.#abort, { once: true });
        if (config.signal.aborted) owner.#abort();
      }
      owner.#ensure();
      if (owner.#core("initial", session => session.revision()) !== 0n
        || owner.#core("initial", session => session.participant_id()) !== 0n
        || owner.#core("initial", session => session.has_snapshot()) !== false
        || owner.#core("initial", session => session.frame_pending()) !== false
        || owner.#core("initial", session => session.leave_written()) !== false
        || owner.#core("initial", session => session.local_final_written()) !== false
        || owner.#core("initial", session => session.local_final_acknowledged()) !== false
        || owner.#core("initial", session => session.progress_complete()) !== false
        || owner.#core("initial", session => session.drain_complete()) !== false
        || owner.#core("initial", session => session.drain_requested()) !== false) {
        throw new BrowserRoomOwnerError("protocol", "open", "Room session is already in use.");
      }
      owner.#session.configure_frame_wait(BigInt(config.ioTimeoutMs) * 1000000n);
      owner.#session.begin_setup(0n, BigInt(config.setupTimeoutMs) * 1000000n);
      owner.#advanceSetup();
      const opening = owner.#track(() => config.channelFactory(url, { signal: owner.#controller.signal,
        setupTimeoutMs: config.setupTimeoutMs, ioTimeoutMs: config.ioTimeoutMs, maxPrefixBytes: MAX_FRAME }));
      opening.then(channel => { if (owner.closed) owner.#closeChannel(channel); }, () => {});
      const channel = await owner.#await(opening);
      owner.#ensure();
      owner.#channel = channel;
      if (!channel || ["readPrefix", "write", "close"].some(name => typeof channel[name] !== "function")) {
        throw new BrowserRoomOwnerError("transport", "open", "Malformed room channel.");
      }
      // Start writing Join first. Receiving admission still requires its actual
      // completed write, as enforced by the common Rust client.
      owner.#loops = [owner.#writeLoop().catch(cause => owner.#fatal(cause, "core", "write")),
        owner.#readLoop().catch(cause => owner.#fatal(cause, "transport", "read"))];
      owner.#ensure();
      return owner;
    } catch (cause) {
      const failure = owner.#fatal(cause, "transport", "open");
      // A failed opening returns no handle. Settle its acquired channel API
      // continuations here so callers can join ownership through open itself.
      try { await owner.close(); } catch (cleanupError) { failure.cleanupError = cleanupError; }
      throw failure;
    }
  }

  #ensure() { if (this.#failure) throw this.#failure; }
  #await(promise) { return Promise.race([promise, this.#closedGate.promise]); }
  #clock() {
    let value;
    try { value = this.#config.now(); }
    catch (cause) { throw new BrowserRoomOwnerError("clock", "clock", "Room clock acquisition failed.", cause); }
    this.#ensure();
    if (typeof value !== "bigint" || value < 0n || value > I64_MAX) {
      throw new BrowserRoomOwnerError("clock", "clock", "Room clock requires nonnegative i64 bigint nanoseconds.");
    }
    return value;
  }
  #elapsed() {
    this.#ensure();
    try {
      const value = this.#clock();
      if (value < this.#lastNow || value < this.#origin) {
        throw new BrowserRoomOwnerError("clock", "clock", "Room clock regressed.");
      }
      this.#lastNow = value;
      return value - this.#origin;
    } catch (cause) {
      throw this.#fatal(cause, "clock", "clock");
    }
  }
  #track(action, fulfilled = value => value) {
    // Register before calling a channel: it may trigger cancellation reentrantly.
    // Join the channel API promise, while platform cleanup remains channel-owned.
    const operation = gate();
    this.#operations.add(operation.promise);
    operation.promise.then(() => this.#operations.delete(operation.promise),
      () => this.#operations.delete(operation.promise));
    try {
      // Capture directly on the channel promise, before this tracked promise
      // and the consuming loop resume in later microtasks.
      Promise.resolve(action()).then(value => {
        try { operation.resolve(fulfilled(value)); } catch (cause) { operation.reject(cause); }
      }, operation.reject);
    }
    catch (cause) { operation.reject(cause); }
    return operation.promise;
  }
  #fatal(cause, code, operation) {
    return this.#fail(cause instanceof BrowserRoomOwnerError ? cause
      : new BrowserRoomOwnerError(code, operation, `Room ${operation} failed.`, cause));
  }
  #core(operation, action) {
    this.#ensure();
    try { return action(this.#session); } catch (cause) {
      if (cause?.code === "timeout" && cause?.operation === "frame") {
        throw this.#fatal(new BrowserRoomOwnerError("timeout", "frame", "Incomplete room frame timed out.", cause), "timeout", "frame");
      }
      throw this.#fatal(cause, "core", operation);
    }
  }
  #closeChannel(channel) {
    if (channel === null || channel === undefined) return;
    try { this.#channelClosing = Promise.resolve(channel.close()).catch(cause => { this.#cleanupError ??= cause; }); }
    catch (cause) { this.#cleanupError ??= cause; }
  }
  #fail(error) {
    if (this.#failure) return this.#failure;
    this.#failure = error;
    clearTimeout(this.#setupTimer); clearTimeout(this.#frameTimer); clearTimeout(this.#drainTimer);
    this.#setupTimer = null; this.#frameTimer = null;
    if (this.#abort !== null) {
      try { this.#config.signal.removeEventListener("abort", this.#abort); } catch {}
      this.#abort = null;
    }
    this.#closedGate.reject(error);
    this.#leaveGate?.reject(error); this.#leaveGate = null;
    this.#completionGate?.reject(error);
    this.#drainGate?.reject(error);
    this.#wake();
    try { this.#controller.abort(); } catch {}
    const channel = this.#channel; this.#channel = null;
    this.#closeChannel(channel);
    const session = this.#session; this.#session = null;
    try { session?.close(); } catch (cause) { this.#cleanupError ??= cause; }
    try { session?.free(); } catch (cause) { this.#cleanupError ??= cause; }
    try { Promise.resolve(this.#config.onClose?.(error)).catch(() => {}); } catch {}
    return error;
  }
  #wake() { this.#wakeGate?.resolve(); this.#wakeGate = null; }
  #request(method) {
    this.#ensure();
    try { this.#session[method](); }
    catch (cause) { throw new BrowserRoomOwnerError("state", method, "Room request is unavailable in this state.", cause); }
    this.#wake();
  }
  requestSeal() { this.#request("request_seal"); }
  requestReady() { this.#request("request_ready"); }

  progressDue(finalPrefix = false) {
    this.#ensure();
    if (typeof finalPrefix !== "boolean") throw new BrowserRoomOwnerError("validation", "publication_due", "Room final prefix flag must be boolean.");
    if (this.#roster === null || this.#leaveGate !== null) throw new BrowserRoomOwnerError("state", "publication_due", "Room progress publication is unavailable.");
    let due;
    try { due = this.#session.publication_due(this.#elapsed(), finalPrefix); }
    catch (cause) {
      if (cause?.code === "state") throw new BrowserRoomOwnerError("state", "publication_due", "Room progress publication is unavailable.", cause);
      throw this.#fatal(cause, "core", "publication_due");
    }
    if (typeof due !== "boolean") throw this.#fatal(new BrowserRoomOwnerError("protocol", "publication_due", "Invalid room publication timing hint."));
    return due;
  }

  publishProgress(words, finalPrefix = false) {
    this.#ensure();
    let owned;
    try {
      if (typeof finalPrefix !== "boolean") throw new Error("Room final prefix flag must be boolean.");
      owned = progressWords(words, this.#roster?.get(this.#participant) ?? null);
    } catch (cause) {
      throw new BrowserRoomOwnerError("validation", "publish_progress", "Invalid room progress publication.", cause);
    }
    if (this.#roster === null || this.#leaveGate !== null) {
      throw new BrowserRoomOwnerError("state", "publish_progress", "Room progress publication is unavailable.");
    }
    let admitted;
    try { admitted = this.#session.publish_progress_at(owned, finalPrefix, this.#elapsed()); }
    catch (cause) {
      if (cause?.code === "state") {
        throw new BrowserRoomOwnerError("state", "publish_progress", "Room progress publication is unavailable.", cause);
      }
      throw this.#fatal(cause, "core", "publish_progress");
    }
    if (typeof admitted !== "boolean") throw this.#fatal(new BrowserRoomOwnerError("protocol", "publish_progress", "Invalid room progress admission."));
    if (admitted) this.#wake();
    return admitted;
  }

  peerFinalAckWritten(participant) {
    this.#ensure();
    if (!u64(participant, 1n) || participant === this.#participant || !this.#roster?.has(participant)) {
      throw new BrowserRoomOwnerError("validation", "peer_final_ack_written", "Unknown prepared room peer.");
    }
    const value = this.#core("peer_final_ack_written", session => session.peer_final_ack_written(participant));
    if (typeof value !== "boolean") {
      throw this.#fatal(new BrowserRoomOwnerError("protocol", "peer_final_ack_written", "Invalid room peer receipt."));
    }
    return value;
  }

  waitForLocalCompletion() {
    if (this.closed) return Promise.reject(this.#failure);
    this.#completionGate ??= gate();
    if (this.#receipts.complete) this.#completionGate.resolve(this.#receipts);
    return this.#completionGate.promise;
  }

  drain() {
    if (this.#drainGate !== null) return this.#drainGate.promise;
    if (this.closed) return Promise.reject(this.#failure);
    if (this.#leaveGate !== null) return Promise.reject(new BrowserRoomOwnerError("state", "drain", "Room is leaving."));
    this.#drainGate = gate();
    try {
      this.#session.begin_drain(this.#elapsed(), BigInt(this.#config.ioTimeoutMs) * 1000000n);
      this.#advanceDrain();
    } catch (cause) { this.#rejectDrain(cause); }
    return this.#drainGate.promise;
  }

  #rejectDrain(cause) {
    if (this.#drainSettled) return;
    this.#drainSettled = true;
    clearTimeout(this.#drainTimer); this.#drainTimer = null;
    const error = cause?.code === "state"
      ? new BrowserRoomOwnerError("state", "drain", "Room drain request is unavailable.", cause)
      : this.#fatal(cause, cause?.code === "timeout" ? "timeout" : "core", "drain");
    this.#drainGate?.reject(error);
  }

  #advanceDrain() {
    if (this.#drainGate === null || this.#drainSettled || this.closed || this.#leaveGate !== null) return;
    clearTimeout(this.#drainTimer); this.#drainTimer = null;
    try {
      const delay = this.#session.drain_wait_step(this.#elapsed());
      const requested = this.#session.drain_requested();
      if (typeof requested !== "boolean" || (this.#drainRequested && !requested)) {
        throw new BrowserRoomOwnerError("protocol", "drain", "Invalid room drain admission.");
      }
      if (requested && !this.#drainRequested) { this.#drainRequested = true; this.#wake(); }
      if (typeof delay !== "bigint" || delay < -1n || delay > 1000000n) {
        throw new BrowserRoomOwnerError("protocol", "drain", "Invalid room drain wait result.");
      }
      if (delay === -1n) {
        const receipts = this.#receipts;
        if (!receipts.localFinalWritten || !receipts.localFinalAcknowledged
          || !receipts.complete || !receipts.drainComplete || !requested) {
          throw new BrowserRoomOwnerError("protocol", "drain", "Room drain completed without actual receipts.");
        }
        clearTimeout(this.#frameTimer); this.#frameTimer = null;
        this.#drainSettled = true;
        this.#drainGate.resolve(receipts);
        this.#wake();
      } else {
        this.#drainTimer = setTimeout(() => this.#advanceDrain(), Math.max(1, Number((delay + 999999n) / 1000000n)));
      }
    } catch (cause) { this.#rejectDrain(cause); }
  }

  leave() {
    if (this.#leaveGate !== null) return this.#leaveGate.promise;
    if (this.#receipts.drainComplete) return Promise.reject(new BrowserRoomOwnerError("state", "leave", "Room drain already completed."));
    try { this.#request("request_leave"); } catch (cause) { return Promise.reject(cause); }
    clearTimeout(this.#setupTimer); this.#setupTimer = null;
    clearTimeout(this.#frameTimer); this.#frameTimer = null;
    this.#leaveGate = gate();
    clearTimeout(this.#drainTimer); this.#drainTimer = null;
    this.#drainGate?.reject(new BrowserRoomOwnerError("closed", "leave", "Room drain cancelled by Leave."));
    return this.#leaveGate.promise;
  }
  close() {
    this.#fail(new BrowserRoomOwnerError("closed", "close", "Room owner closed."));
    this.#closing ??= Promise.allSettled([...this.#loops, ...this.#operations]).then(async () => {
      // Acquisition may have completed after close began. Its continuation
      // installs the actual late channel cleanup before this join resumes.
      await this.#channelClosing;
      if (this.#cleanupError !== null) throw this.#cleanupError;
    });
    return this.#closing;
  }

  #advanceFrame() {
    if (this.closed || this.#leaveGate !== null || this.#receipts.drainComplete) return;
    clearTimeout(this.#frameTimer); this.#frameTimer = null;
    try {
      const delay = this.#core("frame_wait_step", session => session.frame_wait_step(this.#elapsed()));
      if (typeof delay !== "bigint" || delay < -1n || delay === 0n || delay > 120000000000n) {
        throw new BrowserRoomOwnerError("protocol", "frame", "Invalid room frame wait result.");
      }
      if (delay > 0n) this.#frameTimer = setTimeout(() => this.#advanceFrame(), Number((delay + 999999n) / 1000000n));
    } catch (cause) { this.#fatal(cause, "core", "frame"); }
  }

  #advanceSetup() {
    if (this.closed || this.#setupComplete || this.#leaveGate !== null) return;
    clearTimeout(this.#setupTimer); this.#setupTimer = null;
    try {
      const delay = this.#session.setup_wait_step(this.#elapsed());
      if (typeof delay !== "bigint" || delay < -2n || delay === 0n || delay > 120000000000n) {
        throw new BrowserRoomOwnerError("protocol", "setup", "Invalid room setup wait result.");
      }
      if (delay === -1n) this.#setupComplete = true;
      if (delay > 0n) {
        this.#setupTimer = setTimeout(() => {
          try { this.#advanceSetup(); } catch (cause) { this.#fatal(cause, "core", "setup"); }
        }, Number((delay + 999999n) / 1000000n));
      }
    } catch (cause) {
      const error = cause instanceof BrowserRoomOwnerError ? cause
        : new BrowserRoomOwnerError(cause?.code === "timeout" ? "timeout" : "core",
          cause?.operation === "prepared" ? "prepared" : "setup", "Room setup wait failed.", cause);
      throw this.#fail(error);
    }
  }

  #observe() {
    const revision = this.#core("revision", session => session.revision());
    const participant = this.#core("participant", session => session.participant_id());
    const hasSnapshot = this.#core("snapshot", session => session.has_snapshot());
    if (!u64(revision) || revision < this.#revision || revision > this.#revision + 1n
      || !u64(participant) || typeof hasSnapshot !== "boolean"
      || (this.#participant !== 0n && participant !== this.#participant)) {
      throw new BrowserRoomOwnerError("protocol", "snapshot", "Invalid room receipt metadata.");
    }
    if (revision === this.#revision) {
      if (participant !== this.#participant || hasSnapshot !== (this.#snapshot !== null)) {
        throw new BrowserRoomOwnerError("protocol", "snapshot", "Room metadata changed without a receipt.");
      }
      return;
    }
    const snapshot = this.#core("snapshot", session => session.snapshot());
    if (hasSnapshot ? !validSnapshot(snapshot, participant) : snapshot !== null) {
      throw new BrowserRoomOwnerError("protocol", "snapshot", "Malformed complete room snapshot.");
    }
    this.#revision = revision; this.#participant = participant;
    if (!hasSnapshot) return;
    this.#snapshot = snapshot;
    if (snapshot.phase === 2 && !this.#prepared) {
      // Snapshot DTOs are exposed to consumers; preserve independent roster
      // identities before their callback can mutate those arrays or objects.
      this.#roster = new Map(snapshot.members.map(member => [member.participant, Object.freeze(Array.from(member.players))]));
      this.#prepared = true;
    }
    try { Promise.resolve(this.#config.onSnapshot?.(snapshot)).catch(cause => this.#fatal(cause, "callback", "snapshot")); }
    catch (cause) { throw this.#fatal(cause, "callback", "snapshot"); }
  }

  #observeStart() {
    const value = this.#core("take_start", session => session.take_start());
    if (value === null) return;
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new BrowserRoomOwnerError("protocol", "start", "Malformed room start schedule.");
    }
    const { targetNs, songTargetNs, uncertaintyNs } = value;
    if (!this.#prepared || this.#start !== null
      || typeof targetNs !== "bigint" || targetNs < 0n || targetNs > I64_MAX
      || typeof songTargetNs !== "bigint" || songTargetNs < targetNs || songTargetNs > I64_MAX
      || !u64(uncertaintyNs)) {
      throw new BrowserRoomOwnerError("protocol", "start", "Invalid or repeated room start schedule.");
    }
    this.#start = Object.freeze({ targetNs, songTargetNs, uncertaintyNs });

    try { Promise.resolve(this.#config.onStart?.(this.#start, this.#origin)).catch(cause => this.#fatal(cause, "callback", "start")); }
    catch (cause) { throw this.#fatal(cause, "callback", "start"); }
  }

  #observeProgress() {
    const value = this.#core("take_peer_progress", session => session.take_peer_progress());
    if (value === null) return;
    let update;
    try {
      if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Missing room peer prefix.");
      const { participant, sequence, finalPrefix, words } = value;
      if (!u64(participant, 1n) || participant === this.#participant || !u64(sequence, 1n)
        || typeof finalPrefix !== "boolean" || !this.#roster?.has(participant)) {
        throw new Error("Invalid room peer prefix metadata.");
      }
      update = Object.freeze({ participant, sequence, finalPrefix, words: progressWords(words, this.#roster.get(participant)) });
    } catch (cause) {
      throw new BrowserRoomOwnerError("protocol", "progress", "Malformed accepted room peer prefix.", cause);
    }
    // A genuine pending Commit can admit a prefix before local Accept finishes;
    // only the common owner decides that barrier. Do not require onStart here.
    try { Promise.resolve(this.#config.onProgress?.(update)).catch(cause => this.#fatal(cause, "callback", "progress")); }
    catch (cause) { throw this.#fatal(cause, "callback", "progress"); }
  }

  #observeReceipts() {
    const localFinalWritten = this.#core("local_final_written", session => session.local_final_written());
    const localFinalAcknowledged = this.#core("local_final_acknowledged", session => session.local_final_acknowledged());
    const complete = this.#core("progress_complete", session => session.progress_complete());
    const drainComplete = this.#core("drain_complete", session => session.drain_complete());
    if (typeof localFinalWritten !== "boolean" || typeof localFinalAcknowledged !== "boolean" || typeof complete !== "boolean"
      || typeof drainComplete !== "boolean" || (drainComplete && (!complete || !this.#drainRequested))
      || (localFinalAcknowledged && !localFinalWritten) || (complete && !localFinalAcknowledged)) {
      throw new BrowserRoomOwnerError("protocol", "receipts", "Malformed room progress receipts.");
    }
    if (this.#receipts.localFinalWritten === localFinalWritten
      && this.#receipts.localFinalAcknowledged === localFinalAcknowledged && this.#receipts.complete === complete
      && this.#receipts.drainComplete === drainComplete) return;
    this.#receipts = Object.freeze({ localFinalWritten, localFinalAcknowledged, complete, drainComplete });
    try { Promise.resolve(this.#config.onReceipts?.(this.#receipts)).catch(cause => this.#fatal(cause, "callback", "receipts")); }
    catch (cause) { throw this.#fatal(cause, "callback", "receipts"); }
    if (!this.closed && complete) this.#completionGate?.resolve(this.#receipts);
    this.#advanceDrain();
  }

  async #readLoop() {
    while (!this.closed && this.#leaveGate === null && !this.#receipts.drainComplete) {
      const needed = this.#core("needed_bytes", session => session.needed_bytes());
      const pending = this.#core("frame_pending", session => session.frame_pending());
      if (!integer(needed, 1, MAX_FRAME) || typeof pending !== "boolean") {
        throw new BrowserRoomOwnerError("protocol", "read", "Invalid room decoder need.");
      }
      let received;
      try {
        received = await this.#await(this.#track(
          () => this.#channel.readPrefix(needed, !pending),
          bytes => ({ bytes, capturedNs: (this.closed || this.#leaveGate !== null || this.#receipts.drainComplete) ? null : this.#elapsed() })));
      } catch (cause) { if (this.closed || this.#leaveGate !== null || this.#receipts.drainComplete) return; throw cause; }
      if (this.closed || this.#leaveGate !== null || this.#receipts.drainComplete) return;
      const { bytes, capturedNs } = received;
      this.#ensure();
      if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer) || bytes.buffer.resizable === true
        || !integer(bytes.byteLength, 1, needed) || bytes.buffer.byteLength > MAX_CHUNK) {
        throw new BrowserRoomOwnerError("protocol", "read", "Malformed bounded room prefix.");
      }
      try { new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength); }
      catch (cause) { throw new BrowserRoomOwnerError("protocol", "read", "Detached room prefix.", cause); }
      const processingNs = this.#elapsed();
      const consumed = this.#core("receive_bytes", session => session.receive_bytes(bytes, capturedNs, processingNs));
      if (consumed !== bytes.byteLength) throw new BrowserRoomOwnerError("protocol", "read", "Room prefix was not fully consumed.");
      const remains = this.#core("frame_pending", session => session.frame_pending());
      if (typeof remains !== "boolean") throw new BrowserRoomOwnerError("protocol", "read", "Invalid room frame state.");
      this.#advanceSetup();
      if (remains && this.#frameTimer === null) {
        this.#advanceFrame();
      } else if (!remains) { clearTimeout(this.#frameTimer); this.#frameTimer = null; }
      this.#observe();
      this.#observeProgress();
      this.#observeStart();
      this.#observeReceipts();
      this.#wake();
    }
  }

  async #writeLoop() {
    while (!this.closed && !this.#receipts.drainComplete) {
      const processingNs = this.#elapsed();
      const frame = this.#core("next_write", session => session.next_write(processingNs));
      let kind, id, bytes;
      try {
        if (!frame || typeof frame.free !== "function") throw new Error("Missing room write wrapper.");
        kind = frame.kind; id = frame.frame_id;
        if (kind !== 0 && kind !== 1 || !u64(id, kind === 1 ? 1n : 0n) || (kind === 0 && id !== 0n)) throw new Error("Invalid room write metadata.");
        if (kind === 1) {
          if (typeof frame.take_bytes !== "function") throw new Error("Missing room frame bytes.");
          bytes = frame.take_bytes();
          if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer) || bytes.buffer.resizable === true
            || !integer(bytes.byteLength, 11, MAX_FRAME) || bytes.buffer.byteLength > MAX_CHUNK) throw new Error("Invalid room frame extent.");
          new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        }
      } finally { quiet(frame, "free"); }
      if (kind === 0) {
        this.#wakeGate = gate();
        await this.#await(this.#wakeGate.promise);
        continue;
      }
      let completedNs;
      try { completedNs = await this.#await(this.#track(() => this.#channel.write(bytes), () => this.#elapsed())); }
      catch (cause) { throw this.#fatal(cause, "transport", "write"); }
      this.#ensure();
      const writtenNow = this.#elapsed();
      this.#core("written", session => session.written(id, completedNs, writtenNow));
      this.#advanceSetup();
      const left = this.#core("leave_written", session => session.leave_written());
      if (typeof left !== "boolean") throw new BrowserRoomOwnerError("protocol", "leave", "Invalid room leave receipt.");
      if (left) {
        this.#leaveGate?.resolve(); this.#leaveGate = null;
        this.#fail(new BrowserRoomOwnerError("closed", "leave", "Room leave was written."));
      } else {
        this.#observeStart();
        this.#observeReceipts();
        this.#wake();
      }
    }
  }
}

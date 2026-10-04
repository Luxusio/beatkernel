import { WebTransportChannel } from "./multiplayer-transport.mjs";

const TOKEN = Symbol("BrowserRoomOwner");
const MAX_FRAME = 65808;
const MAX_CHUNK = 1024 * 1024;
const U64_MAX = 18446744073709551615n;
const I64_MAX = 9223372036854775807n;
const METHODS = ["request_seal", "request_ready", "request_leave", "needed_bytes",
  "frame_pending", "receive_bytes", "next_write", "written", "participant_id",
  "revision", "has_snapshot", "leave_written", "snapshot", "close", "free"];

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
  const { session, signal, setupTimeoutMs = 10000, ioTimeoutMs = 10000,
    channelFactory = (address, config) => WebTransportChannel.open(address, config), onSnapshot, onClose } = options ?? {};
  if (typeof AbortController !== "function" || !session || METHODS.some(name => typeof session[name] !== "function")
    || typeof channelFactory !== "function" || !integer(setupTimeoutMs, 1, 60000) || !integer(ioTimeoutMs, 1, 60000)
    || (onSnapshot !== undefined && typeof onSnapshot !== "function") || (onClose !== undefined && typeof onClose !== "function")
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
  return { session, signal, setupTimeoutMs, ioTimeoutMs, channelFactory, onSnapshot, onClose };
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
// adapter only moves bounded bytes and publishes validated snapshot DTOs.
export class BrowserRoomOwner {
  #config;
  #session;
  #channel = null;
  #controller = new AbortController();
  #failure = null;
  #closedGate = gate();
  #wakeGate = null;
  #leaveGate = null;
  #setupTimer = null;
  #frameTimer = null;
  #abort = null;
  #loops = [];
  #operations = new Set();
  #closing = null;
  #revision = 0n;
  #participant = 0n;
  #snapshot = null;

  constructor(token, config) {
    if (token !== TOKEN) throw new TypeError("Use BrowserRoomOwner.open().");
    this.#config = config;
    this.#session = config.session;
  }
  get closed() { return this.#failure !== null; }
  get participant() { return this.#participant; }
  get snapshot() { return this.#snapshot; }

  static async open(url, options) {
    const config = configuration(url, options);
    const owner = new BrowserRoomOwner(TOKEN, config);
    try {
      owner.#setupTimer = setTimeout(() => owner.#fail(new BrowserRoomOwnerError("timeout", "setup", "Room admission timed out.")), config.setupTimeoutMs);
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
        || owner.#core("initial", session => session.leave_written()) !== false) {
        throw new BrowserRoomOwnerError("protocol", "open", "Room session is already in use.");
      }
      const opening = owner.#track(() => config.channelFactory(url, { signal: owner.#controller.signal,
        setupTimeoutMs: config.setupTimeoutMs, ioTimeoutMs: config.ioTimeoutMs, maxPrefixBytes: MAX_FRAME }));
      opening.then(channel => { if (owner.closed) quiet(channel, "close"); }, () => {});
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
      await owner.close();
      throw failure;
    }
  }

  #ensure() { if (this.#failure) throw this.#failure; }
  #await(promise) { return Promise.race([promise, this.#closedGate.promise]); }
  #track(action) {
    // Register before calling a channel: it may trigger cancellation reentrantly.
    // Join the channel API promise, while platform cleanup remains channel-owned.
    const operation = gate();
    this.#operations.add(operation.promise);
    operation.promise.then(() => this.#operations.delete(operation.promise),
      () => this.#operations.delete(operation.promise));
    try { Promise.resolve(action()).then(operation.resolve, operation.reject); }
    catch (cause) { operation.reject(cause); }
    return operation.promise;
  }
  #fatal(cause, code, operation) {
    return this.#fail(cause instanceof BrowserRoomOwnerError ? cause
      : new BrowserRoomOwnerError(code, operation, `Room ${operation} failed.`, cause));
  }
  #core(operation, action) {
    this.#ensure();
    try { return action(this.#session); } catch (cause) { throw this.#fatal(cause, "core", operation); }
  }
  #fail(error) {
    if (this.#failure) return this.#failure;
    this.#failure = error;
    clearTimeout(this.#setupTimer); clearTimeout(this.#frameTimer);
    this.#setupTimer = null; this.#frameTimer = null;
    if (this.#abort !== null) {
      try { this.#config.signal.removeEventListener("abort", this.#abort); } catch {}
      this.#abort = null;
    }
    this.#closedGate.reject(error);
    this.#leaveGate?.reject(error); this.#leaveGate = null;
    this.#wake();
    try { this.#controller.abort(); } catch {}
    quiet(this.#channel, "close"); this.#channel = null;
    const session = this.#session; this.#session = null;
    quiet(session, "close"); quiet(session, "free");
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
  leave() {
    if (this.#leaveGate !== null) return this.#leaveGate.promise;
    try { this.#request("request_leave"); } catch (cause) { return Promise.reject(cause); }
    this.#leaveGate = gate();
    return this.#leaveGate.promise;
  }
  close() {
    this.#fail(new BrowserRoomOwnerError("closed", "close", "Room owner closed."));
    this.#closing ??= Promise.allSettled([...this.#loops, ...this.#operations]).then(() => undefined);
    return this.#closing;
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
    clearTimeout(this.#setupTimer); this.#setupTimer = null;
    try { Promise.resolve(this.#config.onSnapshot?.(snapshot)).catch(cause => this.#fatal(cause, "callback", "snapshot")); }
    catch (cause) { throw this.#fatal(cause, "callback", "snapshot"); }
  }

  async #readLoop() {
    while (!this.closed) {
      const needed = this.#core("needed_bytes", session => session.needed_bytes());
      const pending = this.#core("frame_pending", session => session.frame_pending());
      if (!integer(needed, 1, MAX_FRAME) || typeof pending !== "boolean") {
        throw new BrowserRoomOwnerError("protocol", "read", "Invalid room decoder need.");
      }
      const bytes = await this.#await(this.#track(() => this.#channel.readPrefix(needed, !pending)));
      this.#ensure();
      if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer) || bytes.buffer.resizable === true
        || !integer(bytes.byteLength, 1, needed) || bytes.buffer.byteLength > MAX_CHUNK) {
        throw new BrowserRoomOwnerError("protocol", "read", "Malformed bounded room prefix.");
      }
      try { new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength); }
      catch (cause) { throw new BrowserRoomOwnerError("protocol", "read", "Detached room prefix.", cause); }
      const consumed = this.#core("receive_bytes", session => session.receive_bytes(bytes));
      if (consumed !== bytes.byteLength) throw new BrowserRoomOwnerError("protocol", "read", "Room prefix was not fully consumed.");
      const remains = this.#core("frame_pending", session => session.frame_pending());
      if (typeof remains !== "boolean") throw new BrowserRoomOwnerError("protocol", "read", "Invalid room frame state.");
      if (remains && this.#frameTimer === null) {
        this.#frameTimer = setTimeout(() => this.#fail(new BrowserRoomOwnerError("timeout", "frame", "Incomplete room frame timed out.")), this.#config.ioTimeoutMs);
      } else if (!remains) { clearTimeout(this.#frameTimer); this.#frameTimer = null; }
      this.#observe();
    }
  }

  async #writeLoop() {
    while (!this.closed) {
      const frame = this.#core("next_write", session => session.next_write());
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
      try { await this.#await(this.#track(() => this.#channel.write(bytes))); }
      catch (cause) { throw this.#fatal(cause, "transport", "write"); }
      this.#ensure();
      this.#core("written", session => session.written(id));
      const left = this.#core("leave_written", session => session.leave_written());
      if (typeof left !== "boolean") throw new BrowserRoomOwnerError("protocol", "leave", "Invalid room leave receipt.");
      if (left) {
        this.#leaveGate?.resolve(); this.#leaveGate = null;
        this.#fail(new BrowserRoomOwnerError("closed", "leave", "Room leave was written."));
      }
    }
  }
}

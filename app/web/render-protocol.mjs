// Visual packets have no gameplay, audio or completion authority.
export const HEADER_BYTES = 40;
export const U64_MAX = 18446744073709551615n;
export const unsignedIdentity = value => typeof value === "bigint" && value > 0n && value <= U64_MAX;
export const boundedU32 = value => Number.isInteger(value) && value >= 0 && value <= 0xffffffff;
const typed = Object.getPrototypeOf(Uint8Array.prototype);
const byteLength = Object.getOwnPropertyDescriptor(typed, "byteLength").get;
const byteOffset = Object.getOwnPropertyDescriptor(typed, "byteOffset").get;
const backing = Object.getOwnPropertyDescriptor(typed, "buffer").get;
const arrayLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "byteLength").get;
const resizable = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "resizable")?.get;

export function nativePacketView(packet) {
  return new Uint8Array(backing.call(packet), byteOffset.call(packet), byteLength.call(packet));
}

export function preflightPacket(packet, maxPacketBytes) {
  if (!boundedU32(maxPacketBytes) || maxPacketBytes < HEADER_BYTES
    || Object.getPrototypeOf(packet) !== Uint8Array.prototype) throw new Error("Invalid visual packet or trusted packet limit.");
  // Intrinsic getters reject proxies, detached stores and spoofed caller fields.
  const buffer = backing.call(packet);
  const length = byteLength.call(packet);
  const offset = byteOffset.call(packet);
  const backingLength = arrayLength.call(buffer);
  if (resizable?.call(buffer) === true || backingLength > maxPacketBytes || length < HEADER_BYTES || length > maxPacketBytes) throw new Error("Visual packet exceeds its fixed backing or trusted byte limit.");
  const bytes = new Uint8Array(buffer, offset, length);
  const view = new DataView(buffer, offset, length);
  if (bytes[0] !== 66 || bytes[1] !== 75 || bytes[2] !== 82 || bytes[3] !== 86 || view.getUint16(4, true) !== 1) throw new Error("Invalid visual envelope magic or version.");
  const kind = view.getUint16(6, true);
  const generation = view.getBigUint64(8, true);
  const content = view.getBigUint64(16, true);
  const sequence = view.getBigUint64(24, true);
  const payloadLen = view.getBigUint64(32, true);
  if (kind < 1 || kind > 6 || !unsignedIdentity(generation) || !unsignedIdentity(content)
    || payloadLen !== BigInt(length - HEADER_BYTES)
    || ((kind === 2 || kind === 3) ? sequence === 0n : sequence !== 0n)) throw new Error("Invalid visual envelope identity, sequence or exact extent.");
  return Object.freeze({ kind, generation, content, sequence, payloadLen });
}

export function validateRenderLimits(maxPacketBytes, maxDiagnosticBytes) {
  if (!boundedU32(maxPacketBytes) || maxPacketBytes < HEADER_BYTES || !boundedU32(maxDiagnosticBytes)) throw new Error("Invalid trusted render limits.");
}

export function preflightMenuPacket(packet) {
  if (Object.getPrototypeOf(packet) !== Uint8Array.prototype) throw new Error("Invalid menu packet.");
  const buffer = backing.call(packet), length = byteLength.call(packet), offset = byteOffset.call(packet);
  if (resizable?.call(buffer) === true || arrayLength.call(buffer) > 4 * 1024 * 1024 || length < 48 || length > 4 * 1024 * 1024) throw new Error("Menu packet exceeds its fixed byte limit.");
  const bytes = new Uint8Array(buffer, offset, length), view = new DataView(buffer, offset, length);
  const generation = view.getBigUint64(4, true), screen = view.getBigUint64(12, true), revision = view.getBigUint64(20, true);
  const route = view.getUint32(28, true), selected = view.getUint32(32, true), count = view.getUint32(36, true);
  if (bytes[0] !== 66 || bytes[1] !== 75 || bytes[2] !== 77 || bytes[3] !== 78 || !unsignedIdentity(generation)
    || !unsignedIdentity(screen) || !unsignedIdentity(revision) || route < 1 || route > 10 || count > 8192
    || (count === 0 ? selected !== 0 : selected >= count) || bytes[40] > 1 || bytes[41] !== 0 || bytes[42] !== 0 || bytes[43] !== 0) throw new Error("Invalid menu envelope.");
  return Object.freeze({ generation, screen, revision, route, selected, pending: bytes[40] === 1 });
}
function preflightRecordPreview(packet, menu) {
  if (Object.getPrototypeOf(packet) !== Uint8Array.prototype) throw new Error("Invalid record preview packet.");
  const buffer = backing.call(packet), length = byteLength.call(packet), offset = byteOffset.call(packet);
  if (resizable?.call(buffer) === true || arrayLength.call(buffer) > 4 * 1024 * 1024 || length < 28 || length > 4 * 1024 * 1024) throw new Error("Record preview exceeds its fixed byte limit.");
  const bytes = new Uint8Array(buffer, offset, length), view = new DataView(buffer, offset, length);
  if (bytes[0] !== 66 || bytes[1] !== 75 || bytes[2] !== 82 || bytes[3] !== 80
    || view.getBigUint64(4, true) !== menu.generation || view.getBigUint64(12, true) !== menu.screen
    || view.getBigUint64(20, true) !== menu.revision || menu.route !== 4) throw new Error("Foreign record preview token.");
}
export function preflightMenuPayload(packet, recordPreview) {
  const header = preflightMenuPacket(packet);
  if (recordPreview !== undefined && recordPreview !== null) {
    preflightRecordPreview(recordPreview, header);
    const first = backing.call(packet), second = backing.call(recordPreview);
    if (first === second || arrayLength.call(first) + arrayLength.call(second) > 4 * 1024 * 1024) throw new Error("Combined menu preview exceeds its independent bounded stores.");
  }
  return header;
}
export function menuOpponentProjection(input) {
  const { count, own, other } = input ?? { count: 0, own: 0, other: 0 };
  if (![count, own, other].every(boundedU32) || count > 8 || own + other > count) throw new Error("Invalid menu opponent projection.");
  return Object.freeze({ count, own, other });
}

export class RenderClient {
  #port; #generation; #content; #limit; #diagnostics; #timeout; #onError; #onGeometry;
  #pending = null; #dirty = null; #operationId = 0n; #state = "ready"; #failure = null;
  #lastSequence = 0n; #registered = false; #mode = null;
  #geometryRequested = 0n; #geometryAcknowledged = 0n;
  #retiring = false;
  #menu = null;

  constructor(options = {}) {
    const { port, generation, content, maxPacketBytes, maxDiagnosticBytes, timeoutMs, onError, onGeometry } = options;
    validateRenderLimits(maxPacketBytes, maxDiagnosticBytes);
    if (!unsignedIdentity(generation) || !unsignedIdentity(content) || !Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60000
      || !port || !["postMessage", "start", "close"].every(name => typeof port[name] === "function")) throw new Error("Invalid bounded render client configuration.");
    this.#port = port; this.#generation = generation; this.#content = content;
    this.#limit = maxPacketBytes; this.#diagnostics = maxDiagnosticBytes; this.#timeout = timeoutMs;
    this.#onError = onError; this.#onGeometry = onGeometry;
    try {
      port.onmessage = event => this.#message(event.data);
      port.onmessageerror = () => this.#fail(new Error("Render response could not be decoded."));
      port.start();
    } catch (error) { this.#fail(error); throw error; }
  }
  get pending() { return this.#pending !== null; }
  get state() { return this.#state; }
  get failure() { return this.#failure; }
  async menu(packet, options = {}) {
    this.#available();
    const { onAck, geometryVersion, recordPreview } = options;
    const opponents = menuOpponentProjection(options.opponents);
    const header = preflightMenuPayload(packet, recordPreview);
    const transfers = [backing.call(packet)];
    if (recordPreview !== undefined && recordPreview !== null) {
      transfers.push(backing.call(recordPreview));
    }
    if (this.#menu && (header.generation !== this.#menu.generation || header.revision <= this.#menu.revision)) throw new Error("Stale menu snapshot.");
    if (geometryVersion !== undefined) {
      if (!unsignedIdentity(geometryVersion) || geometryVersion <= this.#geometryRequested) throw new Error("Menu geometry version must increase.");
      this.#geometryRequested = geometryVersion;
    }
    return this.#request("menu", { packet, geometryVersion, recordPreview, opponents }, { menuHeader: header, onAck }, transfers);
  }
  #available() {
    if (this.#failure) throw this.#failure;
    if (this.#state !== "ready") throw new Error("Render client is closed.");
    if (this.#retiring) throw new Error("Render client is retiring.");
    if (this.#pending) throw new Error("A render operation is pending.");
    if (this.#operationId === U64_MAX) throw new Error("Render operation identity exhausted.");
  }
  async packet(packet, options = {}) {
    this.#available();
    const { mode, onAck, geometryVersion } = options;
    const header = preflightPacket(packet, this.#limit);
    if (header.generation !== this.#generation || header.content !== this.#content) throw new Error("Foreign render packet identity.");
    if (header.kind === 1 && !["preview", "live", "local", "replay"].includes(mode)) throw new Error("Visual registration needs an explicit mode.");
    const staticPacket = [1, 4, 5, 6].includes(header.kind);
    const combinedRoom = header.kind === 6 && this.#mode === "results";
    if (staticPacket ? this.#registered && !combinedRoom : !this.#registered || header.sequence <= this.#lastSequence) throw new Error("Stale visual registration or state.");
    if (!staticPacket && ((header.kind === 3) !== (this.#mode === "preview"))) throw new Error("Visual packet does not match registered mode.");
    this.#available();
    if (geometryVersion !== undefined) {
      if (!unsignedIdentity(geometryVersion) || geometryVersion <= this.#geometryRequested) throw new Error("Packet geometry version must increase.");
      this.#geometryRequested = geometryVersion;
    }
    return this.#request("packet", { packet, mode, geometryVersion }, { header, onAck, mode }, [backing.call(packet)]);
  }
  publish(buildPacket, onAck, options = {}) {
    if (typeof buildPacket !== "function") throw new Error("Visual publication requires a packet builder.");
    const snapshot = { mode: options.mode, geometryVersion: options.geometryVersion };
    if (this.#failure || this.#state !== "ready" || this.#retiring) this.#available();
    if (this.#pending) { this.#dirty = { buildPacket, onAck, options: snapshot }; return false; }
    const packet = buildPacket();
    this.packet(packet, { ...snapshot, onAck }).catch(error => { if (!this.#retiring) this.#fail(error); });
    return true;
  }
  async control(operation, fields = {}) {
    this.#available();
    const { width, height, page, comparisons, details, geometryVersion } = fields;
    let snapshot;
    if (operation === "retire") snapshot = {};
    else if (operation === "resize" && boundedU32(width) && boundedU32(height) && unsignedIdentity(geometryVersion)) snapshot = { width, height, geometryVersion };
    else if (operation === "page" && boundedU32(page) && typeof comparisons === "boolean" && unsignedIdentity(geometryVersion)) snapshot = { page, comparisons, geometryVersion };
    else if (operation === "room-page" && boundedU32(page) && unsignedIdentity(geometryVersion)) snapshot = { page, geometryVersion };
    else if (operation === "menu-details" && boundedU32(page) && typeof details === "boolean" && unsignedIdentity(geometryVersion)) snapshot = { page, details, geometryVersion };
    else throw new Error("Invalid render control.");
    this.#available();
    if (operation !== "retire") {
      if (geometryVersion <= this.#geometryRequested) throw new Error("Geometry version must increase.");
      this.#geometryRequested = geometryVersion;
    }
    return this.#request(operation, snapshot, { geometryVersion });
  }
  async retire() {
    if (this.#state !== "ready" || this.#retiring) this.#available();
    // Port ordering puts retirement after the already-sent snapshot, while
    // fencing its ACK locally before any queued exporter can run.
    const pending = this.#pending;
    this.#pending = null;
    this.#dirty = null;
    if (pending) { clearTimeout(pending.timer); pending.reject(new Error("Render operation retired.")); }
    this.#available();
    this.#retiring = true;
    const ack = await this.#request("retire", {}, {});
    this.close();
    return ack;
  }
  #request(operation, fields, detail, transfers = []) {
    let resolve, reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    promise.catch(() => {});
    const operationId = ++this.#operationId;
    const pending = { operation, operationId, ...detail, resolve, reject, timer: null };
    this.#pending = pending;
    pending.timer = setTimeout(() => {
      if (this.#pending === pending) this.#fail(new Error("Render acknowledgement timed out."));
    }, this.#timeout);
    try { this.#port.postMessage({ kind: operation, operationId, generation: this.#generation, content: this.#content, ...fields }, transfers); }
    catch (error) { this.#fail(error); }
    return promise;
  }
  #message(input) {
    if (this.#state !== "ready" || !input || typeof input !== "object") return;
    const { kind, generation, content, operationId, sequence, packetKind, operation, geometryVersion, page, width, height, message,
      menuGeneration, screen, revision, details } = input;
    if (kind === "ready") return;
    if (generation !== this.#generation || content !== this.#content) return;
    if (kind === "geometry-ack") {
      if (this.#retiring || !unsignedIdentity(geometryVersion) || geometryVersion !== this.#geometryRequested || geometryVersion <= this.#geometryAcknowledged
        || !boundedU32(page) || !boundedU32(width) || !boundedU32(height) || width === 0 || height === 0) return;
      if (this.#menu && (menuGeneration !== this.#menu.generation || screen !== this.#menu.screen || revision !== this.#menu.revision)) return;
      if (this.#menu && details !== undefined && typeof details !== "boolean") return;
      this.#geometryAcknowledged = geometryVersion;
      try { this.#onGeometry?.(Object.freeze({ generation, content, geometryVersion, page, width, height,
        ...(this.#menu ? { menuGeneration, screen, revision, ...(details === undefined ? {} : { details }) } : {}) })); } catch (error) { this.#fail(error); }
      return;
    }
    if (kind === "render-wait") return;
    const pending = this.#pending;
    if (kind === "render-error") {
      if (typeof message !== "string" || message.length > this.#diagnostics) this.#fail(new Error("Invalid renderer diagnostic."));
      else this.#fail(new Error(message));
      return;
    }
    if (!pending || operationId !== pending.operationId) return;
    if (pending.menuHeader) {
      const header = pending.menuHeader;
      if (kind !== "menu-ack" || menuGeneration !== header.generation || screen !== header.screen || revision !== header.revision) return;
      try { if (pending.onAck?.(header) === false) throw new Error("Menu producer refused its exact acknowledgement."); }
      catch (error) { this.#fail(error); return; }
      if (this.#state !== "ready" || this.#pending !== pending) return;
      clearTimeout(pending.timer); this.#menu = header; this.#pending = null;
      pending.resolve(Object.freeze({ kind, operationId, generation, content, menuGeneration, screen, revision }));
      return;
    }
    if (pending.operation === "packet" ? kind !== "state-ack" || sequence !== pending.header.sequence || packetKind !== pending.header.kind
      : kind !== "control-ack" || operation !== pending.operation || geometryVersion !== pending.geometryVersion) {
      return;
    }
    clearTimeout(pending.timer);
    // Keep pending until the Rust producer has adopted exactly this snapshot.
    try {
      if (pending.header) {
        if (pending.onAck?.(pending.header) === false) throw new Error("Visual producer refused the exact state acknowledgement.");
        this.#registered = true;
        this.#lastSequence = pending.header.sequence;
        this.#mode = pending.header.kind === 1 ? pending.mode : pending.header.kind === 6 && this.#mode === "results" ? "results"
          : ({ 4: "history", 5: "results", 6: "room" }[pending.header.kind] ?? this.#mode);
      }
    } catch (error) { this.#fail(error); return; }
    if (this.#state !== "ready" || this.#pending !== pending) return;
    this.#pending = null;
    pending.resolve(Object.freeze({ kind, operationId, generation, content, sequence, packetKind, operation, geometryVersion }));
    const dirty = this.#dirty;
    this.#dirty = null;
    if (dirty && pending.operation !== "retire") {
      try { this.publish(dirty.buildPacket, dirty.onAck, dirty.options); } catch (error) { this.#fail(error); }
    }
  }
  #fail(error, state = "failed") {
    if (this.#state !== "ready") return;
    this.#state = state; this.#failure = error; this.#dirty = null;
    const pending = this.#pending;
    this.#pending = null;
    if (pending) { clearTimeout(pending.timer); pending.reject(error); }
    // The gameplay Worker owns the lifetime port; a new visual generation may
    // attach another client after this one retires. Never close that port here.
    try { this.#port.onmessage = null; this.#port.onmessageerror = null; } catch {}
    if (state === "failed") { try { this.#onError?.(error); } catch {} }
  }
  close() { this.#fail(new Error("Render client closed."), "closed"); }
}

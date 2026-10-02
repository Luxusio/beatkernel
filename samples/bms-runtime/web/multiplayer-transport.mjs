// Reliable byte transport only. The Rust Session owns BKMP interpretation,
// write receipts, probes and start agreement; this module never creates them.
const OWNER = Symbol("WebTransportChannel");
const MAX_PREFIX = 65547;
const MAX_CHUNK = 1024 * 1024;

export class WebTransportChannelError extends Error {
  constructor(code, operation, message, cause) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "WebTransportChannelError";
    this.code = code;
    this.operation = operation;
  }
}

function integer(value, min, max) {
  return Number.isSafeInteger(value) && value >= min && value <= max;
}

function quietCall(object, method) {
  try { Promise.resolve(object?.[method]()).catch(() => {}); } catch {}
}

function disposeStream(stream) {
  quietCall(stream?.readable, "cancel");
  quietCall(stream?.writable, "abort");
}

function configuration(url, options) {
  if (options === null || typeof options !== "object") {
    throw new WebTransportChannelError("validation", "open", "Invalid WebTransport options.");
  }
  const { signal, setupTimeoutMs = 10000, ioTimeoutMs = 10000 } = options;
  if (typeof url !== "string" || !url.length || url.length > 4096
    || !integer(setupTimeoutMs, 1, 60000) || !integer(ioTimeoutMs, 1, 60000)
    || (signal !== undefined && (signal === null || typeof signal.aborted !== "boolean"
      || typeof signal.addEventListener !== "function" || typeof signal.removeEventListener !== "function"))) {
    throw new WebTransportChannelError("validation", "open", "Invalid bounded WebTransport configuration.");
  }
  let address;
  try { address = new URL(url); }
  catch (cause) { throw new WebTransportChannelError("validation", "open", "WebTransport requires an absolute HTTPS URL.", cause); }
  if (address.protocol !== "https:" || address.username || address.password || url.includes("#")) {
    throw new WebTransportChannelError("validation", "open", "WebTransport requires HTTPS without credentials or a fragment.");
  }
  let factory;
  try { factory = options.factory === undefined ? globalThis.WebTransport : options.factory; }
  catch (cause) { throw new WebTransportChannelError("unavailable", "open", "WebTransport is unavailable.", cause); }
  if (typeof factory !== "function") {
    throw new WebTransportChannelError("unavailable", "open", "WebTransport is unavailable.");
  }
  return { url: address.href, factory, signal, setupTimeoutMs, ioTimeoutMs };
}

export class WebTransportChannel {
  #config;
  #transport = null;
  #reader = null;
  #writer = null;
  #failure = null;
  #pending = new Set();
  #abort = null;
  #reading = false;
  #writing = false;
  #chunk = null;
  #offset = 0;

  constructor(token, config) {
    if (token !== OWNER) throw new TypeError("Use WebTransportChannel.open().");
    this.#config = config;
  }

  get closed() { return this.#failure !== null; }

  static async open(url, options = {}) {
    const config = configuration(url, options);
    if (config.signal?.aborted) {
      throw new WebTransportChannelError("aborted", "open", "WebTransport opening was cancelled.");
    }
    const channel = new WebTransportChannel(OWNER, config);
    if (config.signal !== undefined) {
      channel.#abort = () => channel.#fail(new WebTransportChannelError("aborted", "abort", "WebTransport was cancelled."));
      config.signal.addEventListener("abort", channel.#abort, { once: true });
    }
    if (config.signal?.aborted) channel.#abort();
    await channel.#run("open", config.setupTimeoutMs, async () => {
      channel.#ensureOpen();
      const transport = new config.factory(config.url);
      channel.#transport = transport;
      if (channel.closed) { quietCall(transport, "close"); channel.#ensureOpen(); }
      if (!transport.ready || typeof transport.ready.then !== "function"
        || !transport.closed || typeof transport.closed.then !== "function"
        || typeof transport.createBidirectionalStream !== "function" || typeof transport.close !== "function") {
        throw new WebTransportChannelError("transport", "open", "WebTransport connection is malformed.");
      }
      Promise.resolve(transport.closed).then(
        () => channel.#fail(new WebTransportChannelError("closed", "remote", "WebTransport peer closed the connection.")),
        cause => channel.#fail(new WebTransportChannelError("transport", "remote", "WebTransport connection failed.", cause)),
      );
      await transport.ready;
      channel.#ensureOpen();
      const stream = await transport.createBidirectionalStream();
      if (channel.closed) { disposeStream(stream); channel.#ensureOpen(); }
      try {
        channel.#reader = stream.readable.getReader();
        channel.#writer = stream.writable.getWriter();
      } catch (cause) {
        disposeStream(stream);
        throw cause;
      }
      if (typeof channel.#reader?.read !== "function" || typeof channel.#writer?.write !== "function") {
        throw new WebTransportChannelError("transport", "open", "WebTransport stream is malformed.");
      }
    });
    channel.#ensureOpen();
    return channel;
  }

  #ensureOpen() {
    if (this.#failure) throw this.#failure;
  }

  #fail(error) {
    if (this.#failure) return this.#failure;
    this.#failure = error;
    this.#chunk = null;
    this.#offset = 0;
    if (this.#abort !== null) {
      try { this.#config.signal.removeEventListener("abort", this.#abort); } catch {}
      this.#abort = null;
    }
    for (const cancel of this.#pending) cancel(error);
    quietCall(this.#reader, "cancel");
    quietCall(this.#writer, "abort");
    quietCall(this.#reader, "releaseLock");
    quietCall(this.#writer, "releaseLock");
    quietCall(this.#transport, "close");
    this.#reader = null;
    this.#writer = null;
    return error;
  }

  #run(operation, timeoutMs, work) {
    return new Promise((resolve, reject) => {
      if (this.#failure) { reject(this.#failure); return; }
      let settled = false;
      let timer;
      const finish = (error, value) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.#pending.delete(cancel);
        if (error) reject(error);
        else resolve(value);
      };
      const cancel = error => finish(error);
      const fail = cause => {
        const error = cause instanceof WebTransportChannelError ? cause
          : new WebTransportChannelError("transport", operation, `WebTransport ${operation} failed.`, cause);
        this.#fail(error);
      };
      this.#pending.add(cancel);
      timer = setTimeout(() => this.#fail(new WebTransportChannelError("timeout", operation,
        `WebTransport ${operation} timed out.`)), timeoutMs);
      // The same timer covers ready plus stream creation during open. Every
      // promise has both handlers, even after timeout/cancellation wins.
      try { Promise.resolve(work()).then(value => finish(this.#failure, value), fail); }
      catch (cause) { fail(cause); }
    });
  }

  async readPrefix(maxBytes) {
    this.#ensureOpen();
    if (this.#reading) throw new WebTransportChannelError("busy", "read", "A WebTransport read is already pending.");
    if (!integer(maxBytes, 1, MAX_PREFIX)) {
      throw new WebTransportChannelError("validation", "read", "Read prefix must be between 1 and 65547 bytes.");
    }
    this.#reading = true;
    try {
      return await this.#run("read", this.#config.ioTimeoutMs, async () => {
        let empty = 0;
        while (this.#chunk === null) {
          const result = await this.#reader.read();
          this.#ensureOpen();
          if (result === null || typeof result !== "object" || typeof result.done !== "boolean") {
            throw new WebTransportChannelError("protocol", "read", "WebTransport read result is malformed.");
          }
          if (result.done) throw new WebTransportChannelError("closed", "read", "WebTransport stream reached EOF.");
          const chunk = result.value;
          if (!(chunk instanceof Uint8Array) || !(chunk.buffer instanceof ArrayBuffer)
            || chunk.byteLength > MAX_CHUNK || chunk.buffer.byteLength > MAX_CHUNK) {
            throw new WebTransportChannelError("protocol", "read", "WebTransport chunk exceeds the 1 MiB retention limit or is malformed.");
          }
          if (chunk.byteLength === 0) {
            if (++empty >= 16) throw new WebTransportChannelError("protocol", "read", "Too many empty WebTransport chunks.");
            continue;
          }
          this.#chunk = chunk;
          this.#offset = 0;
        }
        const chunk = this.#chunk;
        const length = Math.min(maxBytes, chunk.byteLength - this.#offset);
        const prefix = new Uint8Array(chunk.buffer, chunk.byteOffset + this.#offset, length);
        this.#offset += length;
        if (this.#offset === chunk.byteLength) { this.#chunk = null; this.#offset = 0; }
        return prefix;
      });
    } finally { this.#reading = false; }
  }

  async write(bytes) {
    this.#ensureOpen();
    if (this.#writing) throw new WebTransportChannelError("busy", "write", "A WebTransport write is already pending.");
    if (!(bytes instanceof Uint8Array) || !(bytes.buffer instanceof ArrayBuffer)
      || !integer(bytes.byteLength, 1, MAX_PREFIX)) {
      throw new WebTransportChannelError("validation", "write", "Write requires 1–65547 bytes in a Uint8Array.");
    }
    // Copy only this bounded view before any await. Caller mutation cannot alter
    // an admitted frame, and no caller buffer is transferred or detached.
    let snapshot;
    try { snapshot = new Uint8Array(bytes.byteLength); snapshot.set(bytes); }
    catch (cause) { throw this.#fail(new WebTransportChannelError("transport", "write", "WebTransport write snapshot failed.", cause)); }
    this.#writing = true;
    try {
      await this.#run("write", this.#config.ioTimeoutMs, () => this.#writer.write(snapshot));
    } finally { this.#writing = false; }
  }

  // Best effort, nonblocking and idempotent. Pending OS/browser cleanup promises
  // are consumed; this does not claim they finished or the peer received bytes.
  close() {
    this.#fail(new WebTransportChannelError("closed", "close", "WebTransport channel was closed."));
  }
}

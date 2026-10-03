import { millisecondsToNanos } from "./play-model.mjs";

const U64_MAX = 18446744073709551615n;
const FILTER_KEYS = new Set(["vendorId", "productId", "usagePage", "usage"]);

function filtersSnapshot(filters) {
  if (!Array.isArray(filters) || filters.length > 16) throw new Error("HID filters require at most sixteen dictionaries.");
  const result = [];
  for (const filter of filters) {
    if (!filter || typeof filter !== "object" || Array.isArray(filter)) throw new Error("Invalid HID filter dictionary.");
    const copy = {};
    const keys = Reflect.ownKeys(filter);
    if (keys.length === 0) throw new Error("HID filter dictionaries must not be empty.");
    for (const key of keys) {
      const value = filter[key];
      const maximum = key === "vendorId" ? 0xffffffff : 65535;
      if (!FILTER_KEYS.has(key) || !Number.isInteger(value) || value < 0 || value > maximum) {
        throw new Error("HID filter fields require known unsigned integers: vendorId is thirty-two-bit, other fields sixteen-bit.");
      }
      copy[key] = value;
    }
    if ((copy.productId !== undefined && copy.vendorId === undefined)
      || (copy.usage !== undefined && copy.usagePage === undefined)) {
      throw new Error("HID product and usage filters require their vendor and usage page.");
    }
    result.push(Object.freeze(copy));
  }
  return Object.freeze(result);
}

// Callbacks are synchronous acquisition hooks. They may queue bounded work,
// but this owner neither decodes descriptors nor waits on report consumers.
export class HidInputOwner {
  #hid;
  #nextSequence;
  #onReport;
  #onDisconnect;
  #onError;
  #maxDevices;
  #maxReportBytes;
  #nextSource;
  #devices = new Map();
  #pending = null;
  #closing = new Map();
  #closePromise = null;
  #closed = false;
  #failure = null;
  #cleanupFailure = null;
  #disconnectListener;

  constructor({ hid, nextSequence, onReport, onDisconnect, onError,
    maxDevices = 16, maxReportBytes = 1024, firstSource = 3n } = {}) {
    if (!hid || ["getDevices", "requestDevice", "addEventListener", "removeEventListener"].some(name => typeof hid[name] !== "function")) {
      throw new Error("WebHID is unavailable or lacks its device ownership API.");
    }
    if ([nextSequence, onReport, onDisconnect, onError].some(callback => typeof callback !== "function")
      || !Number.isInteger(maxDevices) || maxDevices < 1 || maxDevices > 16
      || !Number.isInteger(maxReportBytes) || maxReportBytes < 1 || maxReportBytes > 1024
      || typeof firstSource !== "bigint" || firstSource < 3n || firstSource > U64_MAX) {
      throw new Error("Invalid HID callbacks, device/report limits or first source identity.");
    }
    this.#hid = hid;
    this.#nextSequence = nextSequence;
    this.#onReport = onReport;
    this.#onDisconnect = onDisconnect;
    this.#onError = onError;
    this.#maxDevices = maxDevices;
    this.#maxReportBytes = maxReportBytes;
    this.#nextSource = firstSource;
    this.#disconnectListener = event => this.#disconnect(event);
    try {
      hid.addEventListener("disconnect", this.#disconnectListener);
    } catch (cause) {
      // No interfaces have been opened by this constructor.
      try { hid.removeEventListener("disconnect", this.#disconnectListener); } catch {}
      throw new Error("HID disconnect listener setup failed.", { cause });
    }
  }

  get closed() { return this.#closed; }
  get failure() { return this.#failure; }
  get devices() {
    const result = [];
    for (const entry of this.#devices.values()) {
      if (entry.ready) result.push(Object.freeze({ source: entry.source, device: entry.device }));
    }
    return Object.freeze(result);
  }

  connectAuthorized() {
    return this.#discover(() => this.#hid.getDevices());
  }

  requestDevices(filters = []) {
    let snapshot;
    try { snapshot = filtersSnapshot(filters); } catch (error) { return Promise.reject(error); }
    // #discover invokes this callback synchronously, before its first await.
    return this.#discover(() => this.#hid.requestDevice({ filters: snapshot }));
  }

  #discover(discover) {
    if (this.#closed) return Promise.reject(this.#failure ?? new Error("HID owner is closed."));
    if (this.#pending) return Promise.reject(new Error("HID device setup is busy."));
    let resolve;
    let reject;
    const pending = new Promise((accept, refuse) => { resolve = accept; reject = refuse; });
    // Install ownership before invoking native code, including reentrant close.
    this.#pending = pending;
    const result = pending.catch(cause => {
      if (!this.#closed) this.#fail(new Error("HID device setup failed.", { cause }));
      throw this.#failure ?? cause;
    }).finally(() => {
      if (this.#pending === pending) this.#pending = null;
    });
    try {
      Promise.resolve(discover()).then(devices => this.#adopt(devices)).then(resolve, reject);
    } catch (cause) {
      reject(cause);
    }
    return result;
  }

  async #adopt(devices) {
    if (this.#closed) throw this.#failure ?? new Error("HID owner closed during device discovery.");
    if (!Array.isArray(devices) || devices.length > 16) throw new Error("HID discovery exceeds sixteen interfaces.");
    const seen = new Set();
    const additions = [];
    let nextSource = this.#nextSource;
    for (const device of devices) {
      if (seen.has(device)) continue;
      seen.add(device);
      if (this.#devices.has(device)) continue;
      if (this.#closing.has(device)) throw new Error("HID interface is still closing.");
      if (!device || typeof device !== "object" || typeof device.opened !== "boolean"
        || ["open", "close", "addEventListener", "removeEventListener"].some(name => typeof device[name] !== "function")) {
        throw new Error("Invalid HID device ownership interface.");
      }
      if (device.opened) throw new Error("HID interface is already opened externally.");
      if (this.#devices.size + this.#closing.size + additions.length >= this.#maxDevices) throw new Error("HID device capacity exceeded.");
      if (nextSource > U64_MAX) throw new Error("HID source identity exhausted.");
      additions.push({ device, source: nextSource++, owned: false, ready: false, listening: false, listener: null, closePromise: null });
    }
    this.#nextSource = nextSource;
    for (const entry of additions) {
      if (this.#closed) throw this.#failure ?? new Error("HID owner closed before opening an interface.");
      // Recheck just before open; an unrelated owner may have opened an
      // interface while a preceding native open was still pending.
      if (entry.device.opened) throw new Error("HID interface is already opened externally.");
      this.#devices.set(entry.device, entry);
      await entry.device.open();
      entry.owned = true;
      if (this.#closed) {
        await this.#release(entry);
        throw this.#failure ?? new Error("HID owner closed during interface opening.");
      }
      entry.listener = event => this.#report(entry, event);
      entry.listening = true;
      entry.ready = true;
      entry.device.addEventListener("inputreport", entry.listener);
      if (this.#closed) throw this.#failure ?? new Error("HID owner closed during listener setup.");
    }
    return this.devices;
  }

  #report(entry, event) {
    if (this.#closed || !entry.ready || this.#devices.get(entry.device) !== entry) return;
    try {
      const { device, data: nativeData, reportId, timeStamp } = event ?? {};
      if (device !== entry.device || !(nativeData instanceof DataView)
        || !Number.isInteger(reportId) || reportId < 0 || reportId > 255
        || nativeData.byteLength > this.#maxReportBytes) {
        throw new Error("Invalid HID report device, report ID or payload extent.");
      }
      const hostNs = millisecondsToNanos(timeStamp);
      const data = new Uint8Array(nativeData.buffer, nativeData.byteOffset, nativeData.byteLength).slice();
      const sequence = this.#nextSequence();
      if (typeof sequence !== "bigint" || sequence < 0n || sequence > U64_MAX) {
        throw new Error("HID acquisition sequence must be an unsigned sixty-four-bit BigInt.");
      }
      if (this.#closed || !entry.ready || this.#devices.get(entry.device) !== entry) return;
      this.#onReport(Object.freeze({ kind: "hid", hostNs, source: entry.source, sequence, reportId, data }));
    } catch (cause) {
      this.#fail(new Error("HID report acquisition failed.", { cause }));
    }
  }

  #disconnect(event) {
    if (this.#closed) return;
    const entry = this.#devices.get(event?.device);
    if (!entry) return;
    try {
      const hostNs = millisecondsToNanos(event.timeStamp);
      if (!entry.owned) throw new Error("HID interface disconnected while opening.");
      this.#release(entry);
      if (!this.#closed) this.#onDisconnect(Object.freeze({ source: entry.source, hostNs }));
    } catch (cause) {
      this.#fail(new Error("HID disconnection handling failed.", { cause }));
    }
  }

  #detach(entry) {
    entry.ready = false;
    if (!entry.listening) return;
    entry.listening = false;
    try { entry.device.removeEventListener("inputreport", entry.listener); }
    catch (cause) { this.#cleanupFailed(cause); }
  }

  #release(entry) {
    if (!entry.owned || entry.closePromise) return entry.closePromise;
    entry.owned = false;
    if (this.#devices.get(entry.device) === entry) this.#devices.delete(entry.device);
    const closing = Promise.resolve().then(() => entry.device.close()).catch(cause => {
      this.#cleanupFailed(cause);
    }).finally(() => { this.#closing.delete(entry.device); });
    entry.closePromise = closing;
    this.#closing.set(entry.device, closing);
    this.#detach(entry);
    return closing;
  }

  #notifyError(error) {
    try { this.#onError(error); }
    catch (cause) { this.#cleanupFailure ??= new Error("HID error notification failed.", { cause }); }
  }

  #fail(error) {
    const first = this.#failure === null;
    this.#failure ??= error;
    this.close().catch(() => {});
    if (first) this.#notifyError(error);
  }

  #cleanupFailed(cause) {
    if (this.#cleanupFailure) return;
    const error = new Error("HID interface cleanup failed.", { cause });
    this.#cleanupFailure = error;
    this.#failure ??= error;
    this.close().catch(() => {});
    this.#notifyError(error);
  }

  close() {
    if (this.#closePromise) return this.#closePromise;
    this.#closed = true;
    this.#closePromise = Promise.resolve().then(async () => {
      // Browser discovery/open/close have no cancellation contract. Await late
      // native completion, whose successfully opened handle is released above.
      try { await this.#pending; } catch {}
      await Promise.all(this.#closing.values());
      this.#devices.clear();
      if (this.#cleanupFailure) throw this.#cleanupFailure;
    });
    // Internal failure/disconnect cleanup must never create an unhandled
    // rejection; callers still receive this same rejecting close Promise.
    this.#closePromise.catch(() => {});
    try { this.#hid.removeEventListener("disconnect", this.#disconnectListener); }
    catch (cause) { this.#cleanupFailed(cause); }
    for (const entry of this.#devices.values()) {
      this.#detach(entry);
      this.#release(entry);
    }
    return this.#closePromise;
  }
}

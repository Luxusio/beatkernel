import { millisecondsToNanos } from "./play-model.mjs";

const U64_MAX = 18446744073709551615n;
const unsigned = value => typeof value === "bigint" && value >= 0n && value <= U64_MAX;
const finiteFloat = value => typeof value === "number" && Number.isFinite(value) && Number.isFinite(Math.fround(value));

// Window samples and available movement history. Sources identify aggregates, not hardware.
export class PointerInputOwner {
  #target;
  #nextSequence;
  #onBatch;
  #onError;
  #isCurrent;
  #devices;
  #listeners = [];
  #pointers = new Map();
  #captures = new Set();
  #lastSequence = null;
  #busy = false;
  #closed = false;
  #failure = null;
  #cleanupFailure = null;
  #notified = false;

  constructor({ target, nextSource, nextSequence, onBatch, onError, isCurrent } = {}) {
    if (!target || ["addEventListener", "removeEventListener", "setPointerCapture", "releasePointerCapture"]
      .some(name => typeof target[name] !== "function")
      || [nextSource, nextSequence, onBatch, onError, isCurrent].some(callback => typeof callback !== "function")) {
      throw new Error("Pointer acquisition requires a capture-capable target and ownership callbacks.");
    }
    this.#target = target;
    this.#nextSequence = nextSequence;
    this.#onBatch = onBatch;
    this.#onError = onError;
    this.#isCurrent = isCurrent;
    try {
      const devices = [];
      let previous = 2n;
      for (const pointerType of ["mouse", "pen"]) {
        if (!this.#current()) throw new Error("Pointer owner changed during setup.");
        const source = nextSource();
        if (!this.#current()) throw new Error("Pointer owner changed during source allocation.");
        if (!unsigned(source) || source <= previous) throw new Error("Pointer sources require fresh increasing u64 identities at least three.");
        previous = source;
        devices.push(Object.freeze({ source, pointerType }));
      }
      this.#devices = Object.freeze(devices);
      for (const name of ["pointerdown", "pointermove", "pointerup", "pointercancel", "lostpointercapture"]) {
        if (!this.#current()) throw new Error("Pointer owner changed during listener setup.");
        const listener = event => this.#event(event, name);
        this.#listeners.push([name, listener]); // Cleanup also covers an add that throws after installing.
        target.addEventListener(name, listener);
        if (!this.#current()) {
          // An add callback may have closed before the native registration finished.
          this.#remove(name, listener);
          throw new Error("Pointer owner changed during listener setup.");
        }
      }
    } catch (cause) {
      this.#fail(new Error("Pointer acquisition setup failed.", { cause }));
      throw this.#failure;
    }
  }

  get devices() { return this.#devices; }
  get closed() { return this.#closed; }
  get cleanupFailure() { return this.#cleanupFailure; }

  #current() { return !this.#closed && this.#isCurrent() === true && !this.#closed; }

  #event(event, name) {
    if (this.#closed || this.#busy) return;
    this.#busy = true;
    try {
      if (!this.#current()) return;
      const pointerType = event?.pointerType;
      if (!this.#current() || (pointerType !== "mouse" && pointerType !== "pen")) return;
      const pointerId = event.pointerId;
      if (!this.#current()) return;
      if (!Number.isInteger(pointerId) || pointerId < -2147483648 || pointerId > 2147483647) throw new Error("Invalid signed pointer identity.");
      const previous = this.#pointers.get(pointerId);
      if (previous && previous.pointerType !== pointerType) throw new Error("An active pointer changed its type.");
      const release = name === "pointercancel" || name === "lostpointercapture";
      if (release && !previous) return; // Native up already released this pointer.
      const timestampMs = event.timeStamp;
      if (!this.#current()) return;
      const hostNs = millisecondsToNanos(timestampMs);
      const x = release ? null : event.offsetX;
      if (!this.#current()) return;
      const y = release ? null : event.offsetY;
      if (!this.#current()) return;
      const buttons = release ? 0 : event.buttons;
      if (!this.#current()) return;
      if ((!release && (!finiteFloat(x) || !finiteFloat(y)))
        || !Number.isInteger(buttons) || buttons < 0 || buttons > 0xffffffff
        || (previous && timestampMs < previous.timestampMs)) throw new Error("Invalid pointer sample or acquisition chronology.");
      let history = [{ timestampMs, hostNs, x, y, buttons }];
      if (name === "pointermove") {
        const getCoalescedEvents = event.getCoalescedEvents;
        if (!this.#current()) return;
        if (getCoalescedEvents !== undefined && getCoalescedEvents !== null) {
          if (typeof getCoalescedEvents !== "function") throw new Error("Invalid coalesced pointer acquisition method.");
          const children = getCoalescedEvents.call(event);
          if (!this.#current()) return;
          if (!Array.isArray(children)) throw new Error("Coalesced pointer history must be an array.");
          const count = children.length;
          if (!this.#current()) return;
          if (!Number.isInteger(count) || count < 0 || count > 256) throw new Error("Invalid coalesced pointer history length or more than 256 samples.");
          if (count !== 0) {
            const primary = event.isPrimary;
            const clientX = event.clientX;
            const clientY = event.clientY;
            if (!this.#current()) return;
            if (typeof primary !== "boolean" || typeof clientX !== "number" || !Number.isFinite(clientX)
              || typeof clientY !== "number" || !Number.isFinite(clientY)) throw new Error("Invalid coalesced pointer parent anchor.");
            history = [];
            let lastTime = previous?.timestampMs ?? 0;
            for (let index = 0; index < count; index++) {
              const child = children[index];
              if (!this.#current()) return;
              if (!child || typeof child !== "object" || Array.isArray(child)) throw new Error("Invalid coalesced pointer sample.");
              // Coalesced children are not dispatched; their offset coordinates
              // do not identify this target. Snapshot native fields exactly once.
              const { pointerId: childId, pointerType: childType, isPrimary: childPrimary,
                timeStamp: childTime, clientX: childX, clientY: childY, buttons: childButtons } = child;
              if (!this.#current()) return;
              if (childId !== pointerId || childType !== pointerType || childPrimary !== primary
                || typeof childTime !== "number" || !Number.isFinite(childTime) || childTime < lastTime || childTime > timestampMs
                || typeof childX !== "number" || !Number.isFinite(childX) || typeof childY !== "number" || !Number.isFinite(childY)
                || !Number.isInteger(childButtons) || childButtons < 0 || childButtons > 0xffffffff) {
                throw new Error("Coalesced pointer identity, chronology or sample fields changed.");
              }
              const projectedX = x + (childX - clientX);
              const projectedY = y + (childY - clientY);
              if (!finiteFloat(projectedX) || !finiteFloat(projectedY)) throw new Error("Coalesced pointer position exceeds finite float32 coordinates.");
              history.push({ timestampMs: childTime, hostNs: millisecondsToNanos(childTime),
                x: projectedX, y: projectedY, buttons: childButtons });
              lastTime = childTime;
            }
            if (history.at(-1).buttons !== buttons) throw new Error("Coalesced pointer final buttons differ from the dispatched mask.");
          }
        }
      }
      if (!previous && this.#pointers.size >= 64 && history.some(sample => sample.buttons !== 0)) throw new Error("Active pointer capacity exceeded.");
      let otherButtons = 0;
      for (const [id, pointer] of this.#pointers) {
        if (id !== pointerId && pointer.pointerType === pointerType) otherButtons = (otherButtons | pointer.buttons) >>> 0;
      }
      let before = (otherButtons | (previous?.buttons ?? 0)) >>> 0;
      const source = this.#devices[pointerType === "mouse" ? 0 : 1].source;
      const code = pointerId >>> 0;
      const samples = [];
      for (const sample of history) {
        const { hostNs, x, y } = sample;
        if (!release) {
          if (samples.length === 1024) throw new Error("Expanded pointer batch exceeds 1024 events.");
          samples.push({ kind: "pointer", pointerType, hostNs, source, code, control: 0, mode: 0, x, y });
        }
        const after = (otherButtons | sample.buttons) >>> 0;
        for (let bit = 0; bit < 32; bit++) {
          const wasDown = (before & (1 << bit)) !== 0;
          const down = (after & (1 << bit)) !== 0;
          if (wasDown === down) continue;
          if (samples.length === 1024) throw new Error("Expanded pointer batch exceeds 1024 events.");
          samples.push({ kind: "pointer-button", pointerType, hostNs, source, code, control: bit + 1, state: down ? 0 : 1 });
        }
        before = after;
      }
      for (const sample of samples) {
        if (!this.#current()) return;
        const sequence = this.#nextSequence();
        if (!this.#current()) return;
        if (!unsigned(sequence) || (this.#lastSequence !== null && sequence <= this.#lastSequence)) throw new Error("Pointer sequence must increase within u64.");
        this.#lastSequence = sequence;
        sample.sequence = sequence;
        Object.freeze(sample);
      }
      Object.freeze(samples);
      if (buttons !== 0) this.#pointers.set(pointerId, { pointerType, buttons, timestampMs: history.at(-1).timestampMs });
      else this.#pointers.delete(pointerId);
      if (buttons !== 0 && !this.#captures.has(pointerId)) {
        this.#captures.add(pointerId);
        try { this.#target.setPointerCapture(pointerId); }
        finally {
          if (!this.#current()) { this.#release(pointerId); this.#notify(); }
        }
      } else if (buttons === 0 && this.#captures.delete(pointerId) && name !== "lostpointercapture") this.#release(pointerId);
      if (this.#cleanupFailure !== null) throw this.#cleanupFailure;
      if (!this.#current()) return;
      if (samples.length !== 0) this.#onBatch(samples);
    } catch (cause) {
      if (!this.#closed) this.#fail(new Error("Pointer acquisition failed.", { cause }));
      else this.#notify(); // Preserve a cleanup error observed after a native callback closed ownership.
    } finally { this.#busy = false; }
  }

  #cleanupError(cause) {
    this.#cleanupFailure ??= new Error("Pointer input cleanup failed.", { cause });
    this.#failure ??= this.#cleanupFailure;
    this.#failure.cleanupError = this.#cleanupFailure;
  }

  #remove(name, listener) {
    try { this.#target.removeEventListener(name, listener); }
    catch (cause) { this.#cleanupError(cause); }
  }

  #release(id) {
    try { this.#target.releasePointerCapture(id); }
    catch (cause) { this.#cleanupError(cause); }
  }

  #notify() {
    if (this.#notified || this.#failure === null) return;
    this.#notified = true;
    try { this.#onError(this.#failure); } catch { /* Notification cannot revive ownership. */ }
  }

  #fail(error) {
    this.#failure ??= error;
    this.close();
    this.#notify();
  }

  close() {
    if (this.#closed) return;
    this.#closed = true;
    for (const [name, listener] of this.#listeners) this.#remove(name, listener);
    this.#listeners.length = 0;
    for (const id of this.#captures) this.#release(id);
    this.#captures.clear();
    this.#pointers.clear();
    this.#notify();
  }
}

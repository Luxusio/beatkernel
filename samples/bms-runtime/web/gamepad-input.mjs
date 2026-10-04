import { millisecondsToNanos } from "./play-model.mjs";

const U64_MAX = 18446744073709551615n;
const integer = (value, min, max) => Number.isInteger(value) && value >= min && value <= max;
const unsigned = value => typeof value === "bigint" && value >= 0n && value <= U64_MAX;

// Explicit Window acquisition only: no timer, interpretation or synthetic input.
// Gamepad objects distinguish observed connections, not persistent hardware.
// A completely missed lifecycle with the same reused object cannot be detected.
export class GamepadInputOwner {
  #navigator;
  #target;
  #nextSource;
  #nextSequence;
  #onSample;
  #onDisconnect;
  #onError;
  #limits;
  #devices = new Map();
  #lastSource = 2n;
  #lastSequence = null;
  #revision = 0;
  #busy = false;
  #closed = false;
  #failure = null;
  #cleanupFailure = null;
  #notified = false;
  #connectedListener;
  #disconnectedListener;

  constructor({ navigator, eventTarget, nextSource, nextSequence, onSample, onDisconnect, onError,
    maxDevices = 16, maxSlots = 64, maxButtons = 128, maxAxes = 64 } = {}) {
    if (!navigator || typeof navigator.getGamepads !== "function" || !eventTarget
      || ["addEventListener", "removeEventListener"].some(name => typeof eventTarget[name] !== "function")) {
      throw new Error("Gamepad acquisition is unavailable or lacks lifecycle events.");
    }
    if ([nextSource, nextSequence, onSample, onDisconnect, onError].some(callback => typeof callback !== "function")
      || !integer(maxDevices, 1, 16) || !integer(maxSlots, 1, 64)
      || !integer(maxButtons, 1, 128) || !integer(maxAxes, 1, 64)) {
      throw new Error("Invalid Gamepad callbacks or acquisition limits.");
    }
    this.#navigator = navigator;
    this.#target = eventTarget;
    this.#nextSource = nextSource;
    this.#nextSequence = nextSequence;
    this.#onSample = onSample;
    this.#onDisconnect = onDisconnect;
    this.#onError = onError;
    this.#limits = { maxDevices, maxSlots, maxButtons, maxAxes };
    this.#connectedListener = event => this.#lifecycle(event, true);
    this.#disconnectedListener = event => this.#lifecycle(event, false);
    try {
      eventTarget.addEventListener("gamepadconnected", this.#connectedListener);
      eventTarget.addEventListener("gamepaddisconnected", this.#disconnectedListener);
    } catch (cause) {
      this.#fail(new Error("Gamepad lifecycle listener setup failed.", { cause }));
      throw this.#failure;
    }
  }

  get closed() { return this.#closed; }
  get failure() { return this.#failure; }
  get cleanupFailure() { return this.#cleanupFailure; }
  get devices() {
    return Object.freeze(Array.from(this.#devices.values(), ({ source, index, id, mapping }) =>
      Object.freeze({ source, index, id, mapping })));
  }

  #metadata(gamepad) {
    if (!gamepad || typeof gamepad !== "object" || Array.isArray(gamepad)) throw new Error("Invalid Gamepad object.");
    const { index, id, mapping, connected } = gamepad;
    if (!integer(index, 0, this.#limits.maxSlots - 1) || typeof id !== "string" || id.length > 1024
      || (mapping !== "" && mapping !== "standard") || typeof connected !== "boolean") {
      throw new Error("Invalid Gamepad index, bounded identity, mapping or connection state.");
    }
    return { index, id, mapping, connected };
  }

  #snapshot() {
    const gamepads = this.#navigator.getGamepads();
    if (this.#closed) return [];
    if (!Array.isArray(gamepads) || gamepads.length > this.#limits.maxSlots) throw new Error("Gamepad slot capacity exceeded or invalid slot array.");
    const slots = gamepads.length;
    const samples = [];
    for (let index = 0; index < slots; index++) {
      if (!Object.hasOwn(gamepads, index)) continue;
      const gamepad = gamepads[index];
      if (gamepad === null) continue;
      const metadata = this.#metadata(gamepad);
      if (metadata.index !== index) throw new Error("Gamepad index does not match its slot.");
      if (!metadata.connected) continue;
      if (samples.length === this.#limits.maxDevices) throw new Error("Gamepad device capacity exceeded.");
      const timestampMs = gamepad.timestamp;
      const hostNs = millisecondsToNanos(timestampMs);
      const nativeAxes = gamepad.axes;
      const nativeButtons = gamepad.buttons;
      if (!Array.isArray(nativeAxes) || nativeAxes.length > this.#limits.maxAxes
        || !Array.isArray(nativeButtons) || nativeButtons.length > this.#limits.maxButtons) {
        throw new Error("Invalid Gamepad control arrays or control capacity exceeded.");
      }
      const axisCount = nativeAxes.length;
      const buttonCount = nativeButtons.length;
      const previous = this.#devices.get(index);
      if (previous?.gamepad === gamepad && (timestampMs < previous.timestampMs || metadata.id !== previous.id
        || metadata.mapping !== previous.mapping || axisCount !== previous.axes
        || buttonCount !== previous.buttons)) {
        throw new Error("Gamepad timestamp regressed or connected device metadata changed.");
      }
      const axes = [];
      const buttons = [];
      for (let control = 0; control < axisCount; control++) {
        const axis = nativeAxes[control];
        if (typeof axis !== "number" || !Number.isFinite(axis) || axis < -1 || axis > 1) throw new Error("Invalid Gamepad axis value.");
        axes.push(axis);
      }
      for (let control = 0; control < buttonCount; control++) {
        const button = nativeButtons[control];
        if (!button || typeof button !== "object" || Array.isArray(button)) throw new Error("Invalid Gamepad button.");
        const { pressed, touched, value } = button;
        if (typeof pressed !== "boolean" || typeof touched !== "boolean"
          || typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1) throw new Error("Invalid Gamepad button state.");
        buttons.push(Object.freeze({ pressed, touched, value }));
      }
      samples.push({ gamepad, ...metadata, timestampMs, hostNs,
        axes: Object.freeze(axes), buttons: Object.freeze(buttons) });
    }
    return samples;
  }

  poll() {
    if (this.#closed) throw this.#failure ?? new Error("Gamepad owner is closed.");
    if (this.#busy) throw new Error("Gamepad poll is already active.");
    this.#busy = true;
    const revision = this.#revision;
    const current = () => !this.#closed && this.#revision === revision;
    let published = 0;
    try {
      // Validate and copy every device before calling external allocators or
      // publishing any prefix. Equal timestamps never suppress changed values.
      const samples = this.#snapshot();
      if (!current()) return 0;
      const next = new Map();
      const records = [];
      for (const sample of samples) {
        const previous = this.#devices.get(sample.index);
        let source = previous?.gamepad === sample.gamepad ? previous.source : null;
        if (source === null) {
          source = this.#nextSource();
          if (!unsigned(source) || source < 3n || source <= this.#lastSource) throw new Error("Gamepad source allocator must return fresh increasing u64 identities.");
          this.#lastSource = source; // Burn allocated identities even if later admission fails.
          if (!current()) return 0;
        }
        const sequence = this.#nextSequence();
        if (!unsigned(sequence) || (this.#lastSequence !== null && sequence <= this.#lastSequence)) {
          throw new Error("Gamepad sequence allocator must return fresh increasing u64 values.");
        }
        this.#lastSequence = sequence;
        if (!current()) return 0;
        const { index, id, mapping, hostNs, timestampMs, axes, buttons } = sample;
        next.set(index, { source, index, id, mapping, gamepad: sample.gamepad,
          timestampMs, axes: axes.length, buttons: buttons.length });
        records.push(Object.freeze({ kind: "gamepad", source, index, id, mapping,
          hostNs, timestampMs, sequence, axes, buttons }));
      }
      const retired = [];
      for (const previous of this.#devices.values()) {
        if (next.get(previous.index)?.source !== previous.source) retired.push(previous);
      }
      this.#devices = next;
      for (const entry of retired) {
        if (!current()) return published;
        this.#onDisconnect(Object.freeze({ source: entry.source, index: entry.index }));
      }
      for (const record of records) {
        if (!current()) return published;
        this.#onSample(record);
        published++;
      }
      return published;
    } catch (cause) {
      this.#fail(new Error("Gamepad polling failed.", { cause }));
      throw this.#failure;
    } finally {
      this.#busy = false;
    }
  }

  #lifecycle(event, connected) {
    if (this.#closed) return;
    try {
      const gamepad = event?.gamepad;
      const metadata = this.#metadata(gamepad);
      if (metadata.connected !== connected) {
        if (connected) return; // A delayed connection event for an already gone object.
        throw new Error("Gamepad disconnect event still reports a connected device.");
      }
      const entry = this.#devices.get(metadata.index);
      if (connected && entry?.gamepad === gamepad) return;
      // Invalidate a poll captured before an allocator/consumer triggered this
      // lifecycle event. A stale old object cannot retire a replacement source.
      this.#revision++;
      if (!entry || (connected ? entry.gamepad === gamepad : entry.gamepad !== gamepad)) return;
      this.#devices.delete(entry.index);
      this.#onDisconnect(Object.freeze({ source: entry.source, index: entry.index }));
    } catch (cause) {
      this.#fail(new Error("Gamepad lifecycle handling failed.", { cause }));
    }
  }

  #notifyError() {
    if (this.#notified || this.#failure === null) return;
    this.#notified = true;
    try { this.#onError(this.#failure); } catch { /* Notification cannot revive a failed owner. */ }
  }

  #fail(error) {
    this.#failure ??= error;
    this.close();
    this.#notifyError();
  }

  close() {
    if (this.#closed) return;
    this.#closed = true;
    this.#revision++;
    this.#devices.clear();
    for (const [name, listener] of [["gamepadconnected", this.#connectedListener], ["gamepaddisconnected", this.#disconnectedListener]]) {
      try { this.#target.removeEventListener(name, listener); }
      catch (cause) {
        this.#cleanupFailure ??= new Error("Gamepad listener cleanup failed.", { cause });
        this.#failure ??= this.#cleanupFailure;
      }
    }
    // A constructor failure has not returned an owner for callers to inspect.
    // Keep failed cleanup attached to the original acquisition/setup error too.
    if (this.#cleanupFailure !== null) this.#failure.cleanupError = this.#cleanupFailure;
    this.#notifyError();
  }
}

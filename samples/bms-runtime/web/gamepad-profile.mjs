import { millisecondsToNanos } from "./play-model.mjs";

const U64_MAX = 18446744073709551615n;
const BACKEND = 0x57475044;
const HOST = 0x57494e;
const FORK = Symbol("Gamepad adapter fork");
const integer = (value, min, max) => Number.isInteger(value) && value >= min && value <= max;
const unsigned = value => typeof value === "bigint" && value >= 0n && value <= U64_MAX;
const laneValid = lane => integer(lane, 0x11, 0x19) || integer(lane, 0x21, 0x29);

export function snapshotGamepadSetup(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || !Array.isArray(value.devices) || value.devices.length > 16
    || !(value.bindingWords instanceof Uint32Array) || value.bindingWords.length > 256 * 5
    || value.bindingWords.length % 5 !== 0) throw new Error("Invalid bounded Gamepad setup.");
  const input = value.bindingWords;
  if (!(input.buffer instanceof ArrayBuffer) || input.buffer.resizable === true) throw new Error("Gamepad bindings require fixed ordinary storage.");
  // Reconstructing also refuses detached empty input before it becomes a setup.
  const bindingWords = new Uint32Array(input.buffer, input.byteOffset, input.length).slice();
  const devices = [];
  const bySource = new Map();
  for (let index = 0; index < value.devices.length; index++) {
    const device = value.devices[index];
    if (!device || typeof device !== "object" || Array.isArray(device)) throw new Error("Invalid Gamepad device descriptor.");
    const { source, buttons, axes } = device;
    if (!unsigned(source) || source < 3n || bySource.has(source)
      || !integer(buttons, 0, 128) || !integer(axes, 0, 64)) throw new Error("Invalid Gamepad source or control counts.");
    const snapshot = Object.freeze({ source, buttons, axes });
    devices.push(snapshot);
    bySource.set(source, snapshot);
  }
  const physicalWords = new Uint32Array(bindingWords.length / 5 * 7);
  const rows = new Set();
  const lanes = new Set();
  for (let offset = 0; offset < bindingWords.length; offset += 5) {
    const [lane, low, high, type, index] = bindingWords.subarray(offset, offset + 5);
    const source = BigInt(low) | BigInt(high) << 32n;
    const device = bySource.get(source);
    const key = `${lane}:${source}:${type}:${index}`;
    if (!laneValid(lane) || !device || !integer(type, 0, 3)
      || index >= (type === 1 ? device.axes : device.buttons) || rows.has(key)) {
      throw new Error("Gamepad binding rows require unique valid lanes, sources and controls.");
    }
    rows.add(key);
    if (type === 0) lanes.add(lane); // Other physical kinds do not prove press coverage.
    physicalWords.set([lane, 1, low, high, 1, BACKEND, type * 0x10000 + index], offset / 5 * 7);
  }
  return Object.freeze({ devices: Object.freeze(devices), bindingWords, physicalWords,
    sources: Object.freeze(devices.map(device => device.source)), lanes: Object.freeze([...lanes]) });
}

function encode(sample, field, value) {
  const button = field.type === 0 || field.type === 3;
  const bytes = new Uint8Array(button ? 69 : 73);
  const view = new DataView(bytes.buffer);
  bytes.set([0x42, 0x4b, 0x50, 0x49]);
  view.setUint16(4, 1, true);
  view.setUint8(6, button ? 0 : 1);
  view.setBigUint64(7, sample.source, true);
  view.setBigInt64(15, sample.hostNs, true);
  view.setUint32(23, HOST, true);
  view.setBigUint64(27, sample.sequence, true);
  view.setUint8(35, 1);
  view.setUint32(36, BACKEND, true);
  view.setUint8(40, 1);
  view.setUint32(41, field.code, true);
  view.setUint8(45, 1);
  view.setUint32(46, HOST, true);
  view.setBigInt64(50, sample.hostNs, true);
  // No original_clock_point: the captured timestamp is already in HOST.
  view.setUint8(59, 1); // PhysicalControlId::Native.
  view.setUint32(60, BACKEND, true);
  view.setUint32(64, field.code, true);
  if (button) view.setUint8(68, value ? 0 : 1);
  else {
    view.setFloat32(68, Math.fround(value), true);
    view.setUint8(72, 0); // AxisMode::Absolute.
  }
  return bytes;
}

export class GamepadAdapter {
  #profiles = new Map();
  #states = new Map();

  constructor(value) {
    if (value === FORK) return; // Only fork() can access this private setup bypass.
    const setup = snapshotGamepadSetup(value);
    for (const device of setup.devices) this.#profiles.set(device.source, { ...device, fields: [] });
    const seen = new Set();
    for (let offset = 0; offset < setup.bindingWords.length; offset += 5) {
      const words = setup.bindingWords;
      const source = BigInt(words[offset + 1]) | BigInt(words[offset + 2]) << 32n;
      const type = words[offset + 3];
      const index = words[offset + 4];
      const code = type * 0x10000 + index;
      const key = `${source}:${code}`;
      if (seen.has(key)) continue; // BindingMap performs lane fanout from one event.
      seen.add(key);
      this.#profiles.get(source).fields.push(Object.freeze({ type, index, code }));
    }
    for (const profile of this.#profiles.values()) { Object.freeze(profile.fields); Object.freeze(profile); }
  }

  fork() {
    const draft = new GamepadAdapter(FORK);
    draft.#profiles = this.#profiles;
    for (const [source, state] of this.#states) draft.#states.set(source, { ...state, levels: state.levels.slice() });
    return draft;
  }

  decode(event) {
    if (!event || typeof event !== "object" || Array.isArray(event) || event.kind !== "gamepad") throw new Error("Invalid Gamepad sample.");
    const { source, index, id, mapping, hostNs, timestampMs, sequence, axes, buttons } = event;
    const profile = this.#profiles.get(source);
    if (!profile || !integer(index, 0, 63) || typeof id !== "string" || id.length > 1024
      || (mapping !== "" && mapping !== "standard") || !unsigned(sequence)
      || typeof hostNs !== "bigint" || hostNs !== millisecondsToNanos(timestampMs)
      || !Array.isArray(axes) || axes.length !== profile.axes
      || !Array.isArray(buttons) || buttons.length !== profile.buttons) throw new Error("Gamepad sample differs from its bounded setup or original clock.");
    const previous = this.#states.get(source);
    if (previous && (previous.index !== index || previous.id !== id || previous.mapping !== mapping
      || timestampMs < previous.timestampMs || sequence <= previous.sequence)) {
      throw new Error("Gamepad source metadata or acquisition chronology changed.");
    }
    const axisValues = [];
    const buttonValues = [];
    for (let position = 0; position < profile.axes; position++) {
      const value = axes[position];
      if (typeof value !== "number" || !Number.isFinite(value) || value < -1 || value > 1) throw new Error("Invalid Gamepad axis sample.");
      axisValues.push(value);
    }
    for (let position = 0; position < profile.buttons; position++) {
      const button = buttons[position];
      if (!button || typeof button !== "object" || Array.isArray(button)) throw new Error("Invalid Gamepad button sample.");
      const { pressed, touched, value } = button;
      if (typeof pressed !== "boolean" || typeof touched !== "boolean"
        || typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1) throw new Error("Invalid Gamepad button values.");
      buttonValues.push({ pressed, touched, value });
    }
    const levels = [];
    const output = [];
    const sample = { source, hostNs, sequence };
    for (let position = 0; position < profile.fields.length; position++) {
      const field = profile.fields[position];
      const value = field.type === 1 ? axisValues[field.index]
        : buttonValues[field.index][field.type === 0 ? "pressed" : field.type === 2 ? "value" : "touched"];
      levels.push(value);
      const before = previous?.levels[position];
      if (Object.is(before, value) || (before === undefined && value === false)) continue;
      output.push(encode(sample, field, value));
    }
    // Invalid samples or allocation failures above leave this source untouched.
    this.#states.set(source, { index, id, mapping, timestampMs, sequence, levels });
    return output;
  }
}

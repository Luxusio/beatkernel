// Explicit Worker-side mouse/pen ownership and canonical Native bindings.
const U64_MAX = 18446744073709551615n;
const MOUSE = 0x574d4f55;
const PEN = 0x5750454e;
const laneValid = lane => (lane >= 0x11 && lane <= 0x19) || (lane >= 0x21 && lane <= 0x29);

export function snapshotPointerSetup(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid pointer setup.");
  const { devices: inputDevices, bindingWords: input } = value;
  if (!Array.isArray(inputDevices) || !(input instanceof Uint32Array)) throw new Error("Invalid pointer devices or binding storage.");
  const count = inputDevices.length;
  const { buffer, byteOffset, length } = input;
  if (count < 1 || count > 64 || !Number.isInteger(length) || length < 4 || length > 256 * 4 || length % 4 !== 0
    || !(buffer instanceof ArrayBuffer) || buffer.resizable === true) {
    throw new Error("Pointer setup requires one to 64 devices and one to 256 fixed four-word binding rows.");
  }
  // The native view constructor refuses detached storage; copying without slice
  // avoids caller-defined typed-array species and retains no caller-owned bytes.
  const bindingWords = new Uint32Array(length);
  bindingWords.set(new Uint32Array(buffer, byteOffset, length));
  const devices = [];
  const bySource = new Map();
  for (let index = 0; index < count; index++) {
    const device = inputDevices[index];
    if (!device || typeof device !== "object" || Array.isArray(device)) throw new Error("Invalid pointer device descriptor.");
    const { source, pointerType } = device;
    if (typeof source !== "bigint" || source < 3n || source > U64_MAX || bySource.has(source)
      || (pointerType !== "mouse" && pointerType !== "pen")) {
      throw new Error("Pointer devices require unique full-width sources and mouse or pen types.");
    }
    const snapshot = Object.freeze({ source, pointerType });
    devices.push(snapshot);
    bySource.set(source, snapshot);
  }
  const physicalWords = new Uint32Array(length / 4 * 7);
  const controls = new Set();
  const lanes = new Set();
  for (let offset = 0; offset < length; offset += 4) {
    const lane = bindingWords[offset];
    const low = bindingWords[offset + 1];
    const high = bindingWords[offset + 2];
    const control = bindingWords[offset + 3];
    const source = BigInt(low) | (BigInt(high) << 32n);
    const device = bySource.get(source);
    const key = `${source}:${control}`;
    if (!laneValid(lane) || !device || control > 32 || controls.has(key)) {
      throw new Error("Pointer bindings require valid lanes, owned sources and unique source/control identities.");
    }
    controls.add(key);
    if (control !== 0) lanes.add(lane);
    physicalWords.set([lane, 1, low, high, 1, device.pointerType === "mouse" ? MOUSE : PEN, control], offset / 4 * 7);
  }
  return Object.freeze({ devices: Object.freeze(devices), bindingWords, physicalWords,
    sources: Object.freeze(devices.map(device => device.source)), lanes: Object.freeze([...lanes]) });
}

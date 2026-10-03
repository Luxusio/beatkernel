// Canonical core BKPI v1 boundaries for the existing browser keyboard adapter.
// Historical key IDs are adapter codes, not USB HID usage values. Source 1 is
// the Window keyboard aggregate; the browser does not identify each keyboard.
const KEYBOARD_BACKEND = 0x574b4559;
const HOST_DOMAIN = 0x57494e;
const I64_MAX = 9223372036854775807n;
const U64_MAX = 18446744073709551615n;

export function keyboardBindingWords(pairs) {
  if (!(pairs instanceof Uint32Array) || pairs.length > 36 || pairs.length % 2 !== 0) {
    throw new Error("Keyboard bindings require at most eighteen lane/key pairs.");
  }
  const snapshot = pairs.slice();
  const lanes = new Set();
  const keys = new Set();
  const words = new Uint32Array(snapshot.length / 2 * 7);
  for (let index = 0; index < snapshot.length; index += 2) {
    const lane = snapshot[index];
    const key = snapshot[index + 1];
    if (!((lane >= 0x11 && lane <= 0x19) || (lane >= 0x21 && lane <= 0x29))
      || key < 1 || key > 65535 || lanes.has(lane) || keys.has(key)) {
      throw new Error("Keyboard bindings require valid unique BMS lanes and key codes.");
    }
    lanes.add(lane);
    keys.add(key);
    words.set([lane, 0, 0, 0, 1, KEYBOARD_BACKEND, key], index / 2 * 7);
  }
  return words;
}

export function encodeKeyboardEvent(event) {
  if (!event || typeof event !== "object" || Array.isArray(event)) throw new Error("Invalid keyboard event.");
  const { hostNs, key, down, sequence } = event;
  if (typeof hostNs !== "bigint" || hostNs < 0n || hostNs > I64_MAX
    || typeof sequence !== "bigint" || sequence < 0n || sequence > U64_MAX
    || !Number.isInteger(key) || key < 1 || key > 65535 || typeof down !== "boolean") {
    throw new Error("Keyboard input requires bounded acquisition time, sequence, key and button state.");
  }
  const bytes = new Uint8Array(69);
  const view = new DataView(bytes.buffer);
  bytes.set([0x42, 0x4b, 0x50, 0x49]); // BKPI
  view.setUint16(4, 1, true); // Schema v1; byte 6 is Button (0).
  view.setBigUint64(7, 1n, true); // DeviceId.
  view.setBigInt64(15, hostNs, true);
  view.setUint32(23, HOST_DOMAIN, true);
  view.setBigUint64(27, sequence, true);
  view.setUint8(35, 1); // NativeEventMeta present.
  view.setUint32(36, KEYBOARD_BACKEND, true);
  view.setUint8(40, 1); // Native event code present.
  view.setUint32(41, key, true);
  view.setUint8(45, 1); // Native acquisition clock point present.
  view.setUint32(46, HOST_DOMAIN, true);
  view.setBigInt64(50, hostNs, true);
  // Byte 58: no original_clock_point; acquisition already uses the host domain.
  view.setUint8(59, 1); // PhysicalControlId::Native.
  view.setUint32(60, KEYBOARD_BACKEND, true);
  view.setUint32(64, key, true);
  view.setUint8(68, down ? 0 : 1);
  return bytes;
}

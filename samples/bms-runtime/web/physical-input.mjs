// Canonical core BKPI v1 boundaries for browser physical input adapters.
// Historical key IDs are adapter codes, not USB HID usage values. Source 1 is
// the Window keyboard aggregate; the browser does not identify each keyboard.
const KEYBOARD_BACKEND = 0x574b4559;
const TOUCH_BACKEND = 0x57544f55;
const HID_BACKEND = 0x57484944;
const MOUSE_BACKEND = 0x574d4f55;
const PEN_BACKEND = 0x5750454e;
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

// Source 2 is the Window touch aggregate, not a physical panel identity.
export function touchBindingWords(lanes) {
  if ((!Array.isArray(lanes) && !(lanes instanceof Uint8Array)) || lanes.length > 18) {
    throw new Error("Touch bindings require at most eighteen BMS lanes.");
  }
  const snapshot = lanes.slice();
  const seen = new Set();
  const words = new Uint32Array(snapshot.length * 7);
  for (let index = 0; index < snapshot.length; index++) {
    const lane = snapshot[index];
    if (!Number.isInteger(lane) || !((lane >= 0x11 && lane <= 0x19) || (lane >= 0x21 && lane <= 0x29)) || seen.has(lane)) {
      throw new Error("Touch bindings require unique valid BMS lanes.");
    }
    seen.add(lane);
    words.set([lane, 0, 0, 0, 1, TOUCH_BACKEND, 0], index * 7);
  }
  return words;
}

function finiteFloat(value) {
  return typeof value === "number" && Number.isFinite(value) && Number.isFinite(Math.fround(value));
}

function pointerPacket(snapshot, kind, tag, length) {
  const { pointerType, hostNs, source, sequence, code, control } = snapshot;
  if (snapshot.kind !== kind || (pointerType !== "mouse" && pointerType !== "pen")
    || typeof hostNs !== "bigint" || hostNs < 0n || hostNs > I64_MAX
    || typeof source !== "bigint" || source < 3n || source > U64_MAX
    || typeof sequence !== "bigint" || sequence < 0n || sequence > U64_MAX
    || !Number.isInteger(code) || code < 0 || code > 0xffffffff
    || !Number.isInteger(control) || control < 0 || control > 0xffffffff) {
    throw new Error("Pointer input requires a mouse or pen and bounded acquisition identity and control.");
  }
  const backend = pointerType === "mouse" ? MOUSE_BACKEND : PEN_BACKEND;
  const bytes = new Uint8Array(length);
  const view = new DataView(bytes.buffer);
  bytes.set([0x42, 0x4b, 0x50, 0x49]);
  view.setUint16(4, 1, true);
  view.setUint8(6, tag);
  view.setBigUint64(7, source, true);
  view.setBigInt64(15, hostNs, true);
  view.setUint32(23, HOST_DOMAIN, true);
  view.setBigUint64(27, sequence, true);
  view.setUint8(35, 1); // NativeEventMeta present.
  view.setUint32(36, backend, true);
  view.setUint8(40, 1); // Native event code is independent of binding control.
  view.setUint32(41, code, true);
  view.setUint8(45, 1);
  view.setUint32(46, HOST_DOMAIN, true);
  view.setBigInt64(50, hostNs, true);
  // Byte 58: acquisition is already HOST time, without an original clock point.
  view.setUint8(59, 1); // PhysicalControlId::Native.
  view.setUint32(60, backend, true);
  view.setUint32(64, control, true);
  return bytes;
}

export function encodePointerEvent(event) {
  if (!event || typeof event !== "object" || Array.isArray(event)) throw new Error("Invalid pointer event.");
  // Read every caller field once before validation or packet allocation.
  const { kind, pointerType, hostNs, source, sequence, code, control, mode, x, y } = event;
  if ((mode !== 0 && mode !== 1) || !finiteFloat(x) || !finiteFloat(y)) {
    throw new Error("Pointer input requires absolute or relative mode and finite float32 coordinates.");
  }
  const bytes = pointerPacket({ kind, pointerType, hostNs, source, sequence, code, control }, "pointer", 3, 77);
  const view = new DataView(bytes.buffer);
  view.setFloat32(68, x, true);
  view.setFloat32(72, y, true);
  view.setUint8(76, mode);
  return bytes;
}

export function encodePointerButtonEvent(event) {
  if (!event || typeof event !== "object" || Array.isArray(event)) throw new Error("Invalid pointer button event.");
  const { kind, pointerType, hostNs, source, sequence, code, control, state } = event;
  if (state !== 0 && state !== 1 && state !== 2) {
    throw new Error("Pointer button input requires Down, Up or Repeat state.");
  }
  const bytes = pointerPacket({ kind, pointerType, hostNs, source, sequence, code, control }, "pointer-button", 0, 69);
  new DataView(bytes.buffer).setUint8(68, state);
  return bytes;
}

function validateTouch(event) {
  if (!event || typeof event !== "object" || Array.isArray(event) || event.kind !== "touch") throw new Error("Invalid touch event.");
  if (typeof event.hostNs !== "bigint" || event.hostNs < 0n || event.hostNs > I64_MAX
    || typeof event.sequence !== "bigint" || event.sequence < 0n || event.sequence > U64_MAX
    || typeof event.contact !== "bigint" || event.contact < 0n || event.contact > U64_MAX
    || !Number.isInteger(event.phase) || event.phase < 0 || event.phase > 3
    || !Number.isInteger(event.code) || event.code < 0 || event.code > 0xffffffff
    || !finiteFloat(event.x) || !finiteFloat(event.y)
    || (event.pressure !== null && !finiteFloat(event.pressure))
    || typeof event.width !== "number" || !Number.isFinite(event.width) || event.width <= 0
    || typeof event.height !== "number" || !Number.isFinite(event.height) || event.height <= 0) {
    throw new Error("Touch input requires bounded acquisition identity, finite samples and a positive CSS extent.");
  }
}

export function encodeTouchEvent(event) {
  validateTouch(event);
  const bytes = new Uint8Array(event.pressure === null ? 86 : 90);
  const view = new DataView(bytes.buffer);
  bytes.set([0x42, 0x4b, 0x50, 0x49]);
  view.setUint16(4, 1, true);
  view.setUint8(6, 2); // PhysicalInputEvent::Touch.
  view.setBigUint64(7, 2n, true);
  view.setBigInt64(15, event.hostNs, true);
  view.setUint32(23, HOST_DOMAIN, true);
  view.setBigUint64(27, event.sequence, true);
  view.setUint8(35, 1);
  view.setUint32(36, TOUCH_BACKEND, true);
  view.setUint8(40, 1);
  view.setUint32(41, event.code, true);
  view.setUint8(45, 1);
  view.setUint32(46, HOST_DOMAIN, true);
  view.setBigInt64(50, event.hostNs, true);
  // Byte 58: no extra original clock point; acquisition is already HOST time.
  view.setUint8(59, 1); // PhysicalControlId::Native, one aggregate touch surface.
  view.setUint32(60, TOUCH_BACKEND, true);
  view.setUint32(64, 0, true);
  view.setBigUint64(68, event.contact, true);
  view.setUint8(76, event.phase);
  view.setFloat32(77, event.x, true);
  view.setFloat32(81, event.y, true);
  view.setUint8(85, event.pressure === null ? 0 : 1);
  if (event.pressure !== null) view.setFloat32(86, event.pressure, true);
  return bytes;
}

export function projectTouchEvent(event, logicalWidth, logicalHeight) {
  validateTouch(event);
  if (!Number.isInteger(logicalWidth) || logicalWidth < 1 || logicalWidth > 0xffffffff
    || !Number.isInteger(logicalHeight) || logicalHeight < 1 || logicalHeight > 0xffffffff) {
    throw new Error("Touch projection requires positive 32-bit logical dimensions.");
  }
  const x = Math.fround(event.x / event.width * logicalWidth);
  const y = Math.fround(event.y / event.height * logicalHeight);
  if (!Number.isFinite(x) || !Number.isFinite(y)) throw new Error("Projected touch position exceeds finite float32 coordinates.");
  return { x, y };
}

export function encodeRawHidEvent(event) {
  if (!event || typeof event !== "object" || Array.isArray(event) || event.kind !== "hid") {
    throw new Error("Invalid raw HID event.");
  }
  const { hostNs, source, sequence, reportId, data } = event;
  if (typeof hostNs !== "bigint" || hostNs < 0n || hostNs > I64_MAX
    || typeof source !== "bigint" || source < 3n || source > U64_MAX
    || typeof sequence !== "bigint" || sequence < 0n || sequence > U64_MAX
    || !Number.isInteger(reportId) || reportId < 0 || reportId > 255
    || !(data instanceof Uint8Array) || data.byteLength > 1024) {
    throw new Error("Raw HID input requires bounded acquisition identity, report ID and payload.");
  }
  // Reconstructing even an empty view rejects a detached input buffer. WebHID
  // has already separated the report ID: every payload byte stays unchanged.
  const payload = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  const offset = reportId === 0 ? 68 : 69;
  const bytes = new Uint8Array(offset + payload.byteLength);
  const view = new DataView(bytes.buffer);
  bytes.set([0x42, 0x4b, 0x50, 0x49]);
  view.setUint16(4, 1, true);
  view.setUint8(6, 5); // PhysicalInputEvent::RawHidReport.
  view.setBigUint64(7, source, true);
  view.setBigInt64(15, hostNs, true);
  view.setUint32(23, HOST_DOMAIN, true);
  view.setBigUint64(27, sequence, true);
  view.setUint8(35, 1);
  view.setUint32(36, HID_BACKEND, true);
  view.setUint8(40, 1);
  view.setUint32(41, reportId, true);
  view.setUint8(45, 1);
  view.setUint32(46, HOST_DOMAIN, true);
  view.setBigInt64(50, hostNs, true);
  // Byte 58: no extra original clock point; acquisition is already HOST time.
  view.setUint8(59, reportId === 0 ? 0 : 1);
  if (reportId !== 0) view.setUint8(60, reportId);
  // BKPI v1 payload lengths are u64, even with our 1024-byte report cap.
  view.setBigUint64(offset - 8, BigInt(payload.byteLength), true);
  bytes.set(payload, offset);
  return bytes;
}

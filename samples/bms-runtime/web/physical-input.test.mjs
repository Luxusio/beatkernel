// Deferred canonical acquisition encoding; no browser, WASM or device execution.
import assert from "node:assert/strict";
import test from "node:test";
import { encodeKeyboardEvent, keyboardBindingWords, encodeTouchEvent, touchBindingWords, projectTouchEvent, encodeRawHidEvent,
  encodePointerEvent, encodePointerButtonEvent } from "./physical-input.mjs";

const pointer = fields => ({ kind: "pointer", pointerType: "mouse",
  hostNs: 0x0102030405060708n, source: 0xfedcba9876543210n, sequence: 0x8877665544332211n,
  code: 0x12345678, control: 0x90abcdef, mode: 1, x: 1.5, y: -2.25, ...fields });
const pointerButton = fields => ({ kind: "pointer-button", pointerType: "pen",
  hostNs: 0x0102030405060708n, source: 0xfedcba9876543210n, sequence: 0x8877665544332211n,
  code: 0x12345678, control: 0x90abcdef, state: 2, ...fields });

test("mouse Pointer and pen Button use literal core BKPI layouts with distinct metadata and binding controls", () => {
  // Independent from the JS implementation: crates/beatkernel/src/input/codec.rs
  // writes tag, EventMeta, Native control, then position/mode or button state.
  const mouse = Uint8Array.from([
    66, 75, 80, 73, 1, 0, 3,
    0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
    8, 7, 6, 5, 4, 3, 2, 1, 0x4e, 0x49, 0x57, 0,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    1, 0x55, 0x4f, 0x4d, 0x57, 1, 0x78, 0x56, 0x34, 0x12,
    1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1,
    0, 1, 0x55, 0x4f, 0x4d, 0x57, 0xef, 0xcd, 0xab, 0x90,
    0, 0, 0xc0, 0x3f, 0, 0, 0x10, 0xc0, 1,
  ]);
  const pen = Uint8Array.from([
    66, 75, 80, 73, 1, 0, 0,
    0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
    8, 7, 6, 5, 4, 3, 2, 1, 0x4e, 0x49, 0x57, 0,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    1, 0x4e, 0x45, 0x50, 0x57, 1, 0x78, 0x56, 0x34, 0x12,
    1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1,
    0, 1, 0x4e, 0x45, 0x50, 0x57, 0xef, 0xcd, 0xab, 0x90, 2,
  ]);
  const point = pointer(), button = pointerButton();
  const pointBytes = encodePointerEvent(point), buttonBytes = encodePointerButtonEvent(button);
  assert.equal(pointBytes.length, 77); assert.equal(buttonBytes.length, 69);
  assert.deepEqual(pointBytes, mouse); assert.deepEqual(buttonBytes, pen);
  point.x = 9; point.pointerType = "pen"; point.hostNs = 0n; point.control = 0;
  button.state = 0; button.code = 0; button.source = 3n;
  assert.deepEqual(pointBytes, mouse); assert.deepEqual(buttonBytes, pen);
  assert.notEqual(pointBytes.buffer, buttonBytes.buffer);
  const again = encodePointerEvent(pointer());
  pointBytes.fill(0);
  assert.deepEqual(again, mouse, "separate encodings own separate storage");
});

test("both pointer namespaces retain full integer provenance, exact modes and states, and canonical finite f32 bits", () => {
  for (const [pointerType, namespace] of [["mouse", 0x574d4f55], ["pen", 0x5750454e]]) {
    for (const [hostNs, source, sequence, code, control] of [
      [0n, 3n, 0n, 0, 0xffffffff],
      [9007199254740993n, 9007199254740993n, 9007199254740993n, 0xffffffff, 0],
      [9223372036854775807n, 18446744073709551615n, 18446744073709551615n, 0xffffffff, 0xffffffff],
    ]) {
      const provenance = { pointerType, hostNs, source, sequence, code, control };
      for (const mode of [0, 1]) for (const [x, y, xBits, yBits] of [
        [1 / 3, -0, 0x3eaaaaab, 0x80000000],
        [3.4028234663852886e38, -3.4028234663852886e38, 0x7f7fffff, 0xff7fffff],
        [Number.MIN_VALUE, -Number.MIN_VALUE, 0, 0x80000000],
      ]) {
        const bytes = encodePointerEvent(pointer({ ...provenance, mode, x, y }));
        const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        assert.equal(bytes.length, 77); assert.equal(bytes[6], 3);
        assert.equal(view.getBigUint64(7, true), source);
        assert.equal(view.getBigInt64(15, true), hostNs);
        assert.equal(view.getUint32(23, true), 0x0057494e);
        assert.equal(view.getBigUint64(27, true), sequence);
        assert.equal(view.getUint32(36, true), namespace);
        assert.equal(view.getUint32(41, true), code);
        assert.equal(view.getUint32(46, true), 0x0057494e);
        assert.equal(view.getBigInt64(50, true), hostNs);
        assert.equal(bytes[58], 0, "no fabricated original-clock conversion");
        assert.equal(bytes[59], 1, "the binding remains a Native control");
        assert.equal(view.getUint32(60, true), namespace);
        assert.equal(view.getUint32(64, true), control);
        assert.equal(view.getUint32(68, true), xBits);
        assert.equal(view.getUint32(72, true), yBits);
        assert.equal(bytes[76], mode);
      }
      for (const state of [0, 1, 2]) {
        const bytes = encodePointerButtonEvent(pointerButton({ ...provenance, state }));
        const view = new DataView(bytes.buffer);
        assert.equal(bytes.length, 69); assert.equal(bytes[6], 0);
        assert.equal(view.getBigUint64(7, true), source);
        assert.equal(view.getBigInt64(15, true), hostNs);
        assert.equal(view.getBigUint64(27, true), sequence);
        assert.equal(view.getUint32(36, true), namespace);
        assert.equal(view.getUint32(41, true), code);
        assert.equal(view.getBigInt64(50, true), hostNs);
        assert.equal(view.getUint32(60, true), namespace);
        assert.equal(view.getUint32(64, true), control);
        assert.equal(bytes[68], state);
      }
    }
  }
});

test("pointer encoders refuse malformed acquisition domains and f32 overflow without coercion, clamping or state fallback", () => {
  const common = [
    ["pointerType", [undefined, null, "touch", "Mouse", "", 0]],
    ["hostNs", [undefined, null, 0, "0", -1n, 9223372036854775808n]],
    ["source", [undefined, null, 3, "3", -1n, 0n, 1n, 2n, 18446744073709551616n]],
    ["sequence", [undefined, null, 0, "0", -1n, 18446744073709551616n]],
    ["code", [undefined, null, -1, 4294967296, 1.5, NaN, Infinity, 0n, "0"]],
    ["control", [undefined, null, -1, 4294967296, 1.5, NaN, Infinity, 0n, "0"]],
  ];
  for (const [encode, make, kind] of [[encodePointerEvent, pointer, "pointer"],
    [encodePointerButtonEvent, pointerButton, "pointer-button"]]) {
    for (const [field, values] of common) for (const value of values) {
      const event = make({ [field]: value }), before = { ...event };
      assert.throws(() => encode(event), `${kind}.${field}`);
      assert.deepEqual(event, before);
    }
    for (const value of [undefined, null, [], "event", 1]) assert.throws(() => encode(value));
    for (const value of [undefined, null, "touch", kind === "pointer" ? "pointer-button" : "pointer"]) {
      assert.throws(() => encode(make({ kind: value })));
    }
    let coerced = 0;
    const coercion = { valueOf() { coerced++; return 3; }, toString() { coerced++; return "mouse"; } };
    for (const field of ["pointerType", "hostNs", "source", "sequence", "code", "control"]) {
      assert.throws(() => encode(make({ [field]: coercion })));
    }
    assert.equal(coerced, 0);
  }
  for (const field of ["x", "y"]) for (const value of [undefined, null, "1", 1n, NaN, Infinity, -Infinity,
    3.5e38, -3.5e38, Number.MAX_VALUE]) assert.throws(() => encodePointerEvent(pointer({ [field]: value })));
  for (const value of [undefined, null, -1, 2, 0.5, NaN, "0", 0n, true]) {
    assert.throws(() => encodePointerEvent(pointer({ mode: value })));
  }
  for (const value of [undefined, null, -1, 3, 0.5, NaN, "0", 0n, true]) {
    assert.throws(() => encodePointerButtonEvent(pointerButton({ state: value })));
  }
  assert.equal(encodePointerEvent(pointer({ mode: 0, x: -0, y: 0 })).length, 77);
  assert.equal(encodePointerButtonEvent(pointerButton({ state: 0 })).length, 69);
});

test("every pointer DTO field is acquired once before validation and the resulting packet never retains caller getters", () => {
  for (const [encode, values] of [[encodePointerEvent, pointer()], [encodePointerButtonEvent, pointerButton()]]) {
    const reads = new Map(), event = {};
    for (const [name, value] of Object.entries(values)) Object.defineProperty(event, name, {
      get() { const count = (reads.get(name) ?? 0) + 1; reads.set(name, count); return count === 1 ? value : null; },
    });
    const bytes = encode(event), view = new DataView(bytes.buffer);
    assert.deepEqual([...reads].sort(), Object.keys(values).map(name => [name, 1]).sort());
    assert.equal(view.getBigUint64(7, true), 0xfedcba9876543210n);
    assert.equal(view.getBigInt64(15, true), 0x0102030405060708n);
    assert.equal(view.getBigUint64(27, true), 0x8877665544332211n);
    assert.equal(view.getUint32(41, true), 0x12345678);
    assert.equal(view.getUint32(64, true), 0x90abcdef);
    if (values.kind === "pointer") {
      assert.equal(view.getUint32(68, true), 0x3fc00000);
      assert.equal(view.getUint32(72, true), 0xc0100000);
      assert.equal(bytes[76], 1);
    } else assert.equal(bytes[68], 2);
    for (const name of Object.keys(values)) void event[name];
    values.hostNs = 0n; values.source = 3n; values.control = 0;
    assert.equal(view.getBigInt64(15, true), 0x0102030405060708n);
    assert.equal(view.getBigUint64(7, true), 0xfedcba9876543210n);
    assert.equal(view.getUint32(64, true), 0x90abcdef);
    const throwing = values.kind === "pointer" ? pointer() : pointerButton();
    const failure = new Error("original acquisition getter failed");
    Object.defineProperty(throwing, "source", { get() { throw failure; } });
    assert.throws(() => encode(throwing));
    const invalidReads = new Map(), invalid = {};
    for (const [name, value] of Object.entries(values)) Object.defineProperty(invalid, name, {
      get() { invalidReads.set(name, (invalidReads.get(name) ?? 0) + 1); return name === "kind" ? "touch" : value; },
    });
    assert.throws(() => encode(invalid));
    assert.deepEqual([...invalidReads].sort(), Object.keys(values).map(name => [name, 1]).sort(),
      "all required fields are captured once before semantic validation");
  }
});

const hid = fields => ({ kind: "hid", hostNs: 0x0102030405060708n, source: 0xfedcba9876543210n,
  sequence: 0x8877665544332211n, reportId: 0x7f, data: Uint8Array.from([0x7f, 0, 0xff, 0x80]), ...fields });

test("raw WebHID packets preserve separate report IDs and exact payload in the shared Rust golden", () => {
  // Independent literal repeated in browser_hid_fixtures.rs, decoded by core BKPI.
  const literal = Uint8Array.from([
    66, 75, 80, 73, 1, 0, 5,
    0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
    8, 7, 6, 5, 4, 3, 2, 1, 0x4e, 0x49, 0x57, 0,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    1, 0x44, 0x49, 0x48, 0x57, 1, 0x7f, 0, 0, 0,
    1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1,
    0, 1, 0x7f, 4, 0, 0, 0, 0, 0, 0, 0, 0x7f, 0, 0xff, 0x80,
  ]);
  const backing = Uint8Array.from([99, 0x7f, 0, 0xff, 0x80, 99]);
  const record = hid({ data: backing.subarray(1, 5) });
  const encoded = encodeRawHidEvent(record);
  assert.deepEqual(encoded, literal);
  assert.equal(encoded.length, 73);
  assert.notEqual(encoded.buffer, backing.buffer);
  backing.fill(0);
  record.reportId = 0; record.source = 3n;
  assert.deepEqual(encoded, literal);
  for (const reportId of [0, 1, 255]) for (const size of [0, 1, 1024]) {
    const data = new Uint8Array(size).fill(reportId);
    const bytes = encodeRawHidEvent(hid({ reportId, data, source: 18446744073709551615n,
      sequence: 18446744073709551615n, hostNs: 9223372036854775807n }));
    const view = new DataView(bytes.buffer);
    assert.equal(bytes.length, (reportId === 0 ? 68 : 69) + size);
    assert.equal(bytes[59], reportId === 0 ? 0 : 1);
    if (reportId !== 0) assert.equal(bytes[60], reportId);
    assert.equal(view.getBigUint64(reportId === 0 ? 60 : 61, true), BigInt(size));
    assert.deepEqual(bytes.subarray(reportId === 0 ? 68 : 69), data, "a leading payload byte equal to the separate ID is never stripped");
    assert.equal(view.getUint32(41, true), reportId);
    assert.equal(view.getBigUint64(7, true), 18446744073709551615n);
    assert.equal(view.getBigUint64(27, true), 18446744073709551615n);
    assert.equal(view.getBigInt64(15, true), 9223372036854775807n);
    assert.equal(view.getBigInt64(50, true), 9223372036854775807n);
  }
});

test("raw HID encoding rejects malformed acquisition identity or payload instead of coercing or truncating", () => {
  for (const [field, values] of [
    ["kind", [undefined, "touch", null]], ["hostNs", [1, "1", -1n, 9223372036854775808n]],
    ["source", [undefined, 3, 0n, 2n, -1n, 18446744073709551616n]],
    ["sequence", [1, "1", -1n, 18446744073709551616n]],
    ["reportId", [undefined, -1, 256, 1.5, "1", 1n, NaN, Infinity]],
    ["data", [undefined, null, [], new DataView(new ArrayBuffer(1)), new Uint16Array(1), new Uint8Array(1025)]],
  ]) for (const value of values) assert.throws(() => encodeRawHidEvent(hid({ [field]: value })));
  for (const value of [undefined, null, [], "report"]) assert.throws(() => encodeRawHidEvent(value));
  const minimum = encodeRawHidEvent(hid({ hostNs: 0n, source: 3n, sequence: 0n, reportId: 0, data: new Uint8Array() }));
  assert.equal(minimum.length, 68);
  assert.deepEqual(Array.from(minimum.subarray(59)), [0, 0, 0, 0, 0, 0, 0, 0, 0]);
  for (const length of [0, 1]) {
    const data = new Uint8Array(length);
    structuredClone(data.buffer, { transfer: [data.buffer] });
    assert.throws(() => encodeRawHidEvent(hid({ data })), "a detached view must not become an invented empty report");
  }
});

const touch = fields => ({ kind: "touch", hostNs: 0x0102030405060708n, sequence: 0x8877665544332211n,
  contact: 0xfedcba9876543210n, phase: 0, code: 0xfffffffe, x: 1.5, y: -2.25, pressure: 0.5, width: 480, height: 360, ...fields });

test("touch acquisition has a literal canonical packet shared with the actual Rust decoder", () => {
  const literal = Uint8Array.from([
    66, 75, 80, 73, 1, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0,
    8, 7, 6, 5, 4, 3, 2, 1, 0x4e, 0x49, 0x57, 0,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    1, 0x55, 0x4f, 0x54, 0x57, 1, 0xfe, 0xff, 0xff, 0xff,
    1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1,
    0, 1, 0x55, 0x4f, 0x54, 0x57, 0, 0, 0, 0,
    0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0,
    0, 0, 0xc0, 0x3f, 0, 0, 0x10, 0xc0, 1, 0, 0, 0, 0x3f,
  ]);
  const event = touch();
  const bytes = encodeTouchEvent(event);
  assert.deepEqual(bytes, literal);
  event.x = 99; event.contact = 0n;
  assert.deepEqual(bytes, literal);
  for (const phase of [0, 1, 2, 3]) for (const pressure of [null, -0, 12.5]) {
    const encoded = encodeTouchEvent(touch({ phase, pressure, contact: 18446744073709551615n, hostNs: 9223372036854775807n }));
    const view = new DataView(encoded.buffer, encoded.byteOffset, encoded.byteLength);
    assert.equal(encoded.length, pressure === null ? 86 : 90);
    assert.equal(view.getBigUint64(68, true), 18446744073709551615n);
    assert.equal(view.getBigInt64(15, true), 9223372036854775807n);
    assert.equal(encoded[76], phase);
    assert.equal(encoded[85], pressure === null ? 0 : 1);
    if (pressure !== null) assert.equal(view.getFloat32(86, true), pressure);
  }
});

test("touch bindings keep one native surface and projection never changes original acquisition fields", () => {
  const lanes = new Uint8Array([0x13, 0x11, 0x29]);
  const words = touchBindingWords(lanes);
  assert.deepEqual(Array.from(words), [0x13, 0, 0, 0, 1, 0x57544f55, 0, 0x11, 0, 0, 0, 1, 0x57544f55, 0, 0x29, 0, 0, 0, 1, 0x57544f55, 0]);
  lanes[0] = 0x12;
  assert.equal(words[0], 0x13);
  assert.equal(touchBindingWords([]).length, 0);
  const all = [...Array.from({ length: 9 }, (_, i) => 0x11 + i), ...Array.from({ length: 9 }, (_, i) => 0x21 + i)];
  assert.equal(touchBindingWords(all).length, 126);
  for (const bad of [null, {}, [0x10], [0x11, 0x11], Array(1), Array(19).fill(0x11), ["17"], [17.5]]) assert.throws(() => touchBindingWords(bad));
  const event = touch({ x: 120, y: 90 });
  const before = encodeTouchEvent(event);
  assert.deepEqual(projectTouchEvent(event, 960, 720), { x: 240, y: 180 });
  assert.deepEqual(projectTouchEvent(event, 1920, 1080), { x: 480, y: 270 });
  assert.deepEqual(encodeTouchEvent(event), before);
  assert.equal(event.x, 120);
  assert.equal(event.width, 480);
});

test("touch acquisition and projection refuse malformed extents floats and integer provenance without clamping", () => {
  for (const [field, values] of [
    ["kind", [undefined, "keyboard", null]], ["hostNs", [1, -1n, 9223372036854775808n]],
    ["sequence", [1, -1n, 18446744073709551616n]], ["contact", [1, -1n, 18446744073709551616n]],
    ["phase", [-1, 4, 1.5, "0"]], ["code", [-1, 4294967296, 0.5, 1n]],
    ["x", [NaN, Infinity, -Infinity, 1e100, "1"]], ["y", [NaN, Infinity, 1e100]],
    ["pressure", [undefined, NaN, Infinity, 1e100, "0"]],
    ["width", [0, -1, NaN, Infinity, "480"]], ["height", [0, -1, NaN, Infinity]],
  ]) for (const value of values) assert.throws(() => encodeTouchEvent(touch({ [field]: value })));
  for (const extent of [0, -1, 1.5, 4294967296, NaN, Infinity, "960"]) {
    assert.throws(() => projectTouchEvent(touch(), extent, 720));
    assert.throws(() => projectTouchEvent(touch(), 960, extent));
  }
  assert.throws(() => projectTouchEvent(touch({ width: Number.MIN_VALUE, x: 1 }), 960, 720));
  const finite = touch({ x: 1 / 3, y: -0, pressure: 1 / 3 });
  const encoded = encodeTouchEvent(finite), view = new DataView(encoded.buffer);
  assert.equal(view.getFloat32(77, true), Math.fround(1 / 3));
  assert.equal(view.getFloat32(81, true), -0);
  assert.equal(view.getFloat32(86, true), Math.fround(1 / 3));
});

test("keyboard acquisition encodes canonical native-control BKPI bytes with exact 64-bit provenance", () => {
  const event = { hostNs: 0x0102030405060708n, key: 0x1234, down: true, sequence: 0x8877665544332211n };
  const literal = Uint8Array.from([
    66, 75, 80, 73, 1, 0, 0,
    1, 0, 0, 0, 0, 0, 0, 0,
    8, 7, 6, 5, 4, 3, 2, 1,
    0x4e, 0x49, 0x57, 0,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    1, 0x59, 0x45, 0x4b, 0x57, 1, 0x34, 0x12, 0, 0,
    1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1,
    0, 1, 0x59, 0x45, 0x4b, 0x57, 0x34, 0x12, 0, 0, 0,
  ]);
  const encoded = encodeKeyboardEvent(event);
  assert.deepEqual(encoded, literal);
  assert.equal(encoded.byteLength, 69);
  event.hostNs = 0n;
  assert.deepEqual(encoded, literal, "the owned packet cannot retain a mutable event object");
  for (const hostNs of [0n, 9007199254740993n, 9223372036854775807n]) {
    for (const sequence of [0n, 9007199254740993n, 18446744073709551615n]) {
      const bytes = encodeKeyboardEvent({ hostNs, sequence, key: 65535, down: false });
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      assert.equal(view.getBigInt64(15, true), hostNs);
      assert.equal(view.getBigUint64(27, true), sequence);
      assert.equal(view.getBigInt64(50, true), hostNs, "native acquisition keeps the same original host point");
      assert.equal(view.getUint32(41, true), 65535);
      assert.equal(view.getUint32(64, true), 65535);
      assert.equal(bytes[68], 1);
    }
  }
  const valid = { hostNs: 1n, sequence: 1n, key: 1, down: true };
  for (const [field, values] of [
    ["hostNs", [undefined, null, 1, "1", -1n, 9223372036854775808n]],
    ["sequence", [undefined, null, 1, "1", -1n, 18446744073709551616n]],
    ["key", [undefined, null, 0, -1, 65536, 1.5, NaN, Infinity, 1n, "1"]],
    ["down", [undefined, null, 0, 1, "true"]],
  ]) {
    for (const value of values) assert.throws(() => encodeKeyboardEvent({ ...valid, [field]: value }));
  }
  for (const value of [undefined, null, [], "event"]) assert.throws(() => encodeKeyboardEvent(value));
});

test("keyboard bindings retain bounded native adapter IDs and independent snapshots without inventing HID usages", () => {
  const backing = new Uint32Array([99, 0x11, 2, 0x29, 65535, 99]);
  const pairs = backing.subarray(1, 5);
  const words = keyboardBindingWords(pairs);
  assert.ok(words instanceof Uint32Array);
  assert.deepEqual(Array.from(words), [0x11, 0, 0, 0, 1, 0x574b4559, 2, 0x29, 0, 0, 0, 1, 0x574b4559, 65535]);
  assert.notEqual(words.buffer, pairs.buffer);
  pairs[1] = 3;
  assert.equal(words[6], 2);
  words[13] = 4;
  assert.equal(pairs[3], 65535);
  const lanes = [...Array.from({ length: 9 }, (_, i) => 0x11 + i), ...Array.from({ length: 9 }, (_, i) => 0x21 + i)];
  const complete = Uint32Array.from(lanes.flatMap((lane, index) => [lane, index + 1]));
  const full = keyboardBindingWords(complete);
  assert.equal(full.length, 18 * 7);
  assert.deepEqual(lanes.map((_, i) => full[i * 7]), lanes);
  assert.deepEqual(lanes.map((_, i) => full[i * 7 + 6]), Array.from({ length: 18 }, (_, i) => i + 1));
  assert.deepEqual(keyboardBindingWords(new Uint32Array()), new Uint32Array());
  for (const input of [undefined, null, [], [0x11, 2], new Uint16Array([0x11, 2]),
    new Uint32Array([0x11]), new Uint32Array(38), new Uint32Array([0x10, 2]), new Uint32Array([0x111, 2]),
    new Uint32Array([0x11, 0]), new Uint32Array([0x11, 65536]),
    new Uint32Array([0x11, 2, 0x11, 3]), new Uint32Array([0x11, 2, 0x12, 2])]) {
    assert.throws(() => keyboardBindingWords(input));
  }
  assert.equal(complete[0], 0x11);
  assert.equal(complete.at(-1), 18);
});

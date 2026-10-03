// Deferred canonical acquisition encoding; no browser, WASM or device execution.
import assert from "node:assert/strict";
import test from "node:test";
import { encodeKeyboardEvent, keyboardBindingWords, encodeTouchEvent, touchBindingWords, projectTouchEvent } from "./physical-input.mjs";

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

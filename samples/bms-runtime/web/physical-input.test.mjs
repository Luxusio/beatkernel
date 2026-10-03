// Deferred canonical acquisition encoding; no browser, WASM or device execution.
import assert from "node:assert/strict";
import test from "node:test";
import { encodeKeyboardEvent, keyboardBindingWords } from "./physical-input.mjs";

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

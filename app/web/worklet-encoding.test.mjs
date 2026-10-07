// Deferred: node --experimental-vm-modules --test app/web/*.test.mjs
// Native Node encoders are the oracle; the actual bootstrap runs in an isolated realm.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TextDecoder as NativeDecoder, TextEncoder as NativeEncoder } from "node:util";
import { createContext, SourceTextModule } from "node:vm";

const source = await readFile(new URL("./worklet-encoding.mjs", import.meta.url), "utf8");

async function bootstrap(existing = {}) {
  const context = createContext({
    ArrayBuffer, Uint8Array, Uint16Array, DataView,
    TextEncoder: undefined, TextDecoder: undefined, ...existing,
  });
  const module = new SourceTextModule(source, { context });
  await module.link(() => { throw new Error("encoding bootstrap must have no dependencies"); });
  await module.evaluate();
  return { context, Encoder: context.TextEncoder, Decoder: context.TextDecoder };
}

test("bootstrap installs only missing constructors and leaves existing implementations intact", async () => {
  for (const existing of [
    { TextEncoder: NativeEncoder, TextDecoder: NativeDecoder },
    { TextEncoder: NativeEncoder },
    { TextDecoder: NativeDecoder },
    {},
  ]) {
    const { Encoder, Decoder, context } = await bootstrap(existing);
    if (existing.TextEncoder) assert.equal(Encoder, existing.TextEncoder);
    if (existing.TextDecoder) assert.equal(Decoder, existing.TextDecoder);
    assert.equal(new Encoder().encoding, "utf-8");
    assert.equal(new Decoder().encoding, "utf-8");
    const second = new SourceTextModule(source, { context });
    await second.link(() => { throw new Error("unexpected import"); });
    await second.evaluate();
    assert.equal(context.TextEncoder, Encoder);
    assert.equal(context.TextDecoder, Decoder);
  }
});

test("scalar UTF-8 encoding and lone-surrogate replacement match the native encoder", async () => {
  const { Encoder } = await bootstrap();
  const actual = new Encoder();
  const expected = new NativeEncoder();
  const strings = [
    "", "ASCII\0\t\r\n", "리듬 日本語 é", "𝄞🎵", "\ufeff",
    "\ud800", "\udfff", "\ud800A\udfff", "\ud800\ud800\udc00",
    String.fromCodePoint(0, 0x7f, 0x80, 0x7ff, 0x800, 0xd7ff, 0xe000, 0xffff, 0x10000, 0x10ffff),
  ];
  for (const value of [...strings, undefined, null, false, 123, { toString: () => "곡🎵" }]) {
    assert.deepEqual(Array.from(actual.encode(value)), Array.from(expected.encode(value)));
  }
  assert.deepEqual(Array.from(actual.encode()), []);
  assert.throws(() => actual.encode(Symbol("not text")), { name: "TypeError" });
});

test("every partial encodeInto boundary preserves native read and byte counts and untouched bytes", async () => {
  const { Encoder } = await bootstrap();
  const actual = new Encoder();
  const expected = new NativeEncoder();
  for (const value of ["Aé한🎵Z", "\ud800a\udfff", "🎵🎵", "\0", "", undefined]) {
    const text = String(value);
    const extent = expected.encode(text).length;
    for (let capacity = 0; capacity <= extent + 2; capacity++) {
      const actualBacking = new Uint8Array(capacity + 6).fill(0xa5);
      const expectedBacking = actualBacking.slice();
      const actualResult = actual.encodeInto(value, actualBacking.subarray(3, 3 + capacity));
      const expectedResult = expected.encodeInto(text, expectedBacking.subarray(3, 3 + capacity));
      assert.deepEqual({ read: actualResult.read, written: actualResult.written }, expectedResult,
        `UTF-16 consumption for ${JSON.stringify(text)} at ${capacity} bytes`);
      assert.deepEqual(actualBacking, expectedBacking);
    }
  }
  assert.throws(() => actual.encodeInto("a", new Uint16Array(4)), { name: "TypeError" });
  assert.throws(() => actual.encodeInto(Symbol("not text"), new Uint8Array(8)), { name: "TypeError" });
});

test("decoder respects typed-view byte offsets, aliases and nonstreaming BOM state", async () => {
  const { Decoder } = await bootstrap();
  const bytes = new NativeEncoder().encode("\ufeff한🎵\0");
  const backing = new Uint8Array(bytes.length + 6).fill(0xff);
  backing.set(bytes, 3);
  const views = [
    new Uint8Array(backing.buffer, 3, bytes.length),
    new DataView(backing.buffer, 3, bytes.length),
    bytes.buffer.slice(0),
  ];
  const wideBacking = new Uint8Array([0xff, 0xff, 0xf0, 0x9d, 0x84, 0x9e, 0xff, 0xff]);
  views.push(new Uint16Array(wideBacking.buffer, 2, 2));
  for (const label of ["utf-8", "UTF8", " \tunicode-1-1-utf-8\r\n"]) {
    for (const options of [{}, { ignoreBOM: true }, { fatal: true }, { fatal: true, ignoreBOM: true }]) {
      const actual = new Decoder(label, options);
      const expected = new NativeDecoder(label, options);
      assert.equal(actual.encoding, expected.encoding);
      assert.equal(actual.fatal, expected.fatal);
      assert.equal(actual.ignoreBOM, expected.ignoreBOM);
      for (const view of views) assert.equal(actual.decode(view), expected.decode(view));
      // Independent decode calls each make their own leading-BOM decision.
      assert.equal(actual.decode(bytes), expected.decode(bytes));
      assert.equal(actual.decode(bytes), expected.decode(bytes));
      assert.equal(actual.decode(), "");
    }
  }
});

test("malformed maximal subparts, incomplete tails and fatal decoding match native UTF-8", async () => {
  const { Decoder } = await bootstrap();
  const cases = [
    [0x80, 0xbf], [0xc0, 0xaf], [0xc1, 0xbf], [0xff, 0xfe],
    [0xc2], [0xe2, 0x82], [0xf0, 0x9f, 0x92],
    [0xe1, 0x80, 0x41], [0xf0, 0x90, 0x80, 0x41],
    [0xe0, 0x80, 0x80], [0xed, 0xa0, 0x80], [0xed, 0xbf, 0xbf],
    [0xf0, 0x80, 0x80, 0x80], [0xf4, 0x90, 0x80, 0x80], [0xf5, 0x80, 0x80, 0x80],
    [0xe2, 0xc2, 0xa2, 0x41], [0xf0, 0x90, 0x41, 0x80],
    [0xff, 0xef, 0xbb, 0xbf, 0x41], [0xef, 0xbb, 0xbf, 0xff, 0x41],
  ];
  for (const values of cases) {
    const bytes = Uint8Array.from(values);
    assert.equal(new Decoder().decode(bytes), new NativeDecoder().decode(bytes), values.join(","));
    assert.throws(() => new NativeDecoder("utf-8", { fatal: true }).decode(bytes), { name: "TypeError" });
    assert.throws(() => new Decoder("utf-8", { fatal: true }).decode(bytes), { name: "TypeError" });
  }
  const replacement = Uint8Array.from([0xef, 0xbf, 0xbd]);
  assert.equal(new Decoder("utf-8", { fatal: true }).decode(replacement), "�");
});

test("unsupported encodings, stream requests and invalid buffer inputs fail explicitly", async () => {
  const { Decoder } = await bootstrap();
  for (const label of ["utf-16le", "latin1", "x-not-an-encoding", "utf-8\u00a0", ""]) {
    assert.throws(() => new Decoder(label), { name: "RangeError" });
  }
  assert.throws(() => new Decoder(Symbol("label")), { name: "TypeError" });
  const decoder = new Decoder();
  assert.throws(() => decoder.decode(new Uint8Array([0xe2]), { stream: true }), { name: "TypeError" });
  assert.equal(decoder.decode(new Uint8Array([0x41]), { stream: false }), "A");
  for (const input of ["A", [65], 65, null, {}]) {
    assert.throws(() => decoder.decode(input), { name: "TypeError" });
  }
});

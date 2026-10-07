// Only the non-streaming UTF-8 surface needed by generated wasm-bindgen glue.
// AudioWorklet globals vary by browser; never replace a native implementation.
function stringValue(value) {
  if (typeof value === "symbol") throw new TypeError("A symbol is not a string.");
  return String(value);
}

function scalarAt(text, index) {
  const first = text.charCodeAt(index);
  if (first >= 0xd800 && first <= 0xdbff) {
    const second = text.charCodeAt(index + 1);
    if (second >= 0xdc00 && second <= 0xdfff) return 0x10000 + (first - 0xd800) * 1024 + second - 0xdc00;
    return 0xfffd;
  }
  return first >= 0xdc00 && first <= 0xdfff ? 0xfffd : first;
}

function byteCount(scalar) {
  return scalar < 0x80 ? 1 : scalar < 0x800 ? 2 : scalar < 0x10000 ? 3 : 4;
}

function writeScalar(output, offset, scalar, count) {
  if (count === 1) {
    output[offset] = scalar;
  } else {
    output[offset] = (count === 2 ? 0xc0 : count === 3 ? 0xe0 : 0xf0) | (scalar >> (6 * (count - 1)));
    for (let index = 1; index < count; index++) output[offset + index] = 0x80 | ((scalar >> (6 * (count - 1 - index))) & 0x3f);
  }
}

class WorkletTextEncoder {
  get encoding() { return "utf-8"; }

  encode(input = "") {
    const text = stringValue(input);
    let length = 0;
    for (let index = 0; index < text.length;) {
      const scalar = scalarAt(text, index);
      length += byteCount(scalar);
      index += scalar > 0xffff ? 2 : 1;
    }
    const output = new Uint8Array(length);
    this.encodeInto(text, output);
    return output;
  }

  encodeInto(input, destination) {
    const text = stringValue(input);
    if (!(destination instanceof Uint8Array)) throw new TypeError("UTF-8 destination must be Uint8Array.");
    let read = 0;
    let written = 0;
    while (read < text.length) {
      const scalar = scalarAt(text, read);
      const count = byteCount(scalar);
      if (written + count > destination.length) break;
      writeScalar(destination, written, scalar, count);
      written += count;
      read += scalar > 0xffff ? 2 : 1;
    }
    return { read, written };
  }
}

function bytesOf(input) {
  if (input === undefined) return new Uint8Array(0);
  if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  if (input instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && input instanceof SharedArrayBuffer)) return new Uint8Array(input);
  throw new TypeError("UTF-8 input must be a buffer or buffer view.");
}

class WorkletTextDecoder {
  #fatal;
  #ignoreBOM;

  constructor(label = "utf-8", options = {}) {
    const encoding = stringValue(label).replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, "").toLowerCase();
    if (encoding !== "utf-8" && encoding !== "utf8" && encoding !== "unicode-1-1-utf-8") throw new RangeError("Only UTF-8 is supported in this worklet.");
    this.#fatal = Boolean(options?.fatal);
    this.#ignoreBOM = Boolean(options?.ignoreBOM);
  }

  get encoding() { return "utf-8"; }
  get fatal() { return this.#fatal; }
  get ignoreBOM() { return this.#ignoreBOM; }

  decode(input, options = {}) {
    if (options?.stream) throw new TypeError("Streaming UTF-8 decoding is not supported in this worklet.");
    const bytes = bytesOf(input);
    let output = "";
    let first = true;
    for (let index = 0; index < bytes.length;) {
      const lead = bytes[index++];
      let scalar = lead;
      let remaining = 0;
      let lower = 0x80;
      let upper = 0xbf;
      let valid = true;
      if (lead <= 0x7f) {
        // ASCII needs no continuation.
      } else if (lead >= 0xc2 && lead <= 0xdf) {
        scalar = lead & 0x1f;
        remaining = 1;
      } else if (lead >= 0xe0 && lead <= 0xef) {
        scalar = lead & 0x0f;
        remaining = 2;
        if (lead === 0xe0) lower = 0xa0;
        if (lead === 0xed) upper = 0x9f;
      } else if (lead >= 0xf0 && lead <= 0xf4) {
        scalar = lead & 0x07;
        remaining = 3;
        if (lead === 0xf0) lower = 0x90;
        if (lead === 0xf4) upper = 0x8f;
      } else {
        valid = false;
      }
      while (valid && remaining > 0) {
        if (index === bytes.length || bytes[index] < lower || bytes[index] > upper) {
          valid = false;
          break;
        }
        scalar = (scalar << 6) | (bytes[index++] & 0x3f);
        remaining--;
        lower = 0x80;
        upper = 0xbf;
      }
      if (!valid) {
        if (this.#fatal) throw new TypeError("Malformed UTF-8.");
        scalar = 0xfffd;
      }
      if (!first || this.#ignoreBOM || scalar !== 0xfeff) output += String.fromCodePoint(scalar);
      first = false;
    }
    return output;
  }
}

if (typeof globalThis.TextEncoder === "undefined") globalThis.TextEncoder = WorkletTextEncoder;
if (typeof globalThis.TextDecoder === "undefined") globalThis.TextDecoder = WorkletTextDecoder;

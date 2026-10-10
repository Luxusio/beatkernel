import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

// Require an explicitly compiled artifact: never substitute a mock publisher
// or grant the module host filesystem imports.
const wasmPath = process.env.BEATKERNEL_NATIVE_PUBLICATION_WASM;
assert.ok(wasmPath, "Set BEATKERNEL_NATIVE_PUBLICATION_WASM to the compiled fixture path");
const module = await WebAssembly.compile(await readFile(wasmPath));
assert.deepEqual(WebAssembly.Module.imports(module), []);
const instance = await WebAssembly.instantiate(module, {});

test("native publication fixture has no host imports", () => {
  assert.deepEqual(WebAssembly.Module.imports(module), []);
});

test("ordinary native save returns Unsupported instead of a WASM trap", () => {
  assert.equal(typeof instance.exports.ordinary_refusal, "function");
  assert.equal(instance.exports.ordinary_refusal(), 1);
});

test("reserved native filename returns InvalidInput without a WASM trap", () => {
  assert.equal(typeof instance.exports.reserved_refusal, "function");
  assert.equal(instance.exports.reserved_refusal(), 1);
});

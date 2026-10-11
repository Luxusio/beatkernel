import assert from "node:assert/strict";
import test from "node:test";
import { validateBrowserDisplayFields } from "./display-settings.mjs";

test("browser Display admits literal CSS extent boundaries and preserves leading zeros", () => {
  for (const fields of [
    ["auto", "fifo", "1", "1"],
    ["auto", "fifo", "16384", "16384"],
    ["auto", "fifo", "00001", "00001"],
    ["auto", "fifo", "960", "720"],
    ["auto", "fifo", "00001", "16384"],
    ["auto", "fifo", "16384", "00001"],
  ]) {
    const before = [...fields];
    const accepted = validateBrowserDisplayFields(fields);
    assert.deepEqual(accepted, before);
    assert.ok(Array.isArray(accepted));
    assert.equal(accepted.length, 4);
    assert.ok(Object.isFrozen(accepted));
    assert.notEqual(accepted, fields);
    assert.deepEqual(fields, before);
  }
});

test("browser Display accepted snapshot is independent and immutable", () => {
  const fields = ["auto", "fifo", "00001", "16384"];
  const accepted = validateBrowserDisplayFields(fields);
  fields[0] = "vulkan";
  fields[2] = "0";
  fields[3] = "1";
  fields.push("extra");
  assert.deepEqual(accepted, ["auto", "fifo", "00001", "16384"]);
  assert.throws(() => { accepted[3] = "0"; }, TypeError);
  assert.throws(() => accepted.push("extra"), TypeError);
  assert.deepEqual(accepted, ["auto", "fifo", "00001", "16384"]);
});

test("browser Display refuses invalid dimensions in either slot without changing the source", () => {
  const invalid = ["", "0", "00000", "16385", "99999", "000001", "016384", "100000",
    " 1", "1 ", "\t1", "1\n", "+1", "-1", "1.0", "0.5", "1e3", "0x10",
    "NaN", "Infinity", "１", "١", 1, 16384, null, undefined, true, {}, [], new String("1")];
  for (const index of [2, 3]) {
    for (const value of invalid) {
      const fields = ["auto", "fifo", "960", "720"];
      fields[index] = value;
      const before = [...fields];
      assert.throws(() => validateBrowserDisplayFields(fields), Error,
        `dimension ${index} must refuse ${String(value)}`);
      assert.deepEqual(fields, before);
    }
  }
  // Refusing a bad final field must leave the same mutable draft repairable.
  const draft = ["auto", "fifo", "00001", "16385"];
  assert.throws(() => validateBrowserDisplayFields(draft), Error);
  assert.deepEqual(draft, ["auto", "fifo", "00001", "16385"]);
  draft[3] = "16384";
  assert.deepEqual(validateBrowserDisplayFields(draft), ["auto", "fifo", "00001", "16384"]);
});

test("browser Display accepts only the supported string backend and presentation modes", () => {
  for (const [index, values] of [
    [0, ["", "Auto", "AUTO", "auto ", " auto", "vulkan", "webgpu", "gl", "fifo",
      null, undefined, 0, true, {}, new String("auto")]],
    [1, ["", "Fifo", "FIFO", "fifo ", " fifo", "immediate", "mailbox", "auto",
      null, undefined, 0, true, {}, new String("fifo")]],
  ]) {
    for (const value of values) {
      const fields = ["auto", "fifo", "960", "720"];
      fields[index] = value;
      const before = [...fields];
      assert.throws(() => validateBrowserDisplayFields(fields), Error);
      assert.deepEqual(fields, before);
    }
  }
});

test("browser Display refuses nonarrays, partial, oversized, sparse and unbound drafts", () => {
  const malformed = [undefined, null, false, 4, "auto,fifo,960,720", {},
    { 0: "auto", 1: "fifo", 2: "960", 3: "720", length: 4 },
    [], ["auto"], ["auto", "fifo"], ["auto", "fifo", "960"],
    ["auto", "fifo", "960", "720", "extra"], ["", "", "", ""], Array(4)];
  for (let index = 0; index < 4; index++) {
    const sparse = ["auto", "fifo", "960", "720"];
    delete sparse[index];
    malformed.push(sparse);
    const unbound = ["auto", "fifo", "960", "720"];
    unbound[index] = "";
    malformed.push(unbound);
  }
  for (const fields of malformed) {
    const before = structuredClone(fields);
    assert.throws(() => validateBrowserDisplayFields(fields), Error);
    assert.deepEqual(fields, before);
  }
});

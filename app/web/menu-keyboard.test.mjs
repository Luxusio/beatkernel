import assert from "node:assert/strict";
import test from "node:test";
import { catalogKeyboardSelection } from "./menu-keyboard.mjs";

const state = fields => ({ route: 1, selected: 1, count: 4, available: true, playing: false, ...fields });
const event = (code, fields = {}) => ({ code, repeat: false, isComposing: false, target: { tagName: "CANVAS" }, ...fields });

test("catalog keyboard moves by one row and reaches actual first and last rows", () => {
  for (const [code, selected, expected] of [
    ["ArrowUp", 2, 1], ["ArrowDown", 1, 2], ["Home", 2, 0], ["End", 1, 3],
  ]) assert.equal(catalogKeyboardSelection(event(code), state({ selected })), expected);
  assert.equal(catalogKeyboardSelection(event("End"), state({ count: 8192 })), 8191);
});

test("catalog boundaries and one-row catalogs do not dispatch redundant selection RPCs", () => {
  for (const [code, selected] of [["ArrowUp", 0], ["Home", 0], ["ArrowDown", 3], ["End", 3]]) {
    assert.equal(catalogKeyboardSelection(event(code), state({ selected })), null);
  }
  for (const code of ["ArrowUp", "ArrowDown", "Home", "End"]) {
    assert.equal(catalogKeyboardSelection(event(code), state({ selected: 0, count: 1 })), null);
    assert.equal(catalogKeyboardSelection(event(code), state({ selected: 0, count: 0 })), null);
  }
});

test("catalog keys leave editors and composition ownership untouched", () => {
  for (const target of [
    { tagName: "INPUT" }, { tagName: "TEXTAREA" }, { tagName: "SELECT" },
    { tagName: "DIV", isContentEditable: true },
  ]) for (const code of ["ArrowUp", "ArrowDown", "Home", "End"]) {
    assert.equal(catalogKeyboardSelection(event(code, { target }), state({})), null);
  }
  assert.equal(catalogKeyboardSelection(event("ArrowDown", { isComposing: true }), state({})), null);
  assert.equal(catalogKeyboardSelection(event("ArrowDown", { repeat: true }), state({})), null);
});

test("catalog navigation is unavailable during gameplay or on other retained screens", () => {
  for (const fields of [{ available: false }, { available: undefined }, { playing: true },
    ...[2, 3, 4, 5, 6, 7, 9].map(route => ({ route }))]) {
    assert.equal(catalogKeyboardSelection(event("ArrowDown"), state(fields)), null);
  }
});

test("catalog navigation refuses malformed or stale row projections", () => {
  for (const fields of [
    { selected: -1 }, { selected: 4 }, { selected: 0.5 }, { selected: NaN }, { selected: "1" },
    { count: -1 }, { count: 1.5 }, { count: 8193 }, { count: Infinity }, { count: "4" }, { route: "1" },
  ]) assert.equal(catalogKeyboardSelection(event("ArrowDown"), state(fields)), null);
  for (const invalid of [null, undefined, {}, []]) {
    assert.equal(catalogKeyboardSelection(event("ArrowDown"), invalid), null);
  }
});

test("unrelated and modified keys remain with their original input owner", () => {
  for (const code of ["Enter", "Escape", "KeyZ", "PageUp", "PageDown", "Tab", ""]) {
    assert.equal(catalogKeyboardSelection(event(code), state({})), null);
  }
  for (const modifier of ["ctrlKey", "altKey", "metaKey"]) {
    assert.equal(catalogKeyboardSelection(event("ArrowDown", { [modifier]: true }), state({})), null);
  }
  for (const invalid of [null, undefined, {}]) assert.equal(catalogKeyboardSelection(invalid, state({})), null);
});

test("catalog keyboard computes intent without mutation, DOM work or preventing default", () => {
  const accepted = Object.freeze(state({}));
  const key = Object.freeze(event("ArrowDown", { preventDefault() { assert.fail("Window owns preventDefault after successful admission"); } }));
  assert.equal(catalogKeyboardSelection(key, accepted), 2);
  assert.equal(accepted.selected, 1);
});

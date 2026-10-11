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

import { recordsKeyboardControl } from "./menu-keyboard.mjs";
const recordsState = fields => state({ route: 4, ...fields });

test("Records keyboard intent selects actual rows and Back through existing semantic controls", () => {
  for (const [code, selected, expected] of [
    ["ArrowUp", 2, 50001n], ["ArrowDown", 1, 50002n],
    ["Home", 2, 50000n], ["End", 1, 50003n], ["Escape", 1, 55n],
  ]) assert.equal(recordsKeyboardControl(event(code), recordsState({ selected })), expected);
  assert.equal(recordsKeyboardControl(event("End"), recordsState({ count: 256 })), 50255n);
});

test("Records keyboard intent keeps boundaries quiet and admits empty catalog Back only", () => {
  for (const [code, selected] of [["ArrowUp", 0], ["Home", 0], ["ArrowDown", 3], ["End", 3]]) {
    assert.equal(recordsKeyboardControl(event(code), recordsState({ selected })), null);
  }
  for (const count of [0, 1]) {
    for (const code of ["ArrowUp", "ArrowDown", "Home", "End"]) {
      assert.equal(recordsKeyboardControl(event(code), recordsState({ count, selected: 0 })), null);
    }
    assert.equal(recordsKeyboardControl(event("Escape"), recordsState({ count, selected: 0 })), 55n);
  }
});

test("Records keys preserve editable composition modifier repeat and unavailable owners", () => {
  for (const code of ["ArrowUp", "ArrowDown", "Home", "End", "Escape"]) {
    for (const target of [{ tagName: "INPUT" }, { tagName: "textarea" }, { tagName: "SELECT" },
      { tagName: "DIV", isContentEditable: true }]) {
      assert.equal(recordsKeyboardControl(event(code, { target }), recordsState({})), null);
    }
    for (const flag of ["repeat", "isComposing", "ctrlKey", "altKey", "metaKey", "shiftKey"]) {
      assert.equal(recordsKeyboardControl(event(code, { [flag]: true }), recordsState({})), null);
    }
    for (const fields of [{ available: false }, { available: undefined }, { playing: true },
      ...[1, 2, 3, 5, 6, 7, 9].map(route => ({ route }))]) {
      assert.equal(recordsKeyboardControl(event(code), recordsState(fields)), null);
    }
  }
  assert.equal(catalogKeyboardSelection(event("ArrowDown", { shiftKey: true }), state({})), 2,
    "Records modifier policy must not change existing Selection policy");
});

test("Records malformed projections refuse row actions and Back without granting defaults", () => {
  for (const fields of [{ selected: -1 }, { selected: 4 }, { selected: 0.5 }, { selected: NaN }, { selected: "1" },
    { count: -1 }, { count: 1.5 }, { count: 257 }, { count: Infinity }, { count: "4" },
    { route: "4" }, { count: 0, selected: 1 }, { count: 0, selected: -1 }]) {
    for (const code of ["ArrowDown", "Escape"]) assert.equal(recordsKeyboardControl(event(code), recordsState(fields)), null);
  }
  for (const invalid of [null, undefined, {}, []]) {
    assert.equal(recordsKeyboardControl(event("Escape"), invalid), null);
    assert.equal(recordsKeyboardControl(invalid, recordsState({})), null);
  }
  for (const code of ["Enter", "KeyZ", "PageUp", "PageDown", "Tab", ""]) {
    assert.equal(recordsKeyboardControl(event(code), recordsState({})), null);
  }
});

test("Records keyboard intent does not mutate frozen state or prevent browser defaults", () => {
  const accepted = Object.freeze(recordsState({}));
  const key = Object.freeze(event("ArrowDown", { preventDefault() { assert.fail("Window owns admission and preventDefault"); } }));
  assert.equal(recordsKeyboardControl(key, accepted), 50002n);
  assert.equal(accepted.selected, 1);
});

import { settingsKeyboardControl } from "./menu-keyboard.mjs";
const settingsState = fields => state({ route: 2, count: 16, ...fields });
const settingsKeys = ["ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown", "Tab", "Escape"];

test("Settings keyboard reaches every field with existing semantic controls", () => {
  for (const [code, selected, expected] of [
    ["ArrowUp", 2, 1001n], ["ArrowDown", 1, 1002n],
    ["Home", 12, 1000n], ["End", 1, 1015n],
    ["PageDown", 0, 1010n], ["PageDown", 10, 1015n],
    ["PageUp", 15, 1005n], ["PageUp", 10, 1000n],
    ["Tab", 12, 1013n], ["Escape", 15, 11n],
  ]) assert.equal(settingsKeyboardControl(event(code), settingsState({ selected })), expected);
  assert.equal(settingsKeyboardControl(event("Tab", { shiftKey: true }), settingsState({ selected: 13 })), 1012n);
  assert.equal(settingsKeyboardControl(event("End"), settingsState({ count: 128 })), 1127n);
  for (let selected = 0; selected < 16; selected++) {
    assert.equal(settingsKeyboardControl(event("ArrowDown"), settingsState({ selected })),
      selected === 15 ? null : 1000n + BigInt(selected + 1));
    assert.equal(settingsKeyboardControl(event("Tab", { shiftKey: true }), settingsState({ selected })),
      selected === 0 ? null : 1000n + BigInt(selected - 1));
  }
});

test("Settings navigation clamps without repeating same-row actions or wrapping Tab", () => {
  for (const [code, selected, fields] of [
    ["ArrowUp", 0, {}], ["Home", 0, {}], ["PageUp", 0, {}],
    ["ArrowDown", 15, {}], ["End", 15, {}], ["PageDown", 15, {}],
    ["Tab", 15, {}], ["Tab", 0, { shiftKey: true }],
  ]) assert.equal(settingsKeyboardControl(event(code, fields), settingsState({ selected })), null);
  assert.equal(settingsKeyboardControl(event("PageUp"), settingsState({ selected: 3 })), 1000n);
  assert.equal(settingsKeyboardControl(event("PageDown"), settingsState({ selected: 13 })), 1015n);
  for (const code of settingsKeys.filter(code => code !== "Escape")) {
    assert.equal(settingsKeyboardControl(event(code), settingsState({ selected: 0, count: 1 })), null);
  }
  assert.equal(settingsKeyboardControl(event("Tab", { shiftKey: true }), settingsState({ selected: 0, count: 1 })), null);
  assert.equal(settingsKeyboardControl(event("Escape"), settingsState({ selected: 0, count: 1 })), 11n);
});

test("Settings keys leave editors, IME, repeats and modified input with their owners", () => {
  for (const code of settingsKeys) {
    for (const target of [
      { tagName: "INPUT" }, { tagName: "textarea" }, { tagName: "SELECT" },
      { tagName: "DIV", isContentEditable: true },
    ]) assert.equal(settingsKeyboardControl(event(code, { target }), settingsState({})), null);
    for (const flag of ["isComposing", "repeat", "ctrlKey", "altKey", "metaKey"]) {
      assert.equal(settingsKeyboardControl(event(code, { [flag]: true }), settingsState({})), null);
      assert.equal(settingsKeyboardControl(event(code, { [flag]: true, shiftKey: true }), settingsState({})), null);
    }
    if (code !== "Tab") {
      assert.equal(settingsKeyboardControl(event(code, { shiftKey: true }), settingsState({})), null);
    }
  }
  assert.equal(settingsKeyboardControl(event("Tab", { shiftKey: true }), settingsState({})), 1000n);
});

test("Settings keys refuse unavailable, gameplay and other-screen projections", () => {
  for (const fields of [
    { available: false }, { available: undefined }, { available: 1 }, { playing: true },
    ...[0, 1, 3, 4, 5, 6, 7, 9, "2"].map(route => ({ route })),
  ]) for (const code of settingsKeys) {
    assert.equal(settingsKeyboardControl(event(code), settingsState(fields)), null);
  }
});

test("Settings count and selection bounds also fence Escape without granting defaults", () => {
  for (const fields of [
    { count: 0 }, { count: -1 }, { count: 129 }, { count: 1.5 }, { count: NaN },
    { count: Infinity }, { count: "16" }, { count: undefined },
    { selected: -1 }, { selected: 16 }, { selected: 0.5 }, { selected: NaN },
    { selected: Infinity }, { selected: "1" }, { selected: undefined },
  ]) for (const code of settingsKeys) {
    assert.equal(settingsKeyboardControl(event(code), settingsState(fields)), null);
  }
  for (const invalid of [null, undefined, {}, []]) {
    assert.equal(settingsKeyboardControl(event("Escape"), invalid), null);
    assert.equal(settingsKeyboardControl(invalid, settingsState({})), null);
  }
  for (const code of ["Enter", "KeyZ", "Space", ""]) {
    assert.equal(settingsKeyboardControl(event(code), settingsState({})), null);
  }
});

test("Settings keyboard computes intent without mutating state or preventing defaults", () => {
  const accepted = Object.freeze(settingsState({ selected: 0 }));
  const key = Object.freeze(event("PageDown", {
    target: Object.freeze({ tagName: "CANVAS" }),
    preventDefault() { assert.fail("Window owns admission and preventDefault"); },
  }));
  assert.equal(settingsKeyboardControl(key, accepted), 1010n);
  assert.equal(accepted.selected, 0);
  assert.equal(accepted.count, 16);
});

test("34-field Settings crosses the scalar/binding boundary and retains late editor ownership", () => {
  const retained = selected => Object.freeze(settingsState({ count: 34, selected }));
  for (const [key, selected, expected] of [
    ["ArrowDown", 15, 1016n], ["ArrowUp", 16, 1015n],
    ["PageDown", 20, 1030n], ["PageDown", 30, 1033n],
    ["PageUp", 33, 1023n], ["End", 16, 1033n], ["Home", 33, 1000n],
    ["Tab", 32, 1033n], ["Escape", 33, 11n],
  ]) assert.equal(settingsKeyboardControl(event(key), retained(selected)), expected);
  for (const key of ["ArrowDown", "PageDown", "End", "Tab"])
    assert.equal(settingsKeyboardControl(event(key), retained(33)), null);
  assert.equal(settingsKeyboardControl(event("Tab", { shiftKey: true }), retained(33)), 1032n);
  for (const key of settingsKeys) {
    assert.equal(settingsKeyboardControl(event(key, { target: { tagName: "INPUT", id: "menu-editor" } }), retained(33)), null);
    assert.equal(settingsKeyboardControl(event(key, { isComposing: true }), retained(33)), null);
    assert.equal(settingsKeyboardControl(event(key), settingsState({ count: 34, selected: 34 })), null);
  }
});

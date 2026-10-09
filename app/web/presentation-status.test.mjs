import assert from "node:assert/strict";
import test from "node:test";
import { PresentationStatus } from "./presentation-status.mjs";

const worker = {};
const identity = overrides => ({ owner: 0, worker, selectedId: 1, generation: 1n, content: 1n, ...overrides });
function fixture(text = "Ready", error = false) {
  const writes = [];
  const status = new PresentationStatus((text, error) => writes.push({ text, error }), text, error);
  return { status, writes };
}

test("navigation cancels an obsolete wait overlay without accepting an old presentation", () => {
  const { status, writes } = fixture("Prepared chart", true);
  const old = identity({ menuGeneration: 77n, screen: 3n, revision: 5n });
  status.wait(old, "Waiting for old screen");
  status.invalidate(true);
  assert.deepEqual(writes.at(-1), { text: "Prepared chart", error: true });
  const count = writes.length;
  assert.equal(status.drawn(old), false);
  status.invalidate(true);
  assert.equal(writes.length, count);
  const current = identity({ menuGeneration: 77n, screen: 4n, revision: 6n });
  status.wait(current, "Waiting for current screen");
  assert.equal(status.drawn(old), false);
  assert.equal(status.drawn(current), true);
  assert.deepEqual(writes.at(-1), { text: "Prepared chart", error: true });
});

test("first-run visual ownership permits no prepared chart and validates menu tokens independently", () => {
  const { status, writes } = fixture("Choose a song");
  const menu = identity({ selectedId: 0, menuGeneration: 77n, screen: 3n, revision: 5n });
  status.wait(identity({ selectedId: -1 }), "Invalid selected chart identity");
  status.wait({ ...menu, screen: undefined }, "Incomplete menu wait");
  assert.equal(writes.length, 0);
  status.wait(menu, "Menu graphics waiting");
  assert.equal(status.drawn(menu), true);
  assert.deepEqual(writes.at(-1), { text: "Choose a song", error: false });
  const history = identity({ selectedId: 0, generation: 2n, content: 3n });
  status.wait(history, "Historical graphics waiting");
  assert.equal(status.drawn(history), true);
  assert.deepEqual(writes.at(-1), { text: "Choose a song", error: false });
});

test("matching actual presentation restores exact ordinary text and preceding error flag", () => {
  for (const error of [false, true]) {
    const { status, writes } = fixture("Previous operation", error);
    status.wait(identity(), "Graphics waiting");
    assert.deepEqual(writes, [{ text: "Graphics waiting", error: true }]);
    assert.equal(status.drawn(identity()), true);
    assert.deepEqual(writes.at(-1), { text: "Previous operation", error });
  }
});
test("repeated waits preserve baseline and identical transient feedback does not republish", () => {
  const { status, writes } = fixture();
  status.wait(identity(), "Waiting");
  status.wait(identity(), "Waiting");
  assert.equal(writes.length, 1);
  status.wait(identity(), "Still waiting");
  assert.equal(writes.length, 2);
  status.drawn(identity());
  assert.deepEqual(writes.at(-1), { text: "Ready", error: false });
});
test("idle and duplicate presentations perform zero status publications", () => {
  const { status, writes } = fixture();
  for (let index = 0; index < 1000; index++) assert.equal(status.drawn(identity()), false);
  assert.equal(writes.length, 0);
  status.wait(identity(), "Wait"); status.drawn(identity());
  for (let index = 0; index < 1000; index++) assert.equal(status.drawn(identity()), false);
  assert.equal(writes.length, 2);
});
test("every ordinary message invalidates restoration including identical transient text", () => {
  for (const [text, error] of [["New ordinary message", false], ["New error", true], ["Waiting", false], ["Ready", true]]) {
    const { status, writes } = fixture();
    status.wait(identity(), "Waiting"); status.message(text, error);
    const count = writes.length;
    assert.equal(status.drawn(identity()), false);
    assert.equal(writes.length, count);
    assert.deepEqual(writes.at(-1), { text, error });
  }
});
test("explicit invalidation cancels restore without publishing", () => {
  const { status, writes } = fixture(); status.wait(identity(), "Waiting");
  status.invalidate(); status.invalidate();
  assert.equal(status.drawn(identity()), false); assert.equal(writes.length, 1);
});
test("identity snapshot cannot be retargeted by mutable caller packets", () => {
  const { status, writes } = fixture(); const packet = identity();
  status.wait(packet, "Wait"); packet.generation = 2n; packet.content = 3n; packet.selectedId = 5;
  assert.equal(status.drawn(packet), false);
  assert.equal(status.drawn(identity()), true);
  assert.deepEqual(writes.at(-1), { text: "Ready", error: false });
});
test("all scope fields and generation/content must match before restoration", () => {
  for (const change of [{ owner: 1 }, { worker: {} }, { selectedId: 2 }, { playId: 2 }, { generation: 2n }, { content: 2n }]) {
    const { status, writes } = fixture(); status.wait(identity(), "Wait");
    assert.equal(status.drawn(identity(change)), false); assert.equal(writes.length, 1);
    assert.equal(status.drawn(identity()), true);
  }
  const { status } = fixture(); status.wait(identity({ playId: 2 }), "Wait");
  assert.equal(status.drawn(identity()), false); assert.equal(status.drawn(identity({ playId: 2 })), true);
});
test("older generations and contradictory content cannot displace a current pending wait", () => {
  const { status, writes } = fixture(); const current = identity({ generation: 3n, content: 9n });
  status.wait(current, "Current wait");
  status.wait(identity({ generation: 2n, content: 8n }), "Old wait");
  status.wait(identity({ generation: 3n, content: 10n }), "Contradictory wait");
  assert.equal(writes.length, 1);
  assert.equal(status.drawn(current), true);
});
test("newer generation supersedes wait identity while retaining the ordinary baseline", () => {
  const { status, writes } = fixture(); status.wait(identity(), "First wait");
  const newer = identity({ generation: 2n, content: 8n }); status.wait(newer, "Second wait");
  assert.equal(status.drawn(identity()), false); assert.equal(status.drawn(newer), true);
  assert.deepEqual(writes.at(-1), { text: "Ready", error: false });
});
test("changing active scope requires its own matching presentation", () => {
  for (const scope of [{ owner: 1 }, { worker: {} }, { selectedId: 2 }, { playId: 1 }]) {
    const { status } = fixture(); status.wait(identity(), "Old wait");
    const current = identity(scope); status.wait(current, "Current wait");
    assert.equal(status.drawn(identity()), false); assert.equal(status.drawn(current), true);
  }
});
test("malformed identities cannot publish or replace a valid pending recovery", () => {
  const invalid = [null, undefined, {}, ...[
    { owner: -1 }, { owner: NaN }, { owner: Number.MAX_SAFE_INTEGER + 1 }, { worker: null }, { worker: 1 },
    { selectedId: -1 }, { selectedId: 1.5 }, { selectedId: Number.MAX_SAFE_INTEGER + 1 },
    { playId: 0 }, { playId: null }, { playId: 1n },
    { generation: 0n }, { generation: -1n }, { generation: 1 }, { generation: 1n << 64n },
    { content: 0n }, { content: -1n }, { content: "1" }, { content: 1n << 64n },
  ].map(identity)];
  for (const packet of invalid) {
    const { status, writes } = fixture(); status.wait(identity(), "Valid wait");
    status.wait(packet, "Invalid wait"); assert.equal(status.drawn(packet), false);
    assert.equal(writes.length, 1); assert.equal(status.drawn(identity()), true);
  }
});
test("maximum u64 generation/content remains exact without numeric coercion", () => {
  const { status } = fixture(); const max = (1n << 64n) - 1n;
  status.wait(identity({ generation: max, content: max }), "Wait");
  assert.equal(status.drawn(identity({ generation: max - 1n, content: max })), false);
  assert.equal(status.drawn(identity({ generation: max, content: max })), true);
});

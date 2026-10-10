import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { runBounded, verifyDocument, verifyPresentation } from "./webtransport-play.browser.mjs";

test("deadline permanently rejects an operation that settles successfully later", async () => {
  let settle, token, stopped = false, success = false;
  const operation = runBounded(value => { token = value; return new Promise(done => { settle = done; }); }, 5, () => { stopped = true; });
  const result = operation.then(() => { success = true; }, error => error);
  const failure = await result;
  assert.match(failure.message, /deadline/);
  assert(stopped && token.stopped);
  settle("late success");
  await new Promise(done => setImmediate(done));
  assert.equal(success, false);
});

test("bounded successful operation returns its real result", async () => {
  let stops = 0;
  assert.equal(await runBounded(() => "complete", 100, () => { stops++; }), "complete");
  assert.equal(stops, 0);
});

test("actual navigation bytes must match even when a separately fetched index matches", () => {
  const index = Buffer.from("<html>original production index</html>");
  const digest = createHash("sha256").update(index).digest("hex");
  verifyDocument(index, digest);
  assert.throws(() => verifyDocument(Buffer.from("<html>alternate directory route</html>"), digest), /Actual main navigation/);
});

function presentation() {
  const identity = { generation: "7", content: "11" };
  return {
    probe: { messages: [
      { direction: "sent", kind: "play-results-present", playId: 1, rpcId: 99 },
      { direction: "received", kind: "play-reply", playId: 1, rpcId: 99, result: { kind: "completed-results", completedResults: { failed: false } } },
      { direction: "received", kind: "render-geometry", ...identity, mode: "results", width: 960, height: 720 },
    ] },
    renderer: { errors: [], overflow: false, packets: [
      { ...identity, kind: 5, sequence: "0" }, { ...identity, kind: 6, sequence: "1" },
    ], outgoing: [
      { ...identity, kind: "state-ack", packetKind: 5, sequence: "0" },
      { ...identity, kind: "state-ack", packetKind: 6, sequence: "1" },
      { ...identity, kind: "drawn", mode: "results", sequence: "1" },
      { ...identity, kind: "geometry-ack", width: 960, height: 720 },
    ] },
  };
}

test("correlated combined Results ACK, draw, geometry and successful RPC pass", () => {
  const { probe, renderer } = presentation();
  assert.deepEqual(verifyPresentation(probe, renderer), { generation: "7", content: "11" });
});

for (const [name, mutate] of [
  ["packet delivery alone", x => { x.renderer.outgoing = []; }],
  ["missing room ACK", x => { x.renderer.outgoing.splice(1, 1); }],
  ["stale Results ACK generation", x => { x.renderer.outgoing[0].generation = "6"; }],
  ["stale ACK sequence", x => { x.renderer.outgoing[1].sequence = "0"; }],
  ["stale draw content", x => { x.renderer.outgoing[2].content = "10"; }],
  ["draw before combined room", x => { x.renderer.outgoing[2].sequence = "0"; }],
  ["zero geometry extent", x => { x.renderer.outgoing[3].width = 0; }],
  ["stale Main geometry", x => { x.probe.messages[2].generation = "6"; }],
  ["renderer rejection after delivery", x => { x.renderer.outgoing.push({ kind: "render-error", message: "original GPU error" }); }],
  ["Results display error", x => { x.probe.messages.push({ direction: "received", kind: "play-completed-results", error: "original display failure" }); }],
  ["failed Results RPC", x => { x.probe.messages[1].error = "original RPC failure"; }],
  ["missing Results RPC reply", x => { x.probe.messages.splice(1, 1); }],
  ["wrong Results RPC identity", x => { x.probe.messages[1].playId = 2; }],
]) {
  test(`presentation rejects ${name}`, () => {
    const value = presentation(); mutate(value);
    assert.throws(() => verifyPresentation(value.probe, value.renderer));
  });
}

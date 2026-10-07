// Deferred pure production pager fixtures; no browser or Rust binding runs.
import assert from "node:assert/strict";
import test from "node:test";
import { HistoricalGradePager, validateHistoricalGradeSnapshot } from "./host_model.mjs";

test("grade metadata admits only exact bounded integers and rejects malformed envelopes", () => {
  for (const [page, pages] of [[0, 1], [3, 4], [1023, 1024]]) {
    assert.deepEqual(validateHistoricalGradeSnapshot({ page, pages }), { page, pages });
  }
  for (const value of [null, {}, { page: -1, pages: 1 }, { page: 1, pages: 1 }, { page: 0, pages: 0 },
    { page: 0, pages: 1034 }, { page: 0.5, pages: 2 }, { page: 0, pages: 2.5 }, { page: 0n, pages: 1 },
    { page: NaN, pages: 1 }, { page: 0, pages: Infinity }]) assert.throws(() => validateHistoricalGradeSnapshot(value));
});
test("unselected same-page and busy requests produce no command or renewed request frontier", () => {
  const pager = new HistoricalGradePager();
  assert.equal(pager.snapshot(), null); assert.equal(pager.request(0, 1), null);
  pager.bind(7, 0, 3); assert.deepEqual(pager.snapshot(), { id: 7, page: 0, pages: 3, pending: false });
  assert.equal(pager.request(0, 1), null);
  assert.deepEqual(pager.request(1, 1), { kind: "historical-record-page", id: 7, rpcId: 1, page: 1 });
  assert.equal(pager.request(2, 2), null);
  assert.equal(pager.accept({ kind: "historical-record-page-result", id: 7, rpcId: 1, gradePage: 1, gradePages: 3, error: null }), true);
  assert.deepEqual(pager.request(2, 2), { kind: "historical-record-page", id: 7, rpcId: 2, page: 2 });
});
test("bad selections pages and RPC ids refuse atomically including maximum safe identity", () => {
  const pager = new HistoricalGradePager(); pager.bind(Number.MAX_SAFE_INTEGER, 0, 1024);
  const before = pager.snapshot();
  for (const id of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, 1n]) {
    assert.throws(() => pager.bind(id, 0, 1)); assert.deepEqual(pager.snapshot(), before);
  }
  for (const page of [-1, 1024, Number.MAX_SAFE_INTEGER, 0.5, 0n]) {
    assert.throws(() => pager.request(page, 1)); assert.deepEqual(pager.snapshot(), before);
  }
  for (const rpc of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, 1n]) {
    assert.throws(() => pager.request(1, rpc)); assert.deepEqual(pager.snapshot(), before);
  }
  pager.request(1023, Number.MAX_SAFE_INTEGER); pager.cancel();
  assert.throws(() => pager.request(1, Number.MAX_SAFE_INTEGER));
  assert.deepEqual(pager.snapshot(), before);
});
test("only matching success commits requested page and malformed matched replies preserve pending", () => {
  const pager = new HistoricalGradePager(); pager.bind(7, 0, 3); pager.request(1, 9);
  const before = pager.snapshot();
  assert.equal(pager.accept({ id: 8, rpcId: 9, gradePage: 1, gradePages: 3, error: null }), false);
  assert.equal(pager.accept({ id: 7, rpcId: 8, gradePage: 1, gradePages: 3, error: null }), false);
  for (const change of [{ gradePage: 2 }, { gradePages: 4 }, { gradePage: 1.5 }, { error: undefined }, { error: "refused" }]) {
    assert.throws(() => pager.accept({ kind: "historical-record-page-result", id: 7, rpcId: 9, gradePage: 1, gradePages: 3, error: null, ...change }));
    assert.deepEqual(pager.snapshot(), before);
  }
  const reply = { kind: "historical-record-page-result", id: 7, rpcId: 9, gradePage: 1, gradePages: 3, error: null };
  assert.equal(pager.accept(reply), true); assert.equal(pager.accept(reply), false);
  assert.deepEqual(pager.snapshot(), { id: 7, page: 1, pages: 3, pending: false });
});
test("matched bounded refusal releases pending while retaining confirmed page and RPC frontier", () => {
  const pager = new HistoricalGradePager(); pager.bind(7, 1, 3); pager.request(2, 10);
  const before = pager.snapshot();
  for (const error of ["x".repeat(4097), 9, null]) {
    assert.throws(() => pager.accept({ kind: "historical-record-page-result", id: 7, rpcId: 10, gradePage: null, gradePages: 0, error }));
    assert.deepEqual(pager.snapshot(), before);
  }
  assert.equal(pager.accept({ kind: "historical-record-page-result", id: 7, rpcId: 10, gradePage: null, gradePages: 0, error: "page preparation refused" }), true);
  assert.deepEqual(pager.snapshot(), { id: 7, page: 1, pages: 3, pending: false });
  assert.throws(() => pager.request(2, 10));
  assert.equal(pager.request(2, 11).rpcId, 11);
});
test("cancel clear and replacement selections invalidate stale page receipts without resetting current page", () => {
  const pager = new HistoricalGradePager(); pager.bind(7, 0, 3); pager.request(1, 1); pager.cancel();
  assert.deepEqual(pager.snapshot(), { id: 7, page: 0, pages: 3, pending: false });
  assert.equal(pager.accept({ id: 7, rpcId: 1, gradePage: 1, gradePages: 3, error: null }), false);
  pager.bind(8, 2, 3); pager.request(1, 1);
  assert.equal(pager.accept({ id: 7, rpcId: 1, gradePage: 1, gradePages: 3, error: null }), false);
  assert.deepEqual(pager.snapshot(), { id: 8, page: 2, pages: 3, pending: true });
  pager.clear(); assert.equal(pager.snapshot(), null);
  assert.equal(pager.accept({ id: 8, rpcId: 1, gradePage: 1, gradePages: 3, error: null }), false);
});

test("total stored-detail bound includes 1024 grade pages plus nine comparisons with unchanged RPC atomicity", () => {
  assert.deepEqual(validateHistoricalGradeSnapshot({ page: 1032, pages: 1033 }), { page: 1032, pages: 1033 });
  assert.throws(() => validateHistoricalGradeSnapshot({ page: 0, pages: 1034 }));
  const pager = new HistoricalGradePager(); pager.bind(7, 1023, 1033);
  assert.deepEqual(pager.request(1024, 1), { kind: "historical-record-page", id: 7, rpcId: 1, page: 1024 });
  assert.equal(pager.accept({ kind: "historical-record-page-result", id: 7, rpcId: 1, gradePage: 1024, gradePages: 1033, error: null }), true);
  pager.request(1032, 2); pager.accept({ kind: "historical-record-page-result", id: 7, rpcId: 2, gradePage: 1032, gradePages: 1033, error: null });
  const before = pager.snapshot(); assert.throws(() => pager.request(1033, 3)); assert.deepEqual(pager.snapshot(), before);
  pager.request(0, 3); const pending = pager.snapshot();
  assert.throws(() => pager.accept({ kind: "historical-record-page-result", id: 7, rpcId: 3, gradePage: 0, gradePages: 1034, error: null }));
  assert.deepEqual(pager.snapshot(), pending);
});

// Pure timestamp checks and serialized ISO BMFF evidence. Synthetic NAL payloads
// exercise demuxing only; real WebCodecs/Scene evidence belongs to browser QA.
import assert from "node:assert/strict";
import test from "node:test";
import { demuxMp4, normalizeTrack, nsToUs } from "./video-demux.mjs";
import { createFile } from "./vendor/mp4box.all.mjs";

const description = Uint8Array.of(1, 66, 0, 30, 255, 225, 0, 4, 103, 66, 0, 30, 1, 0, 2, 104, 0);
const track = (fields = {}) => ({ timescale: 90000, codec: "avc1.42001e", video: { width: 2, height: 2 }, ...fields });
const sample = (dts, cts, duration = 3000, key = false) => ({ dts, cts, duration, is_sync: key, data: Uint8Array.of(0, 0, 0, 1, key ? 101 : 65) });
const options = (fields = {}) => ({ description, ...fields });

test("DTS order is independent of CTS order and the first display origin is stable", () => {
  const result = normalizeTrack(track(), [sample(96000, 96000), sample(90000, 93000, 3000, true), sample(93000, 99000)], options());
  assert.equal(result.originNs, 1033333333n);
  assert.deepEqual(result.samples.map(s => s.ptsNs), [0n, 66666666n, 33333333n]);
  assert.deepEqual(result.samples.map(s => s.timestampUs), [0, 66666, 33333]);
  assert.deepEqual(result.samples.map(s => s.key), [true, false, false]);
  assert.ok(result.samples[0].dtsNs < result.samples[1].dtsNs);
  assert.ok(result.samples[1].dtsNs < result.samples[2].dtsNs);
  assert.equal(result.config.codec, "avc1.42001e");
  assert.equal(result.config.codedWidth, 2);
  assert.equal(result.config.codedHeight, 2);
  assert.deepEqual(new Uint8Array(result.config.description), description);
});

test("rational presentation gaps use the original PTS rather than accumulated microseconds", () => {
  const result = normalizeTrack(track({ timescale: 3 }), [sample(0, 1, 1, true), sample(1, 2, 2), sample(3, 4, 1)], options());
  assert.equal(result.originNs, 333333333n);
  assert.deepEqual(result.samples.map(s => s.ptsNs), [0n, 333333333n, 1000000000n]);
  assert.deepEqual(result.samples.map(s => s.timestampUs), [0, 333333, 1000000]);
  assert.equal(result.durationNs, 1333333333n);
});

test("normal-rate edit retains decode preroll while choosing the first valid display origin", () => {
  const edits = [
    { segment_duration: 500, media_time: -1, media_rate_integer: 1, media_rate_fraction: 0 },
    { segment_duration: 3000, media_time: 90000, media_rate_integer: 1, media_rate_fraction: 0 },
  ];
  const result = normalizeTrack(track(), [sample(87000, 87000, 3000, true), sample(90000, 90000), sample(93000, 96000)], options({ movieTimescale: 1000, edits }));
  assert.equal(result.originNs, 500000000n);
  assert.equal(result.samples[0].display, false);
  assert.equal(result.samples[1].ptsNs, 0n);
  assert.equal(result.samples[2].ptsNs, 66666666n);
  assert.equal(result.samples[0].key, true, "the keyframe before the edit remains available to decode");
});

test("unsupported edit forms report an error instead of inventing a timeline", () => {
  for (const edits of [
    [{ segment_duration: 1000, media_time: 0, media_rate_integer: 2, media_rate_fraction: 0 }],
    [{ segment_duration: 1000, media_time: 0, media_rate_integer: 1, media_rate_fraction: 1 }],
    [{ segment_duration: 1000, media_time: 0 }, { segment_duration: 1000, media_time: 90000 }],
    [{ segment_duration: 1000, media_time: -2, media_rate_integer: 1, media_rate_fraction: 0 }],
  ]) assert.throws(() => normalizeTrack(track(), [sample(0, 0, 3000, true)], options({ movieTimescale: 1000, edits })), /edit|rate|media/i);
});

test("integer nanoseconds floor once and reject unsafe WebCodecs timestamps", () => {
  assert.equal(nsToUs(1001n), 1);
  assert.equal(nsToUs(-1n), -1);
  assert.equal(nsToUs(-1001n), -2);
  assert.equal(nsToUs(604800000000000n), 604800000000);
  assert.equal(nsToUs(9007199254740991000n), Number.MAX_SAFE_INTEGER);
  assert.throws(() => nsToUs(9007199254740992000n), /safe|precision|timestamp|range/i);
});

test("twenty-hour and week-long source coordinates retain one-tick rational offsets", () => {
  const result = normalizeTrack(track(), [sample(90000, 90000, 1, true),
    sample(90000 + 90000 * 72000 + 1, 90000 + 90000 * 72000 + 1, 1),
    sample(90000 + 90000 * 604800 + 1, 90000 + 90000 * 604800 + 1, 1)], options());
  assert.deepEqual(result.samples.map(s => s.ptsNs), [0n, 72000000011111n, 604800000011111n]);
  assert.deepEqual(result.samples.map(s => s.timestampUs), [0, 72000000011, 604800000011]);
});

test("invalid times, dimensions, descriptions and ambiguous microsecond collisions fail explicitly", () => {
  for (const timescale of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1])
    assert.throws(() => normalizeTrack(track({ timescale }), [sample(0, 0, 1, true)], options()));
  for (const bad of [sample(0, NaN), sample(0, 0.5), sample(Number.MAX_SAFE_INTEGER + 1, 0), sample(0, 0, -1)])
    assert.throws(() => normalizeTrack(track(), [bad], options()));
  assert.throws(() => normalizeTrack(track({ video: { width: 16385, height: 1 } }), [sample(0, 0, 1, true)], options()), /dimension|size|width|limit/i);
  assert.throws(() => normalizeTrack(track(), [sample(0, 0, 1, true)], options({ maxFrameBytes: 15 })), /byte|frame|limit|size/i);
  assert.throws(() => normalizeTrack(track(), [sample(0, 0, 1, true)], { description: null }), /description|codec/i);
  assert.throws(() => normalizeTrack(track({ timescale: 2000000 }), [sample(0, 0, 1, true), sample(1, 1, 1)], options()), /precision|collision|timestamp/i);
});

test("serialized MP4 bytes supply avcC metadata and distinct DTS/CTS through the shipped demuxer", async () => {
  const bytes = mp4Fixture();
  const result = await demuxMp4(bytes);
  assert.match(result.config.codec, /^avc1\./);
  assert.equal(result.config.codedWidth, 2);
  assert.equal(result.config.codedHeight, 2);
  assert.deepEqual(new Uint8Array(result.config.description), description);
  assert.deepEqual(result.samples.map(s => s.ptsNs), [0n, 66666666n, 33333333n]);
  assert.deepEqual(result.samples.map(s => s.key), [true, false, false]);
  assert.deepEqual(result.samples.map(s => [...s.data]), [
    [0, 0, 0, 1, 101], [0, 0, 0, 1, 65], [0, 0, 0, 1, 65],
  ]);
});

test("malformed and over-budget encoded containers cannot reach decoder setup", async () => {
  await assert.rejects(() => demuxMp4(Uint8Array.of(1, 2, 3, 4)));
  const bytes = mp4Fixture();
  await assert.rejects(() => demuxMp4(bytes, { maxEncodedBytes: bytes.byteLength - 1 }), /byte|encoded|limit|size/i);
  await assert.rejects(() => demuxMp4(bytes, { maxSamples: 2 }), /sample|limit/i);
});

function mp4Fixture() {
  const file = createFile();
  const id = file.addTrack({ type: "avc1", hdlr: "vide", timescale: 90000,
    width: 2, height: 2, duration: 102000, media_duration: 102000,
    avcDecoderConfigRecord: description.slice().buffer });
  for (const s of [sample(90000, 93000, 3000, true), sample(93000, 99000), sample(96000, 96000)])
    file.addSample(id, s.data, { dts: s.dts, cts: s.cts, duration: s.duration, is_sync: s.is_sync });
  return new Uint8Array(file.getBuffer().buffer);
}

// Injected WebCodecs verifies observable ordering, ownership and credits.
// These tests do not establish that a browser supports or decodes H.264.
import assert from "node:assert/strict";
import test from "node:test";
import { BrowserVideoDecoder } from "./video-decoder.mjs";

const tick = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };
const limits = { maxFrames: 3, maxBytes: 48, maxFrameBytes: 16, maxDecodeQueue: 8,
  maxEncodedBytes: 1024, maxEncodedTotalBytes: 2048, maxResources: 2, maxDimension: 16 };
const config = { codec: "avc1.42001e", codedWidth: 2, codedHeight: 2, description: Uint8Array.of(1, 66, 0, 30).buffer };
const clip = () => ({ config, originNs: 9000000000n, durationNs: 250000000n,
  samples: [0, 40000000, 10000000, 100000000, 200000000].map((pts, i) => ({
    dtsNs: BigInt(i) * 10000000n, ptsNs: BigInt(pts), timestampUs: pts / 1000,
    durationUs: 10000, key: i === 0 || i === 3, display: true, data: Uint8Array.of(i + 1),
  })) });
const request = (targetNs, generation = 7) => ({ content: 11, resource: 12, generation, targetNs });
const frames = h => h.messages.filter(m => m.type === "frame");

test("support is checked before configure and chunks follow DTS while frames preserve CTS", async () => {
  const h = harness();
  await h.register();
  await h.owner.demand(request(65000000n));
  assert.equal(h.events[0][0], "support");
  assert.equal(h.events[1][0], "configure");
  const chunks = h.events.filter(e => e[0] === "decode").map(e => e[1]);
  assert.deepEqual(chunks.map(c => c.data[0]), [...chunks].map(c => c.data[0]).sort((a, b) => a - b));
  assert.equal(chunks[0].type, "key");
  assert.equal(chunks[0].timestamp, 0);
  assert.ok(chunks.some(c => c.timestamp === 40000));
  assert.ok(chunks.some(c => c.timestamp === 10000));
  assert.ok(h.created.every(f => f.closes === 1));
  for (const frame of frames(h)) {
    assert.equal(frame.content, 11); assert.equal(frame.resource, 12); assert.equal(frame.generation, 7);
    assert.equal(frame.rgba.byteLength, 16);
    assert.equal(frame.width, 2); assert.equal(frame.height, 2);
    assert.ok(h.transfers.some(t => t.includes(frame.rgba)), "RGBA ownership is transferred");
  }
  h.owner.close();
});

test("reordered output retains the greatest pretarget frame through a VFR gap", async () => {
  const h = harness();
  await h.register();
  await h.owner.demand(request(65000000n));
  const eligible = frames(h).filter(f => f.ptsNs <= 65000000n);
  assert.deepEqual(eligible.map(f => f.ptsNs), [40000000n]);
  assert.equal(new Uint8Array(eligible[0].rgba)[0], 40);
  const marks = h.messages.filter(m => m.type === "watermark" || m.type === "eof");
  assert.ok(marks.length > 0, "completion is explicit after reordered output");
  const completed = marks.reduce((max, m) => {
    const value = m.completedThroughNs ?? m.endNs;
    return value > max ? value : max;
  }, -1n);
  const selected = frames(h).filter(f => f.ptsNs <= 65000000n && f.ptsNs <= completed)
    .sort((a, b) => a.ptsNs < b.ptsNs ? -1 : a.ptsNs > b.ptsNs ? 1 : 0).at(-1);
  assert.equal(selected.ptsNs, 40000000n, "a future frame cannot replace the gap's retained frame");
  h.owner.close();
});

test("pause makes no new decode work and EOF holds the last eligible frame", async () => {
  const h = harness();
  await h.register();
  await h.owner.demand(request(300000000n));
  assert.equal(frames(h).filter(f => f.ptsNs <= 300000000n).at(-1).ptsNs, 200000000n);
  assert.ok(h.messages.some(m => m.type === "eof" && m.generation === 7));
  const decodes = h.events.filter(e => e[0] === "decode").length;
  const count = frames(h).length;
  await h.owner.demand(request(300000000n));
  assert.equal(h.events.filter(e => e[0] === "decode").length, decodes);
  assert.equal(frames(h).length, count);
  h.owner.close();
});

test("backward seek fences the previous generation and keeps the original PTS origin", async () => {
  const h = harness();
  await h.register();
  await h.owner.demand(request(150000000n));
  const oldDecoder = h.decoders.at(-1);
  await h.owner.demand(request(25000000n, 8));
  const fresh = frames(h).filter(f => f.generation === 8 && f.ptsNs <= 25000000n);
  assert.deepEqual(fresh.map(f => f.ptsNs), [10000000n]);
  const count = frames(h).length;
  const late = h.frame(40000);
  oldDecoder.callbacks.output(late);
  await tick();
  assert.equal(late.closes, 1);
  assert.equal(frames(h).length, count);
  assert.ok(oldDecoder.closes > 0 || oldDecoder.resets > 0);
  h.owner.close();
});

test("retirement during asynchronous RGBA copy prevents publication and closes the frame", async () => {
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const h = harness({ copyGate: gate });
  await h.register();
  const pending = h.owner.demand(request(65000000n));
  await tick();
  h.owner.retire({ content: 11, generation: 7 });
  release();
  await pending;
  assert.equal(frames(h).length, 0);
  assert.ok(h.created.length > 0);
  assert.ok(h.created.every(f => f.closes === 1));
  h.owner.close();
});

test("close while decode queue is stalled joins already-output asynchronous frames", async () => {
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const h = harness({ copyGate: gate, immediateOutput: true, stalledQueue: true, limits: { maxDecodeQueue: 1 } });
  await h.register();
  let settled = false;
  const pending = h.owner.demand(request(0n)).then(() => { settled = true; });
  await tick();
  assert.equal(h.created.length, 1);
  h.owner.close();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(settled, false, "demand owns the frame until its copy finishes");
  assert.equal(h.created[0].closes, 0);
  release();
  await pending;
  assert.equal(h.created[0].closes, 1);
  assert.equal(frames(h).length, 0);
});

test("copy errors close every frame and produce an inspectable unavailable result", async () => {
  const h = harness({ copyError: new Error("RGBA copy failed") });
  await h.register();
  await h.owner.demand(request(65000000n));
  assert.equal(frames(h).length, 0);
  assert.ok(h.messages.some(m => m.type === "unavailable"));
  assert.ok(h.created.length > 0);
  assert.ok(h.created.every(f => f.closes === 1));
  h.owner.close();
});

test("decoder faults are observable and retirement rejects later output ownership", async () => {
  const h = harness({ decodeError: new Error("codec failed") });
  await h.register();
  await h.owner.demand(request(65000000n));
  assert.ok(h.messages.some(m => m.type === "unavailable"));
  const late = h.frame(10000);
  h.decoders.at(-1).callbacks.output(late);
  await tick();
  assert.equal(late.closes, 1);
  assert.equal(frames(h).length, 0);
  h.owner.close();
});

test("unsupported codec configurations never configure or decode", async () => {
  const h = harness({ supported: false });
  await h.register();
  await h.owner.demand(request(65000000n));
  assert.ok(h.messages.some(m => m.type === "unavailable"));
  assert.equal(h.events.filter(e => e[0] === "configure" || e[0] === "decode").length, 0);
  h.owner.close();
});

test("decodeQueueSize is bounded while dequeue permits continued DTS submission", async () => {
  const h = harness({ limits: { maxDecodeQueue: 2 } });
  await h.register();
  await h.owner.demand(request(65000000n));
  assert.ok(h.peakDecodeQueue <= 2);
  assert.ok(h.events.filter(e => e[0] === "decode").length >= 3);
  h.owner.close();
});

test("frame bytes stay charged until matching ACK and stale or duplicate ACK cannot mint credits", async () => {
  const h = harness({ limits: { maxFrames: 2, maxBytes: 16 } });
  await h.register();
  await h.owner.demand(request(65000000n));
  assert.equal(frames(h).length, 1, "byte credits bound publication even with two count credits");
  const first = frames(h)[0];
  h.owner.ack({ content: 11, generation: 6, revision: first.revision });
  h.owner.ack({ content: 99, generation: 7, revision: first.revision });
  await h.owner.demand(request(150000000n));
  assert.equal(frames(h).length, 1);
  assert.ok(h.messages.some(m => m.type === "backpressure"));
  h.owner.ack({ content: 11, generation: 7, revision: first.revision });
  await h.owner.demand(request(150000000n));
  assert.equal(frames(h).length, 2);
  h.owner.ack({ content: 11, generation: 7, revision: first.revision });
  await h.owner.demand(request(300000000n));
  assert.equal(frames(h).length, 2, "duplicate ACK cannot release a different frame's bytes");
  h.owner.close();
});

test("retirement releases decoder resources and closed owners cannot publish new frames", async () => {
  const h = harness();
  await h.register();
  await h.owner.demand(request(65000000n));
  h.owner.close(); h.owner.close();
  assert.ok(h.decoders.every(d => d.closes === 1));
  const count = frames(h).length;
  for (const decoder of h.decoders) {
    const late = h.frame(10000); decoder.callbacks.output(late);
    await tick(); assert.equal(late.closes, 1);
  }
  assert.equal(frames(h).length, count);
});

function harness(settings = {}) {
  const h = { messages: [], transfers: [], events: [], created: [], decoders: [], peakDecodeQueue: 0 };
  h.frame = timestamp => {
    const frame = { timestamp, codedWidth: 2, codedHeight: 2, displayWidth: 2, displayHeight: 2, closes: 0,
      allocationSize: () => 16,
      async copyTo(buffer) {
        if (settings.copyGate) await settings.copyGate;
        if (settings.copyError) throw settings.copyError;
        const bytes = buffer instanceof ArrayBuffer ? new Uint8Array(buffer) : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
        bytes.fill(timestamp / 1000);
        return [{ offset: 0, stride: 8 }];
      },
      close() { this.closes++; },
    };
    h.created.push(frame); return frame;
  };
  class Chunk { constructor(fields) { Object.assign(this, fields); } }
  class Decoder {
    static async isConfigSupported(value) { h.events.push(["support", value]); return { supported: settings.supported !== false, config: value }; }
    constructor(callbacks) { this.callbacks = callbacks; this.pending = []; this.decodeQueueSize = 0; this.state = "unconfigured"; this.closes = 0; this.resets = 0; this.listeners = new Map(); h.decoders.push(this); }
    configure(value) { h.events.push(["configure", value]); this.state = "configured"; }
    addEventListener(name, fn) { const set = this.listeners.get(name) ?? new Set(); set.add(fn); this.listeners.set(name, set); }
    removeEventListener(name, fn) { this.listeners.get(name)?.delete(fn); }
    decode(chunk) {
      h.events.push(["decode", chunk]);
      if (settings.decodeError) { this.callbacks.error(settings.decodeError); return; }
      this.pending.push(chunk); this.decodeQueueSize++;
      h.peakDecodeQueue = Math.max(h.peakDecodeQueue, this.decodeQueueSize);
      if (settings.immediateOutput) this.callbacks.output(h.frame(chunk.timestamp));
      if (settings.stalledQueue) return;
      queueMicrotask(() => { this.decodeQueueSize--; this.ondequeue?.(); for (const fn of this.listeners.get("dequeue") ?? []) fn(); });
    }
    async flush() {
      h.events.push(["flush"]);
      const pending = this.pending.splice(0);
      // Deliberately violate presentation arrival order. Flush proves completion.
      for (const chunk of pending.reverse()) this.callbacks.output(h.frame(chunk.timestamp));
      await tick();
    }
    reset() { this.resets++; this.pending.length = 0; this.state = "unconfigured"; }
    close() { this.closes++; this.pending.length = 0; this.state = "closed"; }
  }
  h.owner = new BrowserVideoDecoder({ VideoDecoder: Decoder, EncodedVideoChunk: Chunk,
    demux: async () => settings.movie ?? clip(), limits: { ...limits, ...settings.limits },
    postMessage(message, transfers = []) { h.messages.push(message); h.transfers.push(transfers); } });
  h.register = () => h.owner.register({ content: 11, resource: 12, bytes: Uint8Array.of(1, 2, 3) });
  return h;
}

test("last decode ordinal does not end a stream with unpublished later CTS", async () => {
  const movie = { config, originNs: 0n, durationNs: 300000n,
    samples: [0n, 200000n, 100000n].map((ptsNs, ordinal) => ({ dtsNs: BigInt(ordinal) * 100000n,
      ptsNs, timestampUs: Number(ptsNs / 1000n), durationUs: 100, key: ordinal === 0,
      display: true, data: Uint8Array.of(ordinal) })) };
  const h = harness({ movie, limits: { maxFrames: 2 } }); await h.register();
  await h.owner.demand(request(0n));
  assert.deepEqual(frames(h).map(f => f.ptsNs).sort((a, b) => a < b ? -1 : 1), [0n, 100000n]);
  assert.equal(h.messages.filter(m => m.type === 'eof').length, 0);
  for (const frame of frames(h)) h.owner.ack(frame);
  await h.owner.demand(request(200000n));
  assert.ok(frames(h).some(f => f.ptsNs === 200000n));
  assert.equal(h.messages.filter(m => m.type === 'eof').length, 1);
  for (const frame of frames(h)) h.owner.ack(frame);
  const count = frames(h).length, codecs = h.decoders.length;
  await h.owner.demand(request(1000000n));
  assert.equal(frames(h).length, count); assert.equal(h.decoders.length, codecs);
  assert.equal(h.messages.filter(m => m.type === 'eof').length, 2);
  assert.equal(h.messages.filter(m => m.type === 'watermark').at(-1).completedThroughNs, 200000n);
  h.owner.close();
});
test("long sample tables are indexed once and demand reads only random access span", async () => {
  let reads = 0;
  const samples = Array.from({ length: 4096 }, (_, ordinal) => ({
    dtsNs: BigInt(ordinal) * 10000000n, ptsNs: BigInt(ordinal) * 10000000n,
    timestampUs: ordinal * 10000, durationUs: 10000, key: ordinal % 4 === 0,
    display: true, data: Uint8Array.of(ordinal % 255),
  }));
  const table = new Proxy(samples, { get(target, property, receiver) {
    if (typeof property === 'string' && /^\d+$/.test(property)) reads++;
    return Reflect.get(target, property, receiver);
  } });
  const h = harness({ movie: { config, originNs: 0n, durationNs: 40960000000n, samples: table } });
  await h.register(); reads = 0;
  samples.filter = samples.sort = samples.indexOf = () => { throw new Error('whole sample table hot scan'); };
  await h.owner.demand(request(40920000000n));
  assert.ok(reads <= 8, `demand visited ${reads} decode entries`);
  assert.equal(frames(h).length, 3);
  h.owner.close();
});

test("one-frame demand supplies only predecessor and forward interval reuses completed pixels", async () => {
  const h = harness(); await h.register();
  await h.owner.demand({ ...request(65000000n), maxFrames: 1 });
  assert.deepEqual(frames(h).map(f => f.ptsNs), [40000000n]);
  assert.equal(h.messages.filter(m => m.type === 'eof').length, 0);
  const codecs = h.decoders.length, owned = h.created.length, published = frames(h).length;
  for (const targetNs of [66000000n, 70000000n, 99999999n]) {
    await h.owner.demand({ ...request(targetNs), maxFrames: 1 });
  }
  assert.equal(h.decoders.length, codecs);
  assert.equal(h.created.length, owned);
  assert.equal(frames(h).length, published);
  assert.equal(h.messages.filter(m => m.type === 'watermark').length, 4);
  assert.equal(h.messages.filter(m => m.type === 'watermark').at(-1).completedThroughNs, 40000000n);
  for (const frame of frames(h)) h.owner.ack(frame);
  await h.owner.demand({ ...request(100000000n), maxFrames: 1 });
  assert.equal(h.decoders.length, codecs + 1);
  assert.equal(frames(h).at(-1).ptsNs, 100000000n);
  h.owner.close();
});
test("per-demand frame count is constrained by configured codec credits", async () => {
  const h = harness(); await h.register();
  for (const maxFrames of [0, -1, 4, 1.5, NaN]) await h.owner.demand({ ...request(0n), maxFrames });
  assert.equal(h.decoders.length, 0);
  assert.equal(h.messages.filter(m => m.type === 'unavailable').length, 5);
  h.owner.close();
});

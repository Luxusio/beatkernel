import test from 'node:test';
import assert from 'node:assert/strict';
import { VideoClient } from './video-client.mjs';
function fixture() {
  const sent = [], calls = [], demands = [];
  const worker = { postMessage: (message, transfer) => sent.push({ message, transfer }) };
  const view = {
    register_video: (...args) => calls.push(['register', ...args]), retire_video: () => calls.push(['retire']),
    video_demands: () => demands.map(d => ({ ...d })),
    admit_video_frame: (...args) => { calls.push(['frame', ...args]); return true; },
    video_watermark: (...args) => { calls.push(['watermark', ...args]); return true; },
    video_end: (...args) => { calls.push(['end', ...args]); return true; },
  };
  view.admit_completed_video_frame = (...args) => {
    calls.push(['completed-frame', ...args]);
    return view.admit_video_frame(...args.slice(0, -1));
  };
  let drawn = 0;
  const errors = [];
  const client = new VideoClient({ worker, view, onFrame: () => drawn++, onUnavailable: reason => errors.push(reason) });
  const registration = { resources: [new Uint8Array([1, 2, 3])], images: [{ image: 1, resource: 0, transform: null }] };
  const demand = { slot: 0, content: 1, generation: 10, resource: 0, targetNs: 100n, transform: null };
  const receive = message => worker.onmessage({ data: message });
  return { client, sent, calls, demands, registration, demand, receive, errors, drawn: () => drawn };
}
test('registration transfers a copy, waits for transform and codec readiness, uses committed target', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  const registration = f.sent.find(x => x.message.type === 'register');
  assert.notEqual(registration.message.bytes.buffer, f.registration.resources[0].buffer);
  assert.equal(registration.transfer[0], registration.message.bytes.buffer);
  f.receive({ type: 'registered', content: 1, resource: 0 });
  assert.equal(f.sent.filter(x => x.message.type === 'demand').length, 0);
  f.receive({ type: 'transform-ready' });
  assert.equal(f.sent.at(-1).message.targetNs, 100n);
  assert.equal(f.sent.at(-1).message.maxFrames, 1);
});
test('coalesces committed targets while codec request is pending', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  f.receive({ type: 'transform-ready' }); f.receive({ type: 'registered', content: 1, resource: 0 });
  f.demands[0] = { ...f.demand, targetNs: 200n }; f.client.refresh();
  f.demands[0] = { ...f.demand, targetNs: 300n }; f.client.refresh();
  assert.equal(f.sent.filter(x => x.message.type === 'demand').length, 1);
  f.receive({ type: 'watermark', content: 1, resource: 0, generation: 10, completedThroughNs: 150n });
  assert.equal(f.sent.at(-1).message.targetNs, 300n);
  assert.equal(f.calls.find(x => x[0] === 'watermark')[3], 150n);
});
test('admits timestamped frame and ACKs rejected old generation too', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  const frame = { type: 'frame', content: 1, resource: 0, generation: 10, revision: 1, ptsNs: 90n,
    width: 1, height: 1, rgba: new Uint8Array([255, 0, 0, 255]).buffer };
  f.receive(frame); assert.equal(f.drawn(), 1); assert.equal(f.calls.find(x => x[0] === 'frame')[4], 90n);
  f.receive({ ...frame, generation: 9, revision: 2 });
  assert.equal(f.calls.filter(x => x[0] === 'frame').length, 1);
  assert.equal(f.sent.at(-1).message.type, 'ack'); assert.equal(f.sent.at(-1).message.generation, 9);
});
test('omitted sessions retire before replacement demand', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  f.receive({ type: 'transform-ready' }); f.receive({ type: 'registered', content: 1, resource: 0 });
  f.demands[0] = { ...f.demand, generation: 11, targetNs: 20n }; f.client.refresh();
  assert.deepEqual(f.sent.at(-2).message, { type: 'retire', content: 1, generation: 10 });
  assert.equal(f.sent.at(-1).message.generation, 11);
});
test('pause repeats target without decode or wall clock progress', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  f.receive({ type: 'transform-ready' }); f.receive({ type: 'registered', content: 1, resource: 0 });
  f.receive({ type: 'watermark', content: 1, resource: 0, generation: 10, completedThroughNs: 120n });
  for (let i = 0; i < 30; i++) f.client.refresh();
  assert.equal(f.sent.filter(x => x.message.type === 'demand').length, 1);
});
test('suspension retires generations but preserves encoded resource registration', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  f.receive({ type: 'transform-ready' }); f.receive({ type: 'registered', content: 1, resource: 0 });
  f.client.suspend(); f.client.refresh();
  assert.equal(f.sent.at(-1).message.generation, 10);
  f.demands[0] = { ...f.demand, generation: 11 }; f.client.resume();
  assert.equal(f.sent.at(-1).message.generation, 11);
  assert.equal(f.sent.filter(x => x.message.type === 'register').length, 1);
});
test('frame admission failure still returns decoder ownership credit', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  f.client.view.admit_video_frame = () => { throw new Error('bad extent'); };
  f.receive({ type: 'frame', content: 1, resource: 0, generation: 10, revision: 1, ptsNs: 0n, width: 1, height: 1, rgba: new ArrayBuffer(4) });
  assert.deepEqual(f.errors, ['bad extent']); assert.ok(f.sent.some(x => x.message.type === 'ack'));
  assert.equal(f.sent.at(-1).message.type, 'retire');
});
test('four member channel slots stay independent and close ACKs stale ownership', () => {
  const f = fixture(); f.demands.push(...Array.from({ length: 16 }, (_, slot) => ({ ...f.demand, slot, generation: slot + 10 })));
  f.client.register(1n, 1n, f.registration); f.receive({ type: 'transform-ready' }); f.receive({ type: 'registered', content: 1, resource: 0 });
  assert.equal(f.sent.filter(x => x.message.type === 'demand').length, 16);
  f.client.close();
  f.receive({ type: 'frame', content: 1, resource: 0, generation: 10, revision: 1, ptsNs: 0n, width: 1, height: 1, rgba: new ArrayBuffer(4) });
  assert.equal(f.sent.at(-1).message.type, 'ack'); assert.equal(f.drawn(), 0);
});

test('closed client ignores late transform readiness', () => {
  const f = fixture(); f.client.close();
  const count = f.sent.length;
  f.receive({ type: 'transform-ready' });
  assert.equal(f.client.transformReady, false); assert.equal(f.sent.length, count);
});

test('pressure retains ownership and publishes only the completed prefix before a hole', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  f.receive({ type: 'transform-ready' }); f.receive({ type: 'registered', content: 1, resource: 0 });
  let available = false;
  f.client.view.admit_video_frame = () => available;
  const frame = { type: 'frame', content: 1, resource: 0, generation: 10, revision: 1,
    ptsNs: 150n, width: 1, height: 1, rgba: new ArrayBuffer(4) };
  f.receive(frame);
  assert.equal(f.sent.filter(x => x.message.type === 'ack').length, 0);
  f.receive({ type: 'watermark', content: 1, resource: 0, generation: 10, completedThroughNs: 200n });
  f.receive({ type: 'eof', content: 1, resource: 0, generation: 10, endNs: 300n });
  assert.equal(f.calls.filter(x => x[0] === 'watermark').at(-1)[3], 149n);
  assert.equal(f.calls.filter(x => x[0] === 'end').length, 0);
  const drawn = f.drawn(); f.client.drain(); f.client.drain();
  assert.equal(f.drawn(), drawn, 'unchanged pressure must not cause a redraw loop');
  available = true; f.client.drain();
  assert.equal(f.sent.filter(x => x.message.type === 'ack').length, 1);
  assert.equal(f.calls.filter(x => x[0] === 'watermark').at(-1)[3], 200n);
  assert.equal(f.calls.filter(x => x[0] === 'end').length, 1);
  assert.equal(f.client.blocked.size, 0);
});
test('retiring a pressured generation ACKs the held transferred frame and drops completion', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  f.client.view.admit_video_frame = () => false;
  f.receive({ type: 'frame', content: 1, resource: 0, generation: 10, revision: 1,
    ptsNs: 0n, width: 1, height: 1, rgba: new ArrayBuffer(4) });
  f.receive({ type: 'watermark', content: 1, resource: 0, generation: 10, completedThroughNs: 100n });
  f.demands.length = 0; f.client.refresh();
  assert.equal(f.sent.filter(x => x.message.type === 'ack').length, 1);
  assert.equal(f.client.blocked.size, 0); assert.equal(f.client.completions.size, 0);
  assert.equal(f.calls.filter(x => x[0] === 'end').length, 0);
});

test('retry uses full decoder completion proof only after the worker watermark', () => {
  const f = fixture(); f.demands.push(f.demand); f.client.register(1n, 1n, f.registration);
  let complete = false;
  f.client.view.admit_video_frame = () => false;
  f.client.view.admit_completed_video_frame = (...args) => {
    f.calls.push(['completed-frame', ...args]); complete = true; return true;
  };
  f.receive({ type: 'frame', content: 1, resource: 0, generation: 10, revision: 1,
    ptsNs: 100n, width: 1, height: 1, rgba: new ArrayBuffer(4) });
  f.client.drain(); assert.equal(complete, false);
  assert.equal(f.sent.filter(x => x.message.type === 'ack').length, 0);
  f.receive({ type: 'watermark', content: 1, resource: 0, generation: 10, completedThroughNs: 200n });
  assert.equal(complete, true);
  assert.equal(f.calls.find(x => x[0] === 'completed-frame').at(-1), 200n);
  assert.equal(f.sent.filter(x => x.message.type === 'ack').length, 1);
  assert.equal(f.calls.filter(x => x[0] === 'watermark').at(-1)[3], 200n);
});

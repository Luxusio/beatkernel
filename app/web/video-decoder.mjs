import { demuxMp4 } from './video-demux.mjs';

export const DEFAULT_VIDEO_LIMITS = Object.freeze({
  maxFrames: 3, maxBytes: 192 * 1024 * 1024, maxFrameBytes: 64 * 1024 * 1024,
  maxDecodeQueue: 8, maxEncodedBytes: 64 * 1024 * 1024,
  maxEncodedTotalBytes: 256 * 1024 * 1024, maxResources: 3844,
  maxDimension: 16384, maxSessions: 4,
});
const identity = frame => frame;
function presentationIndex(movie) {
  let rap = -1;
  const presentation = [];
  for (let ordinal = 0; ordinal < movie.samples.length; ordinal++) {
    const sample = movie.samples[ordinal];
    if (sample.key) rap = ordinal;
    if (sample.display !== false) presentation.push({ sample, ordinal, rap });
  }
  presentation.sort((a, b) => a.sample.ptsNs < b.sample.ptsNs ? -1 : a.sample.ptsNs > b.sample.ptsNs ? 1 : a.ordinal - b.ordinal);
  let decodeEnd = 0;
  for (const entry of presentation) {
    decodeEnd = Math.max(decodeEnd, entry.ordinal + 1);
    entry.decodeEnd = decodeEnd;
  }
  return { ...movie, presentation };
}
function upperPresentationBound(entries, target) {
  let lo = 0, hi = entries.length;
  while (lo < hi) {
    const middle = lo + Math.floor((hi - lo) / 2);
    if (entries[middle].sample.ptsNs <= target) lo = middle + 1;
    else hi = middle;
  }
  return lo;
}
function key(content, value) { return `${typeof content}:${content}:${typeof value}:${value}`; }
function id(value) { return Number.isSafeInteger(value) && value >= 0; }

// All codec work belongs to the dedicated Worker. Injected platform constructors
// make the ownership protocol testable without representing mocks as codec QA.
export class BrowserVideoDecoder {
  constructor({ VideoDecoder = globalThis.VideoDecoder, EncodedVideoChunk = globalThis.EncodedVideoChunk,
    postMessage, transformFrame = identity, demux = demuxMp4, limits = {} } = {}) {
    this.Decoder = VideoDecoder;
    this.Chunk = EncodedVideoChunk;
    this.post = postMessage;
    this.transformFrame = transformFrame;
    this.demux = demux;
    this.limits = { ...DEFAULT_VIDEO_LIMITS, ...limits };
    for (const [name, value] of Object.entries(this.limits)) {
      if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`invalid video limit ${name}`);
    }
    if (this.limits.maxFrameBytes > this.limits.maxBytes || this.limits.maxFrames > 64 ||
        this.limits.maxDecodeQueue > 64 || this.limits.maxDimension > 16384 || this.limits.maxSessions > 64) {
      throw new Error('invalid video limits');
    }
    if (typeof this.post !== 'function') throw new Error('video message sink required');
    this.resources = new Map();
    this.sessions = new Map();
    this.credits = new Map();
    this.bytes = 0;
    this.encodedBytes = 0;
    this.revision = 0;
    this.serial = Promise.resolve();
    this.closed = false;
    this.registrationEpoch = 0;
  }
  emit(type, fields, transfer = []) { this.post({ type, ...fields }, transfer); }
  async register({ content, resource, bytes }) {
    const stamp = { content, resource };
    try {
      if (this.closed || !id(content) || !id(resource)) throw new Error('invalid video registration');
      const resourceKey = key(content, resource);
      if (this.resources.has(resourceKey)) throw new Error('duplicate video resource');
      const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
      if (!view.byteLength || view.byteLength > this.limits.maxEncodedBytes ||
          this.encodedBytes + view.byteLength > this.limits.maxEncodedTotalBytes ||
          this.resources.size >= this.limits.maxResources) throw new Error('encoded video resource limit');
      // Reserve compressed bytes before asynchronous metadata/config probing.
      const registration = { bytes: view.byteLength, movie: null };
      this.resources.set(resourceKey, registration);
      this.encodedBytes += registration.bytes;
      try {
        if (!this.Decoder || !this.Chunk) throw new Error('Worker WebCodecs unavailable');
        const movie = await this.demux(view, this.limits);
        const support = await this.Decoder.isConfigSupported(movie.config);
        if (!support.supported) throw new Error(`unsupported Worker codec ${movie.config.codec}`);
        if (this.closed || this.resources.get(resourceKey) !== registration) return;
        registration.movie = presentationIndex(movie);
        this.emit('registered', { ...stamp, codec: movie.config.codec, originNs: movie.originNs, durationNs: movie.durationNs });
      } catch (error) {
        if (this.resources.get(resourceKey) === registration) {
          this.resources.delete(resourceKey);
          this.encodedBytes -= registration.bytes;
        }
        throw error;
      }
    } catch (error) { if (!this.closed) this.emit('unavailable', { ...stamp, reason: String(error.message ?? error) }); }
  }
  demand(request) {
    const { content, resource, generation, targetNs } = request;
    if (this.closed) return Promise.resolve();
    if (!id(content) || !id(resource) || !id(generation) || typeof targetNs !== 'bigint' || targetNs < 0n || targetNs > 9223372036854775807n ||
        (request.maxFrames !== undefined && (!Number.isSafeInteger(request.maxFrames) || request.maxFrames < 1 || request.maxFrames > this.limits.maxFrames))) {
      this.emit('unavailable', { content, resource, generation, reason: 'invalid video demand' });
      return Promise.resolve();
    }
    const sessionKey = key(content, generation);
    let session = this.sessions.get(sessionKey);
    if (session && session.resource !== resource) {
      this.emit('unavailable', { content, resource, generation, reason: 'generation resource changed' });
      return Promise.resolve();
    }
    if (!session) {
      if (this.sessions.size >= this.limits.maxSessions) {
        this.emit('unavailable', { content, resource, generation, reason: 'video session limit' });
        return Promise.resolve();
      }
      session = { content, resource, generation, operation: 0, decoder: null, doneTarget: null };
      this.sessions.set(sessionKey, session);
    }
    if (session.doneTarget === targetNs) return Promise.resolve();
    // Fence output synchronously, before the operation waits behind other work.
    const operation = ++session.operation;
    if (session.decoder && session.decoder.state !== 'closed') session.decoder.close();
    const task = () => this.decode(request, session, operation);
    const result = this.serial.then(task, task);
    this.serial = result.catch(() => {});
    return result;
  }
  current(session, operation) {
    return !this.closed && this.sessions.get(key(session.content, session.generation)) === session && session.operation === operation;
  }
  async decode(request, session, operation) {
    const { content, resource, generation, targetNs, transform = null } = request;
    const stamp = { content, resource, generation };
    if (!this.current(session, operation)) return;
    const movie = this.resources.get(key(content, resource))?.movie;
    if (!movie) { this.emit('unavailable', { ...stamp, reason: 'video resource not registered' }); return; }
    if (session.finalHorizon !== undefined) {
      session.doneTarget = targetNs;
      this.emit('watermark', { ...stamp, completedThroughNs: session.finalHorizon });
      this.emit('eof', { ...stamp, endNs: movie.durationNs });
      return;
    }
    const coverage = session.coverage;
    if (coverage && targetNs >= coverage.from && targetNs < coverage.until) {
      session.doneTarget = targetNs;
      this.emit('watermark', { ...stamp, completedThroughNs: coverage.through });
      return;
    }
    const rawBytes = movie.config.codedWidth * movie.config.codedHeight * 4;
    const canvas = transform?.canvas;
    const transformedBytes = canvas === null || canvas === undefined ? rawBytes : canvas[0] * canvas[1] * 4;
    // Reserve the larger source/output extent until transformation finishes.
    // Byte pressure is an ordinary admission result, before any codec work.
    const frameBudget = Math.max(rawBytes, transformedBytes);
    if (!Number.isSafeInteger(frameBudget) || frameBudget <= 0 || frameBudget > this.limits.maxFrameBytes ||
        (canvas != null && (!Array.isArray(canvas) || canvas.length !== 2 || canvas.some(size =>
          !Number.isSafeInteger(size) || size <= 0 || size > this.limits.maxDimension)))) {
      this.emit('unavailable', { ...stamp, reason: 'video transform extent limit' }); return;
    }
    const available = Math.min(request.maxFrames ?? this.limits.maxFrames, this.limits.maxFrames - this.credits.size,
      Math.floor((this.limits.maxBytes - this.bytes) / frameBudget));
    if (available <= 0) { this.emit('backpressure', stamp); return; }
    const visible = movie.presentation;
    const after = upperPresentationBound(visible, targetNs);
    const first = after > 0 ? after - 1 : 0;
    const desired = visible.slice(first, first + available);
    if (!desired.length) {
      session.finalHorizon = movie.durationNs;
      session.doneTarget = targetNs;
      this.emit('watermark', { ...stamp, completedThroughNs: movie.durationNs });
      this.emit('eof', { ...stamp, endNs: movie.durationNs });
      return;
    }
    const last = desired[desired.length - 1];
    const horizon = last.sample.ptsNs;
    const wanted = new Map(desired.map(entry => [entry.sample.timestampUs, entry.sample]));
    const start = Math.min(...desired.map(entry => entry.rap));
    if (start < 0) { this.emit('unavailable', { ...stamp, reason: 'no preceding random access sample' }); return; }
    // Registration builds the CTS prefix's furthest DTS ordinal once. Each
    // demand binary-searches presentation time and visits only its bounded
    // wanted window and the necessary random-access decode span.
    const end = last.decodeEnd;
    let outputFailure = null;
    const copies = [];
    const delivered = new Set();
    const decoder = new this.Decoder({
      error: error => { outputFailure = error; },
      output: frame => {
        const sample = wanted.get(frame.timestamp);
        if (!this.current(session, operation) || !sample || delivered.has(frame.timestamp)) { frame.close(); return; }
        delivered.add(frame.timestamp);
        copies.push(this.copyFrame(frame, sample, stamp, transform, session, operation, frameBudget).catch(error => { outputFailure = error; }));
      },
    });
    session.decoder = decoder;
    try {
      decoder.configure(movie.config);
      for (let i = start; i < end; i++) {
        if (!this.current(session, operation)) return;
        while (decoder.decodeQueueSize >= this.limits.maxDecodeQueue) {
          // A mid-GOP flush would require the next chunk to be a key frame.
          // Yield only for codec queue progress, never to advance movie time.
          await new Promise(resolve => setTimeout(resolve, 1));
          if (outputFailure) throw outputFailure;
          if (!this.current(session, operation)) return;
        }
        const sample = movie.samples[i];
        decoder.decode(new this.Chunk({ type: sample.key ? 'key' : 'delta', timestamp: sample.timestampUs,
          duration: sample.durationUs, data: sample.data }));
      }
      await decoder.flush();
      await Promise.all(copies);
      if (!this.current(session, operation)) return;
      if (outputFailure) throw outputFailure;
      if (delivered.size !== wanted.size) throw new Error('decoder omitted required presentation frame');
      session.doneTarget = targetNs;
      const next = visible[first + desired.length];
      if (next) session.coverage = { from: after > 0 ? desired[0].sample.ptsNs : 0n,
        until: next.sample.ptsNs, through: horizon };
      this.emit('watermark', { ...stamp, completedThroughNs: horizon });
      if (last === visible[visible.length - 1]) {
        session.finalHorizon = horizon;
        this.emit('eof', { ...stamp, endNs: movie.durationNs });
      }
    } catch (error) {
      await Promise.all(copies);
      if (this.current(session, operation)) this.emit('unavailable', { ...stamp, reason: String(error.message ?? error) });
    } finally {
      // Early cancellation in the queue loop must still join every owned
      // copyTo/transform task so the Worker may close after this demand settles.
      await Promise.allSettled(copies);
      if (decoder.state !== 'closed') decoder.close();
      if (session.decoder === decoder) session.decoder = null;
    }
  }
  async copyFrame(frame, sample, stamp, transform, session, operation, frameBudget) {
    let reservation;
    try {
      const rect = frame.visibleRect ?? { x: 0, y: 0, width: frame.displayWidth, height: frame.displayHeight };
      const width = rect.width, height = rect.height;
      if ((frame.displayWidth !== undefined && frame.displayWidth !== width) ||
          (frame.displayHeight !== undefined && frame.displayHeight !== height)) throw new Error('unsupported video display scaling');
      const size = width * height * 4;
      if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width <= 0 || height <= 0 ||
          width > this.limits.maxDimension || height > this.limits.maxDimension || size > this.limits.maxFrameBytes || size > frameBudget) throw new Error('decoded frame extent limit');
      const revision = ++this.revision;
      reservation = { ...stamp, revision, bytes: frameBudget, published: false };
      if (this.credits.size >= this.limits.maxFrames || this.bytes + frameBudget > this.limits.maxBytes) throw new Error('decoded frame credit limit');
      this.credits.set(revision, reservation);
      this.bytes += frameBudget;
      const rgba = new Uint8Array(size);
      await frame.copyTo(rgba, { format: 'RGBA', rect: { x: rect.x, y: rect.y, width, height }, layout: [{ offset: 0, stride: width * 4 }] });
      if (!this.current(session, operation)) return;
      if (transform !== null && this.transformFrame === identity) throw new Error('video transform helper unavailable');
      const pixels = await this.transformFrame({ width, height, rgba }, transform);
      if (!this.current(session, operation)) return;
      const finalSize = pixels?.width * pixels?.height * 4;
      if (!Number.isSafeInteger(pixels?.width) || !Number.isSafeInteger(pixels?.height) || pixels.width <= 0 || pixels.height <= 0 ||
          pixels.width > this.limits.maxDimension || pixels.height > this.limits.maxDimension || !(pixels.rgba instanceof Uint8Array) ||
          pixels.rgba.byteLength !== finalSize || finalSize > this.limits.maxFrameBytes || finalSize > frameBudget || this.bytes - frameBudget + finalSize > this.limits.maxBytes) throw new Error('transformed frame limit');
      this.bytes += finalSize - frameBudget;
      reservation.bytes = finalSize;
      const buffer = pixels.rgba.byteOffset === 0 && pixels.rgba.byteLength === pixels.rgba.buffer.byteLength
        ? pixels.rgba.buffer : pixels.rgba.slice().buffer;
      reservation.published = true;
      this.emit('frame', { ...stamp, revision, ptsNs: sample.ptsNs, width: pixels.width, height: pixels.height, rgba: buffer }, [buffer]);
    } finally {
      frame.close();
      if (reservation && !reservation.published) this.release(reservation.revision);
    }
  }
  release(revision) {
    const credit = this.credits.get(revision);
    if (!credit) return;
    this.bytes -= credit.bytes;
    this.credits.delete(revision);
  }
  ack({ content, generation, revision }) {
    const credit = this.credits.get(revision);
    if (credit?.published && credit.content === content && credit.generation === generation) this.release(revision);
  }
  retire({ content, generation } = {}) {
    for (const [sessionKey, session] of this.sessions) {
      if ((content === undefined || session.content === content) && (generation === undefined || session.generation === generation)) {
        session.operation++;
        this.sessions.delete(sessionKey);
        if (session.decoder && session.decoder.state !== 'closed') session.decoder.close();
      }
    }
    // Transferred frames remain charged until their matching ACK. Removing a
    // session must not mint extra credits while the renderer owns its buffers.
    if (generation === undefined) {
      this.registrationEpoch++;
      for (const [resourceKey, resource] of this.resources) {
        if (content === undefined || resourceKey.startsWith(`${typeof content}:${content}:`)) {
          this.resources.delete(resourceKey); this.encodedBytes -= resource.bytes;
        }
      }
    }
  }
  close() { this.retire(); this.closed = true; }
}

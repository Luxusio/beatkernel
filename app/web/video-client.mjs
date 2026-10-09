// Independent movie transport. Gameplay visual ACKs never wait for codec work.
export class VideoClient {
  constructor({ worker, view, onFrame = () => {}, onUnavailable = () => {} }) {
    this.worker = worker;
    this.view = view;
    this.onFrame = onFrame;
    this.onUnavailable = onUnavailable;
    this.content = null;
    this.sessions = new Map();
    this.registered = new Set();
    this.pending = new Map();
    this.blocked = new Map();
    this.completions = new Map();
    this.failedSessions = new Set();
    this.closed = false;
    this.transformReady = false;
    this.suspended = false;
    this.registration = null;
    worker.onmessage = ({ data }) => this.receive(data);
    worker.onmessageerror = () => onUnavailable('Movie worker message could not be decoded.');
    worker.onerror = event => onUnavailable(event.message ?? 'Movie worker failed.');
    worker.postMessage({ type: 'configure-transform', moduleUrl: new URL('./video-transform.mjs', import.meta.url).href });
  }
  register(generation, content, registration) {
    this.retire();
    if (typeof content !== 'bigint' || content <= 0n || content > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error('Movie content identity exceeds worker precision.');
    if (!Array.isArray(registration.resources) || registration.resources.length > 3844 || !Array.isArray(registration.images)
      || registration.images.length > 3844 || registration.images.some(image => !Number.isSafeInteger(image.resource)
        || image.resource < 0 || image.resource >= registration.resources.length)) throw new Error('Invalid movie registration.');
    let total = 0;
    for (const bytes of registration.resources) {
      if (!(bytes instanceof Uint8Array) || !bytes.byteLength || bytes.byteLength > 64 * 1024 * 1024) throw new Error('Movie encoded resource limit.');
      total += bytes.byteLength;
    }
    if (total > 256 * 1024 * 1024) throw new Error('Movie encoded bank limit.');
    this.registration = { generation, content, images: registration.images };
    this.suspended = false;
    this.view.register_video(generation, content, registration.images);
    this.content = Number(content);
    registration.resources.forEach((bytes, resource) => {
      const copy = bytes.slice();
      this.worker.postMessage({ type: 'register', content: this.content, resource, bytes: copy }, [copy.buffer]);
    });
    this.refresh();
  }
  refresh() {
    if (this.closed || this.suspended || this.content === null) return;
    const demands = this.view.video_demands();
    const wanted = new Set(demands.map(demand => demand.generation));
    for (const [generation, demand] of this.sessions) {
      if (!wanted.has(generation)) {
        this.worker.postMessage({ type: 'retire', content: this.content, generation });
        this.dropFrames(generation);
        this.completions.delete(generation);
        this.failedSessions.delete(generation);
        this.sessions.delete(generation);
        this.pending.delete(generation);
      }
    }
    for (const demand of demands) {
      if (demand.content !== this.content) throw new Error('Movie demand content differs from registration.');
      const previous = this.sessions.get(demand.generation);
      if (previous?.completedTarget !== undefined) demand.completedTarget = previous.completedTarget;
      this.sessions.set(demand.generation, demand);
      // At most one codec request per generation is outstanding. New committed
      // song state replaces the pending target without repeatedly closing a GOP.
      if (this.failedSessions.has(demand.generation) || !this.transformReady || !this.registered.has(demand.resource) || this.pending.has(demand.generation)) continue;
      if (previous?.completedTarget === demand.targetNs) { demand.completedTarget = previous.completedTarget; continue; }
      this.issue(demand);
    }
    this.drain();
  }
  issue(demand) {
    this.pending.set(demand.generation, demand.targetNs);
    this.worker.postMessage({ type: 'demand', ...demand, maxFrames: 1 });
  }
  receive(message) {
    if (!this.closed && message?.type === 'transform-ready') { this.transformReady = true; this.refresh(); return; }
    if (message?.type === 'frame') {
      const demand = this.sessions.get(message.generation);
      if (this.closed || message.content !== this.content || demand?.resource !== message.resource
        || this.failedSessions.has(message.generation)) { this.ack(message); return; }
      try {
        if (this.admit(message, demand)) { this.ack(message); this.onFrame(); }
        else {
          const bytes = message.rgba.byteLength;
          const held = [...this.blocked.values()].reduce((sum, frame) => sum + frame.rgba.byteLength, 0);
          if (this.blocked.size >= 3 || held + bytes > 192 * 1024 * 1024) throw new Error('Movie renderer pending frame limit.');
          this.blocked.set(message.revision, message);
          this.onFrame();
        }
      } catch (error) { this.ack(message); this.failSession(message.generation, error); }
      return;
    }
    if (this.closed) return;
    if (message?.type === 'unavailable' && message.content === undefined) { this.onUnavailable(message.reason); return; }
    if (message?.content !== this.content) return;
    if (message.type === 'registered') { this.registered.add(message.resource); this.refresh(); return; }
    const demand = this.sessions.get(message.generation);
    if (!demand || demand.resource !== message.resource) {
      if (message.type === 'unavailable') this.onUnavailable(message.reason);
      return;
    }
    try {
      if (message.type === 'watermark' || message.type === 'eof') {
        const completion = this.completions.get(message.generation) ?? {};
        completion[message.type] = message;
        this.completions.set(message.generation, completion);
        this.applyCompletion(message.generation);
        this.drain();
      } else if (message.type === 'unavailable' || message.type === 'backpressure') {
        this.pending.delete(message.generation);
        if (message.type === 'unavailable') this.onUnavailable(message.reason);
      }
    } catch (error) { this.failSession(message.generation, error); }
  }
  ack(message) {
    this.worker.postMessage({ type: 'ack', content: message.content, generation: message.generation, revision: message.revision });
  }
  admit(message, demand) {
    const proof = this.completions.get(message.generation)?.watermark?.completedThroughNs;
    if (proof !== undefined && message.ptsNs <= proof) {
      return this.view.admit_completed_video_frame(demand.slot, BigInt(message.generation), BigInt(message.revision),
        message.ptsNs, message.width, message.height, new Uint8Array(message.rgba), proof);
    }
    return this.view.admit_video_frame(demand.slot, BigInt(message.generation), BigInt(message.revision),
      message.ptsNs, message.width, message.height, new Uint8Array(message.rgba));
  }
  dropFrames(generation = undefined) {
    for (const [revision, message] of this.blocked) {
      if (generation === undefined || message.generation === generation) { this.ack(message); this.blocked.delete(revision); }
    }
  }
  failSession(generation, error) {
    this.failedSessions.add(generation);
    this.dropFrames(generation);
    this.completions.delete(generation);
    this.pending.delete(generation);
    this.worker.postMessage({ type: 'retire', content: this.content, generation });
    this.onUnavailable(String(error.message ?? error));
  }
  applyCompletion(generation) {
    const completion = this.completions.get(generation), demand = this.sessions.get(generation);
    if (!completion || !demand || this.failedSessions.has(generation)) return;
    const holes = [...this.blocked.values()].filter(frame => frame.generation === generation);
    if (completion.watermark) {
      let through = completion.watermark.completedThroughNs;
      for (const frame of holes) if (frame.ptsNs <= through) through = frame.ptsNs - 1n;
      if (completion.appliedThrough !== through) {
        this.view.video_watermark(demand.slot, BigInt(generation), through);
        completion.appliedThrough = through;
        this.onFrame();
      }
      if (!holes.length) {
        completion.watermark = null;
        demand.completedTarget = this.pending.get(generation);
        this.pending.delete(generation);
      }
    }
    if (!holes.length && completion.eof) {
      this.view.video_end(demand.slot, BigInt(generation), completion.eof.endNs);
      completion.eof = null;
      this.onFrame();
    }
    if (!completion.watermark && !completion.eof) {
      this.completions.delete(generation);
      if (demand.targetNs !== demand.completedTarget && !this.pending.has(generation)) this.issue(demand);
    }
  }
  // Called after committed clock state or draw selects an eligible frame and
  // releases older queue entries. This never advances movie time itself.
  drain() {
    if (this.closed || this.suspended) return;
    for (const [revision, message] of [...this.blocked.entries()].sort((a, b) => a[1].ptsNs < b[1].ptsNs ? -1 : a[1].ptsNs > b[1].ptsNs ? 1 : 0)) {
      const demand = this.sessions.get(message.generation);
      try {
        if (!demand || this.admit(message, demand)) {
          this.blocked.delete(revision); this.ack(message);
          if (demand) this.onFrame();
        }
      } catch (error) { this.failSession(message.generation, error); }
    }
    for (const generation of [...this.completions.keys()]) this.applyCompletion(generation);
  }
  suspend() {
    if (this.suspended) return;
    this.suspended = true;
    for (const generation of this.sessions.keys()) this.worker.postMessage({ type: 'retire', content: this.content, generation });
    this.dropFrames(); this.completions.clear(); this.failedSessions.clear();
    this.sessions.clear(); this.pending.clear();
    this.view.retire_video();
  }
  resume() {
    if (!this.suspended || !this.registration) return;
    const { generation, content, images } = this.registration;
    this.view.register_video(generation, content, images);
    this.suspended = false;
    this.refresh();
  }
  retire() {
    if (this.content !== null) this.worker.postMessage({ type: 'retire', content: this.content });
    this.view.retire_video();
    this.content = null;
    this.registration = null;
    this.suspended = false;
    this.dropFrames(); this.completions.clear(); this.failedSessions.clear();
    this.sessions.clear(); this.registered.clear(); this.pending.clear();
  }
  close() {
    if (this.closed) return;
    this.retire();
    this.closed = true;
    this.worker.postMessage({ type: 'close' });
    // Worker closes itself after synchronously retiring codec owners. Late
    // frames already transferred remain ACK-able until that close completes.
  }
}

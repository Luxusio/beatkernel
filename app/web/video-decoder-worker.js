import { BrowserVideoDecoder } from './video-decoder.mjs';

let transformFrame = null;
const decoder = new BrowserVideoDecoder({
  limits: { maxSessions: 16 },
  postMessage: (message, transfer) => self.postMessage(message, transfer),
  transformFrame: async (frame, transform) => {
    if (transform === null) return frame;
    if (!transformFrame) throw new Error('video transform helper unavailable');
    return transformFrame(frame, transform);
  },
});
const active = new Set();
let closing = false;
async function dispatch(data) {
  try {
    switch (data?.type) {
      case 'configure-transform': {
        // The renderer coordinator supplies its local WASM helper module; no
        // pixel semantics are duplicated in this codec adapter.
        const module = await import(data.moduleUrl);
        if (typeof module.transformFrame !== 'function') throw new Error('invalid video transform module');
        if (module.initialize) await module.initialize();
        if (closing) return;
        transformFrame = module.transformFrame;
        self.postMessage({ type: 'transform-ready' });
        break;
      }
      case 'register': await decoder.register(data); break;
      case 'demand': await decoder.demand(data); break;
      case 'ack': decoder.ack(data); break;
      case 'retire': decoder.retire(data); break;
      case 'close': break;
      default: throw new Error('unknown video worker message');
    }
  } catch (error) {
    self.postMessage({ type: 'unavailable', content: data?.content, generation: data?.generation,
      resource: data?.resource, reason: String(error.message ?? error) });
  }
}
self.onmessage = ({ data }) => {
  // ACKs remain accepted while owned copyTo/transform work drains. Fence new
  // work immediately, then close the Worker only after its frames are closed.
  if (data?.type === 'ack') { decoder.ack(data); return; }
  if (data?.type === 'close') {
    if (closing) return;
    closing = true;
    decoder.close();
    void Promise.allSettled([...active]).then(() => self.close());
    return;
  }
  if (closing) return;
  const operation = dispatch(data);
  active.add(operation);
  void operation.finally(() => active.delete(operation));
};

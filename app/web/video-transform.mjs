import init, { transform_video_frame } from './pkg/beatkernel_bms_runtime.js';
let ready;
export function initialize() { return ready ??= init(); }
export async function transformFrame(frame, transform) {
  await initialize();
  if (transform === null) return frame;
  return transform_video_frame(frame.width, frame.height, frame.rgba, transform);
}

// Diagnostic helpers only; these do not supply a production audio clock.
const frameInteger = value => Number.isSafeInteger(value) && value >= 0;

/** Choose a time whose represented sample position has the unambiguous ceil T. */
export function markerTime(targetFrame, sampleRate) {
  if (!frameInteger(targetFrame) || targetFrame === 0
    || !frameInteger(sampleRate) || sampleRate === 0) return null;
  const time = (targetFrame - 0.25) / sampleRate;
  const scaled = time * sampleRate;
  if (!Number.isFinite(time) || !Number.isFinite(scaled)
    || !(scaled > targetFrame - 1 && scaled < targetFrame)
    || Math.ceil(scaled) !== targetFrame) return null;
  return time;
}

/**
 * Recover a callback base from a KNOWN ACTUAL native onset, not a scheduled time.
 * An interior edge alone cannot authenticate onset after native late clamping.
 */
export function markerFrame(values, actualFrames, targetFrame) {
  if (!(values instanceof Float32Array) || !frameInteger(actualFrames)
    || values.length !== actualFrames || !frameInteger(targetFrame)) return null;
  let edge = 0;
  while (edge < actualFrames && values[edge] === 0) edge += 1;
  if (edge === 0 || edge === actualFrames) return null;
  for (let index = edge; index < actualFrames; index += 1) {
    if (values[index] !== 1) return null;
  }
  const frame = targetFrame - edge;
  return frameInteger(frame) ? frame : null;
}

/** Count the observed callback extent without losing integer precision. */
export function advanceFrame(frame, actualFrames) {
  if (!frameInteger(frame) || !frameInteger(actualFrames)) return null;
  const next = frame + actualFrames;
  return frameInteger(next) ? next : null;
}

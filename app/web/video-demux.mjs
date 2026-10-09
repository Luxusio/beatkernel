import { createFile, DataStream, Endianness } from './vendor/mp4box.all.mjs';

const NS = 1_000_000_000n;
function integer(value, name) {
  if (!Number.isSafeInteger(value)) throw new Error(`unsupported ${name} precision`);
  return BigInt(value);
}
function floorDiv(n, d) { return n >= 0n ? n / d : -((-n + d - 1n) / d); }
export function nsToUs(value) {
  if (typeof value !== 'bigint') throw new Error('nanosecond timestamp must be BigInt');
  const us = floorDiv(value, 1000n);
  if (us < BigInt(Number.MIN_SAFE_INTEGER) || us > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error('unsupported WebCodecs timestamp precision');
  }
  return Number(us);
}

// Decode timestamps remain source coordinates; presentation timestamps have one
// rational source origin, computed once for the complete resource, never per seek.
export function normalizeTrack(track, input, options = {}) {
  const scale = integer(track.timescale, 'track timescale');
  if (scale <= 0n) throw new Error('invalid track timescale');
  if (track.matrix && track.matrix.some((value, index) => value !== [65536, 0, 0, 0, 65536, 0, 0, 0, 1073741824][index])) {
    throw new Error('unsupported video track matrix');
  }
  const width = track.video?.width ?? track.width;
  const height = track.video?.height ?? track.height;
  const maximum = options.maxDimension ?? 16384;
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width <= 0 || height <= 0 ||
      width > maximum || height > maximum || width * height * 4 > (options.maxFrameBytes ?? 64 * 1024 * 1024)) {
    throw new Error('video dimensions exceed limit');
  }
  if (!/^(avc[13]\.|hvc1\.|hev1\.|av01\.|vp09\.|vp8$)/.test(track.codec ?? '')) {
    throw new Error('unsupported MP4 video codec');
  }
  const description = options.description;
  if (/^(avc|hvc|hev|av01)/.test(track.codec) && !(description instanceof Uint8Array)) {
    throw new Error('missing codec description');
  }
  if (!input.length || input.length > (options.maxSamples ?? 1_000_000)) throw new Error('video sample limit');
  const edits = options.edits ?? [];
  let mediaStart = 0n, emptyDuration = 0n, editDuration = null;
  let movieScale = scale;
  if (edits.length) {
    movieScale = integer(options.movieTimescale, 'movie timescale');
    if (movieScale <= 0n || edits.length > 2) throw new Error('unsupported edit list');
    for (let i = 0; i < edits.length; i++) {
      const edit = edits[i];
      if (edit.media_rate_integer !== 1 || edit.media_rate_fraction !== 0) throw new Error('unsupported edit rate');
      const duration = integer(edit.segment_duration, 'edit duration');
      const start = integer(edit.media_time, 'edit media time');
      if (duration <= 0n) throw new Error('unsupported edit duration');
      if (start === -1n && i === 0 && edits.length === 2) emptyDuration = duration;
      else if (start >= 0n && i === edits.length - 1) { mediaStart = start; editDuration = duration; }
      else throw new Error('unsupported edit list');
    }
  }
  let encodedBytes = 0;
  const samples = input.map((sample, index) => {
    const cts = integer(sample.cts, 'CTS'), dts = integer(sample.dts, 'DTS');
    const duration = integer(sample.duration, 'sample duration');
    if (duration < 0n || !(sample.data instanceof Uint8Array)) throw new Error('invalid video sample');
    encodedBytes += sample.data.byteLength;
    if (encodedBytes > (options.maxEncodedBytes ?? 64 * 1024 * 1024)) throw new Error('encoded video sample byte limit');
    const display = !edits.length || (cts >= mediaStart && (cts - mediaStart) * movieScale < editDuration * scale);
    return { cts, dts, duration, data: sample.data, key: !!sample.is_sync, display, index };
  }).sort((a, b) => a.dts < b.dts ? -1 : a.dts > b.dts ? 1 : a.index - b.index);
  const visible = samples.filter(sample => sample.display);
  if (!visible.length) throw new Error('edit list contains no video presentation');
  const origin = visible.reduce((min, sample) => sample.cts < min ? sample.cts : min, visible[0].cts);
  const originNs = floorDiv(((origin - mediaStart) * movieScale + emptyDuration * scale) * NS, scale * movieScale);
  if (originNs < -9223372036854775808n || originNs > 9223372036854775807n) throw new Error('unsupported source origin precision');
  const seen = new Set();
  let durationNs = 0n;
  for (const sample of samples) {
    sample.ptsNs = floorDiv((sample.cts - origin) * NS, scale);
    sample.dtsNs = floorDiv(sample.dts * NS, scale);
    sample.timestampUs = nsToUs(sample.ptsNs);
    sample.durationUs = nsToUs(floorDiv(sample.duration * NS, scale));
    // WebCodecs only returns integer microseconds. Ambiguous identities cannot
    // safely restore nanosecond CTS, including duplicate CTS frames.
    if (seen.has(sample.timestampUs)) throw new Error('unsupported ambiguous microsecond CTS collision');
    seen.add(sample.timestampUs);
    if (sample.display) {
      const end = floorDiv((sample.cts - origin + sample.duration) * NS, scale);
      if (end > durationNs) durationNs = end;
    }
    delete sample.cts; delete sample.dts; delete sample.duration; delete sample.index;
  }
  if (editDuration !== null) {
    const editEnd = floorDiv((editDuration * scale - (origin - mediaStart) * movieScale) * NS, scale * movieScale);
    if (durationNs > editEnd) durationNs = editEnd;
  }
  return { config: { codec: track.codec, codedWidth: width, codedHeight: height,
    ...(description ? { description } : {}) }, samples, originNs, durationNs };
}

export async function demuxMp4(bytes, options = {}) {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  if (!view.byteLength || view.byteLength > (options.maxEncodedBytes ?? 64 * 1024 * 1024)) throw new Error('encoded video byte limit');
  const file = createFile();
  let track, failure;
  const samples = [];
  file.onError = error => { failure = new Error(`MP4 demux: ${error}`); };
  file.onReady = info => {
    try {
      track = info.videoTracks[0];
      if (!track || track.nb_samples > (options.maxSamples ?? 1_000_000)) throw new Error('missing video track or sample limit');
      file.setExtractionOptions(track.id, null, { nbSamples: 64 });
      file.start();
    } catch (error) { failure = error; }
  };
  file.onSamples = (id, user, batch) => {
    if (id !== track?.id || failure) return;
    if (samples.length + batch.length > (options.maxSamples ?? 1_000_000)) { failure = new Error('video sample limit'); return; }
    for (const sample of batch) samples.push({ ...sample, data: sample.data.slice() });
    file.releaseUsedSamples(id, samples.length);
  };
  const buffer = view.slice().buffer;
  buffer.fileStart = 0;
  file.appendBuffer(buffer);
  file.flush();
  if (failure) throw failure;
  if (!track || samples.length !== track.nb_samples) throw new Error('incomplete MP4 video samples');
  const trak = file.getTrackById(track.id);
  const entries = trak.mdia.minf.stbl.stsd.entries;
  if (entries.length !== 1) throw new Error('unsupported changing sample description');
  const entry = entries[0];
  const box = entry.avcC ?? entry.hvcC ?? entry.av1C ?? entry.vpcC;
  let description;
  if (box) {
    const stream = new DataStream(undefined, 0, Endianness.BIG_ENDIAN);
    box.write(stream);
    description = new Uint8Array(stream.buffer, 8).slice();
  }
  return normalizeTrack(track, samples, { ...options, description,
    movieTimescale: file.moov.mvhd.timescale, edits: trak.edts?.elst?.entries ?? [] });
}

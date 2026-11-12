// Pure logic shared by scripts/godot-reference.mjs and scripts/compare-to-godot.mjs
// (pat-xggzs): the key timeline -> frame translation both engines replay, the
// trace parsing and alignment, the numeric trace diff, and the image metrics
// with a dependency-free PNG codec. No I/O here except the PNG byte codecs, so
// every function is unit-tested in scripts/tests/godot-reference.test.mjs.

import zlib from 'node:zlib';

// ---------------------------------------------------------------------------
// Key timeline
// ---------------------------------------------------------------------------

// DOM `KeyboardEvent.code` (the browser-playtest --keys spelling) -> the key
// name BOTH engines accept: Godot's `OS.find_keycode_from_string` and Patina's
// `Key::from_name` (the MCP `play` tool's `key`).
const NAMED_KEYS = {
  Space: 'Space', Enter: 'Enter', Escape: 'Escape', Tab: 'Tab', Backspace: 'Backspace',
  ArrowLeft: 'Left', ArrowRight: 'Right', ArrowUp: 'Up', ArrowDown: 'Down',
  ShiftLeft: 'Shift', ShiftRight: 'Shift', Shift: 'Shift',
  ControlLeft: 'Ctrl', ControlRight: 'Ctrl', Control: 'Ctrl', Ctrl: 'Ctrl',
  AltLeft: 'Alt', AltRight: 'Alt', Alt: 'Alt',
  Left: 'Left', Right: 'Right', Up: 'Up', Down: 'Down',
};

/** Maps a --keys key (DOM code like `KeyW`, `ArrowUp`, `Digit1`, or a Godot name like `W`) to the shared key name. */
export function keyName(code) {
  if (NAMED_KEYS[code]) return NAMED_KEYS[code];
  let m = /^Key([A-Za-z])$/.exec(code);
  if (m) return m[1].toUpperCase();
  m = /^Digit([0-9])$/.exec(code);
  if (m) return m[1];
  if (/^[A-Za-z0-9]$/.test(code)) return code.toUpperCase();
  if (/^F([1-9]|1[0-2])$/.test(code)) return code;
  throw new Error(`unknown key "${code}" (use a DOM code like KeyW, ArrowLeft, Space, Digit1, or a Godot name like W)`);
}

/** Milliseconds -> whole frames at `fps`, rounded to nearest. */
export function msToFrames(ms, fps = 60) {
  return Math.round((ms * fps) / 1000);
}

/**
 * Translates a browser-playtest `--keys` spec ("wait:2000,KeyW:1500,Space:300",
 * also `down:KeyW` / `up:KeyW`) into a frame-exact timeline.
 *
 * Frames are 1-based and match the MCP `play` tool: an input with
 * `{start, end}` is pressed before frame `start` steps and released after
 * frame `end` steps (inclusive). `boundaries` is the last frame of every step,
 * where reference pictures are taken. `frames` is the timeline's length.
 */
export function parseTimeline(spec, fps = 60) {
  const steps = String(spec || '').split(',').map((s) => s.trim()).filter(Boolean);
  const inputs = [];
  const boundaries = [];
  const open = new Map();
  let cursor = 1;
  for (const step of steps) {
    const i = step.lastIndexOf(':');
    if (i <= 0) throw new Error(`bad step "${step}" (want Code:ms, wait:ms, down:Code or up:Code)`);
    const head = step.slice(0, i);
    const tail = step.slice(i + 1);
    if (head === 'down') {
      const key = keyName(tail);
      if (open.has(key)) throw new Error(`"${step}": ${tail} is already down`);
      open.set(key, inputs.length);
      inputs.push({ key, code: tail, start: cursor, end: null, label: step });
      continue;
    }
    if (head === 'up') {
      const key = keyName(tail);
      if (!open.has(key)) throw new Error(`"${step}": ${tail} is not down`);
      const input = inputs[open.get(key)];
      input.end = Math.max(input.start, cursor - 1);
      open.delete(key);
      continue;
    }
    const ms = Number(tail);
    if (!Number.isFinite(ms) || ms < 0) throw new Error(`bad duration in "${step}"`);
    const n = msToFrames(ms, fps);
    if (head === 'wait') {
      cursor += n;
    } else {
      const held = Math.max(1, n);
      inputs.push({ key: keyName(head), code: head, start: cursor, end: cursor + held - 1, label: step });
      cursor += held;
    }
    boundaries.push({ frame: Math.max(0, cursor - 1), label: step });
  }
  const frames = Math.max(0, cursor - 1);
  for (const idx of open.values()) inputs[idx].end = Math.max(inputs[idx].start, frames);
  return { fps, frames, inputs, boundaries };
}

/** The timeline as MCP `play` inputs (`{key, at, to}`). */
export function toMcpInputs(timeline) {
  return timeline.inputs.map((i) => ({ key: i.key, at: i.start, to: i.end }));
}

/**
 * The frames to picture: the step boundaries (deduplicated, >= 1, <= frames),
 * thinned evenly to at most `max` (the MCP `play` limit is 8), always keeping
 * the last one.
 */
export function pickShotFrames(timeline, frames, max = 8) {
  const all = [...new Set(timeline.boundaries.map((b) => b.frame).concat([frames]))]
    .filter((f) => f >= 1 && f <= frames)
    .sort((a, b) => a - b);
  if (all.length <= max) return all;
  const out = [];
  for (let k = 0; k < max; k++) out.push(all[Math.round((k * (all.length - 1)) / (max - 1))]);
  return [...new Set(out)];
}

// ---------------------------------------------------------------------------
// Watches and traces
// ---------------------------------------------------------------------------

/** "/root/Game/Player:position" -> {path, prop, id}. The LAST colon splits (node paths have none). */
export function parseWatch(spec) {
  const i = spec.lastIndexOf(':');
  if (i <= 0 || i === spec.length - 1) throw new Error(`bad --watch "${spec}" (want /root/Path/To/Node:property)`);
  return { path: spec.slice(0, i), prop: spec.slice(i + 1), id: spec };
}

/**
 * Every number in a value's text form: Patina's `Vector3(1, 2.5, -3)`,
 * Godot's `(1.0, 2.5, -3.0)`, `true`, `7`. Booleans become 0/1; a value with
 * no numbers (a string, null) returns null so it is compared as text.
 */
export function numbersOf(text) {
  if (Array.isArray(text)) return text.map(Number);
  if (typeof text === 'number') return [text];
  if (typeof text === 'boolean') return [text ? 1 : 0];
  let s = String(text).trim();
  if (s === 'true') return [1];
  if (s === 'false') return [0];
  // Blank out identifiers first: the "3" in `Vector3(...)` is not a value.
  // (An `e` right after a digit is an exponent, not an identifier.)
  s = s.replace(/(?<![\d.])[A-Za-z_][A-Za-z0-9_]*/g, (w) => (w === 'inf' || w === 'nan' ? w : ' '));
  const nums = s.match(/[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?|[-+]?inf|nan/g);
  if (!nums) return null;
  return nums.map((n) => (n === 'inf' || n === '+inf' ? Infinity : n === '-inf' ? -Infinity : n === 'nan' ? NaN : Number(n)));
}

/**
 * Parses the `probes:` section of an MCP `play` report into a trace:
 * `{ "<path>:<prop>": [{frame, value, raw}] }`. A probe line is
 * `f12 /root/Game/Player: position=Vector3(1, 2, 3)`; a probe that could not
 * resolve (`f3 /root/X: no node at ...`) is recorded with `error`.
 */
export function parseMcpProbes(text, watches) {
  const trace = {};
  for (const w of watches) trace[w.id] = [];
  for (const line of String(text).split('\n')) {
    const m = /^\s*f(\d+)\s+(.+?):\s(.*)$/.exec(line);
    if (!m) continue;
    const frame = Number(m[1]);
    const node = m[2];
    const rest = m[3];
    for (const w of watches) {
      if (w.path !== node) continue;
      const pm = new RegExp(`(?:^|\\s)${escapeRe(w.prop)}=(.*)$`).exec(rest);
      if (pm) trace[w.id].push({ frame, value: numbersOf(pm[1]), raw: pm[1].trim() });
      // Another watched property of the SAME node (`velocity=...` while this
      // watch is `position`): not an error for this watch. Recording it as one
      // let `dedupeByFrame` replace the real value with null.
      else if (/^[A-Za-z_][\w/]*=/.test(rest.trim())) continue;
      else trace[w.id].push({ frame, value: null, raw: rest.trim(), error: rest.trim() });
    }
  }
  for (const id of Object.keys(trace)) trace[id] = dedupeByFrame(trace[id]);
  return trace;
}

function escapeRe(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Keeps the last sample per frame, sorted by frame. */
function dedupeByFrame(samples) {
  const by = new Map();
  for (const s of samples) by.set(s.frame, s);
  return [...by.values()].sort((a, b) => a.frame - b.frame);
}

/**
 * Normalises the Godot capture's trace (`frames: [{frame, values: {id: [..]}}]`)
 * to the same `{id: [{frame, value, raw}]}` shape.
 */
export function traceFromGodot(capture, watches) {
  const trace = {};
  for (const w of watches) trace[w.id] = [];
  for (const f of capture.frames || []) {
    for (const w of watches) {
      if (!(w.id in (f.values || {}))) continue;
      const v = f.values[w.id];
      const err = f.errors && f.errors[w.id];
      trace[w.id].push(err ? { frame: f.frame, value: null, raw: String(err), error: String(err) } : { frame: f.frame, value: numbersOf(v), raw: JSON.stringify(v) });
    }
  }
  for (const id of Object.keys(trace)) trace[id] = dedupeByFrame(trace[id]);
  return trace;
}

/**
 * Pairs two traces of one property frame by frame, optionally shifting the
 * second by `offset` frames (Patina frame f compared with Godot frame f+offset).
 * Returns the pairs plus the frames only one side has.
 */
export function alignTraces(a, b, offset = 0) {
  const bm = new Map(b.map((s) => [s.frame - offset, s]));
  const pairs = [];
  const onlyA = [];
  for (const s of a) {
    const t = bm.get(s.frame);
    if (t) {
      pairs.push({ frame: s.frame, a: s, b: t });
      bm.delete(s.frame);
    } else onlyA.push(s.frame);
  }
  return { pairs, onlyA, onlyB: [...bm.keys()].sort((x, y) => x - y) };
}

/** Euclidean distance between two number lists (Infinity when their shapes differ or either is missing). */
export function valueError(a, b) {
  if (!a || !b || a.length !== b.length) return Infinity;
  let sum = 0;
  for (let i = 0; i < a.length; i++) {
    const d = a[i] - b[i];
    if (Number.isNaN(d)) {
      if (Number.isNaN(a[i]) && Number.isNaN(b[i])) continue;
      return Infinity;
    }
    sum += d * d;
  }
  return Math.sqrt(sum);
}

/**
 * Diffs one property's reference (`ref`, Godot) and actual (`act`, Patina)
 * traces. `tolerance` is the largest Euclidean error that still counts as a
 * match. Reports the first diverging frame, the max error and where, the mean
 * error, per-component max abs errors, and the frames missing on either side.
 */
export function diffTrace(ref, act, { tolerance = 1e-3, offset = 0 } = {}) {
  const { pairs, onlyA, onlyB } = alignTraces(ref, act, offset);
  let firstDivergence = null;
  let maxError = 0;
  let maxErrorFrame = null;
  let sum = 0;
  let finite = 0;
  let components = null;
  const series = [];
  for (const p of pairs) {
    const err = p.a.error || p.b.error ? Infinity : valueError(p.a.value, p.b.value);
    series.push({ frame: p.frame, ref: p.a.value, act: p.b.value, error: err });
    if (err > tolerance && firstDivergence === null) firstDivergence = p.frame;
    if (maxErrorFrame === null || err > maxError) {
      maxError = err;
      maxErrorFrame = p.frame;
    }
    if (Number.isFinite(err)) {
      sum += err;
      finite++;
      if (p.a.value && p.b.value && p.a.value.length === p.b.value.length) {
        components = components || new Array(p.a.value.length).fill(0);
        for (let i = 0; i < p.a.value.length; i++) components[i] = Math.max(components[i], Math.abs(p.a.value[i] - p.b.value[i]));
      }
    }
  }
  return {
    compared: pairs.length,
    firstDivergence,
    maxError,
    maxErrorFrame,
    meanError: finite ? sum / finite : null,
    componentMaxError: components,
    missingInActual: onlyA,
    missingInReference: onlyB,
    pass: pairs.length > 0 && firstDivergence === null,
    series,
  };
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/** An RGBA8 image: {width, height, data: Uint8Array(w*h*4)}. */
export function makeImage(width, height, fill = [0, 0, 0, 255]) {
  const data = new Uint8Array(width * height * 4);
  for (let i = 0; i < data.length; i += 4) data.set(fill, i);
  return { width, height, data };
}

/** Nearest-neighbour (upscale) / box-average (downscale) resample to w x h. */
export function resizeImage(img, w, h) {
  if (img.width === w && img.height === h) return img;
  const out = makeImage(w, h);
  const sx = img.width / w;
  const sy = img.height / h;
  for (let y = 0; y < h; y++) {
    const y0 = Math.floor(y * sy);
    const y1 = Math.max(y0 + 1, Math.floor((y + 1) * sy));
    for (let x = 0; x < w; x++) {
      const x0 = Math.floor(x * sx);
      const x1 = Math.max(x0 + 1, Math.floor((x + 1) * sx));
      const acc = [0, 0, 0, 0];
      let n = 0;
      for (let yy = y0; yy < Math.min(y1, img.height); yy++) {
        for (let xx = x0; xx < Math.min(x1, img.width); xx++) {
          const i = (yy * img.width + xx) * 4;
          for (let c = 0; c < 4; c++) acc[c] += img.data[i + c];
          n++;
        }
      }
      const o = (y * w + x) * 4;
      for (let c = 0; c < 4; c++) out.data[o + c] = Math.round(acc[c] / Math.max(1, n));
    }
  }
  return out;
}

/** Heat-map colour for a 0..255 delta: black -> red -> yellow -> white. */
export function heatColour(d) {
  const t = Math.max(0, Math.min(1, d / 255));
  if (t < 1 / 3) return [Math.round(t * 3 * 255), 0, 0];
  if (t < 2 / 3) return [255, Math.round((t - 1 / 3) * 3 * 255), 0];
  return [255, 255, Math.round((t - 2 / 3) * 3 * 255)];
}

/**
 * Compares two RGBA images (alpha ignored; `b` is resampled to `a`'s size when
 * they differ). A pixel mismatches when its largest channel difference exceeds
 * `threshold` (0..255). Returns the mismatch fraction (0..1), the mean RGB
 * Euclidean distance, the per-channel mean signed delta (b - a: which way the
 * colour is off), and a heat-map image of the per-pixel max channel delta.
 */
export function imageDiff(a, b, { threshold = 32 } = {}) {
  const resized = b.width !== a.width || b.height !== a.height;
  const bb = resized ? resizeImage(b, a.width, a.height) : b;
  const n = a.width * a.height;
  const heat = makeImage(a.width, a.height);
  let mismatched = 0;
  let distSum = 0;
  const signed = [0, 0, 0];
  for (let p = 0; p < n; p++) {
    const i = p * 4;
    const dr = bb.data[i] - a.data[i];
    const dg = bb.data[i + 1] - a.data[i + 1];
    const db = bb.data[i + 2] - a.data[i + 2];
    const m = Math.max(Math.abs(dr), Math.abs(dg), Math.abs(db));
    if (m > threshold) mismatched++;
    distSum += Math.sqrt(dr * dr + dg * dg + db * db);
    signed[0] += dr;
    signed[1] += dg;
    signed[2] += db;
    heat.data.set(heatColour(m), i);
    heat.data[i + 3] = 255;
  }
  return {
    width: a.width,
    height: a.height,
    resized,
    threshold,
    mismatch: n ? mismatched / n : 0,
    meanDelta: n ? distSum / n : 0,
    meanSignedDelta: signed.map((s) => (n ? s / n : 0)),
    heat,
  };
}

/** Mean RGB of an image (a cheap "is the whole picture the right colour" number). */
export function meanColour(img) {
  const acc = [0, 0, 0];
  const n = img.width * img.height;
  for (let p = 0; p < n; p++) for (let c = 0; c < 3; c++) acc[c] += img.data[p * 4 + c];
  return acc.map((v) => (n ? v / n : 0));
}

/** Places images left to right (top-aligned) with a `gap`-pixel separator. */
export function sideBySide(images, gap = 4, bg = [40, 40, 40, 255]) {
  const w = images.reduce((s, im) => s + im.width, 0) + gap * Math.max(0, images.length - 1);
  const h = Math.max(...images.map((im) => im.height));
  const out = makeImage(w, h, bg);
  let x0 = 0;
  for (const im of images) {
    for (let y = 0; y < im.height; y++) {
      out.data.set(im.data.subarray(y * im.width * 4, (y + 1) * im.width * 4), (y * w + x0) * 4);
    }
    x0 += im.width + gap;
  }
  return out;
}

// --- PNG codec (8-bit, non-interlaced; what Godot's save_png and Patina emit) ---

const PNG_SIG = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}

/** Encodes an RGBA8 image as a PNG Buffer. */
export function encodePng(img) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(img.width, 0);
  ihdr.writeUInt32BE(img.height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const stride = img.width * 4;
  const raw = Buffer.alloc((stride + 1) * img.height);
  for (let y = 0; y < img.height; y++) {
    raw[y * (stride + 1)] = 0;
    Buffer.from(img.data.buffer, img.data.byteOffset + y * stride, stride).copy(raw, y * (stride + 1) + 1);
  }
  return Buffer.concat([PNG_SIG, chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

/** Decodes an 8-bit PNG (gray, RGB, palette, gray+alpha, RGBA; filters 0-4) to RGBA8. */
export function decodePng(buf) {
  buf = Buffer.from(buf);
  if (buf.length < 8 || !buf.subarray(0, 8).equals(PNG_SIG)) throw new Error('not a PNG');
  let pos = 8;
  let width = 0, height = 0, depth = 0, ctype = 0, interlace = 0;
  let palette = null, trns = null;
  const idat = [];
  while (pos + 8 <= buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString('ascii', pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    pos += 12 + len;
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      depth = data[8];
      ctype = data[9];
      interlace = data[12];
    } else if (type === 'PLTE') palette = data;
    else if (type === 'tRNS') trns = data;
    else if (type === 'IDAT') idat.push(data);
    else if (type === 'IEND') break;
  }
  if (depth !== 8) throw new Error(`PNG bit depth ${depth} unsupported (8 only)`);
  if (interlace) throw new Error('interlaced PNG unsupported');
  const channels = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 }[ctype];
  if (!channels) throw new Error(`PNG colour type ${ctype} unsupported`);
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const px = Buffer.alloc(stride * height);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const f = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const cur = px.subarray(y * stride, (y + 1) * stride);
    for (let x = 0; x < stride; x++) {
      const a = x >= channels ? cur[x - channels] : 0;
      const b = prev[x];
      const c = x >= channels ? prev[x - channels] : 0;
      let v = line[x];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      } else if (f !== 0) throw new Error(`bad PNG filter ${f}`);
      cur[x] = v & 0xff;
    }
    prev = cur;
  }
  const out = makeImage(width, height);
  for (let p = 0; p < width * height; p++) {
    const s = p * channels;
    const o = p * 4;
    if (ctype === 6) out.data.set(px.subarray(s, s + 4), o);
    else if (ctype === 2) out.data.set([px[s], px[s + 1], px[s + 2], 255], o);
    else if (ctype === 0) out.data.set([px[s], px[s], px[s], 255], o);
    else if (ctype === 4) out.data.set([px[s], px[s], px[s], px[s + 1]], o);
    else {
      const i = px[s];
      out.data.set([palette[i * 3], palette[i * 3 + 1], palette[i * 3 + 2], trns && i < trns.length ? trns[i] : 255], o);
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/**
 * Applies the flag thresholds to a comparison. `traces` maps id -> diffTrace
 * result; `images` is a list of {frame, mismatch, meanDelta}. A null threshold
 * is not checked. Returns {pass, failures: [string]}.
 */
export function verdict({ traces = {}, images = [] }, { maxTraceError = null, maxMismatch = null, maxMeanDelta = null } = {}) {
  const failures = [];
  for (const [id, d] of Object.entries(traces)) {
    if (d.compared === 0) failures.push(`${id}: no frames in common`);
    else if (maxTraceError !== null && !(d.maxError <= maxTraceError)) {
      failures.push(`${id}: max error ${fmt(d.maxError)} at f${d.maxErrorFrame} > ${maxTraceError} (first divergence f${d.firstDivergence})`);
    }
  }
  for (const im of images) {
    if (maxMismatch !== null && im.mismatch * 100 > maxMismatch) failures.push(`f${im.frame}: ${fmt(im.mismatch * 100)}% pixels differ > ${maxMismatch}%`);
    if (maxMeanDelta !== null && im.meanDelta > maxMeanDelta) failures.push(`f${im.frame}: mean colour delta ${fmt(im.meanDelta)} > ${maxMeanDelta}`);
  }
  return { pass: failures.length === 0, failures };
}

export function fmt(n) {
  if (n === null || n === undefined) return '-';
  if (!Number.isFinite(n)) return String(n);
  return Math.abs(n) >= 100 ? n.toFixed(1) : Math.abs(n) >= 1 ? n.toFixed(3) : n.toPrecision(3);
}

#!/usr/bin/env node
// compare-to-godot: run one scenario in real Godot AND in Patina, frame-exact,
// and diff them (pat-xggzs).
//
//   node scripts/compare-to-godot.mjs <game_dir> [--port P] [--scene X] [--keys "..."]
//        [--watch /root/Path:prop ...] [--frames N] [--size WxH] [--out dir]
//        [--max-trace-error E] [--max-mismatch PCT] [--max-mean-delta D]
//
// Godot side: scripts/godot-reference.mjs (or --reference <dir> to reuse a
// capture). Patina side: the editor's MCP `play` tool on --port (default
// $PATINA_PORT or 8080) with the same timeline as `inputs`, one `probes` entry
// per watch every frame, and `screenshots` at the same frames. The editor must
// already have the game's project open (scripts/editor-bg.sh start ... <game>).
//
// Reports, per watched property: first divergence frame (error > --tolerance),
// max error and its frame, mean error, per-component max error; per picture:
// mismatch % (pixels whose largest channel differs by > --pixel-threshold),
// mean RGB delta, the mean signed delta per channel, and a coarse (1/8 scale)
// mismatch that ignores texture noise and aliasing. Writes to --out
// (default $TMPDIR/compare-to-godot/<game>):
//   godot/ patina/     each side's trace.json and frames/fNNNNN.png
//   diff/fNNNNN-heat.png  per-pixel max channel delta (black -> red -> yellow -> white)
//   diff/fNNNNN-side.png  Godot | Patina | heat map
//   report.json        everything above, plus both full series
//
// Exit 0 when every given threshold holds, 1 when one fails, 2 on a usage or
// setup error. With no threshold flags it only reports.
//
// Options (plus every godot-reference option):
//   --port P               Patina editor port
//   --reference DIR        reuse a godot-reference output dir instead of running Godot
//   --tolerance E          trace error that still counts as equal (default 0.01)
//   --offset N             compare Patina frame f with Godot frame f+N (default 0)
//   --pixel-threshold T    0..255 channel delta that makes a pixel differ (default 32)
//   --max-trace-error E    fail when any property's max error exceeds E
//   --max-mismatch PCT     fail when any picture's mismatch exceeds PCT percent
//   --max-mean-delta D     fail when any picture's mean RGB delta exceeds D

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  decodePng, encodePng, imageDiff, resizeImage, sideBySide, meanColour,
  diffTrace, parseMcpProbes, traceFromGodot, toMcpInputs, verdict, fmt,
} from './lib/godot-reference-lib.mjs';
import { runGodotReference, parseCommonArgs, resolveScenario } from './godot-reference.mjs';

/** The MCP `play` arguments for a scenario: same timeline, probes every frame, same picture frames. */
export function mcpPlayArgs(sc) {
  const probes = [];
  for (const w of sc.watches) {
    probes.push({ node: w.path, props: [w.prop], at: 0 });
    probes.push({ node: w.path, props: [w.prop], every: 1 });
  }
  return {
    frames: sc.frames,
    inputs: toMcpInputs(sc.timeline),
    probes,
    screenshots: sc.shots.slice(0, 8),
    width: sc.size[0],
    height: sc.size[1],
    // The seed the Godot reference was pinned to (pat-y2ylj): with Patina's
    // Godot-exact PCG32, both engines then draw the same random numbers.
    seed: sc.seed,
  };
}

/** Frame numbers of the screenshots, from the report's "screenshots: f30, f120 (WxH)" line. */
export function screenshotFrames(text) {
  const m = /^screenshots:\s*(.+?)\s*\(\d+x\d+\)/m.exec(String(text));
  if (!m) return [];
  return m[1].split(',').map((s) => Number(s.trim().replace(/^f/, ''))).filter(Number.isFinite);
}

async function mcp(port, name, args) {
  const body = JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name, arguments: args } });
  const headers = { 'Content-Type': 'application/json' };
  if (process.env.PATINA_TOKEN) headers.Authorization = `Bearer ${process.env.PATINA_TOKEN}`;
  let res;
  try {
    res = await fetch(`http://127.0.0.1:${port}/mcp`, { method: 'POST', headers, body, signal: AbortSignal.timeout(900_000) });
  } catch (e) {
    throw new Error(`cannot reach the Patina editor on port ${port} (${e.cause?.code || e.message}); start it with scripts/editor-bg.sh start --release --build --port ${port} <game_dir> <scene>`);
  }
  const reply = await res.json();
  if (reply.error) throw new Error(`MCP ${name}: ${JSON.stringify(reply.error)}`);
  const r = reply.result || {};
  const text = (r.content || []).filter((b) => b.type === 'text').map((b) => b.text).join('\n');
  const images = (r.content || []).filter((b) => b.type === 'image').map((b) => Buffer.from(b.data, 'base64'));
  return { text, images, isError: !!r.isError };
}

export async function runPatina(sc, port, outDir) {
  fs.rmSync(path.join(outDir, 'frames'), { recursive: true, force: true });
  fs.mkdirSync(path.join(outDir, 'frames'), { recursive: true });
  const open = await mcp(port, 'scene', { action: 'open', path: sc.scene });
  if (open.isError) throw new Error(`Patina could not open ${sc.scene}: ${open.text} (is the editor running ${sc.gameDir}?)`);
  const args = mcpPlayArgs(sc);
  const play = await mcp(port, 'play', args);
  fs.writeFileSync(path.join(outDir, 'play-report.txt'), play.text);
  if (play.isError) throw new Error(`Patina play failed:\n${play.text}`);
  const trace = parseMcpProbes(play.text, sc.watches);
  const frames = [];
  const shotFrames = screenshotFrames(play.text);
  play.images.forEach((png, i) => {
    const frame = shotFrames[i];
    if (frame === undefined) return;
    const p = path.join(outDir, 'frames', `f${String(frame).padStart(5, '0')}.png`);
    fs.writeFileSync(p, png);
    frames.push({ frame, png: p });
  });
  const failedShots = play.text.split('\n').filter((l) => /screenshot failed/.test(l)).map((l) => l.trim());
  const warnings = play.text.split('\n').filter((l) => l.startsWith('⚠')).map((l) => l.trim());
  const result = { engine: 'patina', args, trace, frames, failedShots, warnings, header: play.text.split('\n')[0] };
  fs.writeFileSync(path.join(outDir, 'trace.json'), JSON.stringify(result, null, 2));
  return result;
}

/** Image metrics for one frame; writes the heat map and side-by-side PNGs. */
export function compareFrame(frame, godotPng, patinaPng, diffDir, threshold) {
  const g = decodePng(fs.readFileSync(godotPng));
  const p = decodePng(fs.readFileSync(patinaPng));
  const full = imageDiff(g, p, { threshold });
  const cw = Math.max(1, Math.round(g.width / 8));
  const ch = Math.max(1, Math.round(g.height / 8));
  const coarse = imageDiff(resizeImage(g, cw, ch), resizeImage(p, cw, ch), { threshold });
  const tag = `f${String(frame).padStart(5, '0')}`;
  const heat = path.join(diffDir, `${tag}-heat.png`);
  const side = path.join(diffDir, `${tag}-side.png`);
  fs.writeFileSync(heat, encodePng(full.heat));
  const pp = full.resized ? resizeImage(p, g.width, g.height) : p;
  fs.writeFileSync(side, encodePng(sideBySide([g, pp, full.heat])));
  return {
    frame,
    godot: godotPng,
    patina: patinaPng,
    heat,
    side,
    size: { godot: [g.width, g.height], patina: [p.width, p.height] },
    mismatch: full.mismatch,
    meanDelta: full.meanDelta,
    meanSignedDelta: full.meanSignedDelta,
    coarseMismatch: coarse.mismatch,
    coarseMeanDelta: coarse.meanDelta,
    meanColour: { godot: meanColour(g), patina: meanColour(p) },
  };
}

function summarise(report) {
  const lines = [];
  lines.push(`compare-to-godot: ${report.game} ${report.scene}`);
  lines.push(`  keys: ${report.keys} (${report.frames} frames, ${report.size.join('x')})`);
  lines.push(`  godot:  ${report.godot.version} ${report.godot.rendering}`);
  lines.push(`  patina: ${report.patina.header}`);
  for (const n of report.notes) lines.push(`  note: ${n}`);
  for (const [id, d] of Object.entries(report.traces)) {
    lines.push(`  trace ${id}: ${d.compared} frames compared, first divergence ${d.firstDivergence === null ? 'none' : `f${d.firstDivergence}`} (tolerance ${report.tolerance}), max error ${fmt(d.maxError)} at f${d.maxErrorFrame}, mean ${fmt(d.meanError)}${d.componentMaxError ? `, per-axis max [${d.componentMaxError.map(fmt).join(', ')}]` : ''}`);
    if (d.missingInActual.length) lines.push(`    missing in Patina: ${d.missingInActual.length} frame(s) from f${d.missingInActual[0]}`);
    const at = new Map(d.series.map((s) => [s.frame, s]));
    const show = new Set([0, d.firstDivergence, ...report.boundaries, report.frames].filter((f) => f !== null));
    lines.push(`    ${'frame'.padEnd(7)} ${'godot'.padEnd(34)} ${'patina'.padEnd(34)} error`);
    for (const f of [...show].sort((a, b) => a - b)) {
      const s = at.get(f);
      if (!s) continue;
      const v = (x) => (x ? `[${x.map((n) => fmt(n)).join(', ')}]` : 'null');
      lines.push(`    f${String(f).padEnd(6)} ${v(s.ref).padEnd(34)} ${v(s.act).padEnd(34)} ${fmt(s.error)}`);
    }
  }
  for (const im of report.images) {
    lines.push(`  picture f${im.frame}: ${fmt(im.mismatch * 100)}% pixels differ (coarse ${fmt(im.coarseMismatch * 100)}%), mean RGB delta ${fmt(im.meanDelta)}, Patina-Godot per channel [${im.meanSignedDelta.map(fmt).join(', ')}]`);
    lines.push(`    ${im.side}`);
  }
  for (const s of report.patina.failedShots) lines.push(`  patina: ${s}`);
  if (report.verdict.failures.length) {
    lines.push('  FAIL:');
    for (const f of report.verdict.failures) lines.push(`    ${f}`);
  } else lines.push(`  ${report.thresholdsGiven ? 'PASS' : 'report only (no thresholds given)'}`);
  lines.push(`  out: ${report.outDir}`);
  return lines.join('\n');
}

async function main() {
  const num = (k) => (opts, value) => { opts[k] = Number(value()); };
  let parsed;
  try {
    parsed = parseCommonArgs(process.argv.slice(2), {
      '--port': num('port'),
      '--tolerance': num('tolerance'),
      '--offset': num('offset'),
      '--pixel-threshold': num('pixelThreshold'),
      '--max-trace-error': num('maxTraceError'),
      '--max-mismatch': num('maxMismatch'),
      '--max-mean-delta': num('maxMeanDelta'),
      '--reference': (opts, value) => { opts.reference = value(); },
    });
  } catch (e) {
    console.error(`compare-to-godot: ${e.message}`);
    process.exit(2);
  }
  const { opts, rest } = parsed;
  if (opts.help || rest.length !== 1) {
    const src = fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n');
    console.log(src.filter((l) => l.startsWith('//')).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
    process.exit(opts.help ? 0 : 2);
  }
  const gameDir = path.resolve(rest[0]);
  const port = opts.port || Number(process.env.PATINA_PORT) || 8080;
  const outDir = path.resolve(opts.out || path.join(os.tmpdir(), 'compare-to-godot', path.basename(gameDir)));
  const diffDir = path.join(outDir, 'diff');
  fs.rmSync(diffDir, { recursive: true, force: true });
  fs.mkdirSync(diffDir, { recursive: true });

  // Godot first: a reference capture fixes the scenario (frames, shots, size).
  let ref;
  if (opts.reference) {
    const dir = path.resolve(opts.reference);
    const trace = JSON.parse(fs.readFileSync(path.join(dir, 'trace.json'), 'utf8'));
    const frames = fs.readdirSync(path.join(dir, 'frames')).map((f) => /^f(\d+)\.png$/.exec(f)).filter(Boolean)
      .map((m) => ({ frame: Number(m[1]), png: path.join(dir, 'frames', m[0]) }));
    ref = { outDir: dir, trace, frames, notes: trace.meta.notes || [] };
  } else {
    ref = runGodotReference(gameDir, { ...opts, out: path.join(outDir, 'godot') });
  }
  const m = ref.trace.meta;
  const sc = resolveScenario(gameDir, { scene: m.scene, keys: m.keys, watch: m.watches.map((w) => w.id), frames: m.frames, size: m.size, shots: m.shots, fps: m.fps, seed: m.seed });

  const patina = await runPatina(sc, port, path.join(outDir, 'patina'));
  const godotTrace = traceFromGodot(ref.trace, sc.watches);
  const tolerance = opts.tolerance ?? 0.01;
  const traces = {};
  for (const w of sc.watches) traces[w.id] = diffTrace(godotTrace[w.id], patina.trace[w.id], { tolerance, offset: opts.offset || 0 });

  const threshold = opts.pixelThreshold ?? 32;
  const pFrames = new Map(patina.frames.map((f) => [f.frame, f.png]));
  const images = [];
  for (const g of ref.frames) {
    if (pFrames.has(g.frame)) images.push(compareFrame(g.frame, g.png, pFrames.get(g.frame), diffDir, threshold));
  }
  const thresholds = { maxTraceError: opts.maxTraceError ?? null, maxMismatch: opts.maxMismatch ?? null, maxMeanDelta: opts.maxMeanDelta ?? null };
  const v = verdict({ traces, images }, thresholds);
  if ((thresholds.maxMismatch !== null || thresholds.maxMeanDelta !== null) && images.length === 0) {
    v.pass = false;
    v.failures.push(`no picture to compare (Godot frames: ${ref.frames.map((f) => f.frame).join(',') || 'none'}; Patina: ${patina.frames.map((f) => f.frame).join(',') || 'none'}${patina.failedShots.length ? `; ${patina.failedShots[0]}` : ''})`);
  }
  const report = {
    game: path.relative(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'), gameDir),
    scene: sc.scene,
    keys: sc.keys,
    frames: sc.frames,
    size: sc.size,
    boundaries: sc.timeline.boundaries.map((b) => b.frame),
    tolerance,
    pixelThreshold: threshold,
    thresholds,
    thresholdsGiven: Object.values(thresholds).some((x) => x !== null),
    notes: ref.notes,
    godot: { dir: ref.outDir, version: ref.trace.version, rendering: `${ref.trace.rendering_method}/${ref.trace.rendering_driver}` },
    patina: { header: patina.header, failedShots: patina.failedShots, warnings: patina.warnings },
    traces,
    images,
    verdict: v,
    outDir,
  };
  fs.writeFileSync(path.join(outDir, 'report.json'), JSON.stringify(report, null, 2));
  if (opts.json) console.log(JSON.stringify({ ...report, traces: Object.fromEntries(Object.entries(traces).map(([k, d]) => [k, { ...d, series: undefined }])) }, null, 2));
  else console.log(summarise(report));
  process.exit(v.pass ? 0 : 1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`compare-to-godot: ${e.message}`);
    process.exit(2);
  });
}

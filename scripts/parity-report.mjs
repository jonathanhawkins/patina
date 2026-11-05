#!/usr/bin/env node
// parity-report: write .playtest-gate/parity.md from the last
// `node scripts/playtest-gate.mjs --godot` run (.playtest-gate/latest.json)
// and the recorded baselines (scripts/godot-parity-baselines.json).
//
//   node scripts/parity-report.mjs [--latest FILE] [--baselines FILE] [--out FILE] [--print]
//
// Per game: status, every regression and improvement against the baseline,
// the trace scores (first divergence, max error) and the picture scores
// (mismatch %, mean RGB delta), base -> now with a trend, and the
// Godot | Patina | heat side-by-side PNG path. The gate writes it itself after
// a --godot run; this regenerates it (e.g. after re-recording baselines).

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { renderParityReport } from './lib/godot-parity.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const PARITY_BASELINES = path.join(ROOT, 'scripts', 'godot-parity-baselines.json');
export const PARITY_MD = path.join(ROOT, '.playtest-gate', 'parity.md');

export function readBaselines(file = PARITY_BASELINES) {
  try {
    const all = JSON.parse(fs.readFileSync(file, 'utf8'));
    return all.games || {};
  } catch {
    return {};
  }
}

export function writeParityReport({ latest, baselines, out = PARITY_MD }) {
  const md = renderParityReport({ at: latest.at, results: latest.results || [], baselines });
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, md + '\n');
  return { out, md };
}

function main() {
  const a = process.argv.slice(2);
  const get = (k, d) => (a.includes(k) ? a[a.indexOf(k) + 1] : d);
  if (a.includes('-h') || a.includes('--help')) {
    console.log(fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1).filter((l) => l.startsWith('//')).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
    return;
  }
  const latestFile = get('--latest', path.join(ROOT, '.playtest-gate', 'latest.json'));
  let latest;
  try {
    latest = JSON.parse(fs.readFileSync(latestFile, 'utf8'));
  } catch (e) {
    console.error(`parity-report: cannot read ${latestFile} (${e.message}); run node scripts/playtest-gate.mjs --godot first`);
    process.exit(2);
  }
  const { out, md } = writeParityReport({ latest, baselines: readBaselines(get('--baselines', PARITY_BASELINES)), out: get('--out', PARITY_MD) });
  if (a.includes('--print')) console.log(md);
  else console.log(`parity-report: wrote ${out}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();

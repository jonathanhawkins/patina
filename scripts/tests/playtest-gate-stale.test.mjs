// The gate must never measure (or record baselines from) an editor binary
// built before the current engine sources. On 2026-11-05 a pre-merge binary
// re-recorded every game's Godot-parity baseline with the OLD engine's numbers.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseArgs, newestEngineSourceMtime, staleBinaryMessage } from '../playtest-gate.mjs';

test('both baseline-update flags rebuild the editor first', () => {
  assert.equal(parseArgs(['--update-godot-baselines']).build, true);
  assert.equal(parseArgs(['--update-baselines']).build, true);
  assert.equal(parseArgs(['--godot-only']).build, false);
  assert.equal(parseArgs(['--allow-stale']).allowStale, true);
});

function engineTree() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'gate-stale-'));
  const src = path.join(dir, 'crates/gdfoo/src/deep');
  fs.mkdirSync(src, { recursive: true });
  fs.mkdirSync(path.join(dir, 'crates/gdfoo/tests'), { recursive: true });
  fs.mkdirSync(path.join(dir, 'examples'), { recursive: true });
  const at = (f, sec) => { fs.writeFileSync(f, 'x'); fs.utimesSync(f, sec, sec); };
  at(path.join(src, 'lib.rs'), 1000);
  at(path.join(dir, 'crates/gdfoo/Cargo.toml'), 900);
  at(path.join(dir, 'examples/editor.rs'), 1100);
  at(path.join(dir, 'crates/gdfoo/tests/t.rs'), 9999); // tests don't count
  return { dir, at };
}

test('newest engine source counts src, examples and Cargo.toml, not tests', () => {
  const { dir, at } = engineTree();
  try {
    assert.equal(newestEngineSourceMtime(dir), 1100 * 1000);
    at(path.join(dir, 'crates/gdfoo/src/deep/lib.rs'), 2000);
    assert.equal(newestEngineSourceMtime(dir), 2000 * 1000);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('a binary older than the sources is stale; a newer one is not', () => {
  const { dir } = engineTree();
  try {
    const bin = path.join(dir, 'editor');
    fs.writeFileSync(bin, 'bin');
    fs.utimesSync(bin, 1050, 1050);
    const newest = newestEngineSourceMtime(dir);
    assert.match(staleBinaryMessage(bin, newest), /older than the newest engine source/);
    fs.utimesSync(bin, 1200, 1200);
    assert.equal(staleBinaryMessage(bin, newest), null);
    assert.match(staleBinaryMessage(path.join(dir, 'missing'), newest), /does not exist/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

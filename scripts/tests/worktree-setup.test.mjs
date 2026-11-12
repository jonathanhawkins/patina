// scripts/worktree-setup.sh: a fresh agent worktree gets the main checkout's
// untracked test inputs (upstream/godot docs, the gitignored extension API
// JSON) as links, idempotently, and the main checkout itself is left alone.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const script = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'worktree-setup.sh');
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8' });
const run = (...args) => execFileSync('bash', [script, ...args], { encoding: 'utf8' });

function repoWithWorktree() {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'wt-setup-')));
  const main = path.join(root, 'main');
  fs.mkdirSync(main);
  git(main, 'init', '-q', '-b', 'main');
  git(main, 'config', 'user.email', 't@example.com');
  git(main, 'config', 'user.name', 't');
  fs.writeFileSync(path.join(main, '.gitignore'), 'upstream/\nengine-rs/fixtures/editor_oracle/godot_extension_api.json\n');
  fs.writeFileSync(path.join(main, 'README'), 'x\n');
  git(main, 'add', '.');
  git(main, 'commit', '-q', '-m', 'init');
  // The inputs exist only in the main checkout (untracked/ignored there).
  fs.mkdirSync(path.join(main, 'upstream/godot/doc/classes'), { recursive: true });
  fs.writeFileSync(path.join(main, 'upstream/godot/doc/classes/Node.xml'), '<class name="Node"/>');
  fs.mkdirSync(path.join(main, 'engine-rs/fixtures/editor_oracle'), { recursive: true });
  fs.writeFileSync(path.join(main, 'engine-rs/fixtures/editor_oracle/godot_extension_api.json'), '{"ok":1}');
  const wt = path.join(root, 'wt');
  git(main, 'worktree', 'add', '-q', '-b', 'agent', wt);
  return { root, main, wt };
}

test('links the inputs into a worktree, idempotently, with git status clean', () => {
  const { root, wt } = repoWithWorktree();
  try {
    assert.equal(fs.existsSync(path.join(wt, 'upstream/godot/doc/classes/Node.xml')), false);
    const first = run(wt);
    assert.match(first, /linked the contents of upstream\/godot/);
    assert.match(first, /linked engine-rs\/fixtures\/editor_oracle\/godot_extension_api\.json/);
    assert.equal(
      fs.readFileSync(path.join(wt, 'upstream/godot/doc/classes/Node.xml'), 'utf8'),
      '<class name="Node"/>',
    );
    assert.equal(
      fs.readFileSync(path.join(wt, 'engine-rs/fixtures/editor_oracle/godot_extension_api.json'), 'utf8'),
      '{"ok":1}',
    );
    const second = run(wt);
    assert.match(second, /upstream\/godot already populated/);
    assert.match(second, /godot_extension_api\.json already present/);
    assert.equal(git(wt, 'status', '--porcelain'), '');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('--base moves a clean worktree branch, and refuses a dirty one', () => {
  const { root, main, wt } = repoWithWorktree();
  try {
    fs.writeFileSync(path.join(main, 'NEW'), 'n\n');
    git(main, 'add', 'NEW');
    git(main, 'commit', '-q', '-m', 'newer base');
    run('--base', 'main', wt);
    assert.equal(fs.existsSync(path.join(wt, 'NEW')), true);
    assert.equal(git(wt, 'rev-parse', '--abbrev-ref', 'HEAD').trim(), 'agent');

    fs.writeFileSync(path.join(wt, 'README'), 'dirty\n');
    assert.throws(() => run('--base', 'main', wt), /uncommitted changes/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('does nothing in the main checkout', () => {
  const { root, main } = repoWithWorktree();
  try {
    assert.match(run(main), /is the main checkout; nothing to do/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

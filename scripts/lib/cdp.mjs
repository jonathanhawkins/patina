// A throwaway headless Chrome and a DevTools-protocol tab, with no npm
// packages (Node 22+ has WebSocket built in). Same mechanics as
// browser-playtest's own copy: a private profile on a free port, and every
// DevTools call has a deadline so one unanswered call cannot hang a run.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function findChrome() {
  if (process.env.CHROME) return process.env.CHROME;
  const mac = [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary',
  ];
  for (const c of mac) if (fs.existsSync(c)) return c;
  for (const c of ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser']) {
    try {
      return execFileSync('which', [c], { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
    } catch {}
  }
  return null;
}

/// Starts a headless Chrome; resolves { port, cleanup }. Throws if none.
export async function launchChrome() {
  const bin = findChrome();
  if (!bin) throw new Error('no Chrome found: set CHROME=/path/to/chrome, or pass --cdp <port>');
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'patina-tour-chrome-'));
  const child = spawn(bin, [
    '--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check',
    '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows', 'about:blank',
  ], { stdio: 'ignore' });
  const cleanup = () => {
    try { child.kill('SIGTERM'); } catch {}
    try { fs.rmSync(profile, { recursive: true, force: true }); } catch {}
  };
  const portFile = path.join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 200; i++) {
    if (fs.existsSync(portFile)) {
      const port = fs.readFileSync(portFile, 'utf8').split('\n')[0].trim();
      if (port) return { port, cleanup };
    }
    if (child.exitCode !== null) break;
    await sleep(100);
  }
  cleanup();
  throw new Error(`Chrome (${bin}) did not open a DevTools port`);
}

/// Opens a tab; returns { send, evaluate, on, close }.
export async function openTab(port) {
  const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', reject, { once: true });
  });
  let nextId = 1;
  const pending = new Map();
  const listeners = [];
  ws.addEventListener('message', (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    } else if (msg.method) {
      for (const l of listeners) l(msg);
    }
  });
  const send = (method, params = {}, timeoutMs = 30000) => new Promise((resolve, reject) => {
    const id = nextId++;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Chrome did not answer ${method} within ${timeoutMs / 1000}s`));
    }, timeoutMs);
    pending.set(id, (msg) => { clearTimeout(timer); resolve(msg); });
    ws.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text);
    return r.result?.result?.value;
  };
  const close = async () => {
    try { ws.close(); } catch {}
    try { await fetch(`http://127.0.0.1:${port}/json/close/${target.id}`); } catch {}
  };
  return { send, evaluate, on: (fn) => listeners.push(fn), close };
}

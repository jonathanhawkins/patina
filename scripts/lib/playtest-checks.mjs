// Pure checks behind browser-playtest, kept here so node --test can drive
// them with synthetic data (browser-playtest itself runs on import).

/// Editor origin axes drawn over a frame — the 3D Play view rendering the
/// EDITOR viewport (grid, axes, gizmos) instead of the game camera.
///
/// `data` is RGBA (ImageData.data or a Uint8Array), `w`x`h`. A pixel counts
/// as a thin line of a colour when it is strongly red (r - max(g,b)) or blue
/// (b - max(r,g)) and the same colour fades away 3 px to both sides, vertically
/// or horizontally: a 1-2 px line, not the inside of a red sprite. Then a
/// coarse Hough vote finds the straightest run of those pixels.
///
/// An editor axis is ONE long, OBLIQUE straight line: measured on
/// platformer_3d's Play frame (995x807) the red X axis held 776 collinear thin
/// pixels at 108 degrees — 78% of the width and 69% of all thin red pixels.
/// The 2D games' Play frames had at most 146 collinear thin red pixels
/// (platformer's brick rows: 23% of the width but under 2% of its 9319 thin
/// red pixels), and spark_keeper's blue-violet Line2D border made a 1798-pixel
/// line — exactly horizontal/vertical, which is why axis-aligned lines are
/// skipped. So a line must be oblique, reach 30% of the frame's long side AND
/// hold 25% of the colour's thin pixels.
///
/// ⚠️ Self-contained on purpose: browser-playtest ships its source into the
/// page (`fn.toString()`), so it may not reference anything outside itself.
export function editorAxisLines(data, w, h) {
  const D = 3, T = 50;
  const score = [
    (i) => data[i] - Math.max(data[i + 1], data[i + 2]),
    (i) => data[i + 2] - Math.max(data[i], data[i + 1]),
  ];
  const out = {};
  ['red', 'blue'].forEach((name, k) => {
    const f = score[k];
    const pts = [];
    for (let y = D; y < h - D; y++) {
      for (let x = D; x < w - D; x++) {
        const i = (y * w + x) * 4;
        const s = f(i);
        if (s < T) continue;
        const up = f(i - D * w * 4), down = f(i + D * w * 4), left = f(i - D * 4), right = f(i + D * 4);
        if ((up < s - T / 2 && down < s - T / 2) || (left < s - T / 2 && right < s - T / 2)) pts.push(x, y);
      }
    }
    const n = pts.length / 2;
    // Subsample a huge set: the vote only needs the shape.
    const step = Math.max(1, Math.ceil(n / 20000));
    let best = 0, bestAngle = 0;
    for (let a = 0; a < 180; a += 1) {
      // Axis-aligned straight lines are 2D art and UI (a Line2D border, brick
      // rows, a HUD bar); an editor axis seen through a perspective camera is
      // oblique. Skip within 5 degrees of horizontal/vertical.
      if (Math.min(a % 90, 90 - (a % 90)) < 5) continue;
      const t = (a * Math.PI) / 180, c = Math.cos(t), s = Math.sin(t);
      const acc = new Map();
      for (let j = 0; j < n; j += step) {
        const r = Math.round((pts[2 * j] * c + pts[2 * j + 1] * s) / 2);
        const v = (acc.get(r) || 0) + step;
        acc.set(r, v);
        if (v > best) { best = v; bestAngle = a; }
      }
    }
    out[name] = { thin: n, line: best, angle: bestAngle };
  });
  const long = Math.max(w, h);
  const isAxis = (c) => c.line >= 0.3 * long && c.line >= 0.25 * c.thin;
  out.overlay = isAxis(out.red) || isAxis(out.blue);
  return out;
}

/// What the watched values say, as { problems, warnings }. A value that never
/// changed (or never was readable) is a warning — unless the scenario expects
/// the game to react (`expectMove`), then the controls not reaching the game
/// IS the failure.
export function watchVerdict(watches, expectMove = false) {
  const problems = [], warnings = [];
  for (const w of watches || []) {
    const readable = w.values.map((v) => v.value).filter((v) => !String(v).startsWith('('));
    let msg = null;
    if (readable.length >= 2 && new Set(readable).size === 1) {
      msg = `${w.watch} stayed ${readable[0]} through every step: do the controls reach the game?`;
    } else if (!readable.length) {
      msg = `${w.watch} was never readable: ${w.values[0]?.value ?? 'no steps ran'}`;
    }
    if (msg) (expectMove ? problems : warnings).push(msg);
  }
  return { problems, warnings };
}

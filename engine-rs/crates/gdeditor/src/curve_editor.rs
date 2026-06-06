//! Curve editor for animation keyframe interpolation.
//!
//! Provides data structures and preset curves for editing cubic Bézier
//! easing functions on animation keyframes, as well as the HTML/JS/CSS
//! fragment injected into the editor UI.

use gdscene::animation::TransitionType;

// ---------------------------------------------------------------------------
// CurvePreset
// ---------------------------------------------------------------------------

/// A named preset curve with its Bézier control points.
#[derive(Debug, Clone)]
pub struct CurvePreset {
    pub name: &'static str,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl CurvePreset {
    /// Converts this preset into a [`TransitionType`].
    pub fn to_transition(&self) -> TransitionType {
        TransitionType::CubicBezier(self.x1, self.y1, self.x2, self.y2)
    }
}

/// Built-in curve presets matching common easing functions.
pub const CURVE_PRESETS: &[CurvePreset] = &[
    CurvePreset {
        name: "Linear",
        x1: 0.0,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    },
    CurvePreset {
        name: "Ease In",
        x1: 0.42,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    },
    CurvePreset {
        name: "Ease Out",
        x1: 0.0,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    },
    CurvePreset {
        name: "Ease In-Out",
        x1: 0.42,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    },
    CurvePreset {
        name: "Ease In Quad",
        x1: 0.55,
        y1: 0.085,
        x2: 0.68,
        y2: 0.53,
    },
    CurvePreset {
        name: "Ease Out Quad",
        x1: 0.25,
        y1: 0.46,
        x2: 0.45,
        y2: 0.94,
    },
    CurvePreset {
        name: "Ease In-Out Quad",
        x1: 0.455,
        y1: 0.03,
        x2: 0.515,
        y2: 0.955,
    },
    CurvePreset {
        name: "Ease In Cubic",
        x1: 0.55,
        y1: 0.055,
        x2: 0.675,
        y2: 0.19,
    },
    CurvePreset {
        name: "Ease Out Cubic",
        x1: 0.215,
        y1: 0.61,
        x2: 0.355,
        y2: 1.0,
    },
    CurvePreset {
        name: "Ease In-Out Cubic",
        x1: 0.645,
        y1: 0.045,
        x2: 0.355,
        y2: 1.0,
    },
    CurvePreset {
        name: "Ease In Back",
        x1: 0.6,
        y1: -0.28,
        x2: 0.735,
        y2: 0.045,
    },
    CurvePreset {
        name: "Ease Out Back",
        x1: 0.175,
        y1: 0.885,
        x2: 0.32,
        y2: 1.275,
    },
    CurvePreset {
        name: "Ease Out Bounce",
        x1: 0.0,
        y1: 0.0,
        x2: 0.2,
        y2: 1.0,
    },
];

// ---------------------------------------------------------------------------
// CurveEditor state
// ---------------------------------------------------------------------------

/// Editor-side state for the curve editor popup.
#[derive(Debug, Clone)]
pub struct CurveEditor {
    /// Which animation this edit targets.
    pub animation_name: String,
    /// Track index.
    pub track_index: usize,
    /// Keyframe index (the *destination* keyframe whose transition is edited).
    pub keyframe_index: usize,
    /// Current control points.
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl CurveEditor {
    /// Create a new curve editor state from existing control points.
    pub fn new(
        animation_name: String,
        track_index: usize,
        keyframe_index: usize,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> Self {
        Self {
            animation_name,
            track_index,
            keyframe_index,
            x1,
            y1,
            x2,
            y2,
        }
    }

    /// Apply a preset to this editor.
    pub fn apply_preset(&mut self, preset: &CurvePreset) {
        self.x1 = preset.x1;
        self.y1 = preset.y1;
        self.x2 = preset.x2;
        self.y2 = preset.y2;
    }

    /// Returns the transition type for the current control points.
    pub fn to_transition(&self) -> TransitionType {
        TransitionType::CubicBezier(self.x1, self.y1, self.x2, self.y2)
    }
}

// ---------------------------------------------------------------------------
// HTML/CSS/JS fragment
// ---------------------------------------------------------------------------

/// CSS for the curve editor popup, injected into the editor <style> block.
pub const CURVE_EDITOR_CSS: &str = r#"
/* Curve editor popup */
.curve-editor-overlay {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.4); z-index: 500; align-items: center; justify-content: center;
}
.curve-editor-overlay.open { display: flex; }
.curve-editor-popup {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  padding: 12px; width: 340px; box-shadow: 0 8px 24px rgba(0,0,0,0.6);
}
.curve-editor-popup h3 { margin: 0 0 8px; font-size: 13px; color: var(--accent); }
.curve-canvas-wrap { position: relative; width: 200px; height: 200px; margin: 0 auto 8px; }
#curve-editor-canvas {
  display: block; width: 200px; height: 200px; border: 1px solid var(--border);
  border-radius: 3px; cursor: crosshair; background: var(--bg);
}
.curve-presets { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 8px; }
.curve-presets button { font-size: 10px; padding: 2px 6px; }
.curve-inputs { display: flex; gap: 6px; margin-bottom: 8px; align-items: center; }
.curve-inputs label { font-size: 11px; color: var(--text-dim); }
.curve-inputs input { width: 50px; font-size: 11px; }
.curve-actions { display: flex; gap: 6px; justify-content: flex-end; }
"#;

/// JavaScript for the curve editor, injected into the editor <script> block.
pub const CURVE_EDITOR_JS: &str = r##"
  // ---- Curve editor (bezier) ----
  var curveEditorState = null; // { animation, trackIdx, kfIdx, x1, y1, x2, y2 }
  var curveEditorDrag = null;  // 'cp1' | 'cp2' | null

  var CURVE_PRESETS = [
    { name: 'Linear', x1: 0, y1: 0, x2: 1, y2: 1 },
    { name: 'Ease In', x1: 0.42, y1: 0, x2: 1, y2: 1 },
    { name: 'Ease Out', x1: 0, y1: 0, x2: 0.58, y2: 1 },
    { name: 'Ease In-Out', x1: 0.42, y1: 0, x2: 0.58, y2: 1 },
    { name: 'Ease In Quad', x1: 0.55, y1: 0.085, x2: 0.68, y2: 0.53 },
    { name: 'Ease Out Quad', x1: 0.25, y1: 0.46, x2: 0.45, y2: 0.94 },
    { name: 'Ease In-Out Quad', x1: 0.455, y1: 0.03, x2: 0.515, y2: 0.955 },
    { name: 'Ease In Back', x1: 0.6, y1: -0.28, x2: 0.735, y2: 0.045 },
    { name: 'Ease Out Back', x1: 0.175, y1: 0.885, x2: 0.32, y2: 1.275 },
  ];

  function openCurveEditor(animation, trackIdx, kfIdx, x1, y1, x2, y2) {
    curveEditorState = { animation: animation, trackIdx: trackIdx, kfIdx: kfIdx, x1: x1, y1: y1, x2: x2, y2: y2 };
    updateCurveInputs();
    renderCurveCanvas();
    document.getElementById('curve-editor-overlay').classList.add('open');
  }

  function closeCurveEditor() {
    document.getElementById('curve-editor-overlay').classList.remove('open');
    curveEditorState = null;
  }

  function updateCurveInputs() {
    if (!curveEditorState) return;
    document.getElementById('curve-x1').value = curveEditorState.x1.toFixed(3);
    document.getElementById('curve-y1').value = curveEditorState.y1.toFixed(3);
    document.getElementById('curve-x2').value = curveEditorState.x2.toFixed(3);
    document.getElementById('curve-y2').value = curveEditorState.y2.toFixed(3);
  }

  function readCurveInputs() {
    if (!curveEditorState) return;
    curveEditorState.x1 = Math.min(1, Math.max(0, parseFloat(document.getElementById('curve-x1').value) || 0));
    curveEditorState.y1 = parseFloat(document.getElementById('curve-y1').value) || 0;
    curveEditorState.x2 = Math.min(1, Math.max(0, parseFloat(document.getElementById('curve-x2').value) || 0));
    curveEditorState.y2 = parseFloat(document.getElementById('curve-y2').value) || 0;
    renderCurveCanvas();
  }

  function applyCurvePreset(preset) {
    if (!curveEditorState) return;
    curveEditorState.x1 = preset.x1;
    curveEditorState.y1 = preset.y1;
    curveEditorState.x2 = preset.x2;
    curveEditorState.y2 = preset.y2;
    updateCurveInputs();
    renderCurveCanvas();
  }

  function renderCurveCanvas() {
    var canvas = document.getElementById('curve-editor-canvas');
    if (!canvas || !curveEditorState) return;
    var ctx = canvas.getContext('2d');
    var w = canvas.width, h = canvas.height;
    var pad = 16; // padding
    var gw = w - pad * 2, gh = h - pad * 2;

    // Map unit coords to canvas
    function toX(v) { return pad + v * gw; }
    function toY(v) { return pad + (1 - v) * gh; } // y-flipped

    ctx.clearRect(0, 0, w, h);

    // Grid
    ctx.strokeStyle = 'rgba(255,255,255,0.06)';
    ctx.lineWidth = 1;
    for (var i = 0; i <= 4; i++) {
      var frac = i / 4;
      ctx.beginPath(); ctx.moveTo(toX(frac), pad); ctx.lineTo(toX(frac), pad + gh); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(pad, toY(frac)); ctx.lineTo(pad + gw, toY(frac)); ctx.stroke();
    }

    // Diagonal reference (linear)
    ctx.strokeStyle = 'rgba(255,255,255,0.15)';
    ctx.setLineDash([4, 4]);
    ctx.beginPath(); ctx.moveTo(toX(0), toY(0)); ctx.lineTo(toX(1), toY(1)); ctx.stroke();
    ctx.setLineDash([]);

    // Bézier curve
    var s = curveEditorState;
    ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#d4a574';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(toX(0), toY(0));
    ctx.bezierCurveTo(toX(s.x1), toY(s.y1), toX(s.x2), toY(s.y2), toX(1), toY(1));
    ctx.stroke();

    // Control point handles (lines from endpoints to control points)
    ctx.strokeStyle = 'rgba(255,255,255,0.3)';
    ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(toX(0), toY(0)); ctx.lineTo(toX(s.x1), toY(s.y1)); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(toX(1), toY(1)); ctx.lineTo(toX(s.x2), toY(s.y2)); ctx.stroke();

    // Control points
    ctx.fillStyle = '#e05050';
    ctx.beginPath(); ctx.arc(toX(s.x1), toY(s.y1), 5, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = '#50c878';
    ctx.beginPath(); ctx.arc(toX(s.x2), toY(s.y2), 5, 0, Math.PI * 2); ctx.fill();

    // Endpoints
    ctx.fillStyle = 'rgba(255,255,255,0.5)';
    ctx.beginPath(); ctx.arc(toX(0), toY(0), 3, 0, Math.PI * 2); ctx.fill();
    ctx.beginPath(); ctx.arc(toX(1), toY(1), 3, 0, Math.PI * 2); ctx.fill();
  }

  function setupCurveEditor() {
    var canvas = document.getElementById('curve-editor-canvas');
    if (!canvas) return;

    var pad = 16;
    function fromCanvas(cx, cy) {
      var gw = canvas.width - pad * 2, gh = canvas.height - pad * 2;
      return { x: Math.min(1, Math.max(0, (cx - pad) / gw)), y: 1 - (cy - pad) / gh };
    }

    canvas.addEventListener('mousedown', function(e) {
      if (!curveEditorState) return;
      var rect = canvas.getBoundingClientRect();
      var scale = canvas.width / rect.width;
      var pos = fromCanvas((e.clientX - rect.left) * scale, (e.clientY - rect.top) * scale);
      var s = curveEditorState;
      var d1 = Math.hypot(pos.x - s.x1, pos.y - s.y1);
      var d2 = Math.hypot(pos.x - s.x2, pos.y - s.y2);
      if (d1 < 0.08 || d1 < d2) { curveEditorDrag = 'cp1'; }
      else { curveEditorDrag = 'cp2'; }
    });

    canvas.addEventListener('mousemove', function(e) {
      if (!curveEditorDrag || !curveEditorState) return;
      var rect = canvas.getBoundingClientRect();
      var scale = canvas.width / rect.width;
      var pos = fromCanvas((e.clientX - rect.left) * scale, (e.clientY - rect.top) * scale);
      if (curveEditorDrag === 'cp1') {
        curveEditorState.x1 = pos.x;
        curveEditorState.y1 = pos.y;
      } else {
        curveEditorState.x2 = pos.x;
        curveEditorState.y2 = pos.y;
      }
      updateCurveInputs();
      renderCurveCanvas();
    });

    window.addEventListener('mouseup', function() { curveEditorDrag = null; });

    // Preset buttons
    var presetsDiv = document.getElementById('curve-presets');
    if (presetsDiv) {
      CURVE_PRESETS.forEach(function(p) {
        var btn = document.createElement('button');
        btn.textContent = p.name;
        btn.addEventListener('click', function() { applyCurvePreset(p); });
        presetsDiv.appendChild(btn);
      });
    }

    // Input change handlers
    ['curve-x1','curve-y1','curve-x2','curve-y2'].forEach(function(id) {
      var el = document.getElementById(id);
      if (el) el.addEventListener('input', readCurveInputs);
    });

    // Apply button
    var applyBtn = document.getElementById('curve-apply-btn');
    if (applyBtn) {
      applyBtn.addEventListener('click', async function() {
        if (!curveEditorState) return;
        var s = curveEditorState;
        await api('POST', '/api/animation/keyframe/transition', {
          animation: s.animation, track_index: s.trackIdx, keyframe_index: s.kfIdx,
          transition: 'cubic_bezier', x1: s.x1, y1: s.y1, x2: s.x2, y2: s.y2
        });
        await loadAnimation(s.animation);
        closeCurveEditor();
      });
    }

    // Cancel button
    var cancelBtn = document.getElementById('curve-cancel-btn');
    if (cancelBtn) cancelBtn.addEventListener('click', closeCurveEditor);

    // Overlay click to close
    var overlay = document.getElementById('curve-editor-overlay');
    if (overlay) overlay.addEventListener('click', function(e) { if (e.target === overlay) closeCurveEditor(); });
  }
"##;

/// HTML fragment for the curve editor popup overlay.
pub const CURVE_EDITOR_HTML: &str = r#"
<div id="curve-editor-overlay" class="curve-editor-overlay">
  <div class="curve-editor-popup">
    <h3>Curve Editor</h3>
    <div class="curve-canvas-wrap">
      <canvas id="curve-editor-canvas" width="200" height="200"></canvas>
    </div>
    <div id="curve-presets" class="curve-presets"></div>
    <div class="curve-inputs">
      <label>x1</label><input id="curve-x1" type="number" step="0.01" min="0" max="1">
      <label>y1</label><input id="curve-y1" type="number" step="0.01">
      <label>x2</label><input id="curve-x2" type="number" step="0.01" min="0" max="1">
      <label>y2</label><input id="curve-y2" type="number" step="0.01">
    </div>
    <div class="curve-actions">
      <button id="curve-cancel-btn">Cancel</button>
      <button id="curve-apply-btn" style="border-color:var(--accent);color:var(--accent)">Apply</button>
    </div>
  </div>
</div>
"#;

// ---------------------------------------------------------------------------
// Control point manipulation and curve evaluation
// ---------------------------------------------------------------------------

/// Which control point is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPoint {
    /// First control point (near start, affects ease-in).
    CP1,
    /// Second control point (near end, affects ease-out).
    CP2,
}

/// Result of a control point hit test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlPointHit {
    pub point: ControlPoint,
}

impl CurveEditor {
    /// Hit-tests a point in normalized [0,1] canvas space against the control points.
    /// `threshold` is the hit radius in normalized space.
    pub fn hit_test_control_point(
        &self,
        x: f32,
        y: f32,
        threshold: f32,
    ) -> Option<ControlPointHit> {
        let d1 = ((x - self.x1).powi(2) + (y - self.y1).powi(2)).sqrt();
        let d2 = ((x - self.x2).powi(2) + (y - self.y2).powi(2)).sqrt();

        if d1 <= threshold && d1 <= d2 {
            Some(ControlPointHit {
                point: ControlPoint::CP1,
            })
        } else if d2 <= threshold {
            Some(ControlPointHit {
                point: ControlPoint::CP2,
            })
        } else {
            None
        }
    }

    /// Moves a control point to a new position.
    /// x values are clamped to [0, 1]; y values are unclamped (allows overshoot).
    pub fn move_control_point(&mut self, cp: ControlPoint, x: f32, y: f32) {
        let clamped_x = x.clamp(0.0, 1.0);
        match cp {
            ControlPoint::CP1 => {
                self.x1 = clamped_x;
                self.y1 = y;
            }
            ControlPoint::CP2 => {
                self.x2 = clamped_x;
                self.y2 = y;
            }
        }
    }

    /// Returns the control point coordinates for the given point.
    pub fn control_point(&self, cp: ControlPoint) -> (f32, f32) {
        match cp {
            ControlPoint::CP1 => (self.x1, self.y1),
            ControlPoint::CP2 => (self.x2, self.y2),
        }
    }

    /// Resets the curve to linear (diagonal).
    pub fn reset_to_linear(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
        self.x2 = 1.0;
        self.y2 = 1.0;
    }

    /// Returns true if the curve is effectively linear.
    pub fn is_linear(&self) -> bool {
        (self.x1 - 0.0).abs() < 1e-4
            && (self.y1 - 0.0).abs() < 1e-4
            && (self.x2 - 1.0).abs() < 1e-4
            && (self.y2 - 1.0).abs() < 1e-4
    }

    /// Mirrors the curve (swaps start/end behavior).
    pub fn mirror(&mut self) {
        let (ox1, oy1, ox2, oy2) = (self.x1, self.y1, self.x2, self.y2);
        self.x1 = 1.0 - ox2;
        self.y1 = 1.0 - oy2;
        self.x2 = 1.0 - ox1;
        self.y2 = 1.0 - oy1;
    }

    /// Samples the cubic bezier curve at parameter `t` in [0, 1].
    /// Returns the (x, y) point on the curve.
    pub fn sample_at(&self, t: f32) -> (f32, f32) {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        let x = mt3 * 0.0 + 3.0 * mt2 * t * self.x1 + 3.0 * mt * t2 * self.x2 + t3 * 1.0;
        let y = mt3 * 0.0 + 3.0 * mt2 * t * self.y1 + 3.0 * mt * t2 * self.y2 + t3 * 1.0;
        (x, y)
    }

    /// Evaluates the curve's y value at a given x value in [0, 1].
    /// Uses Newton's method to find the t parameter for the given x,
    /// then computes the corresponding y.
    pub fn evaluate_at_x(&self, target_x: f32) -> f32 {
        if target_x <= 0.0 {
            return 0.0;
        }
        if target_x >= 1.0 {
            return 1.0;
        }

        // Newton's method to solve bezier_x(t) = target_x.
        let mut t = target_x; // initial guess
        for _ in 0..8 {
            let (x, _) = self.sample_at(t);
            let dx = self.sample_dx(t);
            if dx.abs() < 1e-7 {
                break;
            }
            t -= (x - target_x) / dx;
            t = t.clamp(0.0, 1.0);
        }

        let (_, y) = self.sample_at(t);
        y
    }

    /// Returns the derivative dx/dt of the bezier x-component at parameter t.
    fn sample_dx(&self, t: f32) -> f32 {
        let mt = 1.0 - t;
        3.0 * mt * mt * self.x1 + 6.0 * mt * t * (self.x2 - self.x1) + 3.0 * t * t * (1.0 - self.x2)
    }

    /// Generates a series of sample points for rendering the curve.
    /// Returns `steps + 1` points as (x, y) pairs.
    pub fn sample_points(&self, steps: usize) -> Vec<(f32, f32)> {
        (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                self.sample_at(t)
            })
            .collect()
    }

    /// Creates a snapshot of the current state for undo purposes.
    pub fn snapshot(&self) -> CurveSnapshot {
        CurveSnapshot {
            x1: self.x1,
            y1: self.y1,
            x2: self.x2,
            y2: self.y2,
        }
    }

    /// Restores from a snapshot.
    pub fn restore(&mut self, snap: &CurveSnapshot) {
        self.x1 = snap.x1;
        self.y1 = snap.y1;
        self.x2 = snap.x2;
        self.y2 = snap.y2;
    }
}

/// Snapshot of curve control points for undo/redo.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveSnapshot {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// State for an active control-point drag operation.
#[derive(Debug, Clone)]
pub struct CurveDragState {
    /// Which control point is being dragged.
    pub point: ControlPoint,
    /// Snapshot of the curve before the drag started.
    pub before: CurveSnapshot,
}

impl CurveDragState {
    pub fn new(point: ControlPoint, editor: &CurveEditor) -> Self {
        Self {
            point,
            before: editor.snapshot(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_count() {
        assert!(CURVE_PRESETS.len() >= 9);
    }

    #[test]
    fn preset_to_transition() {
        let p = &CURVE_PRESETS[1]; // Ease In
        let t = p.to_transition();
        assert!(t.is_bezier());
        match t {
            TransitionType::CubicBezier(x1, y1, x2, y2) => {
                assert!((x1 - 0.42).abs() < 1e-5);
                assert!((y1 - 0.0).abs() < 1e-5);
                assert!((x2 - 1.0).abs() < 1e-5);
                assert!((y2 - 1.0).abs() < 1e-5);
            }
            _ => panic!("expected CubicBezier"),
        }
    }

    #[test]
    fn curve_editor_apply_preset() {
        let mut editor = CurveEditor::new("walk".into(), 0, 1, 0.0, 0.0, 1.0, 1.0);
        editor.apply_preset(&CURVE_PRESETS[3]); // Ease In-Out
        assert!((editor.x1 - 0.42).abs() < 1e-5);
        assert!((editor.x2 - 0.58).abs() < 1e-5);
    }

    #[test]
    fn curve_editor_to_transition() {
        let editor = CurveEditor::new("idle".into(), 0, 0, 0.25, 0.1, 0.25, 1.0);
        match editor.to_transition() {
            TransitionType::CubicBezier(x1, y1, x2, y2) => {
                assert!((x1 - 0.25).abs() < 1e-5);
                assert!((y1 - 0.1).abs() < 1e-5);
                assert!((x2 - 0.25).abs() < 1e-5);
                assert!((y2 - 1.0).abs() < 1e-5);
            }
            _ => panic!("expected CubicBezier"),
        }
    }

    // -- Control point hit testing ------------------------------------------

    #[test]
    fn hit_test_cp1() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let hit = editor.hit_test_control_point(0.42, 0.0, 0.1);
        assert_eq!(hit.map(|h| h.point), Some(ControlPoint::CP1));
    }

    #[test]
    fn hit_test_cp2() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let hit = editor.hit_test_control_point(0.58, 1.0, 0.1);
        assert_eq!(hit.map(|h| h.point), Some(ControlPoint::CP2));
    }

    #[test]
    fn hit_test_miss() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let hit = editor.hit_test_control_point(0.5, 0.5, 0.05);
        assert!(hit.is_none());
    }

    #[test]
    fn hit_test_prefers_closer_point() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.3, 0.3, 0.35, 0.35);
        // Point equidistant but slightly closer to CP1
        let hit = editor.hit_test_control_point(0.3, 0.3, 0.1);
        assert_eq!(hit.map(|h| h.point), Some(ControlPoint::CP1));
    }

    // -- Control point movement ---------------------------------------------

    #[test]
    fn move_control_point_cp1() {
        let mut editor = CurveEditor::new("anim".into(), 0, 0, 0.0, 0.0, 1.0, 1.0);
        editor.move_control_point(ControlPoint::CP1, 0.5, 0.8);
        assert!((editor.x1 - 0.5).abs() < 1e-5);
        assert!((editor.y1 - 0.8).abs() < 1e-5);
    }

    #[test]
    fn move_control_point_clamps_x() {
        let mut editor = CurveEditor::new("anim".into(), 0, 0, 0.5, 0.5, 0.5, 0.5);
        editor.move_control_point(ControlPoint::CP1, -0.5, 0.0);
        assert!((editor.x1 - 0.0).abs() < 1e-5); // clamped to 0
        editor.move_control_point(ControlPoint::CP2, 1.5, 0.0);
        assert!((editor.x2 - 1.0).abs() < 1e-5); // clamped to 1
    }

    #[test]
    fn move_control_point_allows_y_overshoot() {
        let mut editor = CurveEditor::new("anim".into(), 0, 0, 0.0, 0.0, 1.0, 1.0);
        editor.move_control_point(ControlPoint::CP1, 0.5, -0.5);
        assert!((editor.y1 - (-0.5)).abs() < 1e-5); // not clamped
        editor.move_control_point(ControlPoint::CP2, 0.5, 1.5);
        assert!((editor.y2 - 1.5).abs() < 1e-5);
    }

    #[test]
    fn control_point_getter() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.1, 0.2, 0.8, 0.9);
        assert_eq!(editor.control_point(ControlPoint::CP1), (0.1, 0.2));
        assert_eq!(editor.control_point(ControlPoint::CP2), (0.8, 0.9));
    }

    // -- Reset / linear check -----------------------------------------------

    #[test]
    fn reset_to_linear() {
        let mut editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        assert!(!editor.is_linear());
        editor.reset_to_linear();
        assert!(editor.is_linear());
    }

    // -- Mirror -------------------------------------------------------------

    #[test]
    fn mirror_ease_in_becomes_ease_out() {
        let mut editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 1.0, 1.0);
        editor.mirror();
        // Ease-in: (0.42, 0, 1, 1) -> mirrored: (1-1, 1-1, 1-0.42, 1-0) = (0, 0, 0.58, 1)
        assert!((editor.x1 - 0.0).abs() < 1e-4);
        assert!((editor.y1 - 0.0).abs() < 1e-4);
        assert!((editor.x2 - 0.58).abs() < 1e-4);
        assert!((editor.y2 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn mirror_is_involution() {
        let mut editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let snap_before = editor.snapshot();
        editor.mirror();
        editor.mirror();
        assert!((editor.x1 - snap_before.x1).abs() < 1e-4);
        assert!((editor.y1 - snap_before.y1).abs() < 1e-4);
        assert!((editor.x2 - snap_before.x2).abs() < 1e-4);
        assert!((editor.y2 - snap_before.y2).abs() < 1e-4);
    }

    // -- Sampling -----------------------------------------------------------

    #[test]
    fn sample_at_endpoints() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let (x0, y0) = editor.sample_at(0.0);
        assert!((x0).abs() < 1e-5);
        assert!((y0).abs() < 1e-5);
        let (x1, y1) = editor.sample_at(1.0);
        assert!((x1 - 1.0).abs() < 1e-5);
        assert!((y1 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn sample_at_midpoint_linear() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.0, 0.0, 1.0, 1.0);
        let (x, y) = editor.sample_at(0.5);
        assert!((x - 0.5).abs() < 0.01);
        assert!((y - 0.5).abs() < 0.01);
    }

    #[test]
    fn evaluate_at_x_linear() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.0, 0.0, 1.0, 1.0);
        let y = editor.evaluate_at_x(0.5);
        assert!((y - 0.5).abs() < 0.01);
    }

    #[test]
    fn evaluate_at_x_endpoints() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        assert!((editor.evaluate_at_x(0.0)).abs() < 0.01);
        assert!((editor.evaluate_at_x(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn evaluate_at_x_ease_in_out_midpoint() {
        // Ease In-Out: slow at start and end, fast in middle.
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let y = editor.evaluate_at_x(0.5);
        // At x=0.5, ease-in-out should be close to 0.5 (symmetric curve).
        assert!((y - 0.5).abs() < 0.05);
    }

    #[test]
    fn sample_points_count() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let points = editor.sample_points(50);
        assert_eq!(points.len(), 51);
    }

    #[test]
    fn sample_points_start_and_end() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let points = editor.sample_points(20);
        let (x0, y0) = points[0];
        assert!((x0).abs() < 1e-5);
        assert!((y0).abs() < 1e-5);
        let (x_last, y_last) = points[points.len() - 1];
        assert!((x_last - 1.0).abs() < 1e-5);
        assert!((y_last - 1.0).abs() < 1e-5);
    }

    // -- Snapshot / restore -------------------------------------------------

    #[test]
    fn snapshot_and_restore() {
        let mut editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let snap = editor.snapshot();
        editor.reset_to_linear();
        assert!(editor.is_linear());
        editor.restore(&snap);
        assert!((editor.x1 - 0.42).abs() < 1e-5);
        assert!((editor.x2 - 0.58).abs() < 1e-5);
    }

    // -- CurveDragState -----------------------------------------------------

    #[test]
    fn drag_state_captures_before() {
        let editor = CurveEditor::new("anim".into(), 0, 0, 0.42, 0.0, 0.58, 1.0);
        let drag = CurveDragState::new(ControlPoint::CP1, &editor);
        assert_eq!(drag.point, ControlPoint::CP1);
        assert!((drag.before.x1 - 0.42).abs() < 1e-5);
    }
}

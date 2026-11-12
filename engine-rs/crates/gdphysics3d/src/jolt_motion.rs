//! A port of Godot 4.6's Jolt `body_test_motion`
//! (`modules/jolt_physics/spaces/jolt_physics_direct_space_state_3d.cpp`,
//! `_body_motion_recover` / `_body_motion_cast` / `_body_motion_collide`) — the
//! kinematic query under every `CharacterBody3D::move_and_slide` (pat-kiuul).
//!
//! The 3D platformer runs `3d/physics_engine="Jolt Physics"`, and Jolt is the
//! default for new 4.6 projects, so this is the query whose numbers a game's
//! character actually lands on. Three passes, exactly as Godot runs them:
//!
//! 1. **Recovery.** Up to [`RECOVERY_ITERATIONS`] (4) times, collect every
//!    contact within the safe margin and push the body out by
//!    [`RECOVERY_AMOUNT`] (40 %) of each one's depth past the margin.
//! 2. **Cast.** Per obstacle, a biased binary search along the motion for the
//!    last fraction that does not touch (`safe`) and the first that does
//!    (`unsafe`), with `clamp(log2(1000 * |motion|), 4, 16)` steps — five for a
//!    platformer's 0.05 m walking step, so the fraction is only good to 1/32.
//!    That coarseness is WHY a walking body rests 1e-4 above the floor in Godot
//!    and not at the margin: the cast lands it below the margin and the next
//!    slide's recovery only recovers 87 % of the way back.
//! 3. **Collide.** At the unsafe fraction, every contact within the margin that
//!    opposes the motion, deepest first.
//!
//! The narrow phase follows Jolt's `CollideConvexVsConvex`: each convex shape
//! is a CORE (point, segment, shrunk box, shrunk hull, triangle) plus a
//! CONVEX RADIUS, and the contact is the GJK closest points between the cores.
//! A Godot box keeps `min(margin 0.04, 8 % of its smallest half extent)` as
//! convex radius, so a GridMap floor tile has rounded edges at the seams.
//! Contacts go through Jolt's `InternalEdgeRemovingCollector` (the
//! `use_enhanced_internal_edge_removal` motion-query default), which drops an
//! edge contact whose edge another face contact already covers.
//!
//! What is approximated, and why it does not reach the numbers above:
//!
//! * **Deep overlap** (cores intersect, Jolt's `Indeterminate` -> EPA) uses
//!   [`crate::collision::test_collision_rot`] on the full shapes. Only a body
//!   sunk deeper than the convex radii gets there.
//! * **Manifold**: one contact point per hit (`mContactPointOn2`), where Jolt
//!   expands a face pair into up to four points. They share the hit's normal
//!   and depth, which is all `_set_collision_direction` reads.
//! * **Edge-removal grouping**: Jolt flushes the collector per BODY, and Godot
//!   puts a whole GridMap octant in one body; Patina gives every cell its own
//!   body, so all STATIC bodies form one group here (the octant case), and
//!   every other body is its own group.

use std::cell::RefCell;
use std::collections::HashMap;

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

use crate::body::{BodyId3D, PhysicsBody3D};
use crate::collision;
use crate::shape::Shape3D;

/// `Shape3D.margin` default — Godot's `CollisionShape3D` resources all start
/// with it, and Jolt uses it as the convex radius.
pub const DEFAULT_SHAPE_MARGIN: f32 = 0.04;
/// `physics/jolt_physics_3d/collisions/collision_margin_fraction` default.
pub const COLLISION_MARGIN_FRACTION: f32 = 0.08;
/// `physics/jolt_physics_3d/motion_queries/recovery_iterations` default.
pub const RECOVERY_ITERATIONS: usize = 4;
/// `physics/jolt_physics_3d/motion_queries/recovery_amount` default.
pub const RECOVERY_AMOUNT: f32 = 0.4;
/// Godot's `CMP_EPSILON`.
pub const CMP_EPSILON: f32 = 0.00001;
/// `ConvexHullShapeSettings::mMaxErrorConvexRadius`.
const HULL_MAX_ERROR_CONVEX_RADIUS: f64 = 0.05;

// ===========================================================================
// f64 vector maths for the narrow phase
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct D3 {
    x: f64,
    y: f64,
    z: f64,
}

impl D3 {
    const ZERO: D3 = D3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    fn from(v: Vector3) -> Self {
        Self::new(v.x as f64, v.y as f64, v.z as f64)
    }
    fn to_f32(self) -> Vector3 {
        Vector3::new(self.x as f32, self.y as f32, self.z as f32)
    }
    fn dot(self, o: D3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    fn cross(self, o: D3) -> D3 {
        D3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    fn len_sq(self) -> f64 {
        self.dot(self)
    }
    fn len(self) -> f64 {
        self.len_sq().sqrt()
    }
    fn get(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
}

impl std::ops::Add for D3 {
    type Output = D3;
    fn add(self, o: D3) -> D3 {
        D3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl std::ops::Sub for D3 {
    type Output = D3;
    fn sub(self, o: D3) -> D3 {
        D3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl std::ops::Mul<f64> for D3 {
    type Output = D3;
    fn mul(self, s: f64) -> D3 {
        D3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl std::ops::Neg for D3 {
    type Output = D3;
    fn neg(self) -> D3 {
        D3::new(-self.x, -self.y, -self.z)
    }
}

/// A rotation as three world-space column axes.
#[derive(Debug, Clone, Copy)]
struct M3 {
    c: [D3; 3],
}

impl M3 {
    const IDENTITY: M3 = M3 {
        c: [
            D3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            D3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            D3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        ],
    };
    fn from_quat(q: Quaternion) -> Self {
        Self {
            c: [
                D3::from(q.xform(Vector3::new(1.0, 0.0, 0.0))),
                D3::from(q.xform(Vector3::new(0.0, 1.0, 0.0))),
                D3::from(q.xform(Vector3::new(0.0, 0.0, 1.0))),
            ],
        }
    }
    /// local -> world
    fn xform(&self, v: D3) -> D3 {
        self.c[0] * v.x + self.c[1] * v.y + self.c[2] * v.z
    }
    /// world -> local
    fn xform_inv(&self, v: D3) -> D3 {
        D3::new(self.c[0].dot(v), self.c[1].dot(v), self.c[2].dot(v))
    }
}

fn sign(v: f64) -> f64 {
    // Jolt's `Sign` / `Vec3::GetSign`: -1 only for negative values.
    if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}

// ===========================================================================
// Convex hulls, as Jolt's ConvexHullShape builds them
// ===========================================================================

/// One convex hull, with the faces and shrunk points Jolt derives from it.
#[derive(Debug)]
struct Hull {
    /// Hull vertices (shape-local).
    points: Vec<D3>,
    /// Each face as indices into `points`, counter-clockwise seen from outside.
    faces: Vec<Vec<usize>>,
    /// Outward unit normal and offset (`n . p == d` on the plane) per face.
    planes: Vec<(D3, f64)>,
    /// `points` moved inward by `convex_radius` (Jolt `HullNoConvex`).
    shrunk: Vec<D3>,
    convex_radius: f64,
}

thread_local! {
    static HULL_CACHE: RefCell<HashMap<u64, std::rc::Rc<Hull>>> = RefCell::new(HashMap::new());
}

fn hull_key(points: &[Vector3], margin: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    margin.to_bits().hash(&mut h);
    for p in points {
        p.x.to_bits().hash(&mut h);
        p.y.to_bits().hash(&mut h);
        p.z.to_bits().hash(&mut h);
    }
    h.finish()
}

fn hull_for(points: &[Vector3]) -> std::rc::Rc<Hull> {
    let key = hull_key(points, DEFAULT_SHAPE_MARGIN);
    if let Some(h) = HULL_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return h;
    }
    let hull = std::rc::Rc::new(build_hull(points));
    HULL_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        // A level holds a handful of distinct tiles; a script generating hulls
        // every frame must not grow this without bound.
        if c.len() > 4096 {
            c.clear();
        }
        c.insert(key, hull.clone());
    });
    hull
}

/// Faces by brute force over point triples (tiles carry a few dozen points at
/// most; the result is cached by content), then Jolt's convex-radius rules
/// (`ConvexHullShape::ConvexHullShape`) and shrink (`GetSupportFunction`,
/// `ExcludeConvexRadius`).
fn build_hull(input: &[Vector3]) -> Hull {
    // Deduplicate.
    let mut pts: Vec<D3> = Vec::new();
    for p in input {
        let d = D3::from(*p);
        if !pts.iter().any(|q| (*q - d).len_sq() < 1e-12) {
            pts.push(d);
        }
    }
    let empty = |pts: Vec<D3>| Hull {
        shrunk: pts.clone(),
        points: pts,
        faces: Vec::new(),
        planes: Vec::new(),
        convex_radius: 0.0,
    };
    if pts.len() < 4 || pts.len() > 64 {
        return empty(pts);
    }
    let extent = pts
        .iter()
        .fold(0.0f64, |m, p| {
            m.max(p.x.abs()).max(p.y.abs()).max(p.z.abs())
        })
        .max(1e-6);
    let eps = 1e-5 * extent.max(1.0);

    let mut planes: Vec<(D3, f64)> = Vec::new();
    let n = pts.len();
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                let nrm = (pts[j] - pts[i]).cross(pts[k] - pts[i]);
                let l = nrm.len();
                if l < 1e-9 {
                    continue;
                }
                let mut nrm = nrm * (1.0 / l);
                let mut d = nrm.dot(pts[i]);
                let mut above = false;
                let mut below = false;
                for p in &pts {
                    let s = nrm.dot(*p) - d;
                    if s > eps {
                        above = true;
                    } else if s < -eps {
                        below = true;
                    }
                }
                if above && below {
                    continue;
                }
                if above {
                    nrm = -nrm;
                    d = -d;
                }
                if planes
                    .iter()
                    .any(|(pn, pd)| (*pn - nrm).len_sq() < 1e-8 && (pd - d).abs() < eps)
                {
                    continue;
                }
                planes.push((nrm, d));
            }
        }
    }
    if planes.len() < 4 {
        return empty(pts);
    }

    // Faces: the points on each plane, ordered counter-clockwise about the
    // outward normal. Points strictly inside an edge are kept, as Jolt's
    // builder keeps every input point that is on the hull within tolerance.
    let mut faces: Vec<Vec<usize>> = Vec::new();
    for (nrm, d) in &planes {
        let on: Vec<usize> = (0..n)
            .filter(|&i| (nrm.dot(pts[i]) - d).abs() <= eps)
            .collect();
        let centroid = on.iter().fold(D3::ZERO, |a, &i| a + pts[i]) * (1.0 / on.len() as f64);
        let u = {
            let t = pts[on[0]] - centroid;
            let t = t - *nrm * t.dot(*nrm);
            let l = t.len().max(1e-12);
            t * (1.0 / l)
        };
        let v = nrm.cross(u);
        let mut ordered: Vec<(f64, usize)> = on
            .iter()
            .map(|&i| {
                let r = pts[i] - centroid;
                (r.dot(v).atan2(r.dot(u)), i)
            })
            .collect();
        ordered.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        faces.push(ordered.into_iter().map(|(_, i)| i).collect());
    }

    // Only points that are corners of the hull stay (a point no face uses is
    // interior).
    let used: Vec<bool> = (0..n)
        .map(|i| faces.iter().any(|f| f.contains(&i)))
        .collect();

    // Jolt: the margin is clamped to 8 % of the smallest AABB half extent
    // (`JoltConvexPolygonShape3D::_build`), then to half the hull's thinnest
    // dimension, then by the sharp-corner error bound.
    let (mut lo, mut hi) = (pts[0], pts[0]);
    for p in &pts {
        lo = D3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = D3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let size = hi - lo;
    let shortest_half = size.x.min(size.y).min(size.z) * 0.5;
    let mut r = (DEFAULT_SHAPE_MARGIN as f64).min(shortest_half * COLLISION_MARGIN_FRACTION as f64);
    if r > 0.0 {
        let mut min_size = f64::MAX;
        for (nrm, d) in &planes {
            let mut max_dist = 0.0f64;
            for p in &pts {
                max_dist = max_dist.max(-(nrm.dot(*p) - d));
            }
            min_size = min_size.min(max_dist);
        }
        r = r.min(0.5 * min_size);
    }

    // Per point: the faces it uses, reduced to Jolt's best 3 (or 2, or 1).
    let mut point_faces: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (p, pf) in point_faces.iter_mut().enumerate() {
        if !used[p] {
            continue;
        }
        let faces_of: Vec<usize> = (0..faces.len())
            .filter(|&f| faces[f].contains(&p))
            .collect();
        let mut biggest_volume = 0.05f64;
        let mut best3: Option<[usize; 3]> = None;
        let mut smallest_dot = (1.0f64).to_radians().cos();
        let mut best2: Option<[usize; 2]> = None;
        for a in 0..faces_of.len() {
            let n1 = planes[faces_of[a]].0;
            for b in (a + 1)..faces_of.len() {
                let n2 = planes[faces_of[b]].0;
                let cr = n1.cross(n2);
                let dot = n1.dot(n2);
                if dot < smallest_dot {
                    smallest_dot = dot;
                    best2 = Some([faces_of[a], faces_of[b]]);
                }
                for c in (b + 1)..faces_of.len() {
                    let vol = cr.dot(planes[faces_of[c]].0).abs();
                    if vol > biggest_volume {
                        biggest_volume = vol;
                        best3 = Some([faces_of[a], faces_of[b], faces_of[c]]);
                    }
                }
            }
        }
        *pf = if let Some(b) = best3 {
            b.to_vec()
        } else if let Some(b) = best2 {
            b.to_vec()
        } else {
            faces_of.first().map(|f| vec![*f]).unwrap_or_default()
        };
    }

    let solve3 = |n: [D3; 3], rhs: [f64; 3]| -> Option<D3> {
        let det = n[0].dot(n[1].cross(n[2]));
        if det.abs() < 1e-12 {
            return None;
        }
        // Cramer via the cross-product form of the inverse.
        Some(
            (n[1].cross(n[2]) * rhs[0] + n[2].cross(n[0]) * rhs[1] + n[0].cross(n[1]) * rhs[2])
                * (1.0 / det),
        )
    };

    if r > 0.0 {
        for pf in &point_faces {
            if pf.len() < 2 {
                continue;
            }
            let n1 = planes[pf[0]].0;
            let n2 = planes[pf[1]].0;
            let (n3, mask3) = if pf.len() == 3 {
                (planes[pf[2]].0, 1.0)
            } else {
                (n1.cross(n2), 0.0)
            };
            let l3 = n3.len();
            if l3 < 1e-12 {
                r = 0.0;
                break;
            }
            let n3 = n3 * (1.0 / l3);
            // offset = -N^-1 (1, 1, mask)
            match solve3([n1, n2, n3], [1.0, 1.0, mask3]) {
                Some(off) => {
                    let offset = off.len();
                    if offset > 1.0 + 1e-9 {
                        r = r.min(HULL_MAX_ERROR_CONVEX_RADIUS / (offset - 1.0));
                    }
                }
                None => {
                    r = 0.0;
                    break;
                }
            }
        }
    }

    let mut shrunk = pts.clone();
    if r > 0.0 {
        for p in 0..n {
            let pf = &point_faces[p];
            if pf.is_empty() {
                continue;
            }
            let (n1, d1) = planes[pf[0]];
            if pf.len() == 1 {
                shrunk[p] = pts[p] - n1 * r;
                continue;
            }
            let (n2, d2) = planes[pf[1]];
            let (n3, d3) = if pf.len() == 3 {
                let (n3, d3) = planes[pf[2]];
                (n3, d3 - r)
            } else {
                let n3 = n1.cross(n2);
                let l = n3.len().max(1e-12);
                let n3 = n3 * (1.0 / l);
                (n3, n3.dot(pts[p]))
            };
            shrunk[p] = solve3([n1, n2, n3], [d1 - r, d2 - r, d3]).unwrap_or(pts[p] - n1 * r);
        }
    }

    // Drop interior points from the support set.
    let keep: Vec<usize> = (0..n).filter(|&i| used[i]).collect();
    let remap: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    Hull {
        points: keep.iter().map(|&i| pts[i]).collect(),
        shrunk: keep.iter().map(|&i| shrunk[i]).collect(),
        faces: faces
            .iter()
            .map(|f| f.iter().filter_map(|i| remap.get(i).copied()).collect())
            .collect(),
        planes,
        convex_radius: r,
    }
}

// ===========================================================================
// Convex parts: a core plus a convex radius, placed in the world
// ===========================================================================

#[derive(Debug, Clone)]
enum Core {
    Point,
    /// Along local Y, `-half..half`.
    Segment(f64),
    /// Shrunk half extents; `full` is the authored box (for faces).
    Box {
        core: D3,
        full: D3,
    },
    /// Shrunk cylinder along local Y.
    Cylinder {
        half_h: f64,
        r: f64,
    },
    Hull(std::rc::Rc<Hull>),
    /// World-space triangle, Jolt winding (`(b - a) x (c - a)` is the front).
    Triangle([D3; 3]),
    /// World plane `n . p == d`, solid behind.
    Plane {
        n: D3,
        d: f64,
    },
}

#[derive(Debug, Clone)]
struct Part {
    core: Core,
    pos: D3,
    rot: M3,
    radius: f64,
    /// Sweep (Godot's `JoltCustomMotionShape`): the support adds it in any
    /// direction it faces.
    motion: D3,
}

impl Part {
    fn support(&self, d: D3) -> D3 {
        let mut p = match &self.core {
            Core::Triangle(t) => {
                let mut best = t[0];
                let mut bd = t[0].dot(d);
                for v in &t[1..] {
                    let s = v.dot(d);
                    if s > bd {
                        bd = s;
                        best = *v;
                    }
                }
                best
            }
            Core::Plane { .. } => D3::ZERO,
            core => {
                let ld = self.rot.xform_inv(d);
                let lp = match core {
                    Core::Point => D3::ZERO,
                    Core::Segment(h) => D3::new(0.0, sign(ld.y) * h, 0.0),
                    Core::Box { core, .. } => D3::new(
                        sign(ld.x) * core.x,
                        sign(ld.y) * core.y,
                        sign(ld.z) * core.z,
                    ),
                    Core::Cylinder { half_h, r } => {
                        let o = (ld.x * ld.x + ld.z * ld.z).sqrt();
                        if o > 0.0 {
                            D3::new(r * ld.x / o, sign(ld.y) * half_h, r * ld.z / o)
                        } else {
                            D3::new(0.0, sign(ld.y) * half_h, 0.0)
                        }
                    }
                    Core::Hull(h) => {
                        let mut best = h.shrunk[0];
                        let mut bd = best.dot(ld);
                        for p in &h.shrunk[1..] {
                            let s = p.dot(ld);
                            if s > bd {
                                bd = s;
                                best = *p;
                            }
                        }
                        best
                    }
                    Core::Triangle(_) | Core::Plane { .. } => unreachable!(),
                };
                self.pos + self.rot.xform(lp)
            }
        };
        if d.dot(self.motion) > 0.0 {
            p = p + self.motion;
        }
        p
    }

    fn centre(&self) -> D3 {
        match &self.core {
            Core::Triangle(t) => (t[0] + t[1] + t[2]) * (1.0 / 3.0),
            _ => self.pos,
        }
    }

    /// Jolt `GetSupportingFace(direction)`: the face whose outward normal is
    /// most opposed to `dir` (world), counter-clockwise from outside. Fewer
    /// than three vertices means "no face" to the edge-removal collector.
    fn supporting_face(&self, dir: D3) -> Vec<D3> {
        match &self.core {
            Core::Triangle(t) => t.to_vec(),
            Core::Plane { n, d } => {
                // A big quad on the plane, wound to face `n`.
                let t = if n.x.abs() < 0.9 {
                    D3::new(1.0, 0.0, 0.0)
                } else {
                    D3::new(0.0, 1.0, 0.0)
                };
                let u = n.cross(t);
                let u = u * (1.0 / u.len());
                let v = n.cross(u);
                let o = *n * *d;
                let s = 1000.0;
                // (f1 - f0) x (f2 - f0) = 4 s^2 (u x v) = 4 s^2 n.
                vec![
                    o + u * s + v * s,
                    o - u * s + v * s,
                    o - u * s - v * s,
                    o + u * s - v * s,
                ]
            }
            Core::Box { full, .. } => {
                let ld = self.rot.xform_inv(dir);
                let a = [ld.x.abs(), ld.y.abs(), ld.z.abs()];
                let axis = if a[0] >= a[1] && a[0] >= a[2] {
                    0
                } else if a[1] >= a[2] {
                    1
                } else {
                    2
                };
                let (mx, mn) = (*full, -*full);
                let local: [D3; 4] = if ld.get(axis) < 0.0 {
                    match axis {
                        0 => [
                            D3::new(mx.x, mn.y, mn.z),
                            D3::new(mx.x, mx.y, mn.z),
                            D3::new(mx.x, mx.y, mx.z),
                            D3::new(mx.x, mn.y, mx.z),
                        ],
                        1 => [
                            D3::new(mn.x, mx.y, mn.z),
                            D3::new(mn.x, mx.y, mx.z),
                            D3::new(mx.x, mx.y, mx.z),
                            D3::new(mx.x, mx.y, mn.z),
                        ],
                        _ => [
                            D3::new(mn.x, mn.y, mx.z),
                            D3::new(mx.x, mn.y, mx.z),
                            D3::new(mx.x, mx.y, mx.z),
                            D3::new(mn.x, mx.y, mx.z),
                        ],
                    }
                } else {
                    match axis {
                        0 => [
                            D3::new(mn.x, mn.y, mn.z),
                            D3::new(mn.x, mn.y, mx.z),
                            D3::new(mn.x, mx.y, mx.z),
                            D3::new(mn.x, mx.y, mn.z),
                        ],
                        1 => [
                            D3::new(mn.x, mn.y, mn.z),
                            D3::new(mx.x, mn.y, mn.z),
                            D3::new(mx.x, mn.y, mx.z),
                            D3::new(mn.x, mn.y, mx.z),
                        ],
                        _ => [
                            D3::new(mn.x, mn.y, mn.z),
                            D3::new(mn.x, mx.y, mn.z),
                            D3::new(mx.x, mx.y, mn.z),
                            D3::new(mx.x, mn.y, mn.z),
                        ],
                    }
                };
                local
                    .iter()
                    .map(|p| self.pos + self.rot.xform(*p))
                    .collect()
            }
            Core::Hull(h) => {
                if h.faces.is_empty() {
                    return Vec::new();
                }
                let ld = self.rot.xform_inv(dir);
                let mut best = 0;
                let mut best_dot = h.planes[0].0.dot(ld);
                for (i, (n, _)) in h.planes.iter().enumerate().skip(1) {
                    let d = n.dot(ld);
                    if d < best_dot {
                        best_dot = d;
                        best = i;
                    }
                }
                h.faces[best]
                    .iter()
                    .map(|&i| self.pos + self.rot.xform(h.points[i]))
                    .collect()
            }
            // Spheres, capsules and cylinders hand back at most an edge here;
            // the collector then accepts the contact as is.
            _ => Vec::new(),
        }
    }
}

/// Jolt's convex radius for a Godot shape (`Jolt*Shape3D::_build`).
fn box_radius(half: Vector3) -> f64 {
    let min_half = half.x.min(half.y).min(half.z) as f64;
    (DEFAULT_SHAPE_MARGIN as f64).min(min_half * COLLISION_MARGIN_FRACTION as f64)
}

/// The convex part for a bounded convex shape, or `None` for anything else.
fn convex_part(shape: &Shape3D, pos: Vector3, rot: Quaternion) -> Option<Part> {
    let pos = D3::from(pos);
    let rot = M3::from_quat(rot);
    let (core, radius) = match shape {
        Shape3D::Sphere { radius } => (Core::Point, *radius as f64),
        Shape3D::CapsuleShape { radius, height } => (
            Core::Segment((*height as f64 * 0.5 - *radius as f64).max(0.0)),
            *radius as f64,
        ),
        Shape3D::BoxShape { half_extents } => {
            let r = box_radius(*half_extents);
            let full = D3::from(*half_extents);
            (
                Core::Box {
                    core: D3::new(full.x - r, full.y - r, full.z - r),
                    full,
                },
                r,
            )
        }
        Shape3D::CylinderShape { radius, height } => {
            let half_h = *height as f64 * 0.5;
            let r = (DEFAULT_SHAPE_MARGIN as f64)
                .min(half_h.min(*radius as f64) * COLLISION_MARGIN_FRACTION as f64);
            (
                Core::Cylinder {
                    half_h: half_h - r,
                    r: *radius as f64 - r,
                },
                r,
            )
        }
        Shape3D::ConvexPolygonShape { points } => {
            if points.is_empty() {
                return None;
            }
            let h = hull_for(points);
            let r = h.convex_radius;
            (Core::Hull(h), r)
        }
        _ => return None,
    };
    Some(Part {
        core,
        pos,
        rot,
        radius,
        motion: D3::ZERO,
    })
}

// ===========================================================================
// GJK closest points
// ===========================================================================

enum Gjk {
    /// Further apart than the limit asked for.
    Separated,
    /// Closest points on the two cores and their squared distance.
    Closest { dist_sq: f64, pa: D3, pb: D3 },
    /// The cores overlap.
    Overlap,
}

/// Affine closest point of the origin to `w[0..k]`; barycentrics or `None`
/// when degenerate.
fn affine_closest(w: &[D3]) -> Option<(D3, [f64; 4])> {
    let k = w.len();
    if k == 1 {
        return Some((w[0], [1.0, 0.0, 0.0, 0.0]));
    }
    let p0 = w[0];
    let e: Vec<D3> = w[1..].iter().map(|p| *p - p0).collect();
    let m = e.len();
    // Gram system G mu = -E^T p0
    let mut g = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];
    for i in 0..m {
        for j in 0..m {
            g[i][j] = e[i].dot(e[j]);
        }
        rhs[i] = -e[i].dot(p0);
    }
    let mu: [f64; 3] = match m {
        1 => {
            if g[0][0] < 1e-30 {
                return None;
            }
            [rhs[0] / g[0][0], 0.0, 0.0]
        }
        2 => {
            let det = g[0][0] * g[1][1] - g[0][1] * g[1][0];
            if det.abs() < 1e-30 * (g[0][0] * g[1][1]).max(1e-300) || det.abs() < 1e-40 {
                return None;
            }
            [
                (rhs[0] * g[1][1] - g[0][1] * rhs[1]) / det,
                (g[0][0] * rhs[1] - rhs[0] * g[1][0]) / det,
                0.0,
            ]
        }
        _ => {
            let det = g[0][0] * (g[1][1] * g[2][2] - g[1][2] * g[2][1])
                - g[0][1] * (g[1][0] * g[2][2] - g[1][2] * g[2][0])
                + g[0][2] * (g[1][0] * g[2][1] - g[1][1] * g[2][0]);
            if det.abs() < 1e-40 {
                return None;
            }
            let mut out = [0.0; 3];
            for (c, o) in out.iter_mut().enumerate() {
                let mut gc = g;
                for (r, row) in gc.iter_mut().enumerate() {
                    row[c] = rhs[r];
                }
                let d = gc[0][0] * (gc[1][1] * gc[2][2] - gc[1][2] * gc[2][1])
                    - gc[0][1] * (gc[1][0] * gc[2][2] - gc[1][2] * gc[2][0])
                    + gc[0][2] * (gc[1][0] * gc[2][1] - gc[1][1] * gc[2][0]);
                *o = d / det;
            }
            out
        }
    };
    let mut lambda = [0.0f64; 4];
    let mut x = p0;
    let mut sum = 0.0;
    for i in 0..m {
        lambda[i + 1] = mu[i];
        sum += mu[i];
        x = x + e[i] * mu[i];
    }
    lambda[0] = 1.0 - sum;
    Some((x, lambda))
}

/// The closest point of the simplex `w` to the origin: the reduced vertex set
/// (as indices) and its barycentrics.
fn simplex_closest(w: &[D3]) -> (D3, Vec<(usize, f64)>, bool) {
    let n = w.len();
    #[allow(clippy::type_complexity)]
    let mut best: Option<(f64, D3, Vec<(usize, f64)>)> = None;
    for mask in 1u32..(1u32 << n) {
        let idx: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        let pts: Vec<D3> = idx.iter().map(|&i| w[i]).collect();
        let Some((x, lambda)) = affine_closest(&pts) else {
            continue;
        };
        if lambda[..idx.len()].iter().any(|&l| l < -1e-12) {
            continue;
        }
        let d = x.len_sq();
        let better = match &best {
            None => true,
            Some((bd, _, bi)) => d < *bd - 1e-18 || (d <= *bd + 1e-18 && idx.len() < bi.len()),
        };
        if better {
            best = Some((
                d,
                x,
                idx.iter()
                    .enumerate()
                    .map(|(k, &i)| (i, lambda[k]))
                    .collect(),
            ));
        }
    }
    match best {
        Some((_, x, sel)) => {
            let full = n == 4 && sel.len() == 4;
            (x, sel, full)
        }
        None => (w[0], vec![(0, 1.0)], false),
    }
}

/// GJK distance between the cores of `a` and `b` (radii excluded). `limit_sq`
/// is the squared distance beyond which the answer is just "separated".
fn gjk(a: &Part, b: &Part, limit_sq: f64) -> Gjk {
    let mut dir = b.centre() - a.centre();
    if dir.len_sq() < 1e-24 {
        dir = D3::new(1.0, 0.0, 0.0);
    }
    // Simplex of (w = sa - sb, sa, sb).
    let mut simplex: Vec<(D3, D3, D3)> = Vec::with_capacity(4);
    let sa = a.support(-dir);
    let sb = b.support(dir);
    let mut x = sa - sb;
    simplex.push((x, sa, sb));
    let mut lambdas: Vec<(usize, f64)> = vec![(0, 1.0)];
    for _ in 0..96 {
        let xl = x.len_sq();
        if xl < 1e-24 {
            return Gjk::Overlap;
        }
        let sa = a.support(-x);
        let sb = b.support(x);
        let w = sa - sb;
        let xw = x.dot(w);
        if xw > 0.0 && xw * xw > xl * limit_sq {
            return Gjk::Separated;
        }
        // Converged: no support point gets meaningfully closer.
        if xl - xw <= 1e-14 * xl.max(1e-12)
            || simplex.iter().any(|(sw, _, _)| (*sw - w).len_sq() < 1e-24)
        {
            break;
        }
        simplex.push((w, sa, sb));
        let ws: Vec<D3> = simplex.iter().map(|s| s.0).collect();
        let (nx, sel, full) = simplex_closest(&ws);
        if full {
            return Gjk::Overlap;
        }
        if nx.len_sq() >= xl {
            // No progress (numerical): keep the previous answer.
            simplex.pop();
            break;
        }
        simplex = sel.iter().map(|(i, _)| simplex[*i]).collect();
        lambdas = sel.iter().enumerate().map(|(k, (_, l))| (k, *l)).collect();
        x = nx;
    }
    let mut pa = D3::ZERO;
    let mut pb = D3::ZERO;
    for (i, l) in &lambdas {
        pa = pa + simplex[*i].1 * *l;
        pb = pb + simplex[*i].2 * *l;
    }
    let dist_sq = x.len_sq();
    if dist_sq > limit_sq {
        return Gjk::Separated;
    }
    Gjk::Closest { dist_sq, pa, pb }
}

// ===========================================================================
// Contacts
// ===========================================================================

/// Jolt's `CollideShapeResult`, in world space.
#[derive(Debug, Clone)]
struct Hit {
    /// Contact point on the character (`mContactPointOn1`).
    p1: D3,
    /// Contact point on the obstacle (`mContactPointOn2`).
    p2: D3,
    /// Unit direction from the character into the obstacle
    /// (`mPenetrationAxis`, normalised).
    axis: D3,
    /// Positive overlap, negative separation (down to `-max_separation`).
    depth: f64,
    /// The obstacle's supporting face (`mShape2Face`).
    face: Vec<D3>,
    body_index: usize,
}

/// A character part's full shape, for the deep-overlap fallback.
struct FullShape<'s> {
    shape: &'s Shape3D,
    pos: Vector3,
    rot: Quaternion,
}

/// Jolt `CollideConvexVsConvex` / `CollideConvexVsTriangles` for one pair.
fn collide_pair(
    a: &Part,
    a_full: &FullShape<'_>,
    b: &Part,
    b_full: Option<&FullShape<'_>>,
    max_sep: f64,
    body_index: usize,
) -> Option<Hit> {
    // Triangles: Jolt ignores back faces by default (`mBackFaceMode`), judged
    // from the character's centre (shape 1's COM).
    if let Core::Triangle(t) = &b.core {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.dot(t[0] - a.pos) > 0.0 {
            return None;
        }
    }
    let r = a.radius + b.radius;
    if let Core::Plane { n, d } = &b.core {
        let p = a.support(-*n);
        let dist = n.dot(p) - d; // core distance to the plane, + in front
        let depth = r - dist;
        if depth + max_sep < 0.0 {
            return None;
        }
        let axis = -*n;
        let p1 = p + axis * a.radius;
        let p2 = p - *n * dist;
        return Some(Hit {
            p1,
            p2,
            axis,
            depth,
            face: b.supporting_face(axis),
            body_index,
        });
    }
    let limit = r + max_sep;
    match gjk(a, b, limit * limit) {
        Gjk::Separated => None,
        Gjk::Closest { dist_sq, pa, pb } if dist_sq > 0.0 => {
            let dist = dist_sq.sqrt();
            let axis = (pb - pa) * (1.0 / dist);
            let p1 = pa + axis * a.radius;
            let p2 = pb - axis * b.radius;
            Some(Hit {
                p1,
                p2,
                axis,
                depth: r - dist,
                face: b.supporting_face(axis),
                body_index,
            })
        }
        _ => {
            // Deep overlap: Jolt runs EPA on the full shapes. Patina's own
            // narrow phase answers the same question on the same shapes.
            let res = match (&b.core, b_full) {
                (_, Some(bf)) => collision::test_collision_rot(
                    a_full.pos,
                    a_full.rot,
                    a_full.shape,
                    bf.pos,
                    bf.rot,
                    bf.shape,
                ),
                (Core::Triangle(t), None) => {
                    // Jolt winding back to Godot's for the soup.
                    let tri = Shape3D::ConcavePolygonShape {
                        faces: vec![t[0].to_f32(), t[2].to_f32(), t[1].to_f32()],
                        backface_collision: true,
                    };
                    collision::test_collision_rot(
                        a_full.pos,
                        a_full.rot,
                        a_full.shape,
                        Vector3::ZERO,
                        Quaternion::IDENTITY,
                        &tri,
                    )
                }
                _ => return None,
            };
            if !res.colliding {
                return None;
            }
            let mut axis = D3::from(res.normal);
            let l = axis.len();
            if l < 1e-12 {
                axis = D3::new(0.0, -1.0, 0.0);
            } else {
                axis = axis * (1.0 / l);
            }
            let depth = res.depth as f64;
            let p2 = a.support(axis) - axis * depth;
            let p1 = p2 + axis * depth;
            Some(Hit {
                p1,
                p2,
                axis,
                depth,
                face: b.supporting_face(axis),
                body_index,
            })
        }
    }
}

/// Jolt's `InternalEdgeRemovingCollector`, flushed once per group.
fn remove_internal_edges(hits: Vec<Hit>, out: &mut Vec<Hit>) {
    let mut voided: Vec<D3> = Vec::new();
    let is_voided = |voided: &Vec<D3>, v: D3| voided.iter().any(|f| (*f - v).len_sq() <= 1e-16);
    let void_face = |voided: &mut Vec<D3>, face: &[D3]| {
        for v in face {
            if !voided.iter().any(|f| (*f - *v).len_sq() <= 1e-16) {
                voided.push(*v);
            }
        }
    };
    let mut delayed: Vec<Hit> = Vec::new();
    for h in hits {
        if h.face.len() < 3 {
            void_face(&mut voided, &h.face);
            out.push(h);
            continue;
        }
        let tn = (h.face[1] - h.face[0]).cross(h.face[2] - h.face[0]);
        let tl = tn.len();
        if tl < 1e-6 {
            void_face(&mut voided, &h.face);
            out.push(h);
            continue;
        }
        let contact_normal = -h.axis;
        if tn.dot(contact_normal) > 0.999848 * tl {
            void_face(&mut voided, &h.face);
            out.push(h);
            continue;
        }
        delayed.push(h);
    }
    // Biggest penetration first (Jolt's QuickSort is not stable; ties keep
    // insertion order here).
    delayed.sort_by(|a, b| {
        b.depth
            .partial_cmp(&a.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for r in delayed {
        let num_v = r.face.len();
        let mut best_dist_sq = f64::MAX;
        let (mut best_v1, mut best_v2) = (0usize, 0usize);
        let mut v1_idx = num_v - 1;
        let mut v1 = r.face[v1_idx] - r.p2;
        for v2_idx in 0..num_v {
            let v2 = r.face[v2_idx] - r.p2;
            let v1_v2 = v2 - v1;
            let denom = v1_v2.len_sq();
            if denom < (f32::EPSILON as f64).powi(2) {
                let l = v1.len_sq();
                if l < best_dist_sq {
                    best_dist_sq = l;
                    best_v1 = v1_idx;
                    best_v2 = v1_idx;
                }
            } else {
                let fraction = -v1.dot(v1_v2) / denom;
                if fraction < 1.0e-6 {
                    let l = v1.len_sq();
                    if l < best_dist_sq {
                        best_dist_sq = l;
                        best_v1 = v1_idx;
                        best_v2 = v1_idx;
                    }
                } else if fraction < 1.0 - 1.0e-6 {
                    let closest = v1 + v1_v2 * fraction;
                    let l = closest.len_sq();
                    if l < best_dist_sq {
                        best_dist_sq = l;
                        best_v1 = v1_idx;
                        best_v2 = v2_idx;
                    }
                }
            }
            v1_idx = v2_idx;
            v1 = v2;
        }
        let voided_here = is_voided(&voided, r.face[best_v1])
            && (best_v1 == best_v2 || is_voided(&voided, r.face[best_v2]));
        let face = r.face.clone();
        if !voided_here {
            out.push(r);
        }
        void_face(&mut voided, &face);
    }
}

// ===========================================================================
// The query
// ===========================================================================

/// The moving body: its one shape and where that sits on the body.
#[derive(Debug, Clone, Copy)]
pub struct MotionShape<'a> {
    /// The collision shape.
    pub shape: &'a Shape3D,
    /// Shape position relative to the body origin, world-aligned.
    pub offset: Vector3,
    /// Shape rotation in the world.
    pub rotation: Quaternion,
    /// Layers the body collides with.
    pub collision_mask: u32,
    /// Bodies that Godot would hold as ONE body (a GridMap octant), by group
    /// key. Jolt's internal-edge removal runs per body; a body absent from the
    /// map is its own group.
    pub groups: Option<&'a HashMap<BodyId3D, u64>>,
}

/// `PhysicsServer3D::MotionParameters`.
#[derive(Debug, Clone)]
pub struct MotionParameters {
    /// Body origin (Godot's `from.origin`; the rotation lives on the shape).
    pub from: Vector3,
    /// The motion to test.
    pub motion: Vector3,
    /// The safe margin.
    pub margin: f32,
    /// How many collisions to report (Godot caps it at 32).
    pub max_collisions: usize,
    /// Report contacts found only by the recovery pass as collisions.
    pub recovery_as_collision: bool,
    /// Bodies the query ignores.
    pub exclude_bodies: Vec<BodyId3D>,
}

impl MotionParameters {
    /// Godot's `MotionParameters(from, motion, margin)`: one collision, no
    /// recovery-as-collision.
    pub fn new(from: Vector3, motion: Vector3, margin: f32) -> Self {
        Self {
            from,
            motion,
            margin,
            max_collisions: 1,
            recovery_as_collision: false,
            exclude_bodies: Vec::new(),
        }
    }
}

/// `PhysicsServer3D::MotionCollision`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionCollision {
    /// Contact point on the collider.
    pub position: Vector3,
    /// Pointing away from the collider, toward the body.
    pub normal: Vector3,
    /// The collider's velocity at the contact point.
    pub collider_velocity: Vector3,
    /// The collider's angular velocity.
    pub collider_angular_velocity: Vector3,
    /// Penetration depth plus the margin.
    pub depth: f32,
    /// The body hit.
    pub collider: BodyId3D,
    /// Index into the `bodies` slice the query was given.
    pub collider_index: usize,
    /// The collider's collision layer.
    pub collider_layer: u32,
}

impl MotionCollision {
    /// `MotionCollision::get_angle` (Godot's `Math::acos` clamps).
    pub fn get_angle(&self, up: Vector3) -> f32 {
        self.normal.dot(up).clamp(-1.0, 1.0).acos()
    }
}

/// `PhysicsServer3D::MotionResult`.
#[derive(Debug, Clone)]
pub struct MotionResult {
    /// How far the body moved (recovery included).
    pub travel: Vector3,
    /// The motion left after the collision.
    pub remainder: Vector3,
    /// Depth of the deepest collision.
    pub collision_depth: f32,
    /// Last fraction of the motion that does not touch.
    pub collision_safe_fraction: f32,
    /// First fraction of the motion that touches.
    pub collision_unsafe_fraction: f32,
    /// Collisions, deepest first.
    pub collisions: Vec<MotionCollision>,
}

/// One obstacle's convex pieces near the query, with what the fallback needs.
struct Obstacle<'b> {
    body_index: usize,
    group: (bool, u64),
    parts: Vec<(Part, Option<FullShape<'b>>)>,
}

/// Gathers every obstacle piece within `reach` of the sphere `(centre, radius)`.
fn gather<'b>(
    bodies: &[&'b PhysicsBody3D],
    groups: Option<&HashMap<BodyId3D, u64>>,
    mask: u32,
    exclude: &[BodyId3D],
    centre: Vector3,
    radius: f32,
) -> Vec<Obstacle<'b>> {
    let mut out = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        if (mask & body.collision_layer) == 0 || exclude.contains(&body.id) {
            continue;
        }
        let primary = (
            body.position + body.rotation.xform(body.shape_offset),
            body.rotation * body.shape_rotation,
            &body.shape,
        );
        let extras = body.extra_shapes.iter().map(|(off, rot, shape)| {
            (
                body.position + body.rotation.xform(*off),
                body.rotation * *rot,
                shape,
            )
        });
        let mut parts: Vec<(Part, Option<FullShape<'b>>)> = Vec::new();
        for (pos, rot, shape) in std::iter::once(primary).chain(extras) {
            let (sc, sr) = bounding_sphere(shape);
            let c = pos + rot.xform(sc);
            let reach = sr + radius;
            if sr.is_finite() && (c - centre).length_squared() > reach * reach {
                continue;
            }
            match shape {
                Shape3D::WorldBoundaryShape { normal, distance } => {
                    let n = D3::from(rot.xform(*normal));
                    let l = n.len().max(1e-12);
                    let n = n * (1.0 / l);
                    // Plane in local frame: n.p = distance; world offset by pos.
                    let d = *distance as f64 + n.dot(D3::from(pos));
                    parts.push((
                        Part {
                            core: Core::Plane { n, d },
                            pos: D3::from(pos),
                            rot: M3::IDENTITY,
                            radius: 0.0,
                            motion: D3::ZERO,
                        },
                        None,
                    ));
                }
                Shape3D::ConcavePolygonShape {
                    faces,
                    backface_collision,
                } => {
                    push_triangles(
                        &mut parts,
                        faces,
                        pos,
                        rot,
                        *backface_collision,
                        centre,
                        radius,
                    );
                }
                Shape3D::HeightMapShape {
                    map_width,
                    map_depth,
                    map_data,
                } => {
                    let local = rot.inverse().xform(centre - pos);
                    let faces = crate::shape::heightmap_faces(
                        *map_width,
                        *map_depth,
                        map_data,
                        Some((
                            local.x - radius,
                            local.x + radius,
                            local.z - radius,
                            local.z + radius,
                        )),
                    );
                    push_triangles(&mut parts, &faces, pos, rot, false, centre, radius);
                }
                Shape3D::SeparationRayShape { .. } => {}
                _ => {
                    if let Some(p) = convex_part(shape, pos, rot) {
                        parts.push((p, Some(FullShape { shape, pos, rot })));
                    }
                }
            }
        }
        if !parts.is_empty() {
            out.push(Obstacle {
                body_index: i,
                group: match groups.and_then(|g| g.get(&body.id)) {
                    Some(g) => (true, *g),
                    None => (false, i as u64),
                },
                parts,
            });
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn push_triangles<'b>(
    parts: &mut Vec<(Part, Option<FullShape<'b>>)>,
    faces: &[Vector3],
    pos: Vector3,
    rot: Quaternion,
    backface: bool,
    centre: Vector3,
    radius: f32,
) {
    let m = M3::from_quat(rot);
    let p = D3::from(pos);
    let c = D3::from(centre);
    let r = radius as f64;
    for tri in faces.chunks_exact(3) {
        // Godot winding (a, b, c) -> Jolt (a, c, b).
        let a = p + m.xform(D3::from(tri[0]));
        let b = p + m.xform(D3::from(tri[1]));
        let cc = p + m.xform(D3::from(tri[2]));
        let lo = D3::new(
            a.x.min(b.x).min(cc.x),
            a.y.min(b.y).min(cc.y),
            a.z.min(b.z).min(cc.z),
        );
        let hi = D3::new(
            a.x.max(b.x).max(cc.x),
            a.y.max(b.y).max(cc.y),
            a.z.max(b.z).max(cc.z),
        );
        if c.x + r < lo.x
            || c.x - r > hi.x
            || c.y + r < lo.y
            || c.y - r > hi.y
            || c.z + r < lo.z
            || c.z - r > hi.z
        {
            continue;
        }
        let mk = |t: [D3; 3]| Part {
            core: Core::Triangle(t),
            pos: D3::ZERO,
            rot: M3::IDENTITY,
            radius: 0.0,
            motion: D3::ZERO,
        };
        parts.push((mk([a, cc, b]), None));
        if backface {
            parts.push((mk([a, b, cc]), None));
        }
    }
}

/// A conservative bounding sphere for `shape` in its own frame.
pub(crate) fn bounding_sphere(shape: &Shape3D) -> (Vector3, f32) {
    if matches!(shape, Shape3D::WorldBoundaryShape { .. }) {
        return (Vector3::ZERO, f32::INFINITY);
    }
    let aabb = shape.bounding_aabb();
    let half = aabb.size * 0.5;
    (aabb.position + half, half.length())
}

/// The character's part at body origin `at`, with an optional sweep.
fn character_part(shape: &MotionShape<'_>, at: Vector3) -> Option<(Part, Vector3)> {
    let pos = at + shape.offset;
    convex_part(shape.shape, pos, shape.rotation).map(|p| (p, pos))
}

/// All contacts of the character at `at` within `max_sep`, through the
/// edge-removal collector.
fn collide_all(
    shape: &MotionShape<'_>,
    at: Vector3,
    obstacles: &[Obstacle<'_>],
    max_sep: f64,
) -> Vec<Hit> {
    let Some((a, apos)) = character_part(shape, at) else {
        return Vec::new();
    };
    let a_full = FullShape {
        shape: shape.shape,
        pos: apos,
        rot: shape.rotation,
    };
    let mut groups: Vec<((bool, u64), Vec<Hit>)> = Vec::new();
    for ob in obstacles {
        for (part, full) in &ob.parts {
            if let Some(h) = collide_pair(&a, &a_full, part, full.as_ref(), max_sep, ob.body_index)
            {
                match groups.iter_mut().find(|(g, _)| *g == ob.group) {
                    Some((_, v)) => v.push(h),
                    None => groups.push((ob.group, vec![h])),
                }
            }
        }
    }
    let mut out = Vec::new();
    for (_, hits) in groups {
        remove_internal_edges(hits, &mut out);
    }
    out
}

/// `_body_motion_recover`.
fn recover(
    shape: &MotionShape<'_>,
    from: Vector3,
    margin: f32,
    obstacles: &[Obstacle<'_>],
) -> (bool, Vector3) {
    let mut origin = D3::from(from);
    let mut total = D3::ZERO;
    let mut recovered = false;
    for _ in 0..RECOVERY_ITERATIONS {
        let hits = collide_all(shape, origin.to_f32(), obstacles, margin as f64);
        if hits.is_empty() {
            break;
        }
        recovered = true;
        let mut recovery = D3::ZERO;
        for h in &hits {
            let margin_offset = h.axis * margin as f64;
            let point_on_1 = h.p1 + margin_offset;
            let distance_to_1 = h.axis.dot(point_on_1 + recovery);
            let distance_to_2 = h.axis.dot(h.p2);
            let depth = (distance_to_1 - distance_to_2) as f32;
            if depth <= 0.0 {
                continue;
            }
            // Collision priority is 1 for every Patina body.
            let dist = depth * RECOVERY_AMOUNT;
            recovery = recovery - h.axis * dist as f64;
        }
        if recovery == D3::ZERO {
            break;
        }
        total = total + recovery;
        origin = origin + recovery;
    }
    (recovered, total.to_f32())
}

/// `_cast_motion_impl` over every obstacle: `(collided, safe, unsafe)`.
fn cast(
    shape: &MotionShape<'_>,
    from: Vector3,
    motion: Vector3,
    obstacles: &[Obstacle<'_>],
) -> (bool, f32, f32) {
    let mut closest_safe = 1.0f32;
    let mut closest_unsafe = 1.0f32;
    let motion_length = motion.length();
    let Some((base, _)) = character_part(shape, from) else {
        return (false, 1.0, 1.0);
    };
    let m = D3::from(motion);
    let step_count = ((1000.0 * motion_length).ln() / std::f32::consts::LN_2) as i32;
    let step_count = step_count.clamp(4, 16);
    let collides = |part: &Part, f: f32| -> bool {
        let mut a = base.clone();
        a.motion = m * f as f64;
        if let Core::Triangle(t) = &part.core {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if n.dot(t[0] - a.pos) > 0.0 {
                return false;
            }
        }
        let r = a.radius + part.radius;
        if let Core::Plane { n, d } = &part.core {
            let p = a.support(-*n);
            return r - (n.dot(p) - d) >= 0.0;
        }
        !matches!(gjk(&a, part, r * r), Gjk::Separated)
    };
    let mut collided = false;
    for ob in obstacles {
        let any = |f: f32| ob.parts.iter().any(|(p, _)| collides(p, f));
        if !any(1.0) {
            continue;
        }
        let mut lo = 0.0f32;
        let mut hi = 1.0f32;
        let mut coeff = 0.5f32;
        for j in 0..step_count {
            let fraction = lo + (hi - lo) * coeff;
            if any(fraction) {
                collided = true;
                hi = fraction;
                coeff = if j == 0 || lo > 0.0 { 0.5 } else { 0.25 };
            } else {
                lo = fraction;
                coeff = if j == 0 || hi < 1.0 { 0.5 } else { 0.75 };
            }
        }
        if lo < closest_safe {
            closest_safe = lo;
            closest_unsafe = hi;
        }
    }
    (collided, closest_safe, closest_unsafe)
}

/// `_body_motion_collide`.
fn collide(
    shape: &MotionShape<'_>,
    at: Vector3,
    motion: Vector3,
    margin: f32,
    max_collisions: usize,
    bodies: &[&PhysicsBody3D],
    obstacles: &[Obstacle<'_>],
) -> Vec<MotionCollision> {
    if max_collisions == 0 {
        return Vec::new();
    }
    let hits = collide_all(shape, at, obstacles, margin as f64);
    // `JoltQueryCollectorClosestMulti`: deepest first, ties in arrival order,
    // capped at `max_collisions` hits.
    let mut sorted: Vec<Hit> = Vec::new();
    for h in hits {
        let pos = sorted
            .iter()
            .position(|e| -h.depth < -e.depth)
            .unwrap_or(sorted.len());
        sorted.insert(pos, h);
        sorted.truncate(max_collisions);
    }
    let dir = if motion.length_squared() > 0.0 {
        Some(motion.normalized())
    } else {
        None
    };
    let mut out = Vec::new();
    for h in sorted {
        let depth = h.depth as f32 + margin;
        if depth <= 0.0 {
            continue;
        }
        let normal = (-h.axis).to_f32();
        if let Some(d) = dir {
            if d.dot(normal) >= -CMP_EPSILON {
                continue;
            }
        }
        let body = bodies[h.body_index];
        let position = h.p2.to_f32();
        let collider_velocity =
            body.linear_velocity + body.angular_velocity.cross(position - body.position);
        out.push(MotionCollision {
            position,
            normal,
            collider_velocity,
            collider_angular_velocity: body.angular_velocity,
            depth,
            collider: body.id,
            collider_index: h.body_index,
            collider_layer: body.collision_layer,
        });
        if out.len() >= max_collisions {
            break;
        }
    }
    out
}

/// `JoltPhysicsDirectSpaceState3D::body_test_motion`.
pub fn body_test_motion(
    shape: &MotionShape<'_>,
    bodies: &[&PhysicsBody3D],
    p: &MotionParameters,
) -> (bool, MotionResult) {
    let margin = p.margin.max(0.0001);
    let max_collisions = p.max_collisions.min(32);

    // Everything the query can reach: the shape's sphere swept along the
    // motion, plus the margin and the recovery (bounded by the shape size).
    let (sc, sr) = bounding_sphere(shape.shape);
    let start = p.from + shape.offset + shape.rotation.xform(sc);
    let centre = start + p.motion * 0.5;
    let reach = sr * 2.0 + p.motion.length() * 0.5 + margin + 0.1;
    let obstacles = gather(
        bodies,
        shape.groups,
        shape.collision_mask,
        &p.exclude_bodies,
        centre,
        reach,
    );

    let (recovered, recovery) = recover(shape, p.from, margin, &obstacles);
    let from = p.from + recovery;
    let (hit, safe, unsafe_) = cast(shape, from, p.motion, &obstacles);

    let mut collisions = Vec::new();
    if hit || (recovered && p.recovery_as_collision) {
        collisions = collide(
            shape,
            from + p.motion * unsafe_,
            p.motion,
            margin,
            max_collisions,
            bodies,
            &obstacles,
        );
    }
    let collided = !collisions.is_empty();
    let result = if collided {
        MotionResult {
            travel: recovery + p.motion * safe,
            remainder: p.motion - p.motion * safe,
            collision_depth: collisions[0].depth,
            collision_safe_fraction: safe,
            collision_unsafe_fraction: unsafe_,
            collisions,
        }
    } else {
        MotionResult {
            travel: recovery + p.motion,
            remainder: Vector3::ZERO,
            collision_depth: 0.0,
            collision_safe_fraction: 1.0,
            collision_unsafe_fraction: 1.0,
            collisions: Vec::new(),
        }
    };
    (collided, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyType3D;

    fn floor_box(top: f32) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Static,
            Vector3::new(0.0, top - 1.0, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(1.0, 1.0, 1.0),
            },
            1.0,
        )
    }

    fn capsule() -> Shape3D {
        Shape3D::CapsuleShape {
            radius: 0.4,
            height: 2.1,
        }
    }

    fn ms(shape: &Shape3D) -> MotionShape<'_> {
        MotionShape {
            shape,
            offset: Vector3::new(0.0, 1.05, 0.0),
            rotation: Quaternion::IDENTITY,
            collision_mask: 1,
            groups: None,
        }
    }

    #[test]
    fn gjk_capsule_over_box_face_is_the_vertical_gap() {
        let floor = floor_box(-4.0);
        let cap = capsule();
        let shape = ms(&cap);
        let hits = collide_all(
            &shape,
            Vector3::new(0.0, -4.0 + 0.0005, 0.0),
            &gather(&[&floor], None, 1, &[], Vector3::new(0.0, -3.0, 0.0), 5.0),
            0.001,
        );
        assert_eq!(hits.len(), 1);
        let h = &hits[0];
        assert!((h.depth + 0.0005).abs() < 1e-7, "depth {}", h.depth);
        assert!((h.axis.y + 1.0).abs() < 1e-9, "axis {:?}", h.axis);
    }

    #[test]
    fn box_edges_are_rounded_by_the_convex_radius() {
        // A sphere centred diagonally off the box's top edge touches the
        // ROUNDED edge (radius 0.04), not the sharp corner.
        let b = floor_box(0.0);
        let s = Shape3D::Sphere { radius: 0.5 };
        let shape = MotionShape {
            shape: &s,
            offset: Vector3::ZERO,
            rotation: Quaternion::IDENTITY,
            collision_mask: 1,
            groups: None,
        };
        // Sharp corner at (1, 0). Rounded-edge centre at (0.96, -0.04).
        let c = Vector3::new(1.0 + 0.3, 0.3, 0.0);
        let hits = collide_all(&shape, c, &gather(&[&b], None, 1, &[], c, 3.0), 0.5);
        assert_eq!(hits.len(), 1);
        let edge_centre = D3::new(0.96, -0.04, 0.0);
        let want_depth = 0.54 - (D3::from(c) - edge_centre).len();
        assert!(
            (hits[0].depth - want_depth).abs() < 1e-6,
            "{} vs {}",
            hits[0].depth,
            want_depth
        );
    }

    #[test]
    fn recovery_pushes_out_forty_percent_four_times() {
        let floor = floor_box(-4.0);
        let cap = capsule();
        let shape = ms(&cap);
        let obs = gather(&[&floor], None, 1, &[], Vector3::new(0.0, -3.0, 0.0), 5.0);
        // 0.0005 deep into the margin: pen = 0.001 - 0.0005.
        let (rec, r) = recover(&shape, Vector3::new(0.0, -4.0 + 0.0005, 0.0), 0.001, &obs);
        assert!(rec);
        let want = 0.0005 * (1.0 - 0.6f32.powi(4));
        // f32 positions near y = -4 carry ~5e-7 of rounding.
        assert!((r.y - want).abs() < 1e-6, "{} vs {}", r.y, want);
    }

    #[test]
    fn walking_cast_brackets_to_a_thirty_second() {
        let floor = floor_box(-4.0);
        let cap = capsule();
        let shape = ms(&cap);
        let obs = gather(&[&floor], None, 1, &[], Vector3::new(0.0, -3.0, 0.0), 5.0);
        let s1 = 0.000883;
        let motion = Vector3::new(6.0666667 / 120.0, -22.0 / 120.0 / 120.0, 0.0);
        let (hit, safe, unsafe_) = cast(&shape, Vector3::new(0.0, -4.0 + s1, 0.0), motion, &obs);
        assert!(hit);
        // Touch at f = 0.578: the 5-step biased search brackets [0.5625, 0.59375].
        assert_eq!((safe, unsafe_), (0.5625, 0.59375));
    }

    #[test]
    fn hull_faces_and_shrink_of_a_cube() {
        let pts: Vec<Vector3> = (0..8)
            .map(|i| {
                Vector3::new(
                    if i & 1 == 0 { -1.0 } else { 1.0 },
                    if i & 2 == 0 { -1.0 } else { 1.0 },
                    if i & 4 == 0 { -1.0 } else { 1.0 },
                )
            })
            .collect();
        let h = build_hull(&pts);
        assert_eq!(h.faces.len(), 6);
        assert!((h.convex_radius - 0.04).abs() < 1e-9, "{}", h.convex_radius);
        for p in &h.shrunk {
            assert!((p.x.abs() - 0.96).abs() < 1e-9 && (p.y.abs() - 0.96).abs() < 1e-9);
        }
    }

    #[test]
    fn internal_edge_of_two_flush_boxes_is_removed() {
        // Two 2 m floor boxes meeting at x = 0, in one group (a GridMap
        // octant). A sphere
        // sitting right over the seam touches both rounded edges; Jolt keeps
        // the deeper one and voids the other, whose edge is the first box's.
        let mut a = floor_box(0.0);
        a.position.x = -1.0;
        let mut b = floor_box(0.0);
        b.id = BodyId3D(2);
        b.position.x = 1.0;
        let s = Shape3D::Sphere { radius: 0.4 };
        let shape = MotionShape {
            shape: &s,
            offset: Vector3::ZERO,
            rotation: Quaternion::IDENTITY,
            collision_mask: 1,
            groups: None,
        };
        let one_group: HashMap<BodyId3D, u64> = [(BodyId3D(1), 7), (BodyId3D(2), 7)].into();
        let c = Vector3::new(0.01, 0.398, 0.0);
        let hits = collide_all(
            &shape,
            c,
            &gather(&[&a, &b], Some(&one_group), 1, &[], c, 3.0),
            0.001,
        );
        assert_eq!(hits.len(), 1, "{hits:?}");
        // As two separate bodies (Godot: two octants) both contacts stay.
        let hits = collide_all(&shape, c, &gather(&[&a, &b], None, 1, &[], c, 3.0), 0.001);
        assert_eq!(hits.len(), 2, "{hits:?}");
    }
}

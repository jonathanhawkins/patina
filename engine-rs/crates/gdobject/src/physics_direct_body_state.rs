//! `PhysicsDirectBodyState2D` / `PhysicsDirectBodyState3D` — the mutable view of
//! a body handed to `RigidBody2D/3D._integrate_forces(state)` (pat-s3rrv.94).
//!
//! ⚠️ **This module lives in `gdobject`, not in a physics crate, for a
//! dependency reason and not a conceptual one.** The state has to be reachable
//! from BOTH `gdscene` (which builds it during the physics tick) and
//! `gdscript-interop` (which routes a script's `state.foo()` calls onto it). The
//! interpreter's dependencies are `gdcore`, `gdvariant`, `gdobject`,
//! `gdresource`, `gdserver2d` and `gdplatform` — no physics crate is among them,
//! and `gdscene` depends on `gdscript-interop` rather than the other way round,
//! so putting this in `gdscene` or `gdphysics2d` would make the edge cyclic.
//! Nothing here needs a physics type: it is plain vectors, scalars and contacts.
//!
//! ⚠️ The state is a SNAPSHOT PLUS AN ACCUMULATOR, not a live handle onto the
//! body. Reads are filled in before the callback; writes (`linear_velocity`,
//! `angular_velocity`, `transform`) and the `apply_*` accumulators are read back
//! out afterwards and applied to the body before the step. A live borrow would
//! mean handing a script a `&mut` into the solver's own arrays.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gdcore::id::ObjectId;
use gdcore::math::{Transform2D, Vector2, Vector3};
use gdcore::math3d::{Basis, Transform3D};

/// Top byte marking an [`ObjectId`] as a `PhysicsDirectBodyState2D`.
///
/// Handle tags are a PROCESS-WIDE registry of one-byte prefixes — see the other
/// `*_HANDLE_TAG` constants across `gdcore`, `gdobject`, `gdresource`,
/// `gdplatform` and the interpreter. `0x6F` and `0x70` were swept as unused
/// against all 41 currently claimed bytes; reusing a taken one would make two
/// unrelated handle families answer each other's `is_*_handle` check and
/// dispatch into the wrong `call_*_instance`.
pub const DIRECT_BODY_STATE_2D_HANDLE_TAG: u64 = 0x6F00_0000_0000_0000;

/// Top byte marking an [`ObjectId`] as a `PhysicsDirectBodyState3D`.
pub const DIRECT_BODY_STATE_3D_HANDLE_TAG: u64 = 0x7000_0000_0000_0000;

/// Mask selecting the tag byte out of a raw id.
const TAG_MASK: u64 = 0xFF00_0000_0000_0000;

/// One contact reported to a script through the state.
///
/// Populated only when the node has `contact_monitor = true` and
/// `max_contacts_reported > 0`, which is Godot's own gate — a body that has not
/// opted in reports zero contacts even while it is visibly resting on something.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BodyContact2D {
    /// Contact point on THIS body, in its local space.
    pub local_position: Vector2,
    /// Contact normal, pointing away from this body's surface.
    pub local_normal: Vector2,
    /// Index of this body's shape that is touching.
    pub local_shape: i64,
    /// Impulse the solver applied at this contact.
    pub impulse: Vector2,
    /// The other body's server handle.
    pub collider: u64,
    /// The other body's object id, or 0 when it has no node.
    pub collider_id: i64,
    /// Contact point in the other body's space.
    pub collider_position: Vector2,
    /// Index of the other body's shape.
    pub collider_shape: i64,
    /// The other body's velocity at the contact point.
    pub collider_velocity: Vector2,
}

/// The 3D twin of [`BodyContact2D`].
///
/// ⚠️ `Default` is written out rather than derived: `Vector2` derives `Default`
/// in `gdcore` but `Vector3` does NOT, so the derive that compiles for the 2D
/// contact above does not compile here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyContact3D {
    /// Contact point on THIS body, in its local space.
    pub local_position: Vector3,
    /// Contact normal, pointing away from this body's surface.
    pub local_normal: Vector3,
    /// Index of this body's shape that is touching.
    pub local_shape: i64,
    /// Impulse the solver applied at this contact.
    pub impulse: Vector3,
    /// The other body's server handle.
    pub collider: u64,
    /// The other body's object id, or 0 when it has no node.
    pub collider_id: i64,
    /// Contact point in the other body's space.
    pub collider_position: Vector3,
    /// Index of the other body's shape.
    pub collider_shape: i64,
    /// The other body's velocity at the contact point.
    pub collider_velocity: Vector3,
}

impl Default for BodyContact3D {
    fn default() -> Self {
        let zero = Vector3::new(0.0, 0.0, 0.0);
        Self {
            local_position: zero,
            local_normal: zero,
            local_shape: 0,
            impulse: zero,
            collider: 0,
            collider_id: 0,
            collider_position: zero,
            collider_shape: 0,
            collider_velocity: zero,
        }
    }
}

/// `PhysicsDirectBodyState2D`, matching `doc/classes/PhysicsDirectBodyState2D.xml`.
///
/// Field names are the documented property names so the script-facing router can
/// map them one for one.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectBodyState2D {
    /// Physics timestep this callback covers, in seconds.
    pub step: f32,
    /// 1 / mass, or 0 for a body with infinite mass.
    pub inverse_mass: f32,
    /// 1 / moment of inertia.
    pub inverse_inertia: f32,
    /// Gravity acting on the body this step, already scaled.
    pub total_gravity: Vector2,
    /// Linear damping applied this step.
    pub total_linear_damp: f32,
    /// Angular damping applied this step.
    pub total_angular_damp: f32,
    /// Center of mass in GLOBAL space.
    pub center_of_mass: Vector2,
    /// Center of mass in the body's LOCAL space.
    ///
    /// ⚠️ `apply_impulse` measures its lever arm from this one, not from
    /// [`center_of_mass`](Self::center_of_mass) — upstream's
    /// `(p_position - center_of_mass).cross(p_impulse)` reads the local value.
    pub center_of_mass_local: Vector2,
    /// The body's collision layer bits.
    pub collision_layer: i64,
    /// The body's collision mask bits.
    pub collision_mask: i64,
    /// Linear velocity. **Writable** — a script assignment lands on the body.
    pub linear_velocity: Vector2,
    /// Angular velocity in rad/s. **Writable**.
    pub angular_velocity: f32,
    /// The body's transform. **Writable**.
    pub transform: Transform2D,
    /// Whether the body is asleep. **Writable**.
    pub sleeping: bool,
    /// Force accumulated by `apply_force` / `apply_central_force` this callback.
    pub applied_force: Vector2,
    /// Torque accumulated by `apply_torque` this callback.
    pub applied_torque: f32,
    /// Persistent force set by `add_constant_force` / `set_constant_force`.
    pub constant_force: Vector2,
    /// Persistent torque set by `add_constant_torque` / `set_constant_torque`.
    pub constant_torque: f32,
    /// Contacts, empty unless the node opted into contact monitoring.
    pub contacts: Vec<BodyContact2D>,
}

impl Default for DirectBodyState2D {
    fn default() -> Self {
        Self {
            step: 0.0,
            inverse_mass: 0.0,
            inverse_inertia: 0.0,
            total_gravity: Vector2::new(0.0, 0.0),
            total_linear_damp: 0.0,
            total_angular_damp: 0.0,
            center_of_mass: Vector2::new(0.0, 0.0),
            center_of_mass_local: Vector2::new(0.0, 0.0),
            collision_layer: 1,
            collision_mask: 1,
            linear_velocity: Vector2::new(0.0, 0.0),
            angular_velocity: 0.0,
            transform: Transform2D::IDENTITY,
            sleeping: false,
            applied_force: Vector2::new(0.0, 0.0),
            applied_torque: 0.0,
            constant_force: Vector2::new(0.0, 0.0),
            constant_torque: 0.0,
            contacts: Vec::new(),
        }
    }
}

impl DirectBodyState2D {
    /// `apply_central_impulse` — `linear_velocity += impulse * inverse_mass`
    /// (`godot_body_2d.h:228`).
    ///
    /// ⚠️ An impulse is an INSTANT velocity change scaled by inverse mass, not a
    /// force accumulated over the step. An infinite-mass body has
    /// `inverse_mass == 0` and correctly ignores it.
    pub fn apply_central_impulse(&mut self, impulse: Vector2) {
        self.linear_velocity = self.linear_velocity + impulse * self.inverse_mass;
    }

    /// `apply_impulse` at an offset — adds the angular term
    /// (`godot_body_2d.h:232`).
    ///
    /// ⚠️ In 2D the cross product of two vectors is a SCALAR, which is why the
    /// angular term is a plain float here and a `Vector3` in the 3D twin.
    pub fn apply_impulse(&mut self, impulse: Vector2, position: Vector2) {
        self.linear_velocity = self.linear_velocity + impulse * self.inverse_mass;
        self.angular_velocity +=
            self.inverse_inertia * (position - self.center_of_mass_local).cross(impulse);
    }

    /// `apply_torque_impulse` (`godot_body_2d.h:237`).
    pub fn apply_torque_impulse(&mut self, torque: f32) {
        self.angular_velocity += self.inverse_inertia * torque;
    }

    /// `apply_central_force` — accumulated over the step rather than applied now.
    pub fn apply_central_force(&mut self, force: Vector2) {
        self.applied_force = self.applied_force + force;
    }

    /// `apply_force` at an offset, which also produces torque.
    pub fn apply_force(&mut self, force: Vector2, position: Vector2) {
        self.applied_force = self.applied_force + force;
        self.applied_torque += (position - self.center_of_mass_local).cross(force);
    }

    /// `apply_torque`.
    pub fn apply_torque(&mut self, torque: f32) {
        self.applied_torque += torque;
    }

    /// `add_constant_central_force` — persists across steps until cleared.
    pub fn add_constant_central_force(&mut self, force: Vector2) {
        self.constant_force = self.constant_force + force;
    }

    /// `add_constant_force` at an offset.
    pub fn add_constant_force(&mut self, force: Vector2, position: Vector2) {
        self.constant_force = self.constant_force + force;
        self.constant_torque += (position - self.center_of_mass_local).cross(force);
    }

    /// `add_constant_torque`.
    pub fn add_constant_torque(&mut self, torque: f32) {
        self.constant_torque += torque;
    }

    /// `get_contact_count`.
    pub fn contact_count(&self) -> i64 {
        self.contacts.len() as i64
    }

    /// A contact by index, or `None` when out of range.
    ///
    /// ⚠️ Godot ERR_FAILs on an out-of-range index and returns a zero value
    /// rather than panicking, so callers report a default instead of trapping —
    /// a script indexing past the count is a script bug, not an engine abort.
    pub fn contact(&self, index: i64) -> Option<&BodyContact2D> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.contacts.get(i))
    }
}

/// `PhysicsDirectBodyState3D`, matching `doc/classes/PhysicsDirectBodyState3D.xml`.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectBodyState3D {
    /// Physics timestep this callback covers, in seconds.
    pub step: f32,
    /// 1 / mass, or 0 for a body with infinite mass.
    pub inverse_mass: f32,
    /// Per-axis inverse inertia.
    pub inverse_inertia: Vector3,
    /// Inverse inertia as a tensor — what `apply_impulse` actually uses.
    pub inverse_inertia_tensor: Basis,
    /// Body-space principal inertia axes.
    pub principal_inertia_axes: Basis,
    /// Gravity acting on the body this step, already scaled.
    pub total_gravity: Vector3,
    /// Linear damping applied this step.
    pub total_linear_damp: f32,
    /// Angular damping applied this step.
    pub total_angular_damp: f32,
    /// Center of mass in GLOBAL space.
    pub center_of_mass: Vector3,
    /// Center of mass in the body's LOCAL space.
    pub center_of_mass_local: Vector3,
    /// The body's collision layer bits.
    pub collision_layer: i64,
    /// The body's collision mask bits.
    pub collision_mask: i64,
    /// Linear velocity. **Writable**.
    pub linear_velocity: Vector3,
    /// Angular velocity in rad/s. **Writable**.
    pub angular_velocity: Vector3,
    /// The body's transform. **Writable**.
    pub transform: Transform3D,
    /// Whether the body is asleep. **Writable**.
    pub sleeping: bool,
    /// Force accumulated this callback.
    pub applied_force: Vector3,
    /// Torque accumulated this callback.
    pub applied_torque: Vector3,
    /// Persistent force.
    pub constant_force: Vector3,
    /// Persistent torque.
    pub constant_torque: Vector3,
    /// Contacts, empty unless the node opted into contact monitoring.
    pub contacts: Vec<BodyContact3D>,
}

impl Default for DirectBodyState3D {
    fn default() -> Self {
        Self {
            step: 0.0,
            inverse_mass: 0.0,
            inverse_inertia: Vector3::new(0.0, 0.0, 0.0),
            inverse_inertia_tensor: Basis::IDENTITY,
            principal_inertia_axes: Basis::IDENTITY,
            total_gravity: Vector3::new(0.0, 0.0, 0.0),
            total_linear_damp: 0.0,
            total_angular_damp: 0.0,
            center_of_mass: Vector3::new(0.0, 0.0, 0.0),
            center_of_mass_local: Vector3::new(0.0, 0.0, 0.0),
            collision_layer: 1,
            collision_mask: 1,
            linear_velocity: Vector3::new(0.0, 0.0, 0.0),
            angular_velocity: Vector3::new(0.0, 0.0, 0.0),
            transform: Transform3D::IDENTITY,
            sleeping: false,
            applied_force: Vector3::new(0.0, 0.0, 0.0),
            applied_torque: Vector3::new(0.0, 0.0, 0.0),
            constant_force: Vector3::new(0.0, 0.0, 0.0),
            constant_torque: Vector3::new(0.0, 0.0, 0.0),
            contacts: Vec::new(),
        }
    }
}

impl DirectBodyState3D {
    /// `apply_central_impulse` (`godot_body_3d.h:223`).
    pub fn apply_central_impulse(&mut self, impulse: Vector3) {
        self.linear_velocity = self.linear_velocity + impulse * self.inverse_mass;
    }

    /// `apply_impulse` at an offset (`godot_body_3d.h:227`).
    ///
    /// ⚠️ The angular term goes through the inverse inertia **tensor**, not a
    /// per-axis multiply: `_inv_inertia_tensor.xform(r.cross(impulse))`. Scaling
    /// the cross product component-wise by `inverse_inertia` would agree only
    /// for a body whose principal axes are already world-aligned, which is
    /// exactly the fixture a careless test would use.
    pub fn apply_impulse(&mut self, impulse: Vector3, position: Vector3) {
        self.linear_velocity = self.linear_velocity + impulse * self.inverse_mass;
        let r = position - self.center_of_mass_local;
        self.angular_velocity =
            self.angular_velocity + self.inverse_inertia_tensor.xform(r.cross(impulse));
    }

    /// `apply_torque_impulse` (`godot_body_3d.h:232`).
    pub fn apply_torque_impulse(&mut self, torque: Vector3) {
        self.angular_velocity = self.angular_velocity + self.inverse_inertia_tensor.xform(torque);
    }

    /// `apply_central_force`.
    pub fn apply_central_force(&mut self, force: Vector3) {
        self.applied_force = self.applied_force + force;
    }

    /// `apply_force` at an offset.
    pub fn apply_force(&mut self, force: Vector3, position: Vector3) {
        self.applied_force = self.applied_force + force;
        let r = position - self.center_of_mass_local;
        self.applied_torque = self.applied_torque + r.cross(force);
    }

    /// `apply_torque`.
    pub fn apply_torque(&mut self, torque: Vector3) {
        self.applied_torque = self.applied_torque + torque;
    }

    /// `add_constant_central_force`.
    pub fn add_constant_central_force(&mut self, force: Vector3) {
        self.constant_force = self.constant_force + force;
    }

    /// `add_constant_force` at an offset.
    pub fn add_constant_force(&mut self, force: Vector3, position: Vector3) {
        self.constant_force = self.constant_force + force;
        let r = position - self.center_of_mass_local;
        self.constant_torque = self.constant_torque + r.cross(force);
    }

    /// `add_constant_torque`.
    pub fn add_constant_torque(&mut self, torque: Vector3) {
        self.constant_torque = self.constant_torque + torque;
    }

    /// `get_contact_count`.
    pub fn contact_count(&self) -> i64 {
        self.contacts.len() as i64
    }

    /// A contact by index, or `None` when out of range.
    pub fn contact(&self, index: i64) -> Option<&BodyContact3D> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.contacts.get(i))
    }
}

// ---------------------------------------------------------------------------
// Handle tables
// ---------------------------------------------------------------------------

fn registry_2d() -> &'static Mutex<HashMap<u64, DirectBodyState2D>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, DirectBodyState2D>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn registry_3d() -> &'static Mutex<HashMap<u64, DirectBodyState3D>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, DirectBodyState3D>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_seq() -> u64 {
    static NEXT: OnceLock<Mutex<u64>> = OnceLock::new();
    let cell = NEXT.get_or_init(|| Mutex::new(1));
    let mut n = cell.lock().unwrap();
    let v = *n;
    *n += 1;
    v
}

/// Whether `id` names a `PhysicsDirectBodyState2D`.
pub fn is_direct_body_state_2d_handle(id: ObjectId) -> bool {
    id.raw() & TAG_MASK == DIRECT_BODY_STATE_2D_HANDLE_TAG
}

/// Whether `id` names a `PhysicsDirectBodyState3D`.
pub fn is_direct_body_state_3d_handle(id: ObjectId) -> bool {
    id.raw() & TAG_MASK == DIRECT_BODY_STATE_3D_HANDLE_TAG
}

/// Publishes `state` under a fresh handle for the duration of one callback.
pub fn new_direct_body_state_2d_handle(state: DirectBodyState2D) -> ObjectId {
    let id = ObjectId::from_raw(DIRECT_BODY_STATE_2D_HANDLE_TAG | next_seq());
    registry_2d().lock().unwrap().insert(id.raw(), state);
    id
}

/// The 3D twin of [`new_direct_body_state_2d_handle`].
pub fn new_direct_body_state_3d_handle(state: DirectBodyState3D) -> ObjectId {
    let id = ObjectId::from_raw(DIRECT_BODY_STATE_3D_HANDLE_TAG | next_seq());
    registry_3d().lock().unwrap().insert(id.raw(), state);
    id
}

/// Runs `f` against the state behind `id`, if it is still published.
pub fn with_direct_body_state_2d<R>(
    id: ObjectId,
    f: impl FnOnce(&mut DirectBodyState2D) -> R,
) -> Option<R> {
    let mut reg = registry_2d().lock().unwrap();
    reg.get_mut(&id.raw()).map(f)
}

/// The 3D twin of [`with_direct_body_state_2d`].
pub fn with_direct_body_state_3d<R>(
    id: ObjectId,
    f: impl FnOnce(&mut DirectBodyState3D) -> R,
) -> Option<R> {
    let mut reg = registry_3d().lock().unwrap();
    reg.get_mut(&id.raw()).map(f)
}

/// Removes the handle and returns the state, so the caller can apply whatever
/// the script wrote.
///
/// ⚠️ The handle MUST be taken back at the end of the callback. It is scoped to
/// one tick, and leaving it published would both leak a map entry per body per
/// frame and let a script stash the state and mutate it later, when nothing is
/// reading it back any more.
pub fn take_direct_body_state_2d(id: ObjectId) -> Option<DirectBodyState2D> {
    registry_2d().lock().unwrap().remove(&id.raw())
}

/// The 3D twin of [`take_direct_body_state_2d`].
pub fn take_direct_body_state_3d(id: ObjectId) -> Option<DirectBodyState3D> {
    registry_3d().lock().unwrap().remove(&id.raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🔑 The two tag families must not answer each other's predicate.
    ///
    /// ⚠️ A tag collision does not fail loudly — it dispatches a 3D state's
    /// method call into the 2D router, which then reads a handle that is not in
    /// its map and quietly reports "no such state".
    #[test]
    fn handle_tags_do_not_alias_each_other() {
        let two = new_direct_body_state_2d_handle(DirectBodyState2D::default());
        let three = new_direct_body_state_3d_handle(DirectBodyState3D::default());

        assert!(is_direct_body_state_2d_handle(two));
        assert!(!is_direct_body_state_3d_handle(two));
        assert!(is_direct_body_state_3d_handle(three));
        assert!(!is_direct_body_state_2d_handle(three));

        // …and neither answers for a plain small object id, which is what a real
        // node carries.
        let node = ObjectId::from_raw(7);
        assert!(!is_direct_body_state_2d_handle(node));
        assert!(!is_direct_body_state_3d_handle(node));
    }

    /// 🔑 A handle round-trips the script's writes back out.
    #[test]
    fn taking_the_handle_returns_what_the_callback_wrote() {
        let id = new_direct_body_state_2d_handle(DirectBodyState2D {
            inverse_mass: 0.5,
            linear_velocity: Vector2::new(1.0, 0.0),
            ..Default::default()
        });

        with_direct_body_state_2d(id, |s| s.linear_velocity = Vector2::new(0.0, -9.0))
            .expect("the handle is published");

        let taken = take_direct_body_state_2d(id).expect("the handle is still published");
        assert_eq!(taken.linear_velocity, Vector2::new(0.0, -9.0));
        assert!(
            take_direct_body_state_2d(id).is_none(),
            "taking must unpublish the handle; a second take means the entry \
             leaks and a script could keep mutating a state nothing reads back",
        );
        assert!(
            with_direct_body_state_2d(id, |_| ()).is_none(),
            "…and the handle must be dead to further calls",
        );
    }

    /// 🔑 An impulse scales by INVERSE mass, so a heavier body moves less.
    ///
    /// ⚠️ The `inverse_mass = 0` case is the discriminating one: infinite mass
    /// must swallow the impulse entirely. An implementation that divided by a
    /// stored `mass` instead would divide by zero here and produce an infinity
    /// that then poisons the body's position.
    #[test]
    fn central_impulse_scales_by_inverse_mass() {
        let mut light = DirectBodyState2D {
            inverse_mass: 1.0,
            ..Default::default()
        };
        light.apply_central_impulse(Vector2::new(10.0, 0.0));
        assert_eq!(light.linear_velocity, Vector2::new(10.0, 0.0));

        let mut heavy = DirectBodyState2D {
            inverse_mass: 0.25,
            ..Default::default()
        };
        heavy.apply_central_impulse(Vector2::new(10.0, 0.0));
        assert_eq!(heavy.linear_velocity, Vector2::new(2.5, 0.0));

        let mut immovable = DirectBodyState2D::default();
        immovable.apply_central_impulse(Vector2::new(10.0, 0.0));
        assert_eq!(
            immovable.linear_velocity,
            Vector2::new(0.0, 0.0),
            "inverse_mass = 0 is Godot's infinite mass and must ignore impulses \
             entirely, not divide by zero",
        );
    }

    /// 🔑 An offset impulse produces torque; a central one does not.
    ///
    /// ⚠️ The lever arm is measured from `center_of_mass_local`, so the paired
    /// case below — the same impulse applied AT the center of mass — must
    /// produce zero spin. Without that control, an implementation that ignored
    /// the center of mass entirely would still pass the first assertion.
    #[test]
    fn offset_impulse_spins_the_body_and_a_central_one_does_not() {
        let base = DirectBodyState2D {
            inverse_mass: 1.0,
            inverse_inertia: 2.0,
            center_of_mass_local: Vector2::new(1.0, 0.0),
            ..Default::default()
        };

        let mut offset = base.clone();
        offset.apply_impulse(Vector2::new(0.0, 3.0), Vector2::new(3.0, 0.0));
        // r = (3,0) - (1,0) = (2,0); r.cross(j) = 2*3 - 0*0 = 6; 6 * 2.0 = 12.
        assert_eq!(offset.angular_velocity, 12.0);
        assert_eq!(offset.linear_velocity, Vector2::new(0.0, 3.0));

        let mut central = base.clone();
        central.apply_impulse(Vector2::new(0.0, 3.0), Vector2::new(1.0, 0.0));
        assert_eq!(
            central.angular_velocity, 0.0,
            "an impulse applied exactly at the center of mass has no lever arm; \
             a non-zero spin here means the center of mass was ignored",
        );
        assert_eq!(central.linear_velocity, Vector2::new(0.0, 3.0));
    }

    /// 🔑 The 3D angular term goes through the inertia TENSOR.
    ///
    /// ⚠️ The tensor here is deliberately NON-uniform and NON-identity. With an
    /// identity tensor, `tensor.xform(v)` and `v` coincide, so the test would
    /// pass just as well against an implementation that skipped the tensor
    /// entirely — the classic coincidence-point fixture.
    #[test]
    fn three_d_impulse_applies_the_inverse_inertia_tensor() {
        let tensor = Basis {
            x: Vector3::new(2.0, 0.0, 0.0),
            y: Vector3::new(0.0, 4.0, 0.0),
            z: Vector3::new(0.0, 0.0, 8.0),
        };
        let mut s = DirectBodyState3D {
            inverse_mass: 1.0,
            inverse_inertia_tensor: tensor,
            ..Default::default()
        };

        // r = (1,0,0), impulse = (0,1,0) -> r x j = (0,0,1); tensor scales z by 8.
        s.apply_impulse(Vector3::new(0.0, 1.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(s.linear_velocity, Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(
            s.angular_velocity,
            Vector3::new(0.0, 0.0, 8.0),
            "the cross product (0,0,1) must be carried through the tensor, \
             giving (0,0,8); (0,0,1) here means the tensor was skipped",
        );
    }

    /// Forces accumulate rather than changing velocity on the spot.
    #[test]
    fn forces_accumulate_and_do_not_move_the_body_directly() {
        let mut s = DirectBodyState2D {
            inverse_mass: 1.0,
            inverse_inertia: 1.0,
            ..Default::default()
        };
        s.apply_central_force(Vector2::new(5.0, 0.0));
        s.apply_central_force(Vector2::new(0.0, 2.0));
        s.apply_torque(0.5);

        assert_eq!(s.applied_force, Vector2::new(5.0, 2.0), "forces sum");
        assert_eq!(s.applied_torque, 0.5);
        assert_eq!(
            s.linear_velocity,
            Vector2::new(0.0, 0.0),
            "a force is integrated over the step by the solver; it must NOT be \
             applied to the velocity here the way an impulse is",
        );
    }

    /// Contact access is bounds-safe.
    #[test]
    fn contact_access_is_bounds_safe() {
        let mut s = DirectBodyState2D::default();
        assert_eq!(s.contact_count(), 0);
        assert!(s.contact(0).is_none());

        s.contacts.push(BodyContact2D {
            local_normal: Vector2::new(0.0, -1.0),
            ..Default::default()
        });
        assert_eq!(s.contact_count(), 1);
        assert_eq!(
            s.contact(0).map(|c| c.local_normal),
            Some(Vector2::new(0.0, -1.0)),
        );
        assert!(s.contact(1).is_none(), "past the end reports nothing");
        assert!(s.contact(-1).is_none(), "a negative index reports nothing");
    }
}

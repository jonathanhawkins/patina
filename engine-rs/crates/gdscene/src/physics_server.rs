use crate::scene_tree::SceneTree;
pub struct PhysicsServer {
    _p: (),
}
impl std::fmt::Debug for PhysicsServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsServer").finish()
    }
}
impl Default for PhysicsServer {
    fn default() -> Self {
        Self { _p: () }
    }
}
impl PhysicsServer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn sync_to_physics(&mut self, _t: &SceneTree) {}
    pub fn step_physics(&mut self, _dt: f32) {}
    pub fn sync_from_physics(&mut self, _t: &mut SceneTree) {}
}

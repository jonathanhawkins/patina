use crate::scene_tree::SceneTree;

#[derive(Debug, Default)]
pub struct PhysicsServer {
    _p: (),
}
impl PhysicsServer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn sync_to_physics(&mut self, _t: &SceneTree) {}
    pub fn step_physics(&mut self, _dt: f32) {}
    pub fn sync_from_physics(&mut self, _t: &mut SceneTree) {}
}

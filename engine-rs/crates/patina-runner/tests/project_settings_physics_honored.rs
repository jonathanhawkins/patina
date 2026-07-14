//! Acceptance test for pat-nzgdu.15: booting patina-runner honors the physics
//! tick rate and 2D gravity declared in `project.godot`.
//!
//! Godot reads `physics/common/physics_ticks_per_second` and
//! `physics/2d/default_gravity` (× `default_gravity_vector`) at boot and
//! simulates at those values. Patina parsed them but never wired them into the
//! running `MainLoop`, so a project declaring 30 ticks / 490 gravity still ran
//! at the Godot defaults of 60 ticks / 980 gravity. This drives the project
//! boot path against a fixtures project and asserts the honored values reach the
//! `MainLoop` and the physics world, and that a `RigidBody2D` falls at the
//! project gravity — all without a manual `set_physics_ticks_per_second` /
//! `set_gravity` call on the honored loop.

use std::path::PathBuf;

use gdcore::math::Vector2;
use gdscene::{LifecycleManager, MainLoop, NodeId, SceneTree};
use patina_runner::project_boot::boot_project;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/physics_honor"
    ))
}

/// Boots the fixtures project the same way the runner does: parse `project.godot`,
/// enter_tree the main scene, build a `MainLoop`, honor `ProjectSettings`, then
/// register physics bodies so they simulate.
fn boot_main_loop() -> MainLoop {
    let booted = boot_project(&fixtures_dir()).expect("boot from project.godot");
    let mut tree = booted.tree;
    if let Some(main_root) = booted.main_scene_root {
        LifecycleManager::enter_tree(&mut tree, main_root);
    }
    let mut main_loop = MainLoop::new(tree);
    main_loop.configure_from_project_settings(&booted.settings);
    main_loop.register_physics_bodies();
    main_loop
}

/// Finds the `RigidBody2D` "Faller" node id in the booted tree.
fn faller_id(tree: &SceneTree) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|&id| tree.get_node(id).map(|n| n.name()) == Some("Faller"))
        .expect("Faller RigidBody2D exists in booted tree")
}

#[test]
fn project_settings_physics_honored() {
    let mut main_loop = boot_main_loop();

    // (a) Tick rate honored: project.godot declares 30, not the default 60.
    assert_eq!(
        main_loop.physics_ticks_per_second(),
        30,
        "physics tick rate should come from project.godot, not the default 60",
    );

    // (b) 2D world gravity honored: 490 (down), not the default 980.
    let gravity = main_loop.physics_server().gravity();
    assert_eq!(gravity.x, 0.0, "2D gravity x should be 0");
    assert!(
        (gravity.y - 490.0).abs() < 1e-4,
        "2D world gravity y should be 490 (from project.godot), got {}",
        gravity.y,
    );

    // (c) A RigidBody2D actually falls at the project gravity. Integration is
    //     linear in gravity, so a 490-gravity boot must fall exactly half as far
    //     as a Godot-default 980-gravity control over identical frames — proving
    //     the honored 490 (not 980) drives the simulation.
    let faller = faller_id(main_loop.tree());
    let start_y = gdscene::node2d::get_position(main_loop.tree(), faller).y;

    let dt = 1.0 / 30.0;
    for _ in 0..30 {
        main_loop.step(dt);
    }
    let fell_490 = gdscene::node2d::get_position(main_loop.tree(), faller).y - start_y;
    assert!(
        fell_490 > 0.0,
        "RigidBody2D should fall in +Y under gravity, moved {fell_490}",
    );

    // Control: identical boot + frames, but with the Godot-default 980 gravity
    // applied manually (this is the reference, not the honored path).
    let mut control = boot_main_loop();
    control
        .physics_server_mut()
        .set_gravity(Vector2::new(0.0, 980.0));
    let control_faller = faller_id(control.tree());
    let control_start = gdscene::node2d::get_position(control.tree(), control_faller).y;
    for _ in 0..30 {
        control.step(dt);
    }
    let fell_980 =
        gdscene::node2d::get_position(control.tree(), control_faller).y - control_start;

    assert!(fell_980 > 0.0, "control (980) should also fall");
    assert!(
        (fell_490 / fell_980 - 0.5).abs() < 0.01,
        "490-gravity fall ({fell_490}) should be ~half the 980-gravity fall ({fell_980})",
    );
}

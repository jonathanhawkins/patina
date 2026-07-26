//! pat-0q2kx.127 acceptance: the 2D leaf nodes carry their full Godot own-property
//! tables, with Godot's defaults, all the way through the `create_node` seeder.
//!
//! Godot ground truth: `upstream/godot/doc/classes/<Class>.xml` `<member …
//! default=…>`. Every name and every expected value below is transcribed from
//! there.
//!
//! `register_2d_classes()` registered Sprite2D, Camera2D, the particle pair, the
//! Line2D/Polygon2D pair, the Path/Bone/RemoteTransform nodes and the whole
//! Light2D family with `.parent()` and **zero** `.property()`. ClassDB
//! reflection therefore reported nothing class-specific, and
//! `SceneTree::create_node` — which seeds a fresh node from
//! `get_property_list` (`scene_tree.rs:3697`) — had nothing to seed. A freshly
//! created `Sprite2D` read back `Nil` for `centered`, where Godot says `true`.
//!
//! ⚠️ These assert **values**, not presence. A table that lists every property
//! name with the wrong default is exactly as wrong as a missing table, and only
//! a value check tells them apart.

use gdobject::class_db::{self, class_has_property, get_class_info};
use gdscene::SceneTree;
use gdvariant::Variant;

use gdcore::math::{Color, Rect2, Vector2, Vector2i};

/// Every own property Godot documents for each 2D leaf class, in doc order.
///
/// The *count* matters as much as the contents: it is what catches a table that
/// silently loses a property to a later edit.
fn own_property_names() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        (
            "Sprite2D",
            &[
                "centered",
                "flip_h",
                "flip_v",
                "frame",
                "frame_coords",
                "hframes",
                "offset",
                "region_enabled",
                "region_filter_clip_enabled",
                "region_rect",
                "texture",
                "vframes",
            ][..],
        ),
        (
            "Camera2D",
            &[
                "anchor_mode",
                "custom_viewport",
                "drag_bottom_margin",
                "drag_horizontal_enabled",
                "drag_horizontal_offset",
                "drag_left_margin",
                "drag_right_margin",
                "drag_top_margin",
                "drag_vertical_enabled",
                "drag_vertical_offset",
                "editor_draw_drag_margin",
                "editor_draw_limits",
                "editor_draw_screen",
                "enabled",
                "ignore_rotation",
                "limit_bottom",
                "limit_enabled",
                "limit_left",
                "limit_right",
                "limit_smoothed",
                "limit_top",
                "offset",
                "position_smoothing_enabled",
                "position_smoothing_speed",
                "process_callback",
                "rotation_smoothing_enabled",
                "rotation_smoothing_speed",
                "zoom",
            ][..],
        ),
        (
            "GPUParticles2D",
            &[
                "amount",
                "amount_ratio",
                "collision_base_size",
                "draw_order",
                "emitting",
                "explosiveness",
                "fixed_fps",
                "fract_delta",
                "interp_to_end",
                "interpolate",
                "lifetime",
                "local_coords",
                "one_shot",
                "preprocess",
                "process_material",
                "randomness",
                "seed",
                "speed_scale",
                "sub_emitter",
                "texture",
                "trail_enabled",
                "trail_lifetime",
                "trail_section_subdivisions",
                "trail_sections",
                "use_fixed_seed",
                "visibility_rect",
            ][..],
        ),
        (
            "CPUParticles2D",
            &[
                "amount",
                "angle_curve",
                "angle_max",
                "angle_min",
                "angular_velocity_curve",
                "angular_velocity_max",
                "angular_velocity_min",
                "anim_offset_curve",
                "anim_offset_max",
                "anim_offset_min",
                "anim_speed_curve",
                "anim_speed_max",
                "anim_speed_min",
                "color",
                "color_initial_ramp",
                "color_ramp",
                "damping_curve",
                "damping_max",
                "damping_min",
                "direction",
                "draw_order",
                "emission_colors",
                "emission_normals",
                "emission_points",
                "emission_rect_extents",
                "emission_ring_inner_radius",
                "emission_ring_radius",
                "emission_shape",
                "emission_sphere_radius",
                "emitting",
                "explosiveness",
                "fixed_fps",
                "fract_delta",
                "gravity",
                "hue_variation_curve",
                "hue_variation_max",
                "hue_variation_min",
                "initial_velocity_max",
                "initial_velocity_min",
                "lifetime",
                "lifetime_randomness",
                "linear_accel_curve",
                "linear_accel_max",
                "linear_accel_min",
                "local_coords",
                "one_shot",
                "orbit_velocity_curve",
                "orbit_velocity_max",
                "orbit_velocity_min",
                "particle_flag_align_y",
                "physics_interpolation_mode",
                "preprocess",
                "radial_accel_curve",
                "radial_accel_max",
                "radial_accel_min",
                "randomness",
                "scale_amount_curve",
                "scale_amount_max",
                "scale_amount_min",
                "scale_curve_x",
                "scale_curve_y",
                "seed",
                "speed_scale",
                "split_scale",
                "spread",
                "tangential_accel_curve",
                "tangential_accel_max",
                "tangential_accel_min",
                "texture",
                "use_fixed_seed",
            ][..],
        ),
        (
            "Line2D",
            &[
                "antialiased",
                "begin_cap_mode",
                "closed",
                "default_color",
                "end_cap_mode",
                "gradient",
                "joint_mode",
                "points",
                "round_precision",
                "sharp_limit",
                "texture",
                "texture_mode",
                "width",
                "width_curve",
            ][..],
        ),
        (
            "Polygon2D",
            &[
                "antialiased",
                "color",
                "internal_vertex_count",
                "invert_border",
                "invert_enabled",
                "offset",
                "polygon",
                "polygons",
                "skeleton",
                "texture",
                "texture_offset",
                "texture_rotation",
                "texture_scale",
                "uv",
                "vertex_colors",
            ][..],
        ),
        ("Bone2D", &["rest"][..]),
        ("Path2D", &["curve"][..]),
        (
            "PathFollow2D",
            &[
                "cubic_interp",
                "h_offset",
                "loop",
                "progress",
                "progress_ratio",
                "rotates",
                "v_offset",
            ][..],
        ),
        (
            "RemoteTransform2D",
            &[
                "remote_path",
                "update_position",
                "update_rotation",
                "update_scale",
                "use_global_coordinates",
            ][..],
        ),
        (
            "Light2D",
            &[
                "blend_mode",
                "color",
                "editor_only",
                "enabled",
                "energy",
                "range_item_cull_mask",
                "range_layer_max",
                "range_layer_min",
                "range_z_max",
                "range_z_min",
                "shadow_color",
                "shadow_enabled",
                "shadow_filter",
                "shadow_filter_smooth",
                "shadow_item_cull_mask",
            ][..],
        ),
        (
            "PointLight2D",
            &["height", "offset", "texture", "texture_scale"][..],
        ),
        ("DirectionalLight2D", &["height", "max_distance"][..]),
        (
            "LightOccluder2D",
            &["occluder", "occluder_light_mask", "sdf_collision"][..],
        ),
    ]
}

/// The defaults worth pinning: the ones that are *not* the zero value for their
/// type, because those are the ones a careless transcription gets wrong. A table
/// of `false`/`0.0`/`Nil` would pass even if it were generated from thin air.
fn nonzero_defaults() -> Vec<(&'static str, &'static str, Variant)> {
    vec![
        ("Sprite2D", "centered", Variant::Bool(true)),
        ("Sprite2D", "hframes", Variant::Int(1)),
        ("Sprite2D", "vframes", Variant::Int(1)),
        (
            "Sprite2D",
            "frame_coords",
            Variant::Vector2i(Vector2i::new(0, 0)),
        ),
        (
            "Sprite2D",
            "region_rect",
            Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::ZERO)),
        ),
        ("Camera2D", "anchor_mode", Variant::Int(1)),
        ("Camera2D", "zoom", Variant::Vector2(Vector2::new(1.0, 1.0))),
        ("Camera2D", "ignore_rotation", Variant::Bool(true)),
        ("Camera2D", "limit_left", Variant::Int(-10_000_000)),
        ("Camera2D", "limit_right", Variant::Int(10_000_000)),
        ("Camera2D", "limit_top", Variant::Int(-10_000_000)),
        ("Camera2D", "limit_bottom", Variant::Int(10_000_000)),
        ("Camera2D", "limit_enabled", Variant::Bool(true)),
        ("Camera2D", "position_smoothing_speed", Variant::Float(5.0)),
        ("Camera2D", "rotation_smoothing_speed", Variant::Float(5.0)),
        ("Camera2D", "process_callback", Variant::Int(1)),
        ("Camera2D", "drag_left_margin", Variant::Float(0.2)),
        ("Camera2D", "drag_top_margin", Variant::Float(0.2)),
        ("Camera2D", "editor_draw_screen", Variant::Bool(true)),
        ("Camera2D", "enabled", Variant::Bool(true)),
        ("GPUParticles2D", "amount", Variant::Int(8)),
        ("GPUParticles2D", "lifetime", Variant::Float(1.0)),
        ("GPUParticles2D", "emitting", Variant::Bool(true)),
        ("GPUParticles2D", "fixed_fps", Variant::Int(30)),
        ("GPUParticles2D", "draw_order", Variant::Int(1)),
        ("GPUParticles2D", "amount_ratio", Variant::Float(1.0)),
        ("GPUParticles2D", "interpolate", Variant::Bool(true)),
        ("GPUParticles2D", "fract_delta", Variant::Bool(true)),
        ("GPUParticles2D", "speed_scale", Variant::Float(1.0)),
        ("GPUParticles2D", "trail_lifetime", Variant::Float(0.3)),
        (
            "GPUParticles2D",
            "visibility_rect",
            Variant::Rect2(Rect2::new(
                Vector2::new(-100.0, -100.0),
                Vector2::new(200.0, 200.0),
            )),
        ),
        ("CPUParticles2D", "amount", Variant::Int(8)),
        ("CPUParticles2D", "lifetime", Variant::Float(1.0)),
        ("CPUParticles2D", "emitting", Variant::Bool(true)),
        (
            "CPUParticles2D",
            "direction",
            Variant::Vector2(Vector2::new(1.0, 0.0)),
        ),
        (
            "CPUParticles2D",
            "gravity",
            Variant::Vector2(Vector2::new(0.0, 980.0)),
        ),
        ("CPUParticles2D", "spread", Variant::Float(45.0)),
        ("CPUParticles2D", "scale_amount_max", Variant::Float(1.0)),
        ("CPUParticles2D", "scale_amount_min", Variant::Float(1.0)),
        ("CPUParticles2D", "emission_shape", Variant::Int(0)),
        (
            "CPUParticles2D",
            "color",
            Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)),
        ),
        ("Line2D", "width", Variant::Float(10.0)),
        ("Line2D", "round_precision", Variant::Int(8)),
        ("Line2D", "sharp_limit", Variant::Float(2.0)),
        (
            "Line2D",
            "default_color",
            Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)),
        ),
        (
            "Polygon2D",
            "color",
            Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)),
        ),
        ("Polygon2D", "invert_border", Variant::Float(100.0)),
        (
            "Polygon2D",
            "texture_scale",
            Variant::Vector2(Vector2::new(1.0, 1.0)),
        ),
        ("PathFollow2D", "cubic_interp", Variant::Bool(true)),
        ("PathFollow2D", "loop", Variant::Bool(true)),
        ("PathFollow2D", "rotates", Variant::Bool(true)),
        ("RemoteTransform2D", "update_position", Variant::Bool(true)),
        ("RemoteTransform2D", "update_rotation", Variant::Bool(true)),
        ("RemoteTransform2D", "update_scale", Variant::Bool(true)),
        (
            "RemoteTransform2D",
            "use_global_coordinates",
            Variant::Bool(true),
        ),
        ("Light2D", "enabled", Variant::Bool(true)),
        ("Light2D", "energy", Variant::Float(1.0)),
        (
            "Light2D",
            "color",
            Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)),
        ),
        ("Light2D", "range_item_cull_mask", Variant::Int(1)),
        ("Light2D", "range_z_max", Variant::Int(1024)),
        ("Light2D", "range_z_min", Variant::Int(-1024)),
        ("Light2D", "shadow_item_cull_mask", Variant::Int(1)),
        ("PointLight2D", "texture_scale", Variant::Float(1.0)),
        (
            "DirectionalLight2D",
            "max_distance",
            Variant::Float(10000.0),
        ),
        ("LightOccluder2D", "occluder_light_mask", Variant::Int(1)),
        ("LightOccluder2D", "sdf_collision", Variant::Bool(true)),
    ]
}

/// **The acceptance test.** Every 2D leaf class carries its full own-property
/// table, and a node created through `SceneTree::create_node` reads back Godot's
/// defaults.
#[test]
fn classdb_2d_node_property_tables_match_godot() {
    class_db::register_2d_classes();

    // -- own tables are complete, in ClassDB and through create_node ---------
    let mut tree = SceneTree::new();
    for (class, expected) in own_property_names() {
        let info = get_class_info(class)
            .unwrap_or_else(|| panic!("{class} must be registered by register_2d_classes()"));
        let actual: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();

        for name in expected {
            assert!(
                actual.contains(name),
                "{class} is missing own property `{name}`; it has {actual:?}"
            );
            assert!(
                class_has_property(class, name),
                "class_has_property({class:?}, {name:?}) must be true once the \
                 table is populated"
            );
        }
        assert_eq!(
            info.properties.len(),
            expected.len(),
            "{class} must expose exactly the {} own properties Godot documents, \
             got {actual:?}",
            expected.len()
        );

        // The seeder is the other half of the claim: reflection knowing about a
        // property is worthless if a created node does not carry it.
        //
        // Compared against the registered default rather than "is not Nil" —
        // several of these properties (`texture`, `curve`, `process_material`,
        // every `Curve` knob) have `Nil` as their genuine Godot default, so a
        // non-Nil check would demand the wrong thing of exactly them.
        let node_id = tree.create_node(class, class);
        let node = tree.get_node(node_id).expect("created node is in the tree");
        for prop in &info.properties {
            assert_eq!(
                node.get_property(&prop.name),
                prop.default_value,
                "create_node({class:?}) must seed `{}` from the class default",
                prop.name
            );
        }
    }

    // -- and the defaults are Godot's, not merely present --------------------
    for (class, name, expected) in nonzero_defaults() {
        let node_id = tree.create_node(class, "probe");
        let node = tree.get_node(node_id).expect("created node is in the tree");
        assert_eq!(
            node.get_property(name),
            expected,
            "{class}.{name} must seed Godot's documented default"
        );
    }
}

/// `Skeleton2D` genuinely has no own members in Godot's docs — its surface is
/// all inherited. Asserting that keeps a future "fill in the blanks" pass from
/// inventing properties for it.
#[test]
fn skeleton2d_has_no_own_properties_by_design() {
    class_db::register_2d_classes();
    let info = get_class_info("Skeleton2D").expect("Skeleton2D is registered");
    assert!(
        info.properties.is_empty(),
        "Godot documents no own members for Skeleton2D, got {:?}",
        info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

/// The tables must not have been cross-wired: a class picking up its neighbour's
/// property list would still pass every "is this name present" check on the
/// class it was copied from.
#[test]
fn leaf_tables_are_not_cross_wired() {
    class_db::register_2d_classes();
    let own = |class: &str| -> Vec<String> {
        get_class_info(class)
            .map(|i| i.properties.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default()
    };

    let has = |class: &str, prop: &str| own(class).iter().any(|n| n.as_str() == prop);

    assert!(
        !has("Sprite2D", "zoom"),
        "Sprite2D must not carry Camera2D's `zoom`"
    );
    assert!(
        !has("Camera2D", "hframes"),
        "Camera2D must not carry Sprite2D's `hframes`"
    );
    // The light family shares a base, so the split between own and inherited is
    // the easy thing to get wrong: `energy` belongs to Light2D, `texture_scale`
    // only to PointLight2D.
    assert!(
        has("Light2D", "energy"),
        "`energy` is Light2D's own property"
    );
    assert!(
        !has("PointLight2D", "energy"),
        "PointLight2D must inherit `energy`, not redeclare it"
    );
    assert!(
        has("PointLight2D", "texture_scale"),
        "`texture_scale` is PointLight2D's own property"
    );
    // …but it must still be *reachable* on the subclass through the chain.
    assert!(
        class_has_property("PointLight2D", "energy"),
        "PointLight2D must still report `energy` through its inheritance chain"
    );
}

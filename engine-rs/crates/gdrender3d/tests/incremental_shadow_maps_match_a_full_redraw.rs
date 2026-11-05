//! pat-414k3: CPU shadow maps for a running game are rebuilt incrementally —
//! the casters that sit still are rasterized once into a cached base and only
//! the moving ones are redrawn each frame — and every map is bit-identical to
//! a full redraw of all casters.
//!
//! In the official 3D platformer the player, the enemies and the spinning
//! coins change the shadow state every frame, and the old path re-rasterized
//! all of the level's 3000+ GridMap tiles into the atlas each time.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdrender3d::shadow_map::{
    generate_shadow_maps, generate_shadow_maps_incremental,
    generate_shadow_maps_incremental_reusing, ShadowCasterCache, ShadowMap, SHADOW_MAP_SIZE,
};
use gdserver3d::instance::{Instance3D, Instance3DId};
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::mesh::Mesh3D;

fn caster(id: u64, at: Vector3, size: f32) -> Instance3D {
    let mut inst = Instance3D::new(Instance3DId(id));
    inst.mesh = Some(Mesh3D::cube(size));
    inst.transform = Transform3D::IDENTITY.translated(at);
    inst.visible = true;
    inst
}

/// Stands in for the renderer's per-caster key: changes when the caster moves.
fn key(inst: &Instance3D) -> u64 {
    let o = inst.transform.origin;
    (inst.id.0 << 40) ^ ((o.x.to_bits() as u64) << 20) ^ (o.y.to_bits() as u64) ^ ((o.z.to_bits() as u64) << 7)
}

fn sun() -> Light3D {
    let mut l = Light3D::directional(Light3DId(1));
    l.direction = Vector3::new(-0.4, -1.0, -0.3).normalized();
    l.shadow_enabled = true;
    l
}

fn same_maps(a: &[ShadowMap], b: &[ShadowMap]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.view_matrix == y.view_matrix
                && x.proj_matrix == y.proj_matrix
                && (0..SHADOW_MAP_SIZE).all(|ty| {
                    (0..SHADOW_MAP_SIZE)
                        .all(|tx| x.depth.get(tx, ty).to_bits() == y.depth.get(tx, ty).to_bits())
                })
        })
}

/// A floor of static tiles plus three bodies that move every frame, one of
/// which stops for two frames now and then (the case that used to make a
/// one-frame-stable caster join the base and force a rebuild).
fn scene(frame: u32) -> Vec<Instance3D> {
    let mut out = Vec::new();
    let mut id = 1;
    for x in -6..6 {
        for z in -6..6 {
            out.push(caster(id, Vector3::new(x as f32 * 2.0, -1.0, z as f32 * 2.0), 1.0));
            id += 1;
        }
    }
    let t = frame as f32 * 0.1;
    out.push(caster(1000, Vector3::new(t.sin() * 4.0, 1.0, t.cos() * 4.0), 0.8));
    out.push(caster(1001, Vector3::new(-3.0, 0.5 + t, 2.0), 0.6));
    let paused = frame / 3;
    out.push(caster(1002, Vector3::new(paused as f32 * 0.25, 0.0, -2.0), 0.7));
    out
}

#[test]
fn incremental_maps_equal_a_full_redraw_every_frame() {
    let lights = vec![sun()];
    let mut cache = ShadowCasterCache::default();
    for frame in 0..80 {
        let instances = scene(frame);
        let refs: Vec<&Instance3D> = instances.iter().collect();
        let keys: Vec<u64> = instances.iter().map(key).collect();
        let incremental =
            generate_shadow_maps_incremental(&lights, &refs, &keys, 7, &mut cache);
        let full = generate_shadow_maps(&lights, &instances);
        assert!(same_maps(&incremental, &full), "frame {frame}: the maps differ");
        let written = (0..SHADOW_MAP_SIZE * SHADOW_MAP_SIZE)
            .filter(|i| full[0].depth.get(i % SHADOW_MAP_SIZE, i / SHADOW_MAP_SIZE) < f32::MAX)
            .count();
        assert!(written > 1000, "the casters must land in the map ({written} texels)");
    }
    // Once the floor has settled, only the bodies that move are redrawn.
    assert!(
        cache.last_redrawn <= 3,
        "{} casters redrawn per frame — the static floor is not being reused",
        cache.last_redrawn
    );
    assert!(cache.base_rebuilds <= 4, "{} base rebuilds in 80 frames", cache.base_rebuilds);
}

#[test]
fn a_changed_light_or_a_vanished_static_caster_rebuilds_the_base() {
    let mut lights = vec![sun()];
    let mut cache = ShadowCasterCache::default();
    let mut instances = scene(0);
    let mut render = |lights: &[Light3D], instances: &[Instance3D], sig: u64, cache: &mut ShadowCasterCache| {
        let refs: Vec<&Instance3D> = instances.iter().collect();
        let keys: Vec<u64> = instances.iter().map(key).collect();
        let incremental = generate_shadow_maps_incremental(lights, &refs, &keys, sig, cache);
        assert!(same_maps(&incremental, &generate_shadow_maps(lights, instances)));
    };
    for _ in 0..40 {
        render(&lights, &instances, 7, &mut cache);
    }
    // A static tile that was in the base disappears: it must not linger.
    instances.remove(5);
    render(&lights, &instances, 7, &mut cache);
    // The light turns: every caster lands somewhere else in its map.
    lights[0].direction = Vector3::new(0.3, -1.0, 0.2).normalized();
    render(&lights, &instances, 8, &mut cache);
}

/// The same frames with each call handed back the previous call's maps: they
/// are patched in place (only the moving casters' rectangles restored from
/// the base) instead of copying the whole base per frame, and every map is
/// still bit-identical to a full redraw. `last_dirty` covers every texel that
/// changed since the previous call.
#[test]
fn recycled_maps_equal_a_full_redraw_and_dirty_covers_every_change() {
    let lights = vec![sun()];
    let mut cache = ShadowCasterCache::default();
    let mut previous: Vec<ShadowMap> = Vec::new();
    let mut last_full: Option<Vec<ShadowMap>> = None;
    for frame in 0..80 {
        let instances = scene(frame);
        let refs: Vec<&Instance3D> = instances.iter().collect();
        let keys: Vec<u64> = instances.iter().map(key).collect();
        let maps = generate_shadow_maps_incremental_reusing(
            &lights,
            &refs,
            &keys,
            7,
            &mut cache,
            std::mem::take(&mut previous),
        );
        let full = generate_shadow_maps(&lights, &instances);
        assert!(same_maps(&maps, &full), "frame {frame}: the recycled maps differ");
        if let (Some(dirty), Some(prev)) = (&cache.last_dirty, &last_full) {
            let rects = &dirty[0];
            for ty in 0..SHADOW_MAP_SIZE {
                for tx in 0..SHADOW_MAP_SIZE {
                    let (a, b) = (prev[0].depth.get(tx, ty), full[0].depth.get(tx, ty));
                    if a.to_bits() != b.to_bits() {
                        assert!(
                            rects.iter().any(|r| tx >= r.0 && tx <= r.2 && ty >= r.1 && ty <= r.3),
                            "frame {frame}: texel ({tx}, {ty}) changed outside every dirty rectangle"
                        );
                    }
                }
            }
        }
        last_full = Some(full);
        previous = maps;
    }
    assert!(cache.recycled_maps >= 40, "only {} maps were recycled in 80 frames", cache.recycled_maps);
}

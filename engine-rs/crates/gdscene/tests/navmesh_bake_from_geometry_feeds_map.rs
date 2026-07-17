//! pat-nzgdu.114 — a baked NavigationMesh feeds a navigation map you can path across.
//!
//! This is the bead's "wiring requirement": the bake side (`gdserver3d`) and the
//! pathfinding side (`gdscene`) were two disconnected islands. `bake_navigation_mesh`
//! emits vertices + polygons but **no connectivity**, while `NavigationServer3D`
//! needs an adjacency list to run A* over — so a freshly baked mesh could not be
//! pathed across at all. `NavigationMesh3D::compute_adjacency` closes that gap.
//!
//! The two filters the acceptance names (`agent_max_slope` dropping steep faces,
//! `agent_radius` shrinking the walkable area) were already implemented inside
//! `bake_navigation_mesh`; the tests below pin them *and* prove they survive
//! through to a real path, which is what actually makes them observable.
//!
//! Not covered (see the bead notes): NavigationPolygon/NavigationMesh as loadable
//! Resources with the full param surface, .tscn round-trip, source-geometry
//! parsing from scene nodes, and a Recast-style voxelised bake.

use gdscene::navigation::{NavMesh3D, NavPolygon3D, NavigationServer3D};
use gdserver3d::{bake_navigation_mesh, BakeSourceGeometry3D, NavigationMesh3D};

use gdcore::math::Vector3;

fn v(x: f32, y: f32, z: f32) -> Vector3 {
    Vector3::new(x, y, z)
}

/// Adds the two triangles of an axis-aligned quad on the y = `y` plane spanning
/// `[x0,x1] x [z0,z1]`, wound so the normal points up.
fn add_floor_quad(src: &mut BakeSourceGeometry3D, x0: f32, x1: f32, z0: f32, z1: f32, y: f32) {
    src.add_triangle(v(x0, y, z0), v(x0, y, z1), v(x1, y, z1));
    src.add_triangle(v(x0, y, z0), v(x1, y, z1), v(x1, y, z0));
}

/// Turns a baked mesh into a map region: resolve each polygon's positions and
/// pair them with the computed adjacency.
fn region_from(mesh: &NavigationMesh3D) -> NavMesh3D {
    let polys: Vec<NavPolygon3D> = (0..mesh.polygons.len())
        .map(|i| NavPolygon3D::new(mesh.polygon_vertices(i).expect("polygon resolves")))
        .collect();
    NavMesh3D::new(polys, mesh.compute_adjacency())
}

/// A wide, flat corridor: big triangles so the agent-radius inradius filter keeps
/// them, flat so the slope filter keeps them.
fn corridor_params() -> NavigationMesh3D {
    let mut p = NavigationMesh3D::new();
    p.agent_radius = 0.5;
    p.agent_max_slope = 45.0;
    p
}

/// The bead's acceptance: bake from parsed triangles, then path across the result.
#[test]
fn navmesh_bake_from_geometry_feeds_map() {
    // A 12x4 floor laid out as three abutting quads along x, so a path from one
    // end to the other must cross polygons — a single-polygon map would pass a
    // weaker version of this test trivially.
    let mut src = BakeSourceGeometry3D::new();
    add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0, 0.0);
    add_floor_quad(&mut src, 4.0, 8.0, 0.0, 4.0, 0.0);
    add_floor_quad(&mut src, 8.0, 12.0, 0.0, 4.0, 0.0);

    let baked = bake_navigation_mesh(&corridor_params(), &src);
    assert_eq!(baked.polygon_count(), 6, "all six flat triangles are walkable");

    // The bake emits no connectivity; that is the gap this bead closes.
    let adjacency = baked.compute_adjacency();
    assert_eq!(adjacency.len(), 6);
    assert!(
        adjacency.iter().all(|n| !n.is_empty()),
        "every triangle in a connected floor must have a neighbour: {adjacency:?}"
    );

    let mut server = NavigationServer3D::new();
    server.add_region(region_from(&baked));

    let path = server.find_path(v(0.5, 0.0, 2.0), v(11.5, 0.0, 2.0));
    assert!(
        !path.is_empty(),
        "a baked, connected floor must yield a path end to end"
    );
    assert_eq!(path.first().copied(), Some(v(0.5, 0.0, 2.0)), "path starts at the origin");
    assert_eq!(path.last().copied(), Some(v(11.5, 0.0, 2.0)), "path ends at the destination");
    assert!(
        path.len() >= 2,
        "a path across three quads is more than a straight teleport: {path:?}"
    );
}

/// Without adjacency the same mesh is unpathable — this is what the wiring buys,
/// and it fails if `compute_adjacency` ever silently returns empty neighbours.
#[test]
fn a_mesh_without_adjacency_cannot_be_pathed_across() {
    let mut src = BakeSourceGeometry3D::new();
    add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0, 0.0);
    add_floor_quad(&mut src, 4.0, 8.0, 0.0, 4.0, 0.0);
    let baked = bake_navigation_mesh(&corridor_params(), &src);

    // Same polygons, but connectivity stubbed out the way a naive conversion
    // would leave it.
    let polys: Vec<NavPolygon3D> = (0..baked.polygons.len())
        .map(|i| NavPolygon3D::new(baked.polygon_vertices(i).unwrap()))
        .collect();
    let empty = vec![Vec::new(); polys.len()];
    let mut server = NavigationServer3D::new();
    server.add_region(NavMesh3D::new(polys, empty));

    // Start and goal land on different polygons with no edge between them.
    let path = server.find_path(v(0.5, 0.0, 2.0), v(7.5, 0.0, 2.0));
    assert!(
        path.is_empty(),
        "with no adjacency A* cannot cross polygons — got {path:?}"
    );
}

/// Adjacency is matched by vertex POSITION, not index. The bake pushes three
/// fresh vertices per triangle, so abutting faces never share an index — an
/// index-based match would find zero neighbours and silently produce an
/// unpathable mesh.
#[test]
fn adjacency_welds_by_position_because_the_bake_duplicates_vertices() {
    let mut src = BakeSourceGeometry3D::new();
    add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0, 0.0);
    let baked = bake_navigation_mesh(&corridor_params(), &src);

    assert_eq!(baked.polygon_count(), 2, "a quad bakes to two triangles");
    // The two triangles share the diagonal edge, yet share no vertex index.
    assert_eq!(
        baked.vertex_count(),
        6,
        "the bake emits 3 fresh vertices per triangle — no index is reused"
    );
    let a: Vec<u32> = baked.polygons[0].indices.clone();
    let b: Vec<u32> = baked.polygons[1].indices.clone();
    assert!(
        a.iter().all(|i| !b.contains(i)),
        "the two triangles share no vertex index: {a:?} vs {b:?}"
    );

    // ...and yet they are neighbours, because they share the diagonal by position.
    let adjacency = baked.compute_adjacency();
    assert_eq!(adjacency[0], vec![1], "triangle 0 neighbours triangle 1");
    assert_eq!(adjacency[1], vec![0], "and the link is mutual");
}

/// Disconnected islands must not be linked: two floors far apart share no edge.
#[test]
fn disconnected_geometry_yields_no_connections() {
    let mut src = BakeSourceGeometry3D::new();
    add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0, 0.0);
    add_floor_quad(&mut src, 100.0, 104.0, 0.0, 4.0, 0.0);
    let baked = bake_navigation_mesh(&corridor_params(), &src);
    let adjacency = baked.compute_adjacency();

    assert_eq!(baked.polygon_count(), 4);
    // Each triangle links only to its own quad's other triangle.
    for (i, neighbours) in adjacency.iter().enumerate() {
        assert_eq!(
            neighbours.len(),
            1,
            "polygon {i} should touch exactly one neighbour, got {neighbours:?}"
        );
    }
    // Island A (0,1) must never reach island B (2,3).
    assert!(!adjacency[0].contains(&2) && !adjacency[0].contains(&3));
    assert!(!adjacency[1].contains(&2) && !adjacency[1].contains(&3));
}

/// The acceptance's slope clause: a face steeper than `agent_max_slope` is
/// dropped, and raising the limit admits the same geometry — which is what
/// proves the filter reads the parameter rather than rejecting for some other
/// reason (e.g. the radius filter).
#[test]
fn bake_drops_faces_steeper_than_agent_max_slope() {
    // A ramp on the plane y = tan(60 deg) * z. Its normal sits 60 degrees off
    // vertical, so dot(normal, up) == cos(60 deg) == 0.5 exactly.
    //
    // Deliberately 60 and not a vertical wall: a vertical face has dot == 0.0,
    // and cos(90 deg) in f32 is 6.1e-17 rather than 0 — so `dot < max_slope_cos`
    // would still reject it at agent_max_slope = 90 and the "now admitted" half
    // of this test would be testing a floating-point artefact.
    let m = 60f32.to_radians().tan();
    let mut src = BakeSourceGeometry3D::new();
    src.add_triangle(v(0.0, 0.0, 0.0), v(0.0, m, 1.0), v(4.0, m, 1.0));
    src.add_triangle(v(0.0, 0.0, 0.0), v(4.0, m, 1.0), v(4.0, 0.0, 0.0));

    // 60 > 45 -> too steep to walk.
    assert_eq!(
        bake_navigation_mesh(&corridor_params(), &src).polygon_count(),
        0,
        "a 60-degree ramp is steeper than agent_max_slope=45"
    );

    // 60 < 75 -> the very same triangles are now walkable.
    let mut steep = corridor_params();
    steep.agent_max_slope = 75.0;
    assert_eq!(
        bake_navigation_mesh(&steep, &src).polygon_count(),
        2,
        "at agent_max_slope=75 the same 60-degree ramp is walkable"
    );
}

/// The acceptance's radius clause: a corridor too narrow for the agent is
/// dropped, so it yields no polygons to path across.
#[test]
fn bake_drops_faces_narrower_than_agent_radius() {
    // A long, 0.1m-wide sliver: its inscribed circle is far under a 0.5m agent.
    let mut src = BakeSourceGeometry3D::new();
    add_floor_quad(&mut src, 0.0, 8.0, 0.0, 0.1, 0.0);

    let baked = bake_navigation_mesh(&corridor_params(), &src);
    assert_eq!(
        baked.polygon_count(),
        0,
        "a 0.1m sliver cannot fit a 0.5m-radius agent"
    );

    // A smaller agent fits the same geometry — again proving the filter reads
    // the parameter.
    let mut tiny = corridor_params();
    tiny.agent_radius = 0.01;
    assert_eq!(
        bake_navigation_mesh(&tiny, &src).polygon_count(),
        2,
        "a 0.01m-radius agent fits the sliver"
    );
}

//! End-to-end project loader example.
//!
//! Loads a Godot project from disk, parses the main scene, instances nodes,
//! attaches scripts, runs lifecycle callbacks, and executes 60 frames via
//! the main loop.

use std::path::PathBuf;

use gdresource::project::ProjectLoader;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{MainLoop, SceneTree};
use gdvariant::Variant;

fn main() {
    let project_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../fixtures/sample_project"));

    println!("=== Patina Engine: Project Loader ===\n");

    // Step 1: Load project.godot
    print!("Loading project.godot... ");
    let loader = match ProjectLoader::load(&project_path) {
        Ok(l) => {
            println!("OK");
            l
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    let config = loader.config();
    println!("  Project name:   {}", config.project_name);
    println!("  Main scene:     {}", config.main_scene);
    println!(
        "  Viewport:       {}x{}",
        config.viewport_width, config.viewport_height
    );
    println!("  Physics TPS:    {}", config.physics_ticks_per_second);

    // Step 2: Load main scene .tscn
    let main_scene_path = match loader.resolve_path(&config.main_scene) {
        Ok(p) => p,
        Err(e) => {
            println!("\nFAILED to resolve main scene path: {e}");
            std::process::exit(1);
        }
    };

    print!("\nLoading main scene ({})... ", config.main_scene);
    let tscn_content = match std::fs::read_to_string(&main_scene_path) {
        Ok(c) => c,
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    let packed_scene = match PackedScene::from_tscn(&tscn_content) {
        Ok(s) => {
            println!("OK ({} nodes)", s.node_count());
            s
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    // Step 3: Instance into SceneTree
    print!("Instancing into SceneTree... ");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let scene_root_id = match add_packed_scene_to_tree(&mut tree, root_id, &packed_scene) {
        Ok(id) => {
            println!("OK (tree has {} nodes)", tree.node_count());
            id
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    // Step 4: Attach scripts
    println!("\nAttaching scripts:");
    let node_ids = tree.all_nodes_in_tree_order();
    let mut scripted_nodes = Vec::new();

    for &nid in &node_ids {
        let script_path = {
            let node = tree.get_node(nid).unwrap();
            match node.get_property("_script_path") {
                Variant::String(path) => Some(path.clone()),
                _ => None,
            }
        };

        if let Some(ref path) = script_path {
            let abs_path = match loader.resolve_path(path) {
                Ok(p) => p,
                Err(e) => {
                    println!("  FAILED to resolve {path}: {e}");
                    continue;
                }
            };

            let source = match std::fs::read_to_string(&abs_path) {
                Ok(s) => s,
                Err(e) => {
                    println!("  FAILED to read {path}: {e}");
                    continue;
                }
            };

            let node_name = tree.get_node(nid).unwrap().name().to_string();
            match GDScriptNodeInstance::from_source(&source, nid) {
                Ok(instance) => {
                    tree.attach_script(nid, Box::new(instance));
                    scripted_nodes.push((nid, node_name.clone()));
                    println!("  {node_name} <- {path} ... OK");
                }
                Err(e) => {
                    println!("  {node_name} <- {path} ... FAILED: {e}");
                }
            }
        }
    }

    // Step 5: Run lifecycle (enter_tree + _ready)
    println!("\nRunning lifecycle:");

    // enter_tree for all nodes
    let all_ids = tree.all_nodes_in_tree_order();
    for &nid in &all_ids {
        tree.process_script_enter_tree(nid);
    }
    println!("  enter_tree ... OK");

    // _ready for scripted nodes
    for &(nid, ref name) in &scripted_nodes {
        tree.process_script_ready(nid);
        println!("  _ready({name}) ... OK");
    }

    // Step 6: Run 60 frames via MainLoop
    let mut main_loop = MainLoop::new(tree);
    main_loop.set_physics_ticks_per_second(config.physics_ticks_per_second);

    let delta = 1.0 / 60.0;
    println!("\nRunning 60 frames (delta={delta:.6}s)...");
    main_loop.run_frames(60, delta);
    println!(
        "  Completed {} frames, process_time={:.4}s",
        main_loop.frame_count(),
        main_loop.process_time()
    );

    // Step 7: Report results
    println!("\n=== Results ===");
    let tree = main_loop.tree();
    println!("  Total nodes:    {}", tree.node_count());
    println!("  Scene root:     {}", scene_root_id);

    for &(nid, ref name) in &scripted_nodes {
        if let Some(script) = tree.get_script(nid) {
            println!("  [{name}] script properties:");
            for prop in script.list_properties() {
                println!("    {} = {:?}", prop.name, prop.default_value);
            }
        }
    }

    println!("\n=== SUCCESS ===");
}

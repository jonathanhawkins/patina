import type { Metadata } from "next";
import Link from "next/link";
import {
  BookOpen,
  Download,
  Play,
  FileCode,
  Layers,
  Network,
  ArrowRight,
} from "lucide-react";

export const metadata: Metadata = {
  title: "Documentation",
  description:
    "Get started with Patina Engine. Learn how to build a SceneTree, load .tscn files, and run the headless runner.",
};

const sections = [
  {
    id: "installation",
    icon: Download,
    title: "Installation",
    color: "text-emerald-400",
    content: [
      {
        type: "paragraph" as const,
        text: "Add Patina Engine to your Rust project using Cargo. The engine is distributed as a workspace of focused crates — use only what you need.",
      },
      {
        type: "code" as const,
        label: "Cargo.toml",
        code: `[dependencies]
gdscene   = { git = "https://github.com/patinaengine/patina" }
gdvariant = { git = "https://github.com/patinaengine/patina" }`,
      },
      {
        type: "paragraph" as const,
        text: "Patina requires Rust 1.75 or later. We recommend using rustup to manage your Rust toolchain.",
      },
      {
        type: "code" as const,
        label: "Terminal",
        code: `# Install Rust (if you haven't already)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Create a new project
cargo init my-game
cd my-game`,
      },
    ],
  },
  {
    id: "scene-tree",
    icon: Network,
    title: "Building a SceneTree",
    color: "text-cyan-400",
    content: [
      {
        type: "paragraph" as const,
        text: "The SceneTree is the heart of the engine. Create one, add nodes, and traverse the hierarchy — just like Godot.",
      },
      {
        type: "code" as const,
        label: "main.rs",
        code: `use gdscene::scene_tree::SceneTree;
use gdscene::node::Node;

fn main() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add a player node under root
    let player = Node::new("Player", "CharacterBody2D");
    let player_id = tree.add_child(root, player).unwrap();

    // Add a sprite as a child of the player
    let sprite = Node::new("Sprite", "Sprite2D");
    tree.add_child(player_id, sprite).unwrap();

    // Query by path
    let found = tree.get_node_by_path("root/Player/Sprite");
    assert!(found.is_some());

    println!("Tree has {} nodes", tree.node_count());
}`,
      },
    ],
  },
  {
    id: "loading-scenes",
    icon: FileCode,
    title: "Loading .tscn Files",
    color: "text-blue-400",
    content: [
      {
        type: "paragraph" as const,
        text: "Patina natively parses Godot .tscn files using PackedScene. Parse the text format, then instance the nodes into a SceneTree.",
      },
      {
        type: "code" as const,
        label: "main.rs",
        code: `use gdscene::{PackedScene, add_packed_scene_to_tree};
use gdscene::scene_tree::SceneTree;
use gdscene::LifecycleManager;

fn main() {
    let tscn = std::fs::read_to_string("main.tscn").unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();

    println!("Scene has {} nodes", packed.node_count());

    // Instance into a live SceneTree
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ids = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Fire lifecycle callbacks (READY, ENTER_TREE)
    LifecycleManager::enter_tree(&mut tree, ids[0]);

    // Traverse instanced nodes
    for id in &ids {
        if let Some(node) = tree.get_node(*id) {
            println!("{}: {}", node.name(), node.class_name());
        }
    }
}`,
      },
    ],
  },
  {
    id: "running",
    icon: Play,
    title: "Headless Runner",
    color: "text-violet-400",
    content: [
      {
        type: "paragraph" as const,
        text: "The patina-runner binary loads a .tscn file, runs lifecycle callbacks and a configurable number of frames, then dumps the tree state as JSON. Ideal for CI, testing, and server-side logic.",
      },
      {
        type: "code" as const,
        label: "Terminal",
        code: `# Run a scene for 10 frames at 60fps (default)
patina-runner level.tscn

# Run 120 frames with custom delta
patina-runner level.tscn --frames 120 --delta 0.016

# Pipe JSON output for inspection
patina-runner level.tscn | jq '.children[0].name'`,
      },
      {
        type: "paragraph" as const,
        text: "Under the hood, patina-runner uses MainLoop which orchestrates process and physics frames with configurable tick rates — matching Godot's frame stepping model.",
      },
      {
        type: "code" as const,
        label: "Programmatic usage",
        code: `use gdscene::MainLoop;
use gdscene::scene_tree::SceneTree;

let tree = SceneTree::new();
let mut main_loop = MainLoop::new(tree);

main_loop.set_physics_ticks_per_second(60);
main_loop.run_frames(120, 1.0 / 60.0);

println!("Ran {} frames", main_loop.frame_count());`,
      },
    ],
  },
  {
    id: "variant-types",
    icon: Layers,
    title: "Variant Type System",
    color: "text-amber-400",
    content: [
      {
        type: "paragraph" as const,
        text: "Patina implements Godot's Variant — a tagged union that can hold any engine value. Properties on nodes are stored as Variants, enabling dynamic typing while keeping Rust's safety guarantees.",
      },
      {
        type: "code" as const,
        label: "Supported Variant types",
        code: `Variant::Nil                    // null / default
Variant::Bool(bool)             // boolean
Variant::Int(i64)               // 64-bit signed integer
Variant::Float(f64)             // 64-bit float
Variant::String(String)         // UTF-8 string
Variant::StringName(StringName) // interned string
Variant::NodePath(NodePath)     // scene-tree path
Variant::Vector2(Vector2)       // 2D vector
Variant::Vector3(Vector3)       // 3D vector
Variant::Rect2(Rect2)           // axis-aligned 2D rect
Variant::Transform2D(..)        // 2D affine transform
Variant::Transform3D(..)        // 3D affine transform
Variant::Color(Color)           // RGBA color
Variant::Basis(Basis)           // 3×3 rotation/scale
Variant::Quaternion(Quaternion) // quaternion rotation
Variant::Aabb(Aabb)             // axis-aligned bounding box
Variant::Plane(Plane)           // infinite plane
Variant::Array(Vec<Variant>)    // heterogeneous list
Variant::Dictionary(..)         // string-keyed map`,
      },
      {
        type: "code" as const,
        label: "Using Variants on nodes",
        code: `use gdscene::node::Node;
use gdvariant::Variant;
use gdcore::math::Vector2;

let mut node = Node::new("Player", "CharacterBody2D");

// Set properties as Variants
node.set_property("speed", Variant::Float(200.0));
node.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
node.set_property("name", Variant::String("Hero".into()));

// Read them back
let speed = node.get_property("speed");
assert_eq!(speed, Variant::Float(200.0));`,
      },
    ],
  },
];

export default function DocsPage() {
  return (
    <main className="min-h-screen">
      <div className="mx-auto max-w-4xl px-6 pb-24 pt-16 sm:pt-24">
        {/* Header */}
        <div className="mb-16">
          <div className="flex items-center gap-2 text-brand">
            <BookOpen className="size-4" />
            <p className="font-mono text-xs font-medium uppercase tracking-widest">
              Documentation
            </p>
          </div>
          <h1 className="mt-4 text-3xl font-semibold tracking-tight sm:text-4xl">
            Getting Started
          </h1>
          <p className="mt-4 max-w-2xl text-base leading-relaxed text-muted-foreground">
            Everything you need to start building games with Patina Engine.
            Build a SceneTree, load Godot scenes, and run them headlessly in
            Rust.
          </p>
        </div>

        {/* Table of Contents */}
        <nav className="mb-16 rounded-xl border border-border/50 bg-card/40 p-6">
          <p className="mb-3 text-xs font-medium uppercase tracking-widest text-muted-foreground">
            On this page
          </p>
          <ul className="space-y-2">
            {sections.map((section) => (
              <li key={section.id}>
                <a
                  href={`#${section.id}`}
                  className="flex items-center gap-2 text-sm text-muted-foreground transition-colors hover:text-foreground"
                >
                  <section.icon className={`size-3.5 ${section.color}`} />
                  {section.title}
                </a>
              </li>
            ))}
          </ul>

          <div className="mt-4 border-t border-border/30 pt-4">
            <Link
              href="/docs/architecture"
              className="flex items-center gap-2 text-sm text-brand transition-colors hover:text-brand/80"
            >
              <Network className="size-3.5" />
              Architecture &amp; Crate Map
              <ArrowRight className="size-3" />
            </Link>
          </div>
        </nav>

        {/* Sections */}
        <div className="space-y-20">
          {sections.map((section) => (
            <section key={section.id} id={section.id} className="scroll-mt-24">
              <div className="mb-6 flex items-center gap-3">
                <div
                  className={`flex size-8 items-center justify-center rounded-lg bg-muted/50 ${section.color}`}
                >
                  <section.icon className="size-4" />
                </div>
                <h2 className="text-xl font-semibold tracking-tight">
                  {section.title}
                </h2>
              </div>

              <div className="space-y-4">
                {section.content.map((block, i) =>
                  block.type === "paragraph" ? (
                    <p
                      key={i}
                      className="max-w-2xl text-sm leading-relaxed text-muted-foreground"
                    >
                      {block.text}
                    </p>
                  ) : (
                    <div
                      key={i}
                      className="overflow-hidden rounded-xl border border-border/60 bg-[oklch(0.09_0.005_270)]"
                    >
                      <div className="flex items-center border-b border-white/[0.06] px-4 py-2.5">
                        <span className="font-mono text-xs text-muted-foreground/70">
                          {block.label}
                        </span>
                      </div>
                      <pre className="overflow-x-auto px-5 py-4 font-mono text-[13px] leading-[1.7] text-foreground/80">
                        <code>{block.code}</code>
                      </pre>
                    </div>
                  )
                )}
              </div>
            </section>
          ))}
        </div>

        {/* Footer note */}
        <div className="mt-20 rounded-xl border border-border/50 bg-brand-muted p-6">
          <p className="text-sm font-medium text-brand">
            Patina is in early development
          </p>
          <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
            APIs shown here reflect the current engine implementation and may
            evolve as the engine matures. Follow development on{" "}
            <a
              href="https://github.com/patinaengine/patina"
              target="_blank"
              rel="noopener noreferrer"
              className="text-brand underline underline-offset-4 hover:text-brand/80"
            >
              GitHub
            </a>
            .
          </p>
        </div>
      </div>
    </main>
  );
}

import type { Metadata } from "next";
import { BookOpen, Download, Play, FileCode, Layers } from "lucide-react";

export const metadata: Metadata = {
  title: "Documentation",
  description:
    "Get started with Patina Engine. Learn how to install, load Godot scenes, and run your game in Rust.",
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
        text: "Add Patina Engine to your Rust project using Cargo. The engine is distributed as a set of crates that you can include as needed.",
      },
      {
        type: "code" as const,
        label: "Cargo.toml",
        code: `[dependencies]\npatina = "0.1"`,
      },
      {
        type: "paragraph" as const,
        text: "Patina requires Rust 1.75 or later. We recommend using rustup to manage your Rust toolchain.",
      },
      {
        type: "code" as const,
        label: "Terminal",
        code: `# Install Rust (if you haven't already)\ncurl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n\n# Create a new project\ncargo init my-game\ncd my-game\n\n# Add Patina as a dependency\ncargo add patina`,
      },
    ],
  },
  {
    id: "loading-scenes",
    icon: FileCode,
    title: "Loading Scenes",
    color: "text-blue-400",
    content: [
      {
        type: "paragraph" as const,
        text: "Patina natively loads Godot .tscn and .tres files. Design your scenes in the Godot editor, then run them on the Patina runtime.",
      },
      {
        type: "code" as const,
        label: "main.rs",
        code: `use patina::prelude::*;\n\nfn main() {\n    let scene = Scene::load("res://main.tscn");\n    let root = scene.root();\n\n    // Traverse the scene tree\n    for node in root.children() {\n        println!("Node: {} (type: {})", node.name, node.class_name);\n    }\n}`,
      },
      {
        type: "paragraph" as const,
        text: "Place your Godot project files in a directory and point Patina to them. The engine resolves res:// paths relative to your project root.",
      },
    ],
  },
  {
    id: "running",
    icon: Play,
    title: "Running the Engine",
    color: "text-violet-400",
    content: [
      {
        type: "paragraph" as const,
        text: "Run your game with Cargo. Patina supports both windowed and headless modes for development and testing.",
      },
      {
        type: "code" as const,
        label: "Terminal",
        code: `# Run in windowed mode\ncargo run\n\n# Run in headless mode (for testing / CI)\ncargo run -- --headless\n\n# Run with verbose logging\nRUST_LOG=patina=debug cargo run`,
      },
      {
        type: "paragraph" as const,
        text: "In headless mode, the engine runs the full game loop without opening a window — ideal for automated testing, CI pipelines, and server-side game logic.",
      },
    ],
  },
  {
    id: "api-overview",
    icon: Layers,
    title: "API Overview",
    color: "text-amber-400",
    content: [
      {
        type: "paragraph" as const,
        text: "Patina is organized into focused crates, each handling a specific domain. Import what you need or use the prelude for convenience.",
      },
      {
        type: "code" as const,
        label: "Crate overview",
        code: `patina-core      # Scene tree, nodes, resources\npatina-parser    # .tscn / .tres / .gdscript parsing\npatina-physics   # 2D and 3D physics (Godot-compatible)\npatina-render    # Rendering pipeline\npatina-audio     # Audio playback\npatina-script    # GDScript interop`,
      },
      {
        type: "paragraph" as const,
        text: "The API mirrors Godot's class hierarchy where possible, so concepts transfer directly. Nodes, signals, resources, and the scene tree all work as you'd expect.",
      },
      {
        type: "code" as const,
        label: "Example: Querying nodes",
        code: `use patina::prelude::*;\n\nfn find_enemies(scene: &Scene) -> Vec<&Node> {\n    scene\n        .root()\n        .find_children("Enemy*", "CharacterBody2D", true)\n}`,
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
            Install the crate, load a Godot scene, and run it in Rust.
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
            APIs shown here represent the target design and may change as the
            engine matures. Follow development on{" "}
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

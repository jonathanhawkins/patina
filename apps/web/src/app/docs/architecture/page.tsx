import type { Metadata } from "next";
import Link from "next/link";
import { BookOpen, ArrowLeft, Box, Network } from "lucide-react";

export const metadata: Metadata = {
  title: "Architecture",
  description:
    "Patina Engine architecture: 13 Rust crates, their responsibilities, and how they connect.",
};

const crates = [
  {
    name: "gdcore",
    description:
      "Foundation types: math (Vector2/3, Transform2D/3D, Color, Quaternion), object IDs, NodePath, StringName, error handling. Every other crate depends on this.",
    deps: [],
    layer: "foundation",
  },
  {
    name: "gdvariant",
    description:
      "Godot-compatible Variant tagged union. 20 types from Nil to Dictionary, with serialization support. The dynamic type system for all node properties.",
    deps: ["gdcore"],
    layer: "foundation",
  },
  {
    name: "gdobject",
    description:
      "Object system: class registration, property metadata, signal declarations. Bridges static Rust types to Godot's dynamic object model.",
    deps: ["gdcore", "gdvariant"],
    layer: "object",
  },
  {
    name: "gdresource",
    description:
      "Resource loading and caching. Parses .tres files, manages a thread-safe resource cache with Arc-based sharing.",
    deps: ["gdcore", "gdobject", "gdvariant"],
    layer: "object",
  },
  {
    name: "gdscene",
    description:
      "SceneTree, Node, PackedScene, MainLoop, and lifecycle management. Parses .tscn files, instances nodes, manages tree hierarchy, signals, groups, and frame stepping.",
    deps: ["gdcore", "gdobject", "gdresource", "gdvariant"],
    layer: "scene",
  },
  {
    name: "gdphysics2d",
    description:
      "2D physics simulation: rigid bodies, collision shapes, spatial queries, Godot-compatible physics stepping.",
    deps: ["gdcore", "gdvariant"],
    layer: "runtime",
  },
  {
    name: "gdserver2d",
    description:
      "2D rendering server: draw commands, viewport management, render state. Decouples scene logic from rendering backend.",
    deps: ["gdcore", "gdvariant"],
    layer: "runtime",
  },
  {
    name: "gdrender2d",
    description:
      "2D rendering pipeline: sprite batching, z-sorting, camera transforms. Consumes draw commands from gdserver2d.",
    deps: ["gdcore", "gdserver2d"],
    layer: "runtime",
  },
  {
    name: "gdaudio",
    description:
      "Audio playback system: bus routing, stream management, spatial audio foundations.",
    deps: ["gdcore"],
    layer: "runtime",
  },
  {
    name: "gdplatform",
    description:
      "Platform abstraction: window management, input events, OS integration. Thin layer over platform-specific APIs.",
    deps: ["gdcore"],
    layer: "runtime",
  },
  {
    name: "gdscript-interop",
    description:
      "GDScript interoperability: script attachment, method dispatch, and property access for scripted nodes.",
    deps: ["gdcore", "gdobject", "gdvariant"],
    layer: "scripting",
  },
  {
    name: "gdeditor",
    description:
      "Editor support: scene tree dock, inspector, project/editor settings, and editor-side tooling.",
    deps: ["gdcore", "gdobject", "gdscene", "gdvariant"],
    layer: "editor",
  },
  {
    name: "patina-runner",
    description:
      "Headless CLI runner: loads .tscn, runs N frames with MainLoop, dumps tree state as JSON. Used for CI testing and validation.",
    deps: ["gdcore", "gdobject", "gdresource", "gdscene", "gdvariant"],
    layer: "app",
  },
];

const layerLabels: Record<string, { label: string; color: string }> = {
  foundation: { label: "Foundation", color: "text-emerald-400" },
  object: { label: "Object System", color: "text-blue-400" },
  scene: { label: "Scene Management", color: "text-violet-400" },
  runtime: { label: "Runtime Services", color: "text-amber-400" },
  scripting: { label: "Scripting", color: "text-cyan-400" },
  editor: { label: "Editor", color: "text-rose-400" },
  app: { label: "Applications", color: "text-brand" },
};

const layerOrder = [
  "foundation",
  "object",
  "scene",
  "runtime",
  "scripting",
  "editor",
  "app",
];

export default function ArchitecturePage() {
  return (
    <main className="min-h-screen">
      <div className="mx-auto max-w-4xl px-6 pb-24 pt-16 sm:pt-24">
        {/* Back link */}
        <Link
          href="/docs"
          className="mb-8 inline-flex items-center gap-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground"
        >
          <ArrowLeft className="size-3.5" />
          Back to Docs
        </Link>

        {/* Header */}
        <div className="mb-16">
          <div className="flex items-center gap-2 text-brand">
            <Network className="size-4" />
            <p className="font-mono text-xs font-medium uppercase tracking-widest">
              Architecture
            </p>
          </div>
          <h1 className="mt-4 text-3xl font-semibold tracking-tight sm:text-4xl">
            Crate Map
          </h1>
          <p className="mt-4 max-w-2xl text-base leading-relaxed text-muted-foreground">
            Patina is built as 13 focused Rust crates in a Cargo workspace.
            Each crate owns a single domain, with strict dependency boundaries
            and no circular references.
          </p>
        </div>

        {/* Dependency diagram */}
        <section className="mb-20">
          <div className="mb-6 flex items-center gap-3">
            <div className="flex size-8 items-center justify-center rounded-lg bg-muted/50 text-brand">
              <BookOpen className="size-4" />
            </div>
            <h2 className="text-xl font-semibold tracking-tight">
              Dependency Graph
            </h2>
          </div>

          <div className="overflow-hidden rounded-xl border border-border/60 bg-[oklch(0.09_0.005_270)]">
            <div className="flex items-center border-b border-white/[0.06] px-4 py-2.5">
              <span className="font-mono text-xs text-muted-foreground/70">
                Crate dependencies (arrows show &quot;depends on&quot;)
              </span>
            </div>
            <pre className="overflow-x-auto px-5 py-4 font-mono text-[13px] leading-[1.7] text-foreground/80">
              <code>{`                    ┌──────────────┐
                    │ patina-runner │  (CLI app)
                    └──────┬───────┘
                           │
              ┌────────────┼────────────────┐
              ▼            ▼                ▼
        ┌──────────┐ ┌──────────┐    ┌───────────┐
        │ gdeditor │ │ gdscene  │    │ gdresource│
        └────┬─────┘ └────┬─────┘    └─────┬─────┘
             │            │                │
             ├────────────┼────────────────┤
             ▼            ▼                ▼
        ┌──────────┐ ┌──────────┐    ┌──────────────────┐
        │ gdobject │ │gdscript- │    │ gdphysics2d      │
        └────┬─────┘ │interop   │    │ gdserver2d       │
             │       └────┬─────┘    │ gdrender2d       │
             │            │          │ gdaudio           │
             │            │          │ gdplatform        │
             ▼            ▼          └────────┬─────────┘
        ┌──────────┐                          │
        │gdvariant │◄─────────────────────────┘
        └────┬─────┘
             │
             ▼
        ┌──────────┐
        │  gdcore  │  (foundation — no deps)
        └──────────┘`}</code>
            </pre>
          </div>
        </section>

        {/* Crate details by layer */}
        <section>
          <div className="mb-6 flex items-center gap-3">
            <div className="flex size-8 items-center justify-center rounded-lg bg-muted/50 text-amber-400">
              <Box className="size-4" />
            </div>
            <h2 className="text-xl font-semibold tracking-tight">
              All Crates
            </h2>
          </div>

          <div className="space-y-12">
            {layerOrder.map((layerKey) => {
              const layer = layerLabels[layerKey];
              const layerCrates = crates.filter((c) => c.layer === layerKey);
              if (layerCrates.length === 0) return null;

              return (
                <div key={layerKey}>
                  <p
                    className={`mb-4 font-mono text-xs font-medium uppercase tracking-widest ${layer.color}`}
                  >
                    {layer.label}
                  </p>
                  <div className="space-y-3">
                    {layerCrates.map((crate) => (
                      <div
                        key={crate.name}
                        className="rounded-xl border border-border/50 bg-card/40 p-5"
                      >
                        <div className="flex flex-wrap items-center gap-2">
                          <h3 className="font-mono text-sm font-semibold">
                            {crate.name}
                          </h3>
                          {crate.deps.length > 0 && (
                            <span className="text-xs text-muted-foreground/50">
                              depends on {crate.deps.join(", ")}
                            </span>
                          )}
                        </div>
                        <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                          {crate.description}
                        </p>
                      </div>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        </section>

        {/* Footer note */}
        <div className="mt-20 rounded-xl border border-border/50 bg-brand-muted p-6">
          <p className="text-sm font-medium text-brand">
            Architecture is stable
          </p>
          <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
            The crate boundaries and dependency graph are unlikely to change.
            New crates may be added but existing contracts are locked. See the{" "}
            <Link
              href="/docs"
              className="text-brand underline underline-offset-4 hover:text-brand/80"
            >
              getting started guide
            </Link>{" "}
            for usage examples.
          </p>
        </div>
      </div>
    </main>
  );
}

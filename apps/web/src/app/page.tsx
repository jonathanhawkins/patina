import { Button } from "@/components/ui/button";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  Shield,
  Gamepad2,
  Code2,
  Zap,
  Wrench,
  Users,
  Github,
  BookOpen,
  ArrowRight,
  ChevronRight,
} from "lucide-react";

const features = [
  {
    icon: Shield,
    title: "Memory Safety",
    description:
      "Built from the ground up in Rust. No garbage collector, no null pointer exceptions, no data races. Safe by default.",
  },
  {
    icon: Gamepad2,
    title: "Godot Compatible",
    description:
      "Load and run Godot .tscn and .tres files natively. Leverage Godot's mature editor while running on a Rust-powered runtime.",
  },
  {
    icon: Code2,
    title: "Open Source",
    description:
      "MIT licensed and built in the open. Contribute, fork, and build on a transparent foundation with no vendor lock-in.",
  },
  {
    icon: Zap,
    title: "High Performance",
    description:
      "Zero-cost abstractions and compile-time optimizations. Designed for predictable frame times and minimal overhead.",
  },
  {
    icon: Wrench,
    title: "Modern Tooling",
    description:
      "First-class Cargo integration, comprehensive type safety, and a developer experience built for productivity.",
  },
  {
    icon: Users,
    title: "Community Driven",
    description:
      "Shaped by game developers, for game developers. Every design decision is driven by real-world use cases and feedback.",
  },
];

const roadmapStages = [
  {
    phase: "Phase 1",
    title: "Oracle",
    description:
      "Scene parser and inspector. Load Godot scenes, query nodes, validate structure. The foundation for everything else.",
    status: "In Progress" as const,
  },
  {
    phase: "Phase 2",
    title: "GDExtension Lab",
    description:
      "Experimental GDExtension bindings. Run Patina components inside Godot for rapid iteration and testing.",
    status: "Planned" as const,
  },
  {
    phase: "Phase 3",
    title: "Headless Runtime",
    description:
      "Server-side scene execution without rendering. Game logic, physics, and scripting running purely in Rust.",
    status: "Planned" as const,
  },
  {
    phase: "Phase 4",
    title: "2D Slice",
    description:
      "Full 2D rendering pipeline. Sprites, tilemaps, particles, and UI — enough to ship a complete 2D game.",
    status: "Planned" as const,
  },
  {
    phase: "Phase 5",
    title: "3D Runtime",
    description:
      "Complete 3D rendering with modern graphics. The endgame: a full Rust-native alternative to the Godot runtime.",
    status: "Future" as const,
  },
];

export default function Home() {
  return (
    <main className="min-h-screen">
      {/* Hero Section */}
      <section className="relative overflow-hidden">
        <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_top,_var(--tw-gradient-stops))] from-zinc-800/50 via-background to-background" />
        <div className="relative mx-auto max-w-6xl px-6 pb-24 pt-32 text-center">
          <Badge variant="outline" className="mb-6">
            Currently in early development
          </Badge>
          <h1 className="mx-auto max-w-4xl text-5xl font-bold tracking-tight sm:text-6xl lg:text-7xl">
            The game engine{" "}
            <span className="bg-gradient-to-r from-zinc-200 to-zinc-500 bg-clip-text text-transparent">
              Rust deserves
            </span>
          </h1>
          <p className="mx-auto mt-6 max-w-2xl text-lg text-muted-foreground sm:text-xl">
            Patina is a Rust-native game engine with full Godot scene
            compatibility. Memory safe, high performance, and open source.
          </p>
          <div className="mt-10 flex items-center justify-center gap-4">
            <Button
              size="lg"
              render={
                <a
                  href="https://github.com/patinaengine/patina"
                  target="_blank"
                  rel="noopener noreferrer"
                />
              }
            >
              <Github className="size-4" />
              View on GitHub
            </Button>
            <Button
              variant="outline"
              size="lg"
              render={
                <a
                  href="https://docs.patinaengine.com"
                  target="_blank"
                  rel="noopener noreferrer"
                />
              }
            >
              <BookOpen className="size-4" />
              Documentation
            </Button>
          </div>

          {/* Code snippet preview */}
          <div className="mx-auto mt-16 max-w-2xl overflow-hidden rounded-xl border border-border/50 bg-zinc-950 text-left shadow-2xl">
            <div className="flex items-center gap-2 border-b border-border/30 px-4 py-3">
              <div className="size-3 rounded-full bg-zinc-700" />
              <div className="size-3 rounded-full bg-zinc-700" />
              <div className="size-3 rounded-full bg-zinc-700" />
              <span className="ml-2 text-xs text-muted-foreground">
                main.rs
              </span>
            </div>
            <pre className="overflow-x-auto p-6 text-sm leading-relaxed">
              <code className="text-zinc-300">
                <span className="text-zinc-500">
                  {"// Load a Godot scene and query nodes\n"}
                </span>
                <span className="text-blue-400">{"use"}</span>
                {" patina::prelude::*;\n\n"}
                <span className="text-blue-400">{"fn"}</span>{" "}
                <span className="text-yellow-300">{"main"}</span>
                {"() {\n"}
                {"    "}
                <span className="text-blue-400">{"let"}</span>
                {" scene = Scene::"}
                <span className="text-yellow-300">{"load"}</span>
                {"("}
                <span className="text-green-400">{'"res://main.tscn"'}</span>
                {");\n"}
                {"    "}
                <span className="text-blue-400">{"let"}</span>
                {" player = scene."}
                <span className="text-yellow-300">{"get_node"}</span>
                {"("}
                <span className="text-green-400">{'"Player"'}</span>
                {");\n"}
                {"    println!("}
                <span className="text-green-400">{'"Found: {}"'}</span>
                {", player.name);\n"}
                {"}"}
              </code>
            </pre>
          </div>
        </div>
      </section>

      {/* Features Grid */}
      <section id="features" className="mx-auto max-w-6xl px-6 py-24">
        <div className="text-center">
          <h2 className="text-3xl font-bold tracking-tight sm:text-4xl">
            Built for the future of game development
          </h2>
          <p className="mt-4 text-lg text-muted-foreground">
            Combining Rust&apos;s safety guarantees with Godot&apos;s proven
            scene architecture.
          </p>
        </div>

        <div className="mt-16 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {features.map((feature) => (
            <Card
              key={feature.title}
              className="border-border/50 bg-card/50 transition-colors hover:bg-card"
            >
              <CardHeader>
                <div className="mb-2 flex size-10 items-center justify-center rounded-lg bg-muted">
                  <feature.icon className="size-5 text-foreground" />
                </div>
                <CardTitle>{feature.title}</CardTitle>
                <CardDescription>{feature.description}</CardDescription>
              </CardHeader>
            </Card>
          ))}
        </div>
      </section>

      {/* How It Works / Roadmap */}
      <section
        id="how-it-works"
        className="border-t border-border/40 bg-muted/30"
      >
        <div className="mx-auto max-w-6xl px-6 py-24">
          <div className="text-center">
            <h2 className="text-3xl font-bold tracking-tight sm:text-4xl">
              A staged approach to building an engine
            </h2>
            <p className="mt-4 text-lg text-muted-foreground">
              Patina is built incrementally. Each phase delivers a working,
              useful tool — not just a milestone on the way to something else.
            </p>
          </div>

          <div className="mt-16 space-y-4">
            {roadmapStages.map((stage, i) => (
              <div
                key={stage.title}
                className="group flex items-start gap-6 rounded-xl border border-border/50 bg-card/50 p-6 transition-colors hover:bg-card"
              >
                <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted font-mono text-sm font-bold text-muted-foreground">
                  {i + 1}
                </div>
                <div className="flex-1">
                  <div className="flex items-center gap-3">
                    <h3 className="text-lg font-semibold">{stage.title}</h3>
                    <Badge
                      variant={
                        stage.status === "In Progress" ? "default" : "outline"
                      }
                    >
                      {stage.status}
                    </Badge>
                  </div>
                  <p className="mt-1 text-sm text-muted-foreground">
                    {stage.description}
                  </p>
                </div>
                <ChevronRight className="size-5 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="border-t border-border/40">
        <div className="mx-auto max-w-6xl px-6 py-24 text-center">
          <h2 className="text-3xl font-bold tracking-tight sm:text-4xl">
            Ready to explore?
          </h2>
          <p className="mt-4 text-lg text-muted-foreground">
            Patina is open source and in active development. Star the repo, read
            the docs, or jump into the code.
          </p>
          <div className="mt-8 flex items-center justify-center gap-4">
            <Button
              size="lg"
              render={
                <a
                  href="https://github.com/patinaengine/patina"
                  target="_blank"
                  rel="noopener noreferrer"
                />
              }
            >
              <Github className="size-4" />
              Star on GitHub
            </Button>
            <Button
              variant="outline"
              size="lg"
              render={
                <a
                  href="https://docs.patinaengine.com"
                  target="_blank"
                  rel="noopener noreferrer"
                />
              }
            >
              Read the Docs
              <ArrowRight className="size-4" />
            </Button>
          </div>
        </div>
      </section>

      {/* Footer */}
      <footer className="border-t border-border/40">
        <div className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 py-8 sm:flex-row">
          <div className="flex items-center gap-2">
            <span className="font-semibold">Patina Engine</span>
            <span className="text-sm text-muted-foreground">MIT Licensed</span>
          </div>
          <div className="flex items-center gap-6">
            <a
              href="https://github.com/patinaengine/patina"
              target="_blank"
              rel="noopener noreferrer"
              className="text-sm text-muted-foreground transition-colors hover:text-foreground"
            >
              GitHub
            </a>
            <a
              href="https://docs.patinaengine.com"
              className="text-sm text-muted-foreground transition-colors hover:text-foreground"
            >
              Documentation
            </a>
            <a
              href="https://github.com/patinaengine/patina/blob/main/LICENSE"
              target="_blank"
              rel="noopener noreferrer"
              className="text-sm text-muted-foreground transition-colors hover:text-foreground"
            >
              License
            </a>
          </div>
        </div>
      </footer>
    </main>
  );
}

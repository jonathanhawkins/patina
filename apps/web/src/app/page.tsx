"use client";

import { Button } from "@/components/ui/button";
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
  Terminal,
  Clock,
  Circle,
} from "lucide-react";
import { motion } from "framer-motion";

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

const features = [
  {
    icon: Shield,
    title: "Memory Safety",
    description:
      "Built from the ground up in Rust. No garbage collector, no null pointer exceptions, no data races.",
    color: "text-emerald-400",
    bg: "bg-emerald-500/10",
  },
  {
    icon: Gamepad2,
    title: "Godot Compatible",
    description:
      "Load and run Godot .tscn and .tres files natively. Use Godot's editor, run on a Rust runtime.",
    color: "text-blue-400",
    bg: "bg-blue-500/10",
  },
  {
    icon: Code2,
    title: "Open Source",
    description:
      "MIT licensed, built in the open. No vendor lock-in, no surprises. Fork it, extend it, ship it.",
    color: "text-violet-400",
    bg: "bg-violet-500/10",
  },
  {
    icon: Zap,
    title: "High Performance",
    description:
      "Zero-cost abstractions and compile-time optimizations. Predictable frame times, minimal overhead.",
    color: "text-amber-400",
    bg: "bg-amber-500/10",
  },
  {
    icon: Wrench,
    title: "Modern Tooling",
    description:
      "First-class Cargo integration, comprehensive type safety, and a developer experience built for flow.",
    color: "text-rose-400",
    bg: "bg-rose-500/10",
  },
  {
    icon: Users,
    title: "Community Driven",
    description:
      "Shaped by game developers, for game developers. Every decision is driven by real-world use cases.",
    color: "text-cyan-400",
    bg: "bg-cyan-500/10",
  },
];

const roadmapStages = [
  {
    phase: "01",
    title: "Oracle",
    description:
      "Scene parser and inspector. Load Godot scenes, query nodes, validate structure.",
    status: "active" as const,
  },
  {
    phase: "02",
    title: "GDExtension Lab",
    description:
      "Experimental GDExtension bindings. Run Patina components inside Godot for rapid iteration.",
    status: "planned" as const,
  },
  {
    phase: "03",
    title: "Headless Runtime",
    description:
      "Server-side scene execution. Game logic, physics, and scripting running purely in Rust.",
    status: "planned" as const,
  },
  {
    phase: "04",
    title: "2D Slice",
    description:
      "Full 2D rendering pipeline. Sprites, tilemaps, particles, and UI. Ship a complete 2D game.",
    status: "planned" as const,
  },
  {
    phase: "05",
    title: "3D Runtime",
    description:
      "Complete 3D rendering with modern graphics. A full Rust-native alternative to the Godot runtime.",
    status: "future" as const,
  },
];

// ---------------------------------------------------------------------------
// Animation variants
// ---------------------------------------------------------------------------

const fadeUp = {
  hidden: { opacity: 0, y: 24 },
  visible: (i: number = 0) => ({
    opacity: 1,
    y: 0,
    transition: {
      duration: 0.5,
      delay: i * 0.08,
      ease: [0.25, 0.4, 0.25, 1] as [number, number, number, number],
    },
  }),
};

const stagger = {
  visible: {
    transition: {
      staggerChildren: 0.08,
    },
  },
};

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export default function Home() {
  return (
    <main className="min-h-screen overflow-x-hidden">
      <HeroSection />
      <FeaturesSection />
      <RoadmapSection />
      <CTASection />
      <Footer />
    </main>
  );
}

// ---------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------

function HeroSection() {
  return (
    <section className="relative overflow-hidden">
      {/* Background layers */}
      <div className="pointer-events-none absolute inset-0">
        {/* Radial glow behind hero */}
        <div className="absolute left-1/2 top-0 h-[600px] w-[900px] -translate-x-1/2 -translate-y-1/4 rounded-full bg-brand/[0.07] blur-[120px]" />
        {/* Grid pattern */}
        <div
          className="absolute inset-0 opacity-[0.03]"
          style={{
            backgroundImage:
              "linear-gradient(rgba(255,255,255,0.1) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.1) 1px, transparent 1px)",
            backgroundSize: "64px 64px",
          }}
        />
        {/* Bottom fade */}
        <div className="absolute bottom-0 left-0 right-0 h-32 bg-gradient-to-t from-background to-transparent" />
      </div>

      <div className="relative mx-auto max-w-5xl px-6 pb-28 pt-28 text-center sm:pt-36 sm:pb-36">
        <motion.div
          initial="hidden"
          animate="visible"
          variants={stagger}
          className="flex flex-col items-center"
        >
          <motion.div variants={fadeUp} custom={0}>
            <Badge
              variant="outline"
              className="mb-8 border-brand/30 bg-brand-muted px-3 py-1 text-xs font-medium text-brand"
            >
              <Circle className="size-1.5 fill-brand text-brand" />
              Early development -- follow along on GitHub
            </Badge>
          </motion.div>

          <motion.h1
            variants={fadeUp}
            custom={1}
            className="max-w-3xl text-4xl font-bold leading-[1.1] tracking-tight sm:text-5xl md:text-6xl lg:text-7xl"
          >
            The game engine{" "}
            <span className="bg-gradient-to-r from-brand via-amber-300 to-orange-400 bg-clip-text text-transparent">
              Rust deserves
            </span>
          </motion.h1>

          <motion.p
            variants={fadeUp}
            custom={2}
            className="mt-6 max-w-xl text-base leading-relaxed text-muted-foreground sm:text-lg"
          >
            A Rust-native game engine with full Godot scene compatibility.
            Memory safe. High performance. Open source.
          </motion.p>

          <motion.div
            variants={fadeUp}
            custom={3}
            className="mt-10 flex flex-col items-center gap-3 sm:flex-row sm:gap-4"
          >
            <Button
              size="lg"
              className="h-11 gap-2 bg-brand px-5 text-sm font-semibold text-brand-foreground shadow-lg shadow-brand/20 hover:bg-brand/85"
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
              className="h-11 gap-2 px-5 text-sm"
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
          </motion.div>

          {/* Code preview */}
          <motion.div
            variants={fadeUp}
            custom={4}
            className="mx-auto mt-20 w-full max-w-2xl"
          >
            <div className="overflow-hidden rounded-xl border border-border/60 bg-[oklch(0.1_0.005_270)] shadow-2xl shadow-black/40">
              <div className="flex items-center gap-2 border-b border-white/[0.06] px-4 py-3">
                <div className="size-2.5 rounded-full bg-[#ff5f57]" />
                <div className="size-2.5 rounded-full bg-[#febc2e]" />
                <div className="size-2.5 rounded-full bg-[#28c840]" />
                <div className="ml-3 flex items-center gap-1.5 text-xs text-muted-foreground">
                  <Terminal className="size-3" />
                  main.rs
                </div>
              </div>
              <pre className="overflow-x-auto px-5 py-5 text-[13px] leading-[1.7] sm:px-6">
                <code>
                  <span className="text-muted-foreground/60">
                    {"// Load a Godot scene and query nodes\n"}
                  </span>
                  <span className="text-blue-400">{"use"}</span>
                  <span className="text-foreground/80">
                    {" patina::prelude::*;\n\n"}
                  </span>
                  <span className="text-blue-400">{"fn"}</span>{" "}
                  <span className="text-amber-300">{"main"}</span>
                  <span className="text-foreground/80">{"() {\n"}</span>
                  {"    "}
                  <span className="text-blue-400">{"let"}</span>
                  <span className="text-foreground/80">{" scene = Scene::"}</span>
                  <span className="text-amber-300">{"load"}</span>
                  <span className="text-foreground/80">{"("}</span>
                  <span className="text-emerald-400">
                    {'"res://main.tscn"'}
                  </span>
                  <span className="text-foreground/80">{");\n"}</span>
                  {"    "}
                  <span className="text-blue-400">{"let"}</span>
                  <span className="text-foreground/80">
                    {" player = scene."}
                  </span>
                  <span className="text-amber-300">{"get_node"}</span>
                  <span className="text-foreground/80">{"("}</span>
                  <span className="text-emerald-400">{'"Player"'}</span>
                  <span className="text-foreground/80">{");\n"}</span>
                  {"    "}
                  <span className="text-foreground/80">{"println!("}</span>
                  <span className="text-emerald-400">{'"Found: {}"'}</span>
                  <span className="text-foreground/80">
                    {", player.name);\n"}
                  </span>
                  <span className="text-foreground/80">{"}"}</span>
                </code>
              </pre>
            </div>
          </motion.div>
        </motion.div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

function FeaturesSection() {
  return (
    <section id="features" className="relative border-t border-border/40">
      <div className="mx-auto max-w-6xl px-6 py-24 sm:py-32">
        <motion.div
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.3 }}
          variants={stagger}
          className="text-center"
        >
          <motion.p
            variants={fadeUp}
            custom={0}
            className="text-sm font-semibold uppercase tracking-widest text-brand"
          >
            Why Patina
          </motion.p>
          <motion.h2
            variants={fadeUp}
            custom={1}
            className="mt-3 text-3xl font-bold tracking-tight sm:text-4xl"
          >
            Built for the future of game development
          </motion.h2>
          <motion.p
            variants={fadeUp}
            custom={2}
            className="mx-auto mt-4 max-w-2xl text-base text-muted-foreground"
          >
            Rust&apos;s safety guarantees meet Godot&apos;s proven scene
            architecture.
          </motion.p>
        </motion.div>

        <motion.div
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.1 }}
          variants={stagger}
          className="mt-16 grid gap-4 sm:grid-cols-2 lg:grid-cols-3"
        >
          {features.map((feature, i) => (
            <motion.div
              key={feature.title}
              variants={fadeUp}
              custom={i}
              className="group rounded-xl border border-border/50 bg-card/40 p-6 transition-colors duration-300 hover:border-border hover:bg-card/80"
            >
              <div
                className={`mb-4 flex size-10 items-center justify-center rounded-lg ${feature.bg}`}
              >
                <feature.icon className={`size-5 ${feature.color}`} />
              </div>
              <h3 className="text-base font-semibold">{feature.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                {feature.description}
              </p>
            </motion.div>
          ))}
        </motion.div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Roadmap
// ---------------------------------------------------------------------------

function StatusIcon({ status }: { status: "active" | "planned" | "future" }) {
  if (status === "active") {
    return (
      <div className="flex size-6 items-center justify-center rounded-full bg-brand/20 ring-2 ring-brand/40">
        <div className="size-2 rounded-full bg-brand" />
      </div>
    );
  }
  if (status === "planned") {
    return (
      <div className="flex size-6 items-center justify-center rounded-full bg-muted ring-2 ring-border">
        <Clock className="size-3 text-muted-foreground" />
      </div>
    );
  }
  return (
    <div className="flex size-6 items-center justify-center rounded-full bg-muted/50 ring-2 ring-border/50">
      <Circle className="size-2.5 text-muted-foreground/50" />
    </div>
  );
}

function statusLabel(status: "active" | "planned" | "future") {
  if (status === "active") return "In Progress";
  if (status === "planned") return "Planned";
  return "Future";
}

function RoadmapSection() {
  return (
    <section id="roadmap" className="relative border-t border-border/40 bg-card/30">
      <div className="mx-auto max-w-4xl px-6 py-24 sm:py-32">
        <motion.div
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.3 }}
          variants={stagger}
          className="text-center"
        >
          <motion.p
            variants={fadeUp}
            custom={0}
            className="text-sm font-semibold uppercase tracking-widest text-brand"
          >
            Roadmap
          </motion.p>
          <motion.h2
            variants={fadeUp}
            custom={1}
            className="mt-3 text-3xl font-bold tracking-tight sm:text-4xl"
          >
            A staged approach to building an engine
          </motion.h2>
          <motion.p
            variants={fadeUp}
            custom={2}
            className="mx-auto mt-4 max-w-2xl text-base text-muted-foreground"
          >
            Each phase delivers a working, useful tool -- not just a milestone
            on the way to something else.
          </motion.p>
        </motion.div>

        <motion.div
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.1 }}
          variants={stagger}
          className="mt-16 space-y-0"
        >
          {roadmapStages.map((stage, i) => (
            <motion.div
              key={stage.title}
              variants={fadeUp}
              custom={i}
              className="group relative flex gap-6"
            >
              {/* Timeline line + dot */}
              <div className="flex flex-col items-center">
                <StatusIcon status={stage.status} />
                {i < roadmapStages.length - 1 && (
                  <div className="w-px flex-1 bg-border/60" />
                )}
              </div>

              {/* Content */}
              <div className="pb-10">
                <div className="flex items-center gap-3">
                  <span className="font-mono text-xs text-muted-foreground/60">
                    {stage.phase}
                  </span>
                  <h3 className="text-base font-semibold">{stage.title}</h3>
                  <Badge
                    variant={stage.status === "active" ? "default" : "outline"}
                    className={
                      stage.status === "active"
                        ? "border-brand/30 bg-brand/15 text-brand"
                        : "text-muted-foreground"
                    }
                  >
                    {statusLabel(stage.status)}
                  </Badge>
                </div>
                <p className="mt-1.5 max-w-lg text-sm leading-relaxed text-muted-foreground">
                  {stage.description}
                </p>
              </div>
            </motion.div>
          ))}
        </motion.div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// CTA
// ---------------------------------------------------------------------------

function CTASection() {
  return (
    <section className="relative border-t border-border/40">
      {/* Glow */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute left-1/2 top-1/2 h-[400px] w-[600px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-brand/[0.04] blur-[100px]" />
      </div>

      <div className="relative mx-auto max-w-3xl px-6 py-24 text-center sm:py-32">
        <motion.div
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.3 }}
          variants={stagger}
        >
          <motion.h2
            variants={fadeUp}
            custom={0}
            className="text-3xl font-bold tracking-tight sm:text-4xl"
          >
            Ready to explore?
          </motion.h2>
          <motion.p
            variants={fadeUp}
            custom={1}
            className="mt-4 text-base text-muted-foreground"
          >
            Patina is open source and in active development. Star the repo, read
            the docs, or jump into the code.
          </motion.p>
          <motion.div
            variants={fadeUp}
            custom={2}
            className="mt-8 flex flex-col items-center justify-center gap-3 sm:flex-row sm:gap-4"
          >
            <Button
              size="lg"
              className="h-11 gap-2 bg-brand px-5 text-sm font-semibold text-brand-foreground shadow-lg shadow-brand/20 hover:bg-brand/85"
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
              className="h-11 gap-2 px-5 text-sm"
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
          </motion.div>
        </motion.div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

function Footer() {
  return (
    <footer className="border-t border-border/40">
      <div className="mx-auto max-w-6xl px-6 py-12">
        <div className="flex flex-col items-start justify-between gap-8 sm:flex-row sm:items-center">
          <div>
            <div className="flex items-center gap-2.5">
              <div className="flex size-6 items-center justify-center rounded-md bg-brand/90">
                <span className="text-[10px] font-bold text-brand-foreground">
                  P
                </span>
              </div>
              <span className="text-sm font-semibold">Patina Engine</span>
            </div>
            <p className="mt-2 text-xs text-muted-foreground">
              A Rust-native, Godot-compatible game engine. MIT Licensed.
            </p>
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
              Docs
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
      </div>
    </footer>
  );
}

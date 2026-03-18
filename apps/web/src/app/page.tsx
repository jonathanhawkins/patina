"use client";

import { Button } from "@/components/ui/button";
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
        {/* Subtle radial glow -- offset left to match left-aligned content */}
        <div className="absolute -top-32 left-[15%] h-[600px] w-[700px] rounded-full bg-brand/[0.05] blur-[140px]" />
        {/* Fine dot grid -- very subtle texture */}
        <div
          className="absolute inset-0 opacity-[0.02]"
          style={{
            backgroundImage:
              "radial-gradient(rgba(255,255,255,0.3) 1px, transparent 1px)",
            backgroundSize: "32px 32px",
          }}
        />
      </div>

      <div className="relative mx-auto max-w-6xl px-6 pb-24 pt-24 sm:pt-32 sm:pb-32 lg:pt-40 lg:pb-40">
        <motion.div
          initial="hidden"
          animate="visible"
          variants={stagger}
        >
          {/* Two-column layout: hero text left, code right */}
          <div className="grid items-start gap-16 lg:grid-cols-[1fr_minmax(0,520px)] lg:gap-20">
            {/* Left column -- hero text */}
            <div className="max-w-xl">
              <motion.p
                variants={fadeUp}
                custom={0}
                className="mb-6 text-sm font-medium tracking-wide text-brand"
              >
                Early development -- follow along on GitHub
              </motion.p>

              <motion.h1
                variants={fadeUp}
                custom={1}
                className="text-4xl font-bold leading-[1.08] tracking-tight sm:text-5xl lg:text-6xl"
              >
                The game engine{" "}
                <span className="text-brand">
                  Rust deserves
                </span>
              </motion.h1>

              <motion.p
                variants={fadeUp}
                custom={2}
                className="mt-6 text-lg leading-relaxed text-muted-foreground"
              >
                A Rust-native game engine with full Godot scene compatibility.
                Memory safe. High performance. Open source.
              </motion.p>

              <motion.div
                variants={fadeUp}
                custom={3}
                className="mt-10 flex items-center gap-3"
              >
                <Button
                  size="lg"
                  className="h-10 gap-2 rounded-md bg-brand px-5 text-sm font-medium text-brand-foreground hover:bg-brand/85"
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
                  className="h-10 gap-2 rounded-md px-5 text-sm"
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
            </div>

            {/* Right column -- code preview */}
            <motion.div
              variants={fadeUp}
              custom={3}
              className="w-full"
            >
              <div className="overflow-hidden rounded-lg border border-border/60 bg-[oklch(0.1_0.005_270)]">
                <div className="flex items-center gap-2 border-b border-white/[0.06] px-4 py-3">
                  <div className="size-2.5 rounded-full bg-[#ff5f57]/70" />
                  <div className="size-2.5 rounded-full bg-[#febc2e]/70" />
                  <div className="size-2.5 rounded-full bg-[#28c840]/70" />
                  <div className="ml-3 flex items-center gap-1.5 text-xs text-muted-foreground/70">
                    <Terminal className="size-3" />
                    main.rs
                  </div>
                </div>
                <pre className="overflow-x-auto px-5 py-5 text-[13px] leading-[1.7] sm:px-6">
                  <code>
                    <span className="text-muted-foreground/50">
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
          </div>
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
        >
          <motion.p
            variants={fadeUp}
            custom={0}
            className="text-sm font-medium tracking-wide text-brand"
          >
            Why Patina
          </motion.p>
          <motion.h2
            variants={fadeUp}
            custom={1}
            className="mt-3 max-w-lg text-3xl font-bold tracking-tight sm:text-4xl"
          >
            Built for the future of game development
          </motion.h2>
          <motion.p
            variants={fadeUp}
            custom={2}
            className="mt-4 max-w-xl text-base text-muted-foreground"
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
          className="mt-16 grid gap-px overflow-hidden rounded-lg border border-border/50 bg-border/30 sm:grid-cols-2 lg:grid-cols-3"
        >
          {features.map((feature, i) => (
            <motion.div
              key={feature.title}
              variants={fadeUp}
              custom={i}
              className="bg-background p-6 transition-colors duration-300 hover:bg-card/60 sm:p-8"
            >
              <feature.icon className={`mb-4 size-5 ${feature.color}`} />
              <h3 className="text-sm font-semibold">{feature.title}</h3>
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

function StatusIndicator({ status }: { status: "active" | "planned" | "future" }) {
  if (status === "active") {
    return (
      <div className="relative flex size-2.5 items-center justify-center">
        <div className="absolute size-2.5 animate-ping rounded-full bg-brand/40" />
        <div className="size-2 rounded-full bg-brand" />
      </div>
    );
  }
  if (status === "planned") {
    return <div className="size-2 rounded-full bg-muted-foreground/40" />;
  }
  return <div className="size-2 rounded-full bg-muted-foreground/20" />;
}

function statusLabel(status: "active" | "planned" | "future") {
  if (status === "active") return "In Progress";
  if (status === "planned") return "Planned";
  return "Future";
}

function RoadmapSection() {
  return (
    <section id="roadmap" className="relative border-t border-border/40">
      <div className="mx-auto max-w-6xl px-6 py-24 sm:py-32">
        <motion.div
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.3 }}
          variants={stagger}
        >
          <motion.p
            variants={fadeUp}
            custom={0}
            className="text-sm font-medium tracking-wide text-brand"
          >
            Roadmap
          </motion.p>
          <motion.h2
            variants={fadeUp}
            custom={1}
            className="mt-3 max-w-lg text-3xl font-bold tracking-tight sm:text-4xl"
          >
            A staged approach to building an engine
          </motion.h2>
          <motion.p
            variants={fadeUp}
            custom={2}
            className="mt-4 max-w-xl text-base text-muted-foreground"
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
              className="group relative flex items-start gap-4 border-l border-border/50 py-6 pl-8"
            >
              {/* Timeline dot */}
              <div className="absolute -left-[5px] top-7">
                <StatusIndicator status={stage.status} />
              </div>

              {/* Content */}
              <div className="flex-1">
                <div className="flex items-center gap-3">
                  <span className="font-mono text-xs text-muted-foreground/50">
                    {stage.phase}
                  </span>
                  <h3 className="text-sm font-semibold">{stage.title}</h3>
                  {stage.status === "active" && (
                    <span className="rounded-md bg-brand/10 px-2 py-0.5 text-xs font-medium text-brand">
                      {statusLabel(stage.status)}
                    </span>
                  )}
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
      <div className="relative mx-auto max-w-6xl px-6 py-24 sm:py-32">
        <motion.div
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.3 }}
          variants={stagger}
        >
          <motion.h2
            variants={fadeUp}
            custom={0}
            className="max-w-md text-3xl font-bold tracking-tight sm:text-4xl"
          >
            Ready to explore?
          </motion.h2>
          <motion.p
            variants={fadeUp}
            custom={1}
            className="mt-4 max-w-lg text-base text-muted-foreground"
          >
            Patina is open source and in active development. Star the repo, read
            the docs, or jump into the code.
          </motion.p>
          <motion.div
            variants={fadeUp}
            custom={2}
            className="mt-8 flex items-center gap-3"
          >
            <Button
              size="lg"
              className="h-10 gap-2 rounded-md bg-brand px-5 text-sm font-medium text-brand-foreground hover:bg-brand/85"
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
              className="h-10 gap-2 rounded-md px-5 text-sm"
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
              <span className="text-sm font-semibold text-brand">Patina</span>
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

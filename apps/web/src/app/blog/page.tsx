import type { Metadata } from "next";
import { Newspaper } from "lucide-react";

export const metadata: Metadata = {
  title: "Blog",
  description:
    "News, updates, and technical deep dives from the Patina Engine team.",
};

export default function BlogPage() {
  return (
    <main className="min-h-screen">
      <div className="mx-auto max-w-4xl px-6 pb-24 pt-16 sm:pt-24">
        <div className="flex items-center gap-2 text-brand">
          <Newspaper className="size-4" />
          <p className="font-mono text-xs font-medium uppercase tracking-widest">
            Blog
          </p>
        </div>
        <h1 className="mt-4 text-3xl font-semibold tracking-tight sm:text-4xl">
          News & Updates
        </h1>
        <p className="mt-4 max-w-2xl text-base leading-relaxed text-muted-foreground">
          Technical deep dives, development updates, and announcements from the
          Patina Engine project.
        </p>

        {/* Coming soon state */}
        <div className="mt-16 flex flex-col items-center rounded-xl border border-border/50 bg-card/40 px-6 py-20 text-center">
          <div className="flex size-12 items-center justify-center rounded-xl bg-muted/50 text-muted-foreground">
            <Newspaper className="size-5" />
          </div>
          <h2 className="mt-4 text-lg font-semibold tracking-tight">
            Coming soon
          </h2>
          <p className="mt-2 max-w-sm text-sm leading-relaxed text-muted-foreground">
            We&apos;re working on our first posts. Follow the project on{" "}
            <a
              href="https://github.com/patinaengine/patina"
              target="_blank"
              rel="noopener noreferrer"
              className="text-brand underline underline-offset-4 hover:text-brand/80"
            >
              GitHub
            </a>{" "}
            to stay up to date.
          </p>
        </div>
      </div>
    </main>
  );
}

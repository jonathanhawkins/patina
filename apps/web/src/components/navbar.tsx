import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Github } from "lucide-react";

export function Navbar() {
  return (
    <header className="sticky top-0 z-50 w-full border-b border-border/40 bg-background/80 backdrop-blur-lg">
      <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-6">
        <Link href="/" className="flex items-center gap-2">
          <span className="text-lg font-bold tracking-tight">Patina</span>
        </Link>

        <nav className="hidden items-center gap-6 md:flex">
          <Link
            href="#features"
            className="text-sm text-muted-foreground transition-colors hover:text-foreground"
          >
            Features
          </Link>
          <Link
            href="#how-it-works"
            className="text-sm text-muted-foreground transition-colors hover:text-foreground"
          >
            Roadmap
          </Link>
          <Link
            href="https://docs.patinaengine.com"
            className="text-sm text-muted-foreground transition-colors hover:text-foreground"
          >
            Docs
          </Link>
        </nav>

        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="icon"
            render={
              <a
                href="https://github.com/patinaengine/patina"
                target="_blank"
                rel="noopener noreferrer"
              />
            }
          >
            <Github className="size-4" />
            <span className="sr-only">GitHub</span>
          </Button>
          <Button
            size="sm"
            render={
              <a
                href="https://github.com/patinaengine/patina"
                target="_blank"
                rel="noopener noreferrer"
              />
            }
          >
            Get Started
          </Button>
        </div>
      </div>
    </header>
  );
}

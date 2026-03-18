"use client";

import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Github, Menu, X } from "lucide-react";
import { motion } from "framer-motion";
import { useState } from "react";

const navLinks = [
  { href: "#features", label: "Features" },
  { href: "#roadmap", label: "Roadmap" },
  { href: "https://docs.patinaengine.com", label: "Docs", external: true },
];

export function Navbar() {
  const [mobileOpen, setMobileOpen] = useState(false);

  return (
    <motion.header
      initial={{ y: -20, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      transition={{ duration: 0.4, ease: "easeOut" }}
      className="sticky top-0 z-50 w-full border-b border-border/50 bg-background/60 backdrop-blur-xl backdrop-saturate-150"
    >
      <div className="mx-auto flex h-16 max-w-6xl items-center justify-between px-6">
        <Link href="/" className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-md bg-brand/90">
            <span className="text-xs font-bold text-brand-foreground">P</span>
          </div>
          <span className="text-base font-semibold tracking-tight">
            Patina Engine
          </span>
        </Link>

        <nav className="hidden items-center gap-1 md:flex">
          {navLinks.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              {...(link.external
                ? { target: "_blank", rel: "noopener noreferrer" }
                : {})}
              className="rounded-md px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground"
            >
              {link.label}
            </Link>
          ))}
        </nav>

        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="icon"
            nativeButton={false}
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
            nativeButton={false}
            className="hidden bg-brand text-brand-foreground hover:bg-brand/80 sm:inline-flex"
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
          <Button
            variant="ghost"
            size="icon"
            className="md:hidden"
            onClick={() => setMobileOpen(!mobileOpen)}
          >
            {mobileOpen ? <X className="size-4" /> : <Menu className="size-4" />}
          </Button>
        </div>
      </div>

      {/* Mobile nav */}
      {mobileOpen && (
        <motion.div
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          className="border-t border-border/50 bg-background/95 backdrop-blur-xl md:hidden"
        >
          <nav className="flex flex-col gap-1 px-6 py-4">
            {navLinks.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                onClick={() => setMobileOpen(false)}
                className="rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                {link.label}
              </Link>
            ))}
          </nav>
        </motion.div>
      )}
    </motion.header>
  );
}

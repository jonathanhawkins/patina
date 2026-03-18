---
name: Patina Engine Website Stack
description: Key details about the patinaengine.com website tech stack and design system
type: project
---

The website at apps/web/ uses:
- Next.js 16, React 19, TypeScript, Tailwind CSS v4
- shadcn/ui base-nova style with base-ui/react primitives (NOT radix)
- Deployed to Cloudflare Workers via @opennextjs/cloudflare
- framer-motion for animations (added during redesign)
- Geist + Geist Mono fonts
- Brand color: amber/orange (oklch 0.78 0.138 55) -- evokes Rust and "patina" warmth
- Dark mode only (html class="dark")
- Custom CSS variables: --brand, --brand-foreground, --brand-muted

**Why:** The site was redesigned from a plain grey template to a polished game engine landing page.

**How to apply:** Use the brand color system for CTAs and accents. Keep dark-mode-only aesthetic. Use motion animations for scroll reveals.

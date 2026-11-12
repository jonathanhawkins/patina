import type { Metadata } from "next";
import { EditorShell } from "@/components/editor/editor-shell";

export const metadata: Metadata = {
  title: "Editor",
  description:
    "Patina Engine 3D editor — streams rendered frames from the Rust editor_server and forwards camera input.",
};

export default function EditorPage() {
  return (
    <main className="flex h-[calc(100vh-4rem)] w-full flex-col bg-neutral-950 text-neutral-200">
      <header className="flex items-center justify-between border-b border-neutral-800 px-4 py-2">
        <div className="flex items-center gap-2">
          <span className="font-mono text-xs font-medium uppercase tracking-widest text-brand">
            Editor
          </span>
          <span className="text-xs text-muted-foreground">
            3D viewport · streaming from editor_server
          </span>
        </div>
        <div className="font-mono text-[10px] uppercase tracking-widest text-muted-foreground">
          LMB orbit · MMB/RMB pan · wheel zoom · WASD/QE move · R reset
        </div>
      </header>
      <EditorShell />
    </main>
  );
}

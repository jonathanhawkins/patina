"use client";

import { useState } from "react";
import { InspectorPanel } from "./inspector-panel";
import { SceneTreePanel } from "./scene-tree-panel";
import { Viewport3D } from "./viewport-3d";

export function EditorShell() {
  const [selectedNodeId, setSelectedNodeId] = useState<number | null>(null);

  return (
    <div className="flex flex-1 overflow-hidden">
      <SceneTreePanel onSelect={(node) => setSelectedNodeId(node.id)} />
      <div className="relative flex-1">
        <Viewport3D />
      </div>
      <InspectorPanel selectedNodeId={selectedNodeId} />
    </div>
  );
}

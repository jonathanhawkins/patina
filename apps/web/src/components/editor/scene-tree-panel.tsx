"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
} from "react";

type SceneNode = {
  id: number;
  name: string;
  class: string;
  path: string;
  visible: boolean;
  is_instance: boolean;
  has_script: boolean;
  has_signals: boolean;
  groups: string[];
  children: SceneNode[];
};

type SceneTreePanelProps = {
  serverUrl?: string;
  pollIntervalMs?: number;
  className?: string;
  onSelect?: (node: SceneNode) => void;
};

type Status = "loading" | "ready" | "error";

const DEFAULT_SERVER_URL =
  process.env.NEXT_PUBLIC_EDITOR_SERVER_URL ?? "http://localhost:8080";

const DEFAULT_NODE_CLASS = "Node3D";

/**
 * Scene tree panel: fetches the live scene tree from editor_server and
 * exposes add / rename / reparent / delete mutations over HTTP. Matches the
 * contract exercised by the `scene_tree_panel` integration test.
 */
export function SceneTreePanel({
  serverUrl = DEFAULT_SERVER_URL,
  pollIntervalMs = 1500,
  className,
  onSelect,
}: SceneTreePanelProps) {
  const [root, setRoot] = useState<SceneNode | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const mountedRef = useRef(true);
  const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await fetch(`${serverUrl}/api/scene`, { cache: "no-store" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body = (await res.json()) as { nodes: SceneNode };
      if (!mountedRef.current) return;
      setRoot(body.nodes);
      setStatus("ready");
      setErrorMessage(null);
      // Auto-expand root on first successful load.
      setExpanded((prev) => {
        if (prev.size > 0) return prev;
        const next = new Set<number>();
        next.add(body.nodes.id);
        for (const child of body.nodes.children) next.add(child.id);
        return next;
      });
    } catch (err) {
      if (!mountedRef.current) return;
      setStatus("error");
      setErrorMessage(err instanceof Error ? err.message : String(err));
    }
  }, [serverUrl]);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    const schedule = () => {
      pollTimerRef.current = setTimeout(async () => {
        await refresh();
        if (mountedRef.current) schedule();
      }, pollIntervalMs);
    };
    schedule();
    return () => {
      mountedRef.current = false;
      if (pollTimerRef.current !== null) clearTimeout(pollTimerRef.current);
    };
  }, [refresh, pollIntervalMs]);

  const postJson = useCallback(
    async (path: string, body: Record<string, unknown>) => {
      const res = await fetch(`${serverUrl}${path}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        throw new Error(`${path} failed: HTTP ${res.status}`);
      }
      return (await res.json()) as Record<string, unknown>;
    },
    [serverUrl],
  );

  const selectNode = useCallback(
    (node: SceneNode) => {
      setSelectedId(node.id);
      onSelect?.(node);
      void postJson("/api/node/select", { node_id: node.id }).catch((err) => {
        if (process.env.NODE_ENV !== "production") {
          console.warn("select failed", err);
        }
      });
    },
    [onSelect, postJson],
  );

  const toggleExpand = useCallback((nodeId: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
  }, []);

  const handleAddChild = useCallback(
    async (parent: SceneNode) => {
      const name = window.prompt(`New child of "${parent.name}" — name?`, "Node");
      if (!name) return;
      const className =
        window.prompt("Class name?", DEFAULT_NODE_CLASS) ?? DEFAULT_NODE_CLASS;
      try {
        await postJson("/api/node/add", {
          parent_id: parent.id,
          name,
          class_name: className,
        });
        setExpanded((prev) => {
          const next = new Set(prev);
          next.add(parent.id);
          return next;
        });
        await refresh();
      } catch (err) {
        setErrorMessage(err instanceof Error ? err.message : String(err));
      }
    },
    [postJson, refresh],
  );

  const handleDelete = useCallback(
    async (node: SceneNode) => {
      try {
        await postJson("/api/node/delete", { node_id: node.id });
        if (selectedId === node.id) setSelectedId(null);
        await refresh();
      } catch (err) {
        setErrorMessage(err instanceof Error ? err.message : String(err));
      }
    },
    [postJson, refresh, selectedId],
  );

  const startRename = useCallback((node: SceneNode) => {
    setRenamingId(node.id);
    setRenameDraft(node.name);
  }, []);

  const commitRename = useCallback(async () => {
    if (renamingId === null) return;
    const id = renamingId;
    const newName = renameDraft.trim();
    setRenamingId(null);
    if (!newName) return;
    try {
      await postJson("/api/node/rename", { node_id: id, new_name: newName });
      await refresh();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : String(err));
    }
  }, [postJson, refresh, renameDraft, renamingId]);

  const handleReparent = useCallback(
    async (dragId: number, targetId: number) => {
      if (dragId === targetId) return;
      try {
        await postJson("/api/node/reparent", {
          node_id: dragId,
          new_parent_id: targetId,
        });
        await refresh();
      } catch (err) {
        setErrorMessage(err instanceof Error ? err.message : String(err));
      }
    },
    [postJson, refresh],
  );

  const rootChildren = useMemo(() => root?.children ?? [], [root]);

  return (
    <div
      className={
        className ??
        "flex h-full w-72 flex-col border-r border-neutral-800 bg-neutral-950 text-neutral-200"
      }
    >
      <div className="flex items-center justify-between border-b border-neutral-800 px-3 py-2">
        <span className="font-mono text-[10px] uppercase tracking-widest text-brand">
          Scene
        </span>
        <button
          type="button"
          onClick={() => void refresh()}
          className="rounded border border-neutral-800 px-2 py-0.5 text-[10px] uppercase tracking-widest text-neutral-400 hover:border-neutral-600 hover:text-neutral-200"
        >
          Refresh
        </button>
      </div>
      <div className="flex-1 overflow-auto px-1 py-1 font-mono text-xs">
        {status === "loading" && (
          <div className="px-2 py-1 text-neutral-500">loading…</div>
        )}
        {status === "error" && (
          <div className="px-2 py-1 text-red-400">{errorMessage}</div>
        )}
        {root && (
          <SceneNodeRow
            node={root}
            depth={0}
            selectedId={selectedId}
            expanded={expanded}
            renamingId={renamingId}
            renameDraft={renameDraft}
            setRenameDraft={setRenameDraft}
            commitRename={commitRename}
            cancelRename={() => setRenamingId(null)}
            onSelect={selectNode}
            onToggle={toggleExpand}
            onAddChild={handleAddChild}
            onDelete={handleDelete}
            onStartRename={startRename}
            onReparent={handleReparent}
            isRoot
          />
        )}
        {root && rootChildren.length === 0 && (
          <div className="px-2 py-1 text-neutral-500">(empty scene)</div>
        )}
      </div>
      {errorMessage && status === "ready" && (
        <div className="border-t border-red-900 bg-red-950/40 px-3 py-1 text-[11px] text-red-300">
          {errorMessage}
        </div>
      )}
    </div>
  );
}

type SceneNodeRowProps = {
  node: SceneNode;
  depth: number;
  selectedId: number | null;
  expanded: Set<number>;
  renamingId: number | null;
  renameDraft: string;
  setRenameDraft: (value: string) => void;
  commitRename: () => void;
  cancelRename: () => void;
  onSelect: (node: SceneNode) => void;
  onToggle: (id: number) => void;
  onAddChild: (node: SceneNode) => void;
  onDelete: (node: SceneNode) => void;
  onStartRename: (node: SceneNode) => void;
  onReparent: (dragId: number, targetId: number) => void;
  isRoot?: boolean;
};

function SceneNodeRow({
  node,
  depth,
  selectedId,
  expanded,
  renamingId,
  renameDraft,
  setRenameDraft,
  commitRename,
  cancelRename,
  onSelect,
  onToggle,
  onAddChild,
  onDelete,
  onStartRename,
  onReparent,
  isRoot = false,
}: SceneNodeRowProps) {
  const isExpanded = expanded.has(node.id);
  const isSelected = selectedId === node.id;
  const isRenaming = renamingId === node.id;
  const hasChildren = node.children.length > 0;

  const handleDragStart = (event: DragEvent<HTMLDivElement>) => {
    if (isRoot) {
      event.preventDefault();
      return;
    }
    event.dataTransfer.setData("application/x-scene-node-id", String(node.id));
    event.dataTransfer.effectAllowed = "move";
  };

  const handleDragOver = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
  };

  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const raw = event.dataTransfer.getData("application/x-scene-node-id");
    const dragId = Number.parseInt(raw, 10);
    if (Number.isFinite(dragId) && dragId !== node.id) {
      onReparent(dragId, node.id);
    }
  };

  return (
    <>
      <div
        draggable={!isRoot}
        onDragStart={handleDragStart}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
        onClick={() => onSelect(node)}
        className={`group flex items-center gap-1 rounded px-1 py-0.5 ${
          isSelected
            ? "bg-brand/20 text-brand"
            : "hover:bg-neutral-900 text-neutral-200"
        }`}
        style={{ paddingLeft: `${depth * 12 + 4}px` }}
      >
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onToggle(node.id);
          }}
          className="w-3 text-[10px] text-neutral-500"
        >
          {hasChildren ? (isExpanded ? "▾" : "▸") : " "}
        </button>
        <span className="text-[10px] uppercase text-neutral-500">
          {shortClass(node.class)}
        </span>
        {isRenaming ? (
          <input
            autoFocus
            value={renameDraft}
            onChange={(e) => setRenameDraft(e.target.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRename();
              if (e.key === "Escape") cancelRename();
            }}
            onClick={(e) => e.stopPropagation()}
            className="flex-1 rounded border border-neutral-700 bg-neutral-900 px-1 text-xs text-neutral-100 outline-none"
          />
        ) : (
          <span
            onDoubleClick={(e) => {
              e.stopPropagation();
              onStartRename(node);
            }}
            className="flex-1 truncate"
          >
            {node.name}
          </span>
        )}
        <div className="hidden gap-1 text-[10px] text-neutral-500 group-hover:flex">
          <button
            type="button"
            title="Add child"
            onClick={(e) => {
              e.stopPropagation();
              void onAddChild(node);
            }}
            className="px-1 hover:text-neutral-100"
          >
            +
          </button>
          {!isRoot && (
            <button
              type="button"
              title="Rename"
              onClick={(e) => {
                e.stopPropagation();
                onStartRename(node);
              }}
              className="px-1 hover:text-neutral-100"
            >
              ✎
            </button>
          )}
          {!isRoot && (
            <button
              type="button"
              title="Delete"
              onClick={(e) => {
                e.stopPropagation();
                void onDelete(node);
              }}
              className="px-1 hover:text-red-400"
            >
              ×
            </button>
          )}
        </div>
      </div>
      {isExpanded &&
        node.children.map((child) => (
          <SceneNodeRow
            key={child.id}
            node={child}
            depth={depth + 1}
            selectedId={selectedId}
            expanded={expanded}
            renamingId={renamingId}
            renameDraft={renameDraft}
            setRenameDraft={setRenameDraft}
            commitRename={commitRename}
            cancelRename={cancelRename}
            onSelect={onSelect}
            onToggle={onToggle}
            onAddChild={onAddChild}
            onDelete={onDelete}
            onStartRename={onStartRename}
            onReparent={onReparent}
          />
        ))}
    </>
  );
}

function shortClass(className: string): string {
  if (!className) return "?";
  return className.length > 7 ? className.slice(0, 7) : className;
}

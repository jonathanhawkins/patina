"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

type PropertyValue = {
  type: string;
  value: unknown;
};

type PropertyEntry = {
  name: string;
  type: string;
  value: PropertyValue;
};

type CategoryName =
  | "Transform"
  | "Rendering"
  | "Physics"
  | "Script"
  | "Misc";

type InspectorPayload = {
  id: number;
  name: string;
  class: string;
  path: string;
  categories: Record<CategoryName, PropertyEntry[]>;
};

type InspectorPanelProps = {
  selectedNodeId: number | null;
  serverUrl?: string;
  pollIntervalMs?: number;
  className?: string;
};

type Status = "idle" | "loading" | "ready" | "error";

const DEFAULT_SERVER_URL =
  process.env.NEXT_PUBLIC_EDITOR_SERVER_URL ?? "http://localhost:8080";

const CATEGORY_ORDER: CategoryName[] = [
  "Transform",
  "Rendering",
  "Physics",
  "Script",
  "Misc",
];

export function InspectorPanel({
  selectedNodeId,
  serverUrl = DEFAULT_SERVER_URL,
  pollIntervalMs = 1500,
  className,
}: InspectorPanelProps) {
  const [payload, setPayload] = useState<InspectorPayload | null>(null);
  const [status, setStatus] = useState<Status>("idle");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const mountedRef = useRef(true);
  const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refresh = useCallback(async () => {
    if (selectedNodeId === null) {
      setPayload(null);
      setStatus("idle");
      setErrorMessage(null);
      return;
    }
    try {
      const res = await fetch(
        `${serverUrl}/api/inspector?node_id=${selectedNodeId}`,
        { cache: "no-store" },
      );
      if (res.status === 404) {
        if (!mountedRef.current) return;
        setPayload(null);
        setStatus("idle");
        return;
      }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body = (await res.json()) as InspectorPayload;
      if (!mountedRef.current) return;
      setPayload(body);
      setStatus("ready");
      setErrorMessage(null);
    } catch (err) {
      if (!mountedRef.current) return;
      setStatus("error");
      setErrorMessage(err instanceof Error ? err.message : String(err));
    }
  }, [selectedNodeId, serverUrl]);

  useEffect(() => {
    mountedRef.current = true;
    setDrafts({});
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

  const setProperty = useCallback(
    async (name: string, value: PropertyValue) => {
      if (selectedNodeId === null) return;
      try {
        const res = await fetch(`${serverUrl}/api/property/set`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            node_id: selectedNodeId,
            property: name,
            value,
          }),
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        await refresh();
      } catch (err) {
        setErrorMessage(err instanceof Error ? err.message : String(err));
      }
    },
    [refresh, selectedNodeId, serverUrl],
  );

  const categories = useMemo(() => payload?.categories ?? null, [payload]);

  return (
    <div
      className={
        className ??
        "flex h-full w-80 flex-col border-l border-neutral-800 bg-neutral-950 text-neutral-200"
      }
    >
      <div className="flex items-center justify-between border-b border-neutral-800 px-3 py-2">
        <span className="font-mono text-[10px] uppercase tracking-widest text-brand">
          Inspector
        </span>
        <button
          type="button"
          onClick={() => void refresh()}
          className="rounded border border-neutral-800 px-2 py-0.5 text-[10px] uppercase tracking-widest text-neutral-400 hover:border-neutral-600 hover:text-neutral-200"
        >
          Refresh
        </button>
      </div>
      <div className="flex-1 overflow-auto px-2 py-2 font-mono text-xs">
        {selectedNodeId === null && (
          <div className="px-1 py-1 text-neutral-500">(no selection)</div>
        )}
        {selectedNodeId !== null && status === "loading" && (
          <div className="px-1 py-1 text-neutral-500">loading…</div>
        )}
        {selectedNodeId !== null && status === "idle" && !payload && (
          <div className="px-1 py-1 text-neutral-500">
            (node has no inspectable data)
          </div>
        )}
        {payload && (
          <div className="mb-2 border-b border-neutral-900 pb-2">
            <div className="text-[10px] uppercase text-neutral-500">
              {payload.class}
            </div>
            <div className="truncate text-sm text-neutral-100">
              {payload.name}
            </div>
            <div className="truncate text-[10px] text-neutral-600">
              {payload.path}
            </div>
          </div>
        )}
        {categories &&
          CATEGORY_ORDER.map((category) => {
            const entries = categories[category] ?? [];
            if (entries.length === 0) return null;
            return (
              <section key={category} className="mb-3">
                <div className="mb-1 border-b border-neutral-900 pb-0.5 text-[10px] uppercase tracking-widest text-brand">
                  {category}
                </div>
                <div className="flex flex-col gap-1.5">
                  {entries.map((entry) => (
                    <PropertyRow
                      key={entry.name}
                      entry={entry}
                      drafts={drafts}
                      setDrafts={setDrafts}
                      onCommit={setProperty}
                    />
                  ))}
                </div>
              </section>
            );
          })}
      </div>
      {errorMessage && (
        <div className="border-t border-red-900 bg-red-950/40 px-3 py-1 text-[11px] text-red-300">
          {errorMessage}
        </div>
      )}
    </div>
  );
}

type PropertyRowProps = {
  entry: PropertyEntry;
  drafts: Record<string, string>;
  setDrafts: React.Dispatch<React.SetStateAction<Record<string, string>>>;
  onCommit: (name: string, value: PropertyValue) => void;
};

function PropertyRow({ entry, drafts, setDrafts, onCommit }: PropertyRowProps) {
  const label = (
    <div className="flex items-baseline justify-between gap-2">
      <span className="truncate text-neutral-300">{entry.name}</span>
      <span className="text-[9px] uppercase text-neutral-600">
        {entry.type}
      </span>
    </div>
  );

  const type = entry.type;
  const raw = entry.value.value;

  if (type === "Bool") {
    return (
      <div className="rounded border border-neutral-900 bg-neutral-950 px-2 py-1">
        {label}
        <label className="mt-1 flex items-center gap-2 text-neutral-200">
          <input
            type="checkbox"
            checked={Boolean(raw)}
            onChange={(e) =>
              onCommit(entry.name, { type: "Bool", value: e.target.checked })
            }
          />
          <span className="text-[10px] text-neutral-500">
            {raw ? "true" : "false"}
          </span>
        </label>
      </div>
    );
  }

  if (type === "Int" || type === "Float") {
    const draftKey = entry.name;
    const current =
      drafts[draftKey] ??
      (typeof raw === "number" ? String(raw) : String(raw ?? ""));
    return (
      <div className="rounded border border-neutral-900 bg-neutral-950 px-2 py-1">
        {label}
        <input
          type="number"
          value={current}
          onChange={(e) =>
            setDrafts((prev) => ({ ...prev, [draftKey]: e.target.value }))
          }
          onBlur={() => {
            const parsed =
              type === "Int" ? Number.parseInt(current, 10) : Number(current);
            if (Number.isFinite(parsed)) {
              onCommit(entry.name, { type, value: parsed });
            }
            setDrafts((prev) => {
              const next = { ...prev };
              delete next[draftKey];
              return next;
            });
          }}
          className="mt-1 w-full rounded border border-neutral-800 bg-neutral-900 px-1 py-0.5 text-xs text-neutral-100 outline-none focus:border-neutral-600"
        />
      </div>
    );
  }

  if (type === "String") {
    const draftKey = entry.name;
    const current = drafts[draftKey] ?? (typeof raw === "string" ? raw : "");
    return (
      <div className="rounded border border-neutral-900 bg-neutral-950 px-2 py-1">
        {label}
        <input
          type="text"
          value={current}
          onChange={(e) =>
            setDrafts((prev) => ({ ...prev, [draftKey]: e.target.value }))
          }
          onBlur={() => {
            onCommit(entry.name, { type: "String", value: current });
            setDrafts((prev) => {
              const next = { ...prev };
              delete next[draftKey];
              return next;
            });
          }}
          className="mt-1 w-full rounded border border-neutral-800 bg-neutral-900 px-1 py-0.5 text-xs text-neutral-100 outline-none focus:border-neutral-600"
        />
      </div>
    );
  }

  if (type === "Vector2" || type === "Vector3" || type === "Vector4") {
    const components =
      type === "Vector2" ? ["x", "y"] : type === "Vector3" ? ["x", "y", "z"] : ["x", "y", "z", "w"];
    const arr = Array.isArray(raw) ? (raw as number[]) : [];
    return (
      <div className="rounded border border-neutral-900 bg-neutral-950 px-2 py-1">
        {label}
        <div className="mt-1 grid grid-cols-2 gap-1 sm:grid-cols-3">
          {components.map((axis, idx) => {
            const draftKey = `${entry.name}.${axis}`;
            const current =
              drafts[draftKey] ??
              (arr[idx] !== undefined ? String(arr[idx]) : "0");
            return (
              <label
                key={axis}
                className="flex items-center gap-1 text-[10px] text-neutral-500"
              >
                <span className="w-3 uppercase">{axis}</span>
                <input
                  type="number"
                  value={current}
                  onChange={(e) =>
                    setDrafts((prev) => ({
                      ...prev,
                      [draftKey]: e.target.value,
                    }))
                  }
                  onBlur={() => {
                    const nextArr = components.map((ax, i) => {
                      const key = `${entry.name}.${ax}`;
                      const d = drafts[key];
                      if (d !== undefined) {
                        const n = Number(d);
                        return Number.isFinite(n) ? n : arr[i] ?? 0;
                      }
                      return arr[i] ?? 0;
                    });
                    // Include the just-edited axis even if state hasn't
                    // flushed yet.
                    const parsed = Number(current);
                    if (Number.isFinite(parsed)) nextArr[idx] = parsed;
                    onCommit(entry.name, { type, value: nextArr });
                    setDrafts((prev) => {
                      const next = { ...prev };
                      for (const ax of components) delete next[`${entry.name}.${ax}`];
                      return next;
                    });
                  }}
                  className="w-full rounded border border-neutral-800 bg-neutral-900 px-1 py-0.5 text-xs text-neutral-100 outline-none focus:border-neutral-600"
                />
              </label>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <div className="rounded border border-neutral-900 bg-neutral-950 px-2 py-1 text-neutral-400">
      {label}
      <div className="mt-1 truncate text-[10px] text-neutral-500">
        {JSON.stringify(raw)}
      </div>
    </div>
  );
}

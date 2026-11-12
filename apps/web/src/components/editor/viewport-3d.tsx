"use client";

import { useCallback, useEffect, useRef, useState } from "react";

type ViewportStatus = "connecting" | "streaming" | "idle" | "error";

type PointerMode = "idle" | "orbit" | "pan";

type Viewport3DProps = {
  serverUrl?: string;
  targetFps?: number;
  className?: string;
};

const DEFAULT_SERVER_URL =
  process.env.NEXT_PUBLIC_EDITOR_SERVER_URL ?? "http://localhost:8080";

/**
 * Streams rendered frames from the Rust editor_server and forwards
 * mouse/keyboard input for 3D camera control.
 */
export function Viewport3D({
  serverUrl = DEFAULT_SERVER_URL,
  targetFps = 30,
  className,
}: Viewport3DProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const pointerModeRef = useRef<PointerMode>("idle");
  const lastPointerRef = useRef<{ x: number; y: number } | null>(null);
  const inFlightRef = useRef<boolean>(false);
  const mountedRef = useRef<boolean>(true);

  const [status, setStatus] = useState<ViewportStatus>("connecting");
  const [lastError, setLastError] = useState<string | null>(null);
  const [frameCount, setFrameCount] = useState(0);
  const [dimensions, setDimensions] = useState<{ w: number; h: number } | null>(
    null,
  );

  const postCameraEvent = useCallback(
    async (path: string, body: Record<string, number>) => {
      try {
        await fetch(`${serverUrl}${path}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        });
      } catch (err) {
        // Camera input failures are non-fatal for the viewport stream; the
        // frame loop reports connectivity issues separately.
        if (process.env.NODE_ENV !== "production") {
          console.warn("viewport camera input failed", path, err);
        }
      }
    },
    [serverUrl],
  );

  useEffect(() => {
    mountedRef.current = true;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      setStatus("error");
      setLastError("2D canvas context unavailable");
      return;
    }

    const frameIntervalMs = Math.max(16, Math.round(1000 / targetFps));
    let timer: ReturnType<typeof setTimeout> | null = null;

    const tick = async () => {
      if (!mountedRef.current) return;
      if (inFlightRef.current) {
        timer = setTimeout(tick, frameIntervalMs);
        return;
      }
      inFlightRef.current = true;
      try {
        const res = await fetch(`${serverUrl}/api/viewport/png`, {
          cache: "no-store",
        });
        if (res.status === 404) {
          if (mountedRef.current) setStatus("idle");
        } else if (!res.ok) {
          throw new Error(`frame fetch failed: HTTP ${res.status}`);
        } else {
          const blob = await res.blob();
          const bitmap = await createImageBitmap(blob);
          if (!mountedRef.current) {
            bitmap.close?.();
            return;
          }
          if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
            canvas.width = bitmap.width;
            canvas.height = bitmap.height;
            setDimensions({ w: bitmap.width, h: bitmap.height });
          }
          ctx.drawImage(bitmap, 0, 0);
          bitmap.close?.();
          setStatus("streaming");
          setLastError(null);
          setFrameCount((n) => n + 1);
        }
      } catch (err) {
        if (mountedRef.current) {
          setStatus("error");
          setLastError(err instanceof Error ? err.message : String(err));
        }
      } finally {
        inFlightRef.current = false;
        if (mountedRef.current) {
          timer = setTimeout(tick, frameIntervalMs);
        }
      }
    };

    tick();

    return () => {
      mountedRef.current = false;
      if (timer !== null) clearTimeout(timer);
    };
  }, [serverUrl, targetFps]);

  const handlePointerDown = useCallback(
    (event: React.PointerEvent<HTMLCanvasElement>) => {
      event.currentTarget.setPointerCapture(event.pointerId);
      lastPointerRef.current = { x: event.clientX, y: event.clientY };
      // Left button = orbit, middle/right = pan.
      pointerModeRef.current =
        event.button === 0 ? "orbit" : "pan";
    },
    [],
  );

  const handlePointerMove = useCallback(
    (event: React.PointerEvent<HTMLCanvasElement>) => {
      const mode = pointerModeRef.current;
      if (mode === "idle") return;
      const last = lastPointerRef.current;
      if (!last) return;
      const dx = event.clientX - last.x;
      const dy = event.clientY - last.y;
      lastPointerRef.current = { x: event.clientX, y: event.clientY };
      if (dx === 0 && dy === 0) return;
      if (mode === "orbit") {
        void postCameraEvent("/api/viewport3d/orbit", { dx, dy });
      } else {
        void postCameraEvent("/api/viewport3d/pan", { dx, dy });
      }
    },
    [postCameraEvent],
  );

  const handlePointerUp = useCallback(
    (event: React.PointerEvent<HTMLCanvasElement>) => {
      event.currentTarget.releasePointerCapture(event.pointerId);
      pointerModeRef.current = "idle";
      lastPointerRef.current = null;
    },
    [],
  );

  const handleWheel = useCallback(
    (event: React.WheelEvent<HTMLCanvasElement>) => {
      event.preventDefault();
      const delta = event.deltaY;
      if (delta === 0) return;
      void postCameraEvent("/api/viewport3d/zoom", { delta });
    },
    [postCameraEvent],
  );

  const handleContextMenu = useCallback(
    (event: React.MouseEvent<HTMLCanvasElement>) => {
      event.preventDefault();
    },
    [],
  );

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const key = event.key.toLowerCase();
      const moveMap: Record<string, { dx: number; dy: number }> = {
        w: { dx: 0, dy: -10 },
        s: { dx: 0, dy: 10 },
        a: { dx: -10, dy: 0 },
        d: { dx: 10, dy: 0 },
      };
      const delta = moveMap[key];
      if (delta) {
        event.preventDefault();
        void postCameraEvent("/api/viewport3d/pan", delta);
      } else if (key === "q") {
        void postCameraEvent("/api/viewport3d/zoom", { delta: 120 });
      } else if (key === "e") {
        void postCameraEvent("/api/viewport3d/zoom", { delta: -120 });
      } else if (key === "r") {
        void postCameraEvent("/api/viewport3d/reset", {});
      }
    },
    [postCameraEvent],
  );

  return (
    <div
      ref={containerRef}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      className={
        className ??
        "relative flex h-full w-full flex-col items-center justify-center bg-neutral-950 outline-none"
      }
    >
      <canvas
        ref={canvasRef}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onWheel={handleWheel}
        onContextMenu={handleContextMenu}
        className="max-h-full max-w-full touch-none select-none border border-neutral-800 bg-black"
        style={{ imageRendering: "pixelated" }}
      />
      <ViewportHud
        status={status}
        frameCount={frameCount}
        dimensions={dimensions}
        error={lastError}
        serverUrl={serverUrl}
      />
    </div>
  );
}

function ViewportHud({
  status,
  frameCount,
  dimensions,
  error,
  serverUrl,
}: {
  status: ViewportStatus;
  frameCount: number;
  dimensions: { w: number; h: number } | null;
  error: string | null;
  serverUrl: string;
}) {
  const statusColor: Record<ViewportStatus, string> = {
    connecting: "text-amber-400",
    streaming: "text-emerald-400",
    idle: "text-sky-400",
    error: "text-red-400",
  };
  return (
    <div className="pointer-events-none absolute left-3 top-3 rounded border border-neutral-800 bg-black/70 px-3 py-2 font-mono text-xs text-neutral-300">
      <div>
        <span className={statusColor[status]}>● </span>
        {status}
      </div>
      <div>frames: {frameCount}</div>
      {dimensions && (
        <div>
          size: {dimensions.w}×{dimensions.h}
        </div>
      )}
      <div className="max-w-[280px] truncate opacity-60">{serverUrl}</div>
      {error && <div className="text-red-400">{error}</div>}
    </div>
  );
}

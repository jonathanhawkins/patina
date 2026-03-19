import { ImageResponse } from "next/og";

export const size = {
  width: 1200,
  height: 630,
};

export const contentType = "image/png";

export default function OpenGraphImage() {
  return new ImageResponse(
    (
      <div
        style={{
          height: "100%",
          width: "100%",
          display: "flex",
          position: "relative",
          overflow: "hidden",
          background:
            "radial-gradient(circle at top, rgba(240, 180, 80, 0.18), transparent 42%), linear-gradient(180deg, #181615 0%, #0f0f10 100%)",
          color: "#f7f4ef",
          fontFamily:
            'Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
        }}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            backgroundImage:
              "radial-gradient(rgba(255,255,255,0.08) 1px, transparent 1px)",
            backgroundSize: "28px 28px",
            opacity: 0.12,
          }}
        />

        <div
          style={{
            position: "absolute",
            top: -140,
            right: -120,
            height: 360,
            width: 360,
            borderRadius: 9999,
            background: "rgba(237, 174, 73, 0.18)",
            filter: "blur(18px)",
          }}
        />

        <div
          style={{
            position: "relative",
            display: "flex",
            height: "100%",
            width: "100%",
            padding: "54px",
          }}
        >
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              justifyContent: "space-between",
              width: "100%",
              borderRadius: "32px",
              border: "1px solid rgba(255,255,255,0.1)",
              background: "rgba(22, 21, 20, 0.86)",
              boxShadow: "0 24px 90px rgba(0,0,0,0.34)",
              padding: "42px 46px",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "14px",
                  fontSize: 24,
                  fontWeight: 700,
                  letterSpacing: "-0.03em",
                }}
              >
                <div
                  style={{
                    height: 18,
                    width: 18,
                    borderRadius: 9999,
                    background: "#edae49",
                    boxShadow: "0 0 24px rgba(237, 174, 73, 0.45)",
                  }}
                />
                <span>Patina Engine</span>
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  borderRadius: 9999,
                  border: "1px solid rgba(255,255,255,0.12)",
                  background: "rgba(255,255,255,0.03)",
                  color: "#edae49",
                  padding: "10px 16px",
                  fontSize: 18,
                  fontWeight: 600,
                }}
              >
                Rust-native
              </div>
            </div>

            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "20px",
                maxWidth: 900,
              }}
            >
              <div
                style={{
                  display: "flex",
                  fontSize: 68,
                  lineHeight: 1.02,
                  fontWeight: 800,
                  letterSpacing: "-0.055em",
                  textWrap: "balance",
                }}
              >
                Rust-Native, Godot-Compatible Game Engine
              </div>
              <div
                style={{
                  display: "flex",
                  fontSize: 30,
                  lineHeight: 1.3,
                  color: "rgba(247, 244, 239, 0.72)",
                  maxWidth: 860,
                  letterSpacing: "-0.02em",
                }}
              >
                Memory-safe runtime. Native Godot scene compatibility. Open
                source and built for serious game development.
              </div>
            </div>

            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
              }}
            >
              <div
                style={{
                  display: "flex",
                  gap: "14px",
                  color: "rgba(247, 244, 239, 0.72)",
                  fontSize: 22,
                }}
              >
                <span>Memory Safe</span>
                <span style={{ color: "rgba(255,255,255,0.28)" }}>•</span>
                <span>High Performance</span>
                <span style={{ color: "rgba(255,255,255,0.28)" }}>•</span>
                <span>Open Source</span>
              </div>
              <div
                style={{
                  display: "flex",
                  fontSize: 24,
                  fontWeight: 600,
                  color: "#bcb5ab",
                }}
              >
                patinaengine.com
              </div>
            </div>
          </div>
        </div>
      </div>
    ),
    size,
  );
}

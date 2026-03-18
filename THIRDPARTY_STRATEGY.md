# THIRDPARTY_STRATEGY.md - Third-Party Dependency Strategy

This document defines how upstream Godot's third-party dependencies are classified, the decision framework for each, and a template for recording classification decisions.

---

## Core Rule

> No team should start reimplementing third-party code until the classification decision has been made and recorded in this document.

---

## Four Classification Buckets

Every dependency in upstream Godot's `thirdparty/` directory is classified into exactly one bucket:

### 1. Replace with Rust Crate

Use an existing Rust crate from the ecosystem that provides equivalent functionality.

**When to choose**: Mature Rust alternative exists with compatible license, adequate performance, and active maintenance.

**Advantages**: Native Rust safety, no FFI overhead, idiomatic API, better compiler integration.

**Risks**: API mismatch, behavioral differences from upstream, additional dependency.

### 2. Wrap via FFI

Keep the C/C++ library and call it through Rust FFI bindings.

**When to choose**: Library is mature, well-tested, and performance-critical; no adequate Rust alternative; wrapping is straightforward.

**Advantages**: Proven implementation, performance parity with upstream, minimal behavioral risk.

**Risks**: Unsafe FFI boundary, build complexity, cross-platform build issues, harder debugging.

### 3. Vendor Unchanged

Include the library source directly in the repository without modification.

**When to choose**: Library is small, stable, rarely changes, and its existing build integrates easily; or it is needed temporarily during transition.

**Advantages**: Exact upstream behavior, no adaptation work, known-good code.

**Risks**: Maintenance burden, license tracking, potential staleness.

### 4. Reimplement Cleanly

Write a new Rust implementation from behavioral specifications (not source translation).

**When to choose**: Library is small, its behavior is well-specified, no good Rust or C alternative exists, and clean-room implementation is tractable.

**Advantages**: Full Rust safety, no external dependencies, tailored API.

**Risks**: Highest effort, risk of behavioral divergence, maintenance burden.

---

## Decision Factors

When classifying a dependency, evaluate these factors:

| Factor | Questions to Answer |
|--------|-------------------|
| **License** | Is the license compatible with our project? Any copyleft contamination risk? Attribution requirements? |
| **Maintenance** | Is the upstream library actively maintained? What is the release cadence? |
| **Performance** | Is this performance-critical? Would FFI overhead matter? Would a Rust crate meet performance needs? |
| **API Surface Size** | How large is the API we actually use? Can we wrap a small subset? |
| **Ecosystem Maturity** | Are there mature Rust crates? How battle-tested are they? |
| **Portability** | Does it compile on all our target platforms? Any platform-specific concerns? |
| **Debugging** | How important is debuggability? Is FFI boundary acceptable for debugging? |
| **Behavioral Criticality** | How important is exact behavioral match with upstream? Would a different implementation cause compatibility issues? |

---

## Dependency Entry Template

Use this template when recording a classification decision:

```markdown
### <Dependency Name>

| Field | Value |
|-------|-------|
| **Upstream Location** | `thirdparty/<name>/` |
| **Upstream Version** | x.y.z |
| **License** | MIT / Apache-2.0 / zlib / etc. |
| **Used By** | Which Godot subsystems use this |
| **Classification** | Replace / Wrap / Vendor / Reimplement |
| **Rust Alternative** | Crate name and version (if Replace) |
| **Rationale** | Why this classification was chosen |
| **Risk Notes** | Any risks specific to this decision |
| **Decided By** | Who made the decision |
| **Date** | When the decision was recorded |
```

---

## Key Dependencies to Classify

The following are major third-party dependencies in upstream Godot's `thirdparty/` directory. Each must be classified before any implementation work involving that dependency begins.

### Rendering and Graphics

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| vulkan / volk | Vulkan loader and headers | MIT / Apache-2.0 | Wrap or replace with ash/vulkano |
| glslang | GLSL to SPIR-V compiler | BSD-3-Clause | Wrap via FFI or replace with naga |
| spirv-reflect | SPIR-V reflection | Apache-2.0 | Replace with spirv-reflect-rs or naga |
| glad | OpenGL loader | MIT | Replace with glow or gl |
| meshoptimizer | Mesh optimization | MIT | Wrap or replace with meshopt-rs |

### Physics

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| bullet | 3D physics (legacy) | zlib | Defer (3D is Phase 6+) |
| godot-jolt | 3D physics (modern) | MIT | Defer (3D is Phase 6+) |
| clipper2 | 2D polygon clipping | BSL-1.0 | Classify when needed for 2D physics |

### Audio

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| libvorbis / libogg | OGG Vorbis audio | BSD-3-Clause | Replace with lewton or wrap |
| libtheora | Theora video | BSD-3-Clause | Defer (video is low priority) |
| minimp3 | MP3 decoding | CC0 | Replace with minimp3-rs or symphonia |

### Compression and Serialization

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| zlib | Compression | zlib | Replace with flate2 |
| zstd | Compression | BSD-3-Clause | Replace with zstd-rs |
| lz4 | Compression | BSD-2-Clause | Replace with lz4_flex |
| brotli | Compression | MIT | Replace with brotli-rs |

### Image and Texture

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| libpng | PNG encoding/decoding | libpng | Replace with png crate |
| libjpeg-turbo | JPEG decoding | IJG / BSD-3-Clause | Replace with image or jpeg-decoder |
| libwebp | WebP encoding/decoding | BSD-3-Clause | Replace with webp crate or wrap |
| etcpak | ETC texture compression | BSD-2-Clause | Wrap or defer |
| astcenc | ASTC texture compression | Apache-2.0 | Wrap or defer |
| basis_universal | Basis Universal textures | Apache-2.0 | Wrap or defer |
| squish | S3TC/DXT compression | MIT | Replace or defer |

### Text and Font

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| freetype | Font rendering | FTL / GPL-2.0 | Wrap or replace with rusttype/ab_glyph |
| harfbuzz | Text shaping | MIT | Wrap or replace with rustybuzz |
| icu4c | Unicode/internationalization | ICU | Replace with icu4x |
| msdfgen | SDF font generation | MIT | Wrap or reimplement |

### Networking

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| enet | Reliable UDP | MIT | Replace with enet-rs or defer |
| mbedtls | TLS | Apache-2.0 | Replace with rustls |
| wslay | WebSocket | MIT | Replace with tungstenite |

### Platform and System

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| SDL2 (optional) | Platform abstraction | zlib | Replace with winit |
| wayland / xkbcommon | Linux windowing | MIT | Replace with winit + xkbcommon-rs |

### Math and Utility

| Dependency | Upstream Purpose | License | Initial Assessment |
|-----------|-----------------|---------|-------------------|
| pcre2 | Regular expressions | BSD-3-Clause | Replace with regex crate |
| doctest | C++ testing | MIT | Not needed (use Rust test framework) |
| rvo2 | Crowd navigation | Apache-2.0 | Defer or reimplement when needed |

---

## Classification Status

| Status | Count |
|--------|-------|
| Classified | 0 |
| Pending | All |

This table will be updated as classification decisions are made.

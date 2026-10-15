# Architecture

Three repositories, one family:

| Repo | What | Depends on |
|---|---|---|
| `anicraft-libs` | Shared engine crates (`anim-*`) | Craft crates by git rev |
| `anicraft` | 2D animation editor, file-compatible with Adobe Animate (XFL/FLA, SWF export) | `anicraft-libs` by git |
| `charcraft` | Live character animation, file-compatible with Adobe Character Animator (.puppet, .chproj) | `anicraft-libs` by git |

Apps depend on the libs with `anim-x = { git = "https://github.com/soof-golan/anicraft-libs", branch = "main" }` while
we build (Cargo.lock pins the commit; `cargo update -p anim-x` picks up new work) and switch to `rev = "..."` pins for
releases, the way the Craft apps pin `craft-libs`.

## Data flow

```
 files ──► format crates ──► models ──► producers ──► DisplayList ──► anim-render ──► pixels ──► UI / exporters
 (XFL, PSD, AI, CA)        (anim-scene,   (anim-timeline,    (anim-display)                    (PNG, GIF, MP4,
                            CA puppet)     puppet engine)                                       SVG, SWF)
```

- **Models** are plain serde data. Editing = applying commands to a model; undo = model snapshots.
- **Producers** evaluate a model at a time/frame into an [`anim_display::DisplayList`]. Text is shaped into glyph
  outlines here (via `anim-text`), so renderers and exporters never shape text.
- **`anim-render`** rasterises a `DisplayList` with an `ImageStore`. Exporters either rasterise (PNG, GIF, MP4) or
  translate display lists (SVG, SWF).

## anicraft-libs crates (`crates/<name>`, package `anim-<name>`)

| Crate | Owns | Key dependencies |
|---|---|---|
| `anim-core` | `Tick`/`FrameRate` (from `filmcraft-time`), kurbo re-exports, `Rgba8`, `PremulRgba`, `ColorTransform`, `BlendMode`, `num` (the only `as` casts) | filmcraft-time, kurbo |
| `anim-display` | `DisplayList`, `Node` (Group/Fill/Stroke/Bitmap/Mesh), `Paint`, `Gradient` (Flash gradient space ±819.2), `StrokeStyle`, `Filter`, `ImageStore` | anim-core |
| `anim-archive` | Reading/writing zip containers robustly (Animate's FLA zips have a central directory 54 bytes short; names may be CP437/Windows-125x, not UTF-8; XML entity-escaped names) and a `Container` abstraction over "zip file" vs "folder" (XFL folders, CA project folders) | zip, encoding_rs |
| `anim-shape` | Animate's edge-based vector shapes: XFL `edges` strings (twips, hex numbers, `!`, `|`, `/`, `[`, `S` commands) decode/encode; planar edge sets with `fillStyle0`/`fillStyle1`/`strokeStyle` → closed fill paths + stroke paths; building edge shapes from plain closed paths (drawing tools) | anim-core, kurbo |
| `anim-scene` | The document model (Animate semantics; see below), library, media store for bitmaps/sounds bytes, validation, queries ("which keyframe covers frame N") | anim-core, anim-shape |
| `anim-morph` | Shape tweens: contour correspondence, shape hints, interpolation of fills/strokes between two `anim-shape` shapes | anim-shape |
| `anim-text` | Rich text layout with right-to-left and bidi (parley + skrifa): `RichText` (paragraphs of styled runs) → `TextEngine::layout` → lines, glyph runs, bounds → `to_display` glyph outlines as `anim-display` nodes; system font discovery, per-script fallback chains, PostScript/full/missing font-name resolution (`Arial-BoldMT` → Liberation Sans bold); caret, hit-test, selection and visual-movement geometry for editing (correct in RTL and mixed text); never panics on hostile text (parley is guarded) | parley, skrifa |
| `anim-timeline` | Evaluation: `Document` + scene + frame → `DisplayList`. Keyframe spans, classic/motion/shape tweens with easing (classic ease −100..100 and custom bezier ease), nested symbol timelines (graphic loop/play-once/single-frame, movie clips advance with the parent), masks and guides, onion skins | anim-scene, anim-morph, anim-text, anim-display |
| `anim-render` | CPU rasteriser (tiny-skia): fills, strokes, gradients (linear/radial/focal, pad/reflect/repeat), bitmaps and bitmap fills, colour transforms, blend modes, masks, filters (blur, drop shadow, glow, colour matrix), textured meshes; deterministic; golden-image tests | anim-display, tiny-skia, rayon |
| `anim-rig` | Deformation: silhouette triangulation (constrained Delaunay), ARAP mesh deformation with pinned handles, bone chains with 2-bone IK, damped-spring dangles | anim-core |
| `anim-audio` | Decode (symphonia: MP3/WAV/AAC/FLAC/Vorbis) to `f32` PCM, resampling, mixing, waveform peaks; playback and microphone actors over cpal (owning their streams; talk via channels) | symphonia, cpal |
| `anim-lipsync` | PCM → timed viseme track, offline and streaming (live mic). Character Animator viseme set (Neutral, Ah, D, Ee, F, L, M, Oh, R, S, Uh, W-Oo) and Animate's lip-sync set | anim-core |
| `anim-artwork` | Layered artwork import into a neutral tree: PSD/PSB (photocraft-psd) and AI/PDF (layers, sublayers, groups, vector paths) and SVG; group/layer names preserved exactly (puppet tags live in names) | photocraft-psd, craft-pdf / vectorcraft crates |
| `anim-export` | PNG sequence, animated GIF, sprite sheet + JSON, animated SVG; progress/cancel callbacks | anim-render, png, gif |
| `anim-video` | MP4 (H.264 + AAC) and MOV (ProRes 4444 with alpha) encoding with filmcraft's pure-Rust codecs; an actor that accepts frames and audio and reports progress | filmcraft-h264enc/isobmff/aac/prores |
| `anim-ui` | Shared egui widgets: stage view (pan/zoom/rulers/guides, texture upload), timeline grid (layers, frames, keyframes, tweens, playhead, onion skin), layer list, property inspector, library browser, transport bar, docking via craft-ui, theme | egui, craft-ui |
| `anim-session` | App framework: command registry (`id`, label, menu, shortcut, `run(&mut Doc, params) -> Result`), snapshot undo/redo with merge keys, background jobs as actors with progress/cancel, settings (craft-settings), atomic save (craft-atomic), recent files | craft-settings, craft-atomic |
| `anim-mcp` | MCP (Model Context Protocol) for both apps, see `docs/mcp.md`: control protocol (requests, replies with images, structured errors), `SessionHost` (commands, atomic batches, properties, queries, render, screenshot, export, open/save, path confinement, call journal), headless actor backend, bridge backend and `ControlServer` (loopback JSON lines + token) for running apps, rmcp front-end (tools generated from the command registry, resources, prompts, progress, cancellation), `anim-mcp` stdio bridge binary | anim-session, rmcp, tokio, schemars |

### What `anim-render` guarantees producers and exporters

Entry points: `render(&DisplayList, &ImageStore, &RenderOptions) -> Result<Image>` (premultiplied RGBA8), the reusable
`Renderer` (keeps scratch buffers; `recycle(image)` hands a finished frame back; `render_nodes` draws a node list
without a `DisplayList`), `RenderStats` (what the last render skipped), `content_bounds`, `pick` / `pick_all` (hit
testing by geometry, reporting the `Group::key`s around the hit and the index path to the node) and the
straight/premultiplied pixel converters.

- **Coordinates**: stage pixels, y down; `RenderOptions::view` maps stage to output pixels. Geometry beyond ±1e7
  device pixels, non-finite numbers and degenerate matrices skip the node (counted in `RenderStats::invalid`), never
  panic; nesting deeper than `MAX_DEPTH` (64) is not drawn.
- **Groups** that only carry a transform draw straight onto the parent. Otherwise (colour transform, blend other than
  normal/pass-through, mask, filters) the group is rendered into a bounds-limited offscreen layer and processed in this
  order: children, mask alpha, filters in list order, colour transform, composite with the blend mode. `Normal` and
  `PassThrough` groups are not isolated (their children blend with the backdrop); `Layer` isolates.
- **Blend modes**: Flash `Alpha` and `Erase` act on the *current layer buffer*, so give their parent `BlendMode::Layer`
  (as Animate does); `Subtract` and `Invert` use the W3C general compositing formula; the stage background is part of the
  root buffer, so blend modes see it.
- **Colour transforms** (and colour-matrix filters) leave fully transparent pixels alone, so results never depend on
  internal layer bounds. Offsets are unit-range (`1.0` = 255).
- **Strokes**: width 0 is a one-pixel hairline in every scale mode. `StrokeScale::Horizontal` means the thickness does
  not follow horizontal scaling (SWF `NoHScale`), `Vertical` the reverse, `None` is device pixels; dash lengths are in
  local units. Odd dash arrays repeat.
- **Filters**: blur sizes and shadow distances are in the group's own pixels and scale with the matrix; `strength` 1.0 is
  100 %; blur `quality` is the number of box passes (shadows and glows use 3); the shadow direction stays fixed in device
  space. Layers over 64 M pixels skip blur/shadow/glow (`degraded_groups`).
- **Bitmaps**: smoothing off is nearest-neighbour; smoothing on is bilinear, with box-filtered mip levels when drawn at
  under half size (built per render; `Renderer::keep_image_cache(true)` keeps them across frames when the store's image
  ids keep meaning the same pixels, which is what playback of large layered artwork wants). Clamped bitmap fills extend
  their edge pixels.
- **Meshes**: an undeformed mesh (target == source) reproduces the plain render exactly and has hard edges; a deformed
  mesh renders its content at the fitted pixel density, fills triangles seamlessly and anti-aliases its silhouette.
- **Determinism**: the same inputs give the same bytes on one machine, regardless of thread count. tiny-skia's SIMD paths
  may differ in the last bit between CPU architectures, so compare across platforms with a small tolerance.

### The document model (`anim-scene`)

Mirrors XFL so loading and saving lose nothing:

- `Document { width, height, frame_rate, background, scenes: Vec<Timeline>, library: Library, extra: Unknown }`.
- `Library { items: Vec<LibraryItem> }`, `LibraryItem::{Symbol(Symbol{ kind: Graphic|MovieClip|Button, timeline, ... }),
  Bitmap{ media: MediaId, ... }, Sound{ media: MediaId, ... }, Font, Video, Folder}`, addressed by library path names.
- `Timeline { name, layers: Vec<Layer> }`, `Layer { name, kind: Normal|Guide|Guided|Mask|Masked|Folder|Camera,
  parent: Option<LayerIndex>, color, visible, locked, outline, frames: Vec<Frame> }`.
- `Frame { index, duration, key_mode, tween: None|Classic|Shape|Motion, ease, label, sound, script (kept verbatim),
  elements: Vec<Element> }`.
- `Element::{Shape, SymbolInstance{ item, matrix, transformation_point, color: ColorTransform, blend, filters,
  loop_mode, first_frame, ... }, Bitmap, Text{ runs with direction/alignment/font/size/colour }, Group}`.
- Unknown XML attributes and elements are kept (`extra`) and written back so round trips are lossless.

## anicraft crates

`anicraft-xfl` (XFL folder / zipped FLA ⇄ `anim-scene`, lossless round trip, tolerant reading), `anicraft-swf`
(SWF writer from display lists and symbols: shapes, gradients, bitmaps, sprites, sounds), `anicraft-tools` (drawing
and selection tools as pure functions from pointer events to commands), `anicraft-app` (eframe app, binary
`anicraft`), `anicraft-cli` (headless convert/render/export, used by tests).

## charcraft crates

`charcraft-ltab` (the Lua-table text format CA uses for metadata, read and write), `charcraft-rope` (the
`Rope10LM` log and git-style object store inside `Ch Data/repo.noindex`), `charcraft-project` (.chproj folders and
.puppet zips ⇄ project model), `charcraft-puppet` (puppet model: layer tree from artwork, tags from names such as
`+Head`, `Mouth`, visemes, eyes, handles, behaviours and their parameters, rigging), `charcraft-behaviors` (Face,
Lip Sync, Eye Gaze, Auto Blink, Dragger, Physics/Dangle, Breathe, Walk, Triggers, Transform), `charcraft-tracking`
(webcam via nokhwa + face landmarks via ort ONNX Runtime, as an actor), `charcraft-record` (takes, scene timeline,
playback), `charcraft-app` (binary `charcraft`), `charcraft-cli`.

## Crate template

```toml
[package]
name = "anim-<name>"
description = "..."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true

[lints]
workspace = true
```

Every crate has a crate-level doc comment saying what it owns, unit tests, and (for formats) round-trip tests.

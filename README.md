# anicraft-libs

Shared engine crates for [AniCraft](https://github.com/soof-golan/anicraft) (2D animation) and
[CharCraft](https://github.com/soof-golan/charcraft) (live character animation): vector shapes, timelines and
tweens, display lists, a CPU renderer, right-to-left text layout, puppet deformation, audio and lip sync, artwork
import, and image/video export. Pure Rust; runs on macOS, Windows and Linux.

Built on the open-source [Craft](https://github.com/storytold) crates (FilmCraft codecs, PhotoCraft PSD,
craft-libs UI and settings).

See [ARCHITECTURE.md](ARCHITECTURE.md) for the crate map and [AGENTS.md](AGENTS.md) for the rules every change
follows.

## Crates

| Crate | Purpose |
| --- | --- |
| `anim-core` | Time (`Tick`, `FrameRate`), geometry and colour primitives, checked numeric helpers |
| `anim-display` | Display list: groups, fills, strokes, bitmaps, meshes, paints, filters |
| `anim-archive` | Tolerant zip and folder containers (FLA, XFL, puppets, projects) |
| `anim-shape` | Edge-based vector shapes (XFL edges) to fills and strokes |
| `anim-scene` | Document model, library, validation, editing operations |
| `anim-morph` | Shape tweens |
| `anim-text` | Rich text layout with bidi and right-to-left support |
| `anim-timeline` | Evaluates a document at a frame into a display list |
| `anim-render` | Deterministic CPU rasteriser |
| `anim-rig` | Meshing, ARAP deformation, IK, dangles |
| `anim-audio` | Audio decode, DSP, playback and capture actors |
| `anim-lipsync` | Audio to viseme tracks, offline and live |
| `anim-artwork` | PSD, AI/PDF and SVG import into a layer tree |
| `anim-export` | PNG, GIF, sprite sheet and SVG export |
| `anim-video` | MP4 (H.264 + AAC) and MOV (ProRes 4444) encoding |
| `anim-ui` | Shared egui widgets: stage, timeline, panels, docking, theme |
| `anim-session` | Command registry, undo/redo, jobs, settings |
| `anim-mcp` | Model Context Protocol server and control bridge for both apps |

## Building

```sh
cargo test --workspace
```

Rust 1.99.1 is pinned in `rust-toolchain.toml`.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [BSD-3-Clause](LICENSE-BSD-3-Clause), at your option. Third-party code
and assets are listed in [ATTRIBUTION.md](ATTRIBUTION.md).

This project is not affiliated with or endorsed by Adobe. Adobe, Animate and Character Animator are trademarks of
Adobe Inc.; they are named here only to describe file compatibility.

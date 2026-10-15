# anicraft-libs

Shared engine crates for [AniCraft](https://github.com/soof-golan/anicraft) (2D animation) and
[CharCraft](https://github.com/soof-golan/charcraft) (live character animation): vector shapes, timelines and
tweens, display lists, a CPU renderer, right-to-left text layout, puppet deformation, audio and lip sync, artwork
import, and image/video export. Pure Rust; runs on macOS, Windows and Linux.

Built on the open-source [Craft](https://github.com/storytold) crates (FilmCraft codecs, PhotoCraft PSD,
craft-libs UI and settings).

See [ARCHITECTURE.md](ARCHITECTURE.md) for the crate map and [AGENTS.md](AGENTS.md) for the rules every change
follows.

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

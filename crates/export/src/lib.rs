//! Image-based exports of animations: PNG sequences, animated GIF, sprite sheets, and static or animated SVG.
//!
//! Everything starts from a [`FrameSource`] (a count, a stage size, a frame rate and a [`DisplayList`] per frame
//! plus the [`ImageStore`] they share). Bitmap exporters rasterise through `anim-render`; the SVG exporters walk
//! the display list directly. Every export takes a progress callback `&mut dyn FnMut(Progress) -> bool`; returning
//! `false` cancels it with [`ExportError::Cancelled`]. Pass [`no_progress`] when you do not care.
//!
//! | Output | Entry points |
//! |---|---|
//! | PNG | [`export_png_frame`], [`png_frame_bytes`], [`export_png_sequence`] |
//! | GIF | [`export_gif`], [`export_gif_file`] |
//! | Sprite sheet (PNG + JSON hash) | [`build_sprite_sheet`], [`export_sprite_sheet`] |
//! | SVG | [`display_list_to_svg`], [`frame_to_svg`], [`export_svg_file`] |
//! | Animated SVG (SMIL or CSS) | [`export_animated_svg`], [`export_animated_svg_file`] |
//!
//! Text is shaped into outlines upstream (`anim-text`), so right-to-left scripts export like any other path.
//!
//! [`DisplayList`]: anim_display::DisplayList
//! [`ImageStore`]: anim_display::ImageStore

mod error;
mod gif_out;
mod png_out;
mod progress;
mod raster;
mod sheet;
mod source;
mod svg;

pub use error::{ExportError, Result};
pub use gif_out::{GifLoop, GifOptions, export_gif, export_gif_file, frame_delays_cs};
pub use png_out::{ExportReport, PngSequenceOptions, encode_png, export_png_frame, export_png_sequence, png_frame_bytes};
pub use progress::{Progress, ProgressFn, Stage, no_progress};
pub use raster::{ExportBackground, RasterOptions, RgbaFrame, render_frame};
pub use sheet::{PxRect, PxSize, SheetFrame, SheetLayout, SheetMeta, SheetMetadata, SheetOptions, SpriteSheet, build_sprite_sheet, export_sprite_sheet};
pub use source::{FrameSource, VecFrameSource};
pub use svg::{AnimatedSvgOptions, SvgAnimation, SvgOptions, display_list_to_svg, export_animated_svg, export_animated_svg_file, export_svg_file, frame_to_svg};

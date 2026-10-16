//! Helpers shared by the visual verification tests: render to pixels, PNG output, crops and zooms, probes, goldens.

#![allow(dead_code, clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use std::path::{Path, PathBuf};

use anim_artwork::{ArtworkTree, to_display};
use anim_core::Rgba8;
use anim_display::ImageStore;
use anim_render::{Background, RenderOptions, render};

/// Straight-alpha-free (opaque) render of a tree on white: `(width, height, rgba)`.
pub fn render_tree(tree: &ArtworkTree) -> (u32, u32, Vec<u8>) {
    let mut store = ImageStore::new();
    let list = to_display(tree, &mut store);
    let mut options = RenderOptions::for_stage(tree.size);
    options.background = Background::Solid(Rgba8::WHITE);
    let image = render(&list, &store, &options).unwrap();
    (image.width, image.height, image.premultiplied_rgba)
}

/// `$ANICRAFT_VISUAL_OUT/<component>`, or `None` when the variable is unset (CI): then nothing is written.
pub fn out_dir(component: &str) -> Option<PathBuf> {
    std::env::var_os("ANICRAFT_VISUAL_OUT").map(|root| PathBuf::from(root).join(component))
}

pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// Nearest-neighbour crop and zoom: the `cw` x `ch` region at `(x, y)` scaled by `scale`.
pub fn crop_zoom(rgba: &[u8], width: u32, x: u32, y: u32, cw: u32, ch: u32, scale: u32) -> (u32, u32, Vec<u8>) {
    let mut out = Vec::with_capacity((cw * ch * scale * scale * 4) as usize);
    for oy in 0..ch * scale {
        for ox in 0..cw * scale {
            let (sx, sy) = (x + ox / scale, y + oy / scale);
            let at = ((sy * width + sx) * 4) as usize;
            out.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    (cw * scale, ch * scale, out)
}

/// Writes `<out>/<name>.png` 1:1 and `<out>/<name>@<scale>x.png` zoomed, for looking at with an image viewer.
pub fn save(component: &str, name: &str, width: u32, height: u32, rgba: &[u8], scale: u32) {
    let Some(dir) = out_dir(component) else { return };
    write_png(&dir.join(format!("{name}.png")), width, height, rgba);
    let (w, h, z) = crop_zoom(rgba, width, 0, 0, width, height, scale);
    write_png(&dir.join(format!("{name}@{scale}x.png")), w, h, &z);
}

pub fn px(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
}

/// Asserts the RGB of pixel `(x, y)` is within `tol` of `want` on every channel.
#[track_caller]
pub fn assert_px(what: &str, rgba: &[u8], width: u32, (x, y): (u32, u32), want: [u8; 3], tol: u8) {
    let got = px(rgba, width, x, y);
    let ok = (0..3).all(|i| got[i].abs_diff(want[i]) <= tol);
    assert!(ok, "{what}: pixel ({x},{y}) is {got:?}, expected {want:?} +-{tol}");
}

/// Assembles a PDF from object bodies (object `n` is `objects[n - 1]`; object 1 must be the catalog), with a
/// correct cross-reference table.
pub fn assemble_pdf(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// A stream object body.
pub fn stream_body(dict: &str, data: &str) -> String {
    format!("<< {dict} /Length {} >>\nstream\n{data}\nendstream", data.len())
}

/// A PDF text string in UTF-16BE with a byte order mark, as a hex string literal.
pub fn utf16_hex(text: &str) -> String {
    let mut hex = String::from("<FEFF");
    for unit in text.encode_utf16() {
        hex.push_str(&format!("{unit:04X}"));
    }
    hex.push('>');
    hex
}

/// Compares against `tests/golden/<name>.png` (written when missing or when `UPDATE_GOLDENS` is set).
#[track_caller]
pub fn golden(name: &str, width: u32, height: u32, rgba: &[u8], tol: u8) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"));
    if std::env::var_os("UPDATE_GOLDENS").is_some() || !path.exists() {
        write_png(&path, width, height, rgba);
        return;
    }
    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (width, height), "golden {name}: size");
    let worst = buf.iter().zip(rgba).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(worst <= tol, "golden {name}: worst channel difference {worst} > {tol}");
}

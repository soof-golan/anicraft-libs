//! PSD / PSB import through `photocraft-psd`.

use anim_core::{BlendMode, Point, Rect, Size, num};
use anim_display::Image;
use photocraft_psd::{BlendMode as PsdBlend, GrayImage, LayerNode, LayerRecord, PsdFile};

use crate::raster::{mul255, premultiply};
use crate::tree::{ArtKind, ArtMask, ArtNode, ArtworkTree, blends_backdrop};
use crate::vmask::vector_mask;
use crate::{ArtworkError, Limits, check_input};

/// Imports a Photoshop document or large document.
///
/// Groups become [`ArtKind::Group`] nodes, layers become [`ArtKind::Raster`] nodes with premultiplied pixels at
/// the layer's offset. Names are the layers' Unicode names, exactly. A layer's pixel mask is multiplied into its
/// pixels; a group's mask becomes its [`ArtNode::mask`]. Layer opacity and fill opacity are combined. Layers
/// whose pixels cannot be decoded (unsupported colour mode, over the limits) stay in the tree as empty vector
/// nodes with a warning.
///
/// # Errors
/// [`ArtworkError::Psd`] for malformed files, [`ArtworkError::Limit`] when the file has too many layers.
pub fn import_psd(bytes: &[u8], limits: &Limits) -> Result<ArtworkTree, ArtworkError> {
    check_input(bytes, limits)?;
    let file = PsdFile::from_bytes(bytes).map_err(|e| ArtworkError::Psd(e.to_string()))?;
    let size = Size::new(f64::from(file.header.width), f64::from(file.header.height));
    let tree = file.layer_tree_max_depth(limits.max_depth);
    let mut ctx = Ctx { file: &file, limits, warnings: Vec::new(), pixels: 0, nodes: 0, size };
    let mut children = Vec::new();
    ctx.build(&tree, &mut children)?;
    if file.layers().is_empty() {
        ctx.warnings.push("the file has no layers".to_owned());
    }
    Ok(ArtworkTree { size, root: ArtNode::group("root", children), warnings: ctx.warnings })
}

struct Ctx<'a> {
    file: &'a PsdFile,
    limits: &'a Limits,
    warnings: Vec<String>,
    pixels: u64,
    nodes: usize,
    size: Size,
}

impl Ctx<'_> {
    fn build(&mut self, nodes: &[LayerNode], out: &mut Vec<ArtNode>) -> Result<(), ArtworkError> {
        for node in nodes {
            self.nodes = self.nodes.saturating_add(1);
            if self.nodes > self.limits.max_nodes {
                return Err(ArtworkError::Limit("layers"));
            }
            match node {
                LayerNode::Layer { index } => {
                    let Some(layer) = self.file.layer(*index) else { continue };
                    let kind = self.raster_kind(*index);
                    let mut art = common(layer.record, kind);
                    art.opacity *= f32::from(layer.record.fill_opacity()) / 255.0;
                    // The pixel mask is baked into the pixels by `raster_kind`; the vector mask clips them.
                    art.mask = vector_mask(layer.record, self.size, &mut self.warnings);
                    out.push(art);
                }
                LayerNode::Group { index, children, .. } => {
                    let mut kids = Vec::new();
                    self.build(children, &mut kids)?;
                    let Some(layer) = self.file.layer(*index) else { continue };
                    let mut art = common(layer.record, ArtKind::Group(kids));
                    let pixel_mask = self.group_mask(*index);
                    let shape_mask = vector_mask(layer.record, self.size, &mut self.warnings);
                    if pixel_mask.is_some() && shape_mask.is_some() {
                        self.warn(&art.name, "has a pixel mask and a vector mask; only the pixel mask is used");
                    }
                    art.mask = pixel_mask.or(shape_mask);
                    // A Normal group is isolated in Photoshop: what is inside blends with the group, not the canvas.
                    if art.blend == BlendMode::Normal && blends_backdrop(art.children()) {
                        art.blend = BlendMode::Layer;
                    }
                    if art.blend == BlendMode::PassThrough && art.opacity < 1.0 && blends_backdrop(art.children()) {
                        self.warn(&art.name, "a pass through group below full opacity is drawn isolated; blend modes inside see only the group");
                    }
                    out.push(art);
                }
            }
        }
        Ok(())
    }

    fn warn(&mut self, name: &str, why: &str) {
        self.warnings.push(format!("layer {name:?}: {why}"));
    }

    fn raster_kind(&mut self, index: usize) -> ArtKind {
        let empty = ArtKind::Vector { nodes: Vec::new(), bounds: Rect::ZERO };
        let Some(layer) = self.file.layer(index) else { return empty };
        let name = layer.record.name();
        let Ok((w, h)) = layer.record.rect.size() else {
            self.warn(&name, "invalid bounds");
            return empty;
        };
        if w == 0 || h == 0 {
            return empty;
        }
        let count = u64::try_from(w).ok().and_then(|w| w.checked_mul(u64::try_from(h).ok()?));
        let Some(count) = count.filter(|c| *c <= self.limits.max_layer_pixels) else {
            self.warn(&name, "too many pixels, left out");
            return empty;
        };
        if self.pixels.saturating_add(count) > self.limits.max_total_pixels {
            self.warn(&name, "the file's total pixel budget is used up, left out");
            return empty;
        }
        let rgba = match layer.rgba8() {
            Ok(rgba) => rgba,
            Err(e) => {
                self.warn(&name, &format!("pixels could not be decoded ({e}), left out"));
                return empty;
            }
        };
        self.pixels = self.pixels.saturating_add(count);
        let (left, top) = (rgba.left, rgba.top);
        let (width, height) = (rgba.width, rgba.height);
        let mut data = rgba.data;
        premultiply(&mut data);
        if !layer.record.layer_mask().is_some_and(photocraft_psd::LayerMask::disabled)
            && let Some(mask) = layer.user_mask()
        {
            match mask {
                Ok(mask) => {
                    let w = num::u32_to_usize(width);
                    let mut row = vec![0_u8; w];
                    for (y, pixels) in data.chunks_exact_mut(w.saturating_mul(4).max(4)).enumerate() {
                        mask_row(&mask, i64::from(left), i64::from(top), y, &mut row);
                        scale_row(pixels, &row);
                    }
                }
                Err(e) => self.warn(&name, &format!("mask could not be decoded ({e}), ignored")),
            }
        }
        match Image::new(width, height, data) {
            Ok(image) => ArtKind::Raster { image, offset: Point::new(f64::from(left), f64::from(top)) },
            Err(e) => {
                self.warn(&name, &format!("bad pixel buffer ({e}), left out"));
                empty
            }
        }
    }

    /// A group's pixel mask as a canvas mask. `None` if it has none, it is disabled, or it is too big.
    fn group_mask(&mut self, index: usize) -> Option<ArtMask> {
        let layer = self.file.layer(index)?;
        if layer.record.layer_mask().is_some_and(photocraft_psd::LayerMask::disabled) {
            return None;
        }
        let name = layer.record.name();
        let mask = match layer.user_mask()? {
            Ok(mask) => mask,
            Err(e) => {
                self.warn(&name, &format!("group mask could not be decoded ({e}), ignored"));
                return None;
            }
        };
        // An empty rectangle with a zero background hides the whole group.
        if mask.default_value == 0 && (mask.width == 0 || mask.height == 0) {
            let image = Image::new(1, 1, vec![0; 4]).ok()?;
            return Some(ArtMask::Raster { image, offset: Point::ORIGIN });
        }
        // Outside its rectangle the mask has a constant value; when that is not zero it needs the whole canvas.
        let (left, top, width, height) = if mask.default_value == 0 {
            (mask.left, mask.top, mask.width, mask.height)
        } else {
            (0, 0, num::round_f64_to_u32(self.size.width), num::round_f64_to_u32(self.size.height))
        };
        let count = u64::from(width).checked_mul(u64::from(height))?;
        if count == 0 || count > self.limits.max_layer_pixels {
            self.warn(&name, "group mask too large, ignored");
            return None;
        }
        let mut data = Vec::with_capacity(Image::byte_len(width, height)?);
        let mut row = vec![0_u8; num::u32_to_usize(width)];
        for y in 0..num::u32_to_usize(height) {
            mask_row(&mask, i64::from(left), i64::from(top), y, &mut row);
            data.extend(row.iter().flat_map(|m| [0, 0, 0, *m]));
        }
        let image = Image::new(width, height, data).ok()?;
        Some(ArtMask::Raster { image, offset: Point::new(f64::from(left), f64::from(top)) })
    }
}

/// Fills `row` with the mask values of row `y` of an image whose top-left corner is at `(left, top)` on the canvas:
/// the mask's own samples where they overlap it, its default value elsewhere.
fn mask_row(mask: &GrayImage, left: i64, top: i64, y: usize, row: &mut [u8]) {
    row.fill(mask.default_value);
    let mut copy = || -> Option<()> {
        let my = usize::try_from(top.checked_add(i64::try_from(y).ok()?)?.checked_sub(i64::from(mask.top))?).ok()?;
        if my >= num::u32_to_usize(mask.height) {
            return None;
        }
        // Canvas columns covered by both the row and the mask.
        let start = left.max(i64::from(mask.left));
        let end = left.checked_add(i64::try_from(row.len()).ok()?)?.min(i64::from(mask.left).checked_add(i64::from(mask.width))?);
        let from = usize::try_from(start.checked_sub(i64::from(mask.left))?).ok()?;
        let len = usize::try_from(end.checked_sub(start)?).ok().filter(|len| *len > 0)?;
        let base = my.checked_mul(num::u32_to_usize(mask.width))?.checked_add(from)?;
        let src = mask.data.get(base..base.checked_add(len)?)?;
        row.get_mut(usize::try_from(start.checked_sub(left)?).ok()?..)?.get_mut(..len)?.copy_from_slice(src);
        Some(())
    };
    let _ = copy();
}

/// Scales every channel of a row of premultiplied RGBA pixels by `mask / 255`.
fn scale_row(pixels: &mut [u8], mask: &[u8]) {
    for (px, m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        match *m {
            255 => {}
            0 => *px = [0; 4],
            m => px.iter_mut().for_each(|c| *c = mul255(*c, m)),
        }
    }
}

fn common(record: &LayerRecord, kind: ArtKind) -> ArtNode {
    ArtNode {
        name: record.name(),
        visible: record.is_visible(),
        opacity: f32::from(record.opacity) / 255.0,
        blend: map_blend(record.blend_mode),
        clipping: record.clipping != 0,
        mask: None,
        kind,
    }
}

/// Photoshop blend modes to the closest of ours (modes with no equivalent fall back to the nearest look).
const fn map_blend(mode: PsdBlend) -> BlendMode {
    match mode {
        PsdBlend::PassThrough => BlendMode::PassThrough,
        PsdBlend::Normal | PsdBlend::Dissolve | PsdBlend::Unknown(_) | PsdBlend::PinLight | PsdBlend::HardMix | PsdBlend::Divide => BlendMode::Normal,
        PsdBlend::Darken | PsdBlend::DarkerColor => BlendMode::Darken,
        PsdBlend::Multiply | PsdBlend::LinearBurn => BlendMode::Multiply,
        PsdBlend::ColorBurn => BlendMode::ColorBurn,
        PsdBlend::Lighten | PsdBlend::LighterColor => BlendMode::Lighten,
        PsdBlend::Screen => BlendMode::Screen,
        PsdBlend::ColorDodge => BlendMode::ColorDodge,
        PsdBlend::LinearDodge => BlendMode::Add,
        PsdBlend::Overlay => BlendMode::Overlay,
        PsdBlend::SoftLight => BlendMode::SoftLight,
        PsdBlend::HardLight | PsdBlend::VividLight | PsdBlend::LinearLight => BlendMode::HardLight,
        PsdBlend::Difference => BlendMode::Difference,
        PsdBlend::Exclusion => BlendMode::Exclusion,
        PsdBlend::Subtract => BlendMode::Subtract,
        PsdBlend::Hue => BlendMode::Hue,
        PsdBlend::Saturation => BlendMode::Saturation,
        PsdBlend::Color => BlendMode::Color,
        PsdBlend::Luminosity => BlendMode::Luminosity,
    }
}

#[cfg(test)]
mod tests {
    use photocraft_psd::{GroupSpec, LayerSpec, MaskSpec, PixelData, PsdBuilder, Rect as PsdRect};

    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> PixelData {
        let n = usize::try_from(w * h).unwrap();
        PixelData::Rgba8(rgba.iter().copied().cycle().take(n * 4).collect())
    }

    fn build(f: impl FnOnce(&mut PsdBuilder)) -> Vec<u8> {
        let mut b = PsdBuilder::new(16, 12);
        f(&mut b);
        b.to_bytes().unwrap()
    }

    #[test]
    fn groups_names_and_properties() {
        let bytes = build(|b| {
            b.push_layer(LayerSpec::new("Background", 0, 0, 16, 12, solid(16, 12, [10, 20, 30, 255])));
            b.begin_group(GroupSpec::new("+Head"));
            let mut eye = LayerSpec::new("Left Eye \u{05E2}\u{05D9}\u{05DF}", 2, 3, 4, 4, solid(4, 4, [255, 0, 0, 255]));
            eye.opacity = 128;
            eye.blend_mode = photocraft_psd::BlendMode::Multiply;
            b.push_layer(eye);
            let mut hidden = LayerSpec::new("Hidden", 0, 0, 2, 2, solid(2, 2, [0, 0, 0, 255]));
            hidden.visible = false;
            b.push_layer(hidden);
            b.end_group().unwrap();
        });
        let tree = import_psd(&bytes, &Limits::default()).unwrap();
        assert_eq!(tree.size, Size::new(16.0, 12.0));
        assert_eq!(tree.names(), ["root", "Background", "+Head", "Left Eye \u{05E2}\u{05D9}\u{05DF}", "Hidden"]);
        let head = tree.find("+Head").unwrap();
        assert!(head.is_group());
        assert_eq!(head.blend, BlendMode::PassThrough);
        let eye = tree.find("Left Eye \u{05E2}\u{05D9}\u{05DF}").unwrap();
        assert_eq!(eye.blend, BlendMode::Multiply);
        assert!((eye.opacity - 128.0 / 255.0).abs() < 1e-6);
        let ArtKind::Raster { image, offset } = &eye.kind else { panic!("raster expected") };
        assert_eq!((image.width, image.height), (4, 4));
        assert_eq!(*offset, Point::new(2.0, 3.0));
        assert_eq!(image.premultiplied_rgba.get(..4), Some([255, 0, 0, 255].as_slice()));
        assert!(!tree.find("Hidden").unwrap().visible);
        assert!(tree.find("Background").unwrap().visible);
    }

    fn first_pixel(tree: &ArtworkTree, name: &str) -> Vec<u8> {
        let ArtKind::Raster { image, .. } = &tree.find(name).unwrap().kind else { panic!("{name} should be a raster: {:?}", tree.warnings) };
        image.premultiplied_rgba.get(..4).unwrap().to_vec()
    }

    #[test]
    fn grayscale_cmyk_and_16_bit_layers_decode_to_rgba() {
        let gray = {
            let mut b = PsdBuilder::new(4, 4).color_mode(photocraft_psd::ColorMode::Grayscale);
            b.push_layer(LayerSpec::new("Gray", 0, 0, 1, 1, PixelData::GrayA8(vec![128, 255])));
            b.to_bytes().unwrap()
        };
        assert_eq!(first_pixel(&import_psd(&gray, &Limits::default()).unwrap(), "Gray"), [128, 128, 128, 255]);

        let deep = {
            let mut b = PsdBuilder::new(4, 4).depth(16);
            b.push_layer(LayerSpec::new("Deep", 0, 0, 1, 1, PixelData::Rgba16(vec![65535, 0, 32768, 65535])));
            b.to_bytes().unwrap()
        };
        let px = first_pixel(&import_psd(&deep, &Limits::default()).unwrap(), "Deep");
        assert_eq!((px[0], px[1], px[3]), (255, 0, 255));
        assert!(px[2].abs_diff(128) <= 1, "{px:?}");

        let cmyk = {
            let mut b = PsdBuilder::new(4, 4).color_mode(photocraft_psd::ColorMode::Cmyk);
            b.push_layer(LayerSpec::new("Paper", 0, 0, 1, 1, PixelData::Cmyka8(vec![0, 0, 0, 0, 255])));
            b.push_layer(LayerSpec::new("Ink", 1, 0, 1, 1, PixelData::Cmyka8(vec![0, 0, 0, 255, 255])));
            b.to_bytes().unwrap()
        };
        let tree = import_psd(&cmyk, &Limits::default()).unwrap();
        assert_eq!(first_pixel(&tree, "Paper"), [255, 255, 255, 255]);
        let ink = first_pixel(&tree, "Ink");
        assert!(ink.iter().take(3).all(|c| *c < 60), "{ink:?}");
    }

    #[test]
    fn group_mask_becomes_a_canvas_mask() {
        let mut builder = PsdBuilder::new(8, 8);
        builder.begin_group(GroupSpec::new("G"));
        builder.push_layer(LayerSpec::new("inside", 0, 0, 2, 1, solid(2, 1, [9, 9, 9, 255])));
        builder.end_group().unwrap();
        let mut file = builder.build().unwrap();
        let version = file.header.version;
        let group = file.layers_mut().iter_mut().find(|r| r.name() == "G" && r.section_type() != photocraft_psd::SectionType::BoundingDivider).unwrap();
        group.mask = photocraft_psd::MaskData::Mask(photocraft_psd::LayerMask::new(PsdRect::from_xywh(3, 2, 2, 1), 0, 0));
        group.channels.push(photocraft_psd::ChannelData::encode(-2, photocraft_psd::Compression::Raw, &[0, 255], 2, 1, 8, version).unwrap());
        let tree = import_psd(&file.to_bytes().unwrap(), &Limits::default()).unwrap();
        let g = tree.find("G").unwrap();
        let Some(ArtMask::Raster { image, offset }) = &g.mask else { panic!("mask expected on {g:?}") };
        assert_eq!(*offset, Point::new(3.0, 2.0));
        assert_eq!(image.premultiplied_rgba, [0, 0, 0, 0, 0, 0, 0, 255]);
        // Group masks reach the display list too.
        let list = crate::to_display(&tree, &mut anim_display::ImageStore::new());
        let anim_display::Node::Group(outer) = &list.nodes[0] else { panic!("group expected") };
        assert!(outer.mask.is_some());
    }

    #[test]
    fn mask_rows_clip_to_the_mask_rectangle() {
        // A 3 x 2 mask at canvas (2, 1) with background 9.
        let mask = GrayImage { left: 2, top: 1, width: 3, height: 2, data: vec![1, 2, 3, 4, 5, 6], default_value: 9 };
        let mut row = [0_u8; 6];
        mask_row(&mask, 0, 0, 0, &mut row);
        assert_eq!(row, [9; 6], "above the mask");
        mask_row(&mask, 0, 0, 1, &mut row);
        assert_eq!(row, [9, 9, 1, 2, 3, 9]);
        mask_row(&mask, 0, 0, 2, &mut row);
        assert_eq!(row, [9, 9, 4, 5, 6, 9]);
        mask_row(&mask, 0, 0, 3, &mut row);
        assert_eq!(row, [9; 6], "below the mask");
        // A layer starting inside the mask's columns, and one starting left of the canvas.
        let mut short = [0_u8; 2];
        mask_row(&mask, 3, 0, 1, &mut short);
        assert_eq!(short, [2, 3]);
        mask_row(&mask, -4, 0, 1, &mut row);
        assert_eq!(row, [9; 6]);
        mask_row(&mask, -1, 0, 1, &mut row);
        assert_eq!(row, [9, 9, 9, 1, 2, 3]);
    }

    #[test]
    fn colour_is_premultiplied() {
        let bytes = build(|b| {
            b.push_layer(LayerSpec::new("Half", 0, 0, 1, 1, PixelData::Rgba8(vec![200, 100, 50, 128])));
        });
        let tree = import_psd(&bytes, &Limits::default()).unwrap();
        let ArtKind::Raster { image, .. } = &tree.find("Half").unwrap().kind else { panic!("raster expected") };
        assert_eq!(image.premultiplied_rgba, [100, 50, 25, 128]);
    }

    #[test]
    fn clipping_flag_and_pixel_mask() {
        let bytes = build(|b| {
            b.push_layer(LayerSpec::new("Base", 0, 0, 4, 4, solid(4, 4, [255, 255, 255, 255])));
            let mut clipped = LayerSpec::new("Clipped", 0, 0, 4, 4, solid(4, 4, [0, 0, 255, 255]));
            clipped.clipping = true;
            b.push_layer(clipped);
            let mut masked = LayerSpec::new("Masked", 0, 0, 2, 1, solid(2, 1, [255, 255, 255, 255]));
            masked.mask = Some(MaskSpec { rect: PsdRect::from_xywh(0, 0, 2, 1), data: vec![0, 255], default_color: 0, disabled: false });
            b.push_layer(masked);
        });
        let tree = import_psd(&bytes, &Limits::default()).unwrap();
        assert!(tree.find("Clipped").unwrap().clipping);
        assert!(!tree.find("Base").unwrap().clipping);
        let ArtKind::Raster { image, .. } = &tree.find("Masked").unwrap().kind else { panic!("raster expected") };
        assert_eq!(image.premultiplied_rgba, [0, 0, 0, 0, 255, 255, 255, 255]);
    }

    #[test]
    fn layer_outside_the_canvas_keeps_negative_offset() {
        let bytes = build(|b| {
            b.push_layer(LayerSpec::new("Off", -3, -2, 2, 2, solid(2, 2, [1, 2, 3, 255])));
        });
        let tree = import_psd(&bytes, &Limits::default()).unwrap();
        let ArtKind::Raster { offset, .. } = &tree.find("Off").unwrap().kind else { panic!("raster expected") };
        assert_eq!(*offset, Point::new(-3.0, -2.0));
    }

    #[test]
    fn nested_groups_keep_order() {
        let bytes = build(|b| {
            b.begin_group(GroupSpec::new("A"));
            b.begin_group(GroupSpec::new("B"));
            b.push_layer(LayerSpec::new("leaf", 0, 0, 1, 1, solid(1, 1, [0, 0, 0, 255])));
            b.end_group().unwrap();
            b.push_layer(LayerSpec::new("after", 0, 0, 1, 1, solid(1, 1, [0, 0, 0, 255])));
            b.end_group().unwrap();
        });
        let tree = import_psd(&bytes, &Limits::default()).unwrap();
        assert_eq!(tree.names(), ["root", "A", "B", "leaf", "after"]);
        assert!(tree.root.find_path(&["A", "B", "leaf"]).is_some());
    }

    #[test]
    fn pixel_limit_leaves_a_warning_not_an_error() {
        let bytes = build(|b| {
            b.push_layer(LayerSpec::new("Big", 0, 0, 16, 12, solid(16, 12, [0, 0, 0, 255])));
        });
        let limits = Limits { max_layer_pixels: 10, ..Limits::default() };
        let tree = import_psd(&bytes, &limits).unwrap();
        assert_eq!(tree.warnings.len(), 1);
        assert_eq!(tree.find("Big").unwrap().kind_name(), "vector");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(import_psd(b"8BPS\0\x01nonsense", &Limits::default()).is_err());
        assert!(import_psd(b"", &Limits::default()).is_err());
        let bytes = build(|b| {
            b.push_layer(LayerSpec::new("L", 0, 0, 3, 3, solid(3, 3, [9, 9, 9, 255])));
        });
        for cut in [10, bytes.len() / 3, bytes.len() / 2, bytes.len().saturating_sub(7)] {
            let _ = import_psd(bytes.get(..cut).unwrap(), &Limits::default());
        }
    }
}

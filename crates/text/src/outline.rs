//! Glyph outlines and display lists.
//!
//! Outlines are read with skrifa at one em (so the cache is independent of font size), cached per font face, and placed
//! with an affine per glyph. A run becomes one filled path; synthetic bold adds a stroke of the same path, and an
//! underline is a filled rectangle, so renderers and exporters never see text, only paths.

use std::collections::HashMap;

use anim_core::kurbo::Shape;
use anim_core::{Affine, BezPath};
use anim_display::{Cap, Fill, FillRule, Join, Node, Paint, Stroke, StrokeScale, StrokeStyle};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::layout::{FontFace, GlyphRun, TextLayout};

/// Glyph outlines kept before the cache is flushed.
const MAX_CACHED_GLYPHS: usize = 60_000;
/// Synthetic bold strokes the outline with this fraction of the em.
const EMBOLDEN_EM: f64 = 0.032;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FaceKey {
    id: u64,
    index: u32,
    coords: Vec<i16>,
}

/// Outlines of the glyphs seen so far, per font face, in em units with y up.
#[derive(Debug, Default)]
pub(crate) struct OutlineCache {
    faces: HashMap<FaceKey, HashMap<u32, BezPath>>,
    glyphs: usize,
}

struct PathPen(BezPath);

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((f64::from(cx0), f64::from(cy0)), (f64::from(x), f64::from(y)));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((f64::from(cx0), f64::from(cy0)), (f64::from(cx1), f64::from(cy1)), (f64::from(x), f64::from(y)));
    }

    fn close(&mut self) {
        self.0.close_path();
    }
}

/// Reads one glyph's outline at one em. Missing glyphs and glyphs without outline (spaces) give an empty path.
fn read_outline(font: &FontRef<'_>, coords: &[NormalizedCoord], glyph: u32) -> BezPath {
    let mut pen = PathPen(BezPath::new());
    if let Some(outline) = font.outline_glyphs().get(GlyphId::new(glyph)) {
        let settings = DrawSettings::unhinted(Size::new(1.0), LocationRef::new(coords));
        if outline.draw(settings, &mut pen).is_err() {
            return BezPath::new();
        }
    }
    pen.0
}

impl OutlineCache {
    /// The outlines of every glyph of `run` merged into one path in layout space.
    pub(crate) fn run_path(&mut self, run: &GlyphRun) -> BezPath {
        if self.glyphs > MAX_CACHED_GLYPHS {
            self.faces.clear();
            self.glyphs = 0;
        }
        let key = FaceKey { id: run.font.id, index: run.font.index, coords: run.font.coords.clone() };
        let cached = self.faces.entry(key).or_default();
        let mut font: Option<Option<FontRef<'_>>> = None;
        let coords: Vec<NormalizedCoord> = run.font.coords.iter().map(|&c| NormalizedCoord::from_bits(c)).collect();
        let shear = Affine::new([1.0, 0.0, run.skew_degrees.to_radians().tan(), 1.0, 0.0, 0.0]);
        let scale = Affine::scale_non_uniform(run.size, -run.size);
        let mut out = BezPath::new();
        let mut added = 0usize;
        for g in &run.glyphs {
            if !cached.contains_key(&g.id) {
                let face = font.get_or_insert_with(|| face_of(&run.font));
                let path = face.as_ref().map(|f| read_outline(f, &coords, g.id)).unwrap_or_default();
                cached.insert(g.id, path);
                added = added.saturating_add(1);
            }
            let Some(path) = cached.get(&g.id) else { continue };
            if path.elements().is_empty() {
                continue;
            }
            let mut placed = path.clone();
            placed.apply_affine(Affine::translate((g.x, g.y)) * scale * shear);
            out.extend(placed.iter());
        }
        self.glyphs = self.glyphs.saturating_add(added);
        out
    }

    /// The nodes that draw `layout`: per run a fill (plus a stroke for synthetic bold) and an optional underline.
    pub(crate) fn display(&mut self, layout: &TextLayout) -> Vec<Node> {
        let mut nodes = Vec::new();
        for run in &layout.runs {
            if run.color.a == 0 {
                continue;
            }
            let paint = Paint::Solid(run.color);
            let path = self.run_path(run);
            if !path.elements().is_empty() {
                if run.embolden {
                    let style = StrokeStyle {
                        width: run.size * EMBOLDEN_EM,
                        cap: Cap::Round,
                        join: Join::Round,
                        miter_limit: 4.0,
                        scale: StrokeScale::Normal,
                        dashes: Vec::new(),
                    };
                    nodes.push(Node::Stroke(Stroke { transform: Affine::IDENTITY, path: path.clone(), style, paint: paint.clone() }));
                }
                nodes.push(Node::Fill(Fill { transform: Affine::IDENTITY, path, rule: FillRule::NonZero, paint: paint.clone() }));
            }
            if let Some(rect) = run.underline {
                nodes.push(Node::Fill(Fill { transform: Affine::IDENTITY, path: rect.to_path(0.1), rule: FillRule::NonZero, paint }));
            }
        }
        nodes
    }
}

fn face_of(face: &FontFace) -> Option<FontRef<'_>> {
    FontRef::from_index(face.data(), face.index).ok()
}

impl TextLayout {
    /// The display nodes for this layout, with a throw-away outline cache. Prefer
    /// [`TextEngine::to_display`](crate::TextEngine::to_display) when laying out repeatedly: it keeps the cache.
    #[must_use]
    pub fn to_display(&self) -> Vec<Node> {
        OutlineCache::default().display(self)
    }
}

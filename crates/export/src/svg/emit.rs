//! Display list nodes to SVG elements.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anim_core::{Affine, BlendMode, Point, Rect, Rgba8, Size};
use anim_display::{Bitmap, Cap, DisplayList, Fill, FillRule, Gradient, Group, ImageId, ImageStore, Join, Mesh, Node, Paint, Spread, Stroke, StrokeScale};
use anim_render::{MAX_DEPTH, straight_rgba};
use base64::Engine as _;

use super::SvgOptions;
use super::filter;
use super::fmt::{hex, matrix, num, opacity, path_data, transform_attr};
use crate::error::{ExportError, Result};
use crate::png_out::encode_png;

/// Upper bound on nodes written for one export (hostile display lists).
const MAX_NODES: usize = 4_000_000;
/// Triangles written per mesh.
const MAX_TRIANGLES: usize = 20_000;

pub(super) struct Emitter<'a> {
    images: &'a ImageStore,
    opts: &'a SvgOptions,
    stage: Rect,
    pub(super) defs: String,
    cache: BTreeMap<String, String>,
    image_defs: BTreeMap<ImageId, Option<String>>,
    counter: u32,
    nodes: usize,
}

const fn blend_css(mode: BlendMode) -> Option<&'static str> {
    Some(match mode {
        BlendMode::Normal | BlendMode::PassThrough | BlendMode::Layer | BlendMode::Subtract | BlendMode::Invert | BlendMode::Alpha | BlendMode::Erase => {
            return None;
        }
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
        BlendMode::Add => "plus-lighter",
    })
}

/// The affine map that sends triangle `s` onto triangle `d`, or `None` when `s` is degenerate.
pub(super) fn triangle_affine(s: [Point; 3], d: [Point; 3]) -> Option<Affine> {
    let basis = |t: [Point; 3]| Affine::new([t[1].x - t[0].x, t[1].y - t[0].y, t[2].x - t[0].x, t[2].y - t[0].y, t[0].x, t[0].y]);
    let from = basis(s);
    let det = from.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let m = basis(d) * from.inverse();
    m.is_finite().then_some(m)
}

impl<'a> Emitter<'a> {
    pub(super) const fn new(images: &'a ImageStore, opts: &'a SvgOptions, stage: Size) -> Self {
        Self {
            images,
            opts,
            stage: Rect::new(0.0, 0.0, stage.width, stage.height),
            defs: String::new(),
            cache: BTreeMap::new(),
            image_defs: BTreeMap::new(),
            counter: 0,
            nodes: 0,
        }
    }

    const fn p(&self) -> u8 {
        self.opts.precision
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.counter = self.counter.saturating_add(1);
        format!("{prefix}{}", self.counter)
    }

    /// Adds `<tag id=..{body}</tag>` to the defs unless an identical definition exists; returns its id.
    fn define(&mut self, prefix: &str, tag: &str, body: &str) -> String {
        let key = format!("{tag}\u{0}{body}");
        if let Some(id) = self.cache.get(&key) {
            return id.clone();
        }
        let id = self.next_id(prefix);
        let _ = writeln!(self.defs, "<{tag} id=\"{id}\"{body}</{tag}>");
        self.cache.insert(key, id.clone());
        id
    }

    fn check(&mut self, out_len: usize) -> Result<()> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAX_NODES {
            return Err(ExportError::TooLarge(format!("more than {MAX_NODES} SVG nodes")));
        }
        if self.defs.len().saturating_add(out_len) > self.opts.max_bytes {
            return Err(ExportError::TooLarge(format!("SVG output exceeds {} bytes", self.opts.max_bytes)));
        }
        Ok(())
    }

    /// Defines the `<image>` for `id` (once) and returns its element id.
    fn image(&mut self, id: ImageId) -> Option<String> {
        if let Some(known) = self.image_defs.get(&id) {
            return known.clone();
        }
        let made = self.images.get(id).and_then(|img| {
            let png = encode_png(img.width, img.height, &straight_rgba(img)).ok()?;
            let data = base64::engine::general_purpose::STANDARD.encode(png);
            Some((img.width, img.height, data))
        });
        let element = made.map(|(w, h, data)| {
            let el = self.next_id("i");
            let _ = writeln!(self.defs, "<image id=\"{el}\" width=\"{w}\" height=\"{h}\" xlink:href=\"data:image/png;base64,{data}\"/>");
            el
        });
        self.image_defs.insert(id, element.clone());
        element
    }

    /// The attribute for `prop` (`fill` or `stroke`), or `None` when the paint draws nothing.
    fn paint(&mut self, paint: &Paint, prop: &str) -> Option<String> {
        match paint {
            Paint::Solid(c) => solid(*c, prop),
            Paint::Linear(g) => self.gradient(g, false, prop),
            Paint::Radial(g) => self.gradient(g, true, prop),
            Paint::Bitmap { image, transform, repeat: _, smoothing } => {
                if !transform.is_finite() {
                    return None;
                }
                let el = self.image(*image)?;
                let (w, h) = self.images.get(*image).map(|i| (i.width, i.height))?;
                let style = if *smoothing { "" } else { " style=\"image-rendering:pixelated\"" };
                let body = format!(
                    " patternUnits=\"userSpaceOnUse\" width=\"{w}\" height=\"{h}\"{}><use xlink:href=\"#{el}\"{style}/>",
                    pattern_transform(*transform, self.p())
                );
                let id = self.define("p", "pattern", &body);
                Some(format!(" {prop}=\"url(#{id})\""))
            }
        }
    }

    fn gradient(&mut self, g: &Gradient, radial: bool, prop: &str) -> Option<String> {
        if !g.transform.is_finite() {
            return None;
        }
        match g.stops.as_slice() {
            [] => return None,
            [only] => return solid(only.color, prop),
            _ => {}
        }
        let extent = anim_display::GRADIENT_EXTENT;
        let mut attrs = String::from(" gradientUnits=\"userSpaceOnUse\"");
        if radial {
            let focal = if g.focal.is_finite() { g.focal.clamp(-0.98, 0.98) } else { 0.0 };
            let _ = write!(attrs, " cx=\"0\" cy=\"0\" r=\"{}\" fx=\"{}\" fy=\"0\"", num(extent, 1), num(focal * extent, 3));
        } else {
            let _ = write!(attrs, " x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"0\"", num(-extent, 1), num(extent, 1));
        }
        attrs.push_str(&pattern_gradient_transform(g.transform, self.p()));
        match g.spread {
            Spread::Pad => {}
            Spread::Reflect => attrs.push_str(" spreadMethod=\"reflect\""),
            Spread::Repeat => attrs.push_str(" spreadMethod=\"repeat\""),
        }
        if g.interpolation == anim_display::Interpolation::LinearRgb {
            attrs.push_str(" color-interpolation=\"linearRGB\"");
        }
        attrs.push('>');
        let mut last = 0.0f64;
        for stop in &g.stops {
            let offset = f64::from(stop.offset).clamp(0.0, 1.0).max(last);
            last = offset;
            let _ = write!(attrs, "<stop offset=\"{}\" stop-color=\"{}\"", num(offset, 4), hex(stop.color));
            if stop.color.a < 255 {
                let _ = write!(attrs, " stop-opacity=\"{}\"", opacity(stop.color.a));
            }
            attrs.push_str("/>");
        }
        let tag = if radial { "radialGradient" } else { "linearGradient" };
        let id = self.define("g", tag, &attrs);
        Some(format!(" {prop}=\"url(#{id})\""))
    }

    pub(super) fn nodes(&mut self, nodes: &[Node], out: &mut String, ctm: Affine, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        for node in nodes {
            self.check(out.len())?;
            match node {
                Node::Fill(f) => self.fill(f, out),
                Node::Stroke(s) => self.stroke(s, out),
                Node::Bitmap(b) => self.bitmap(b, out),
                Node::Group(g) => self.group(g, out, ctm, depth)?,
                Node::Mesh(m) => self.mesh(m, out, ctm, depth)?,
            }
        }
        Ok(())
    }

    fn fill(&mut self, f: &Fill, out: &mut String) {
        if !f.transform.is_finite() {
            return;
        }
        let Some(d) = path_data(&f.path, self.p()) else { return };
        let Some(paint) = self.paint(&f.paint, "fill") else { return };
        let rule = if f.rule == FillRule::EvenOdd { " fill-rule=\"evenodd\"" } else { "" };
        let _ = writeln!(out, "<path d=\"{d}\"{}{paint}{rule}/>", transform_attr(f.transform, self.p()));
    }

    fn stroke(&mut self, s: &Stroke, out: &mut String) {
        if !s.transform.is_finite() || !s.style.width.is_finite() || s.style.width < 0.0 {
            return;
        }
        let Some(d) = path_data(&s.path, self.p()) else { return };
        let Some(paint) = self.paint(&s.paint, "stroke") else { return };
        let style = &s.style;
        let hairline = style.width <= 0.0;
        let fixed = hairline || style.scale == StrokeScale::None;
        let width = if hairline { 1.0 } else { style.width };
        let cap = match style.cap {
            Cap::None => "butt",
            Cap::Round => "round",
            Cap::Square => "square",
        };
        let join = match style.join {
            Join::Miter => "miter",
            Join::Round => "round",
            Join::Bevel => "bevel",
        };
        let mut el = format!("<path d=\"{d}\"{} fill=\"none\"{paint} stroke-width=\"{}\"", transform_attr(s.transform, self.p()), num(width, self.p().max(3)));
        let _ = write!(el, " stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"");
        if style.join == Join::Miter && style.miter_limit.is_finite() && style.miter_limit >= 1.0 {
            let _ = write!(el, " stroke-miterlimit=\"{}\"", num(style.miter_limit, 3));
        }
        let dashes_ok = !style.dashes.is_empty() && style.dashes.iter().all(|d| d.is_finite() && *d >= 0.0) && style.dashes.iter().any(|d| *d > 0.0);
        if dashes_ok {
            let list: Vec<String> = style.dashes.iter().map(|d| num(*d, 3)).collect();
            let _ = write!(el, " stroke-dasharray=\"{}\"", list.join(" "));
        }
        if fixed {
            el.push_str(" vector-effect=\"non-scaling-stroke\"");
        }
        el.push_str("/>\n");
        out.push_str(&el);
    }

    fn bitmap(&mut self, b: &Bitmap, out: &mut String) {
        if !b.transform.is_finite() {
            return;
        }
        let Some(el) = self.image(b.image) else { return };
        let style = if b.smoothing { "" } else { " style=\"image-rendering:pixelated\"" };
        let _ = writeln!(out, "<use xlink:href=\"#{el}\"{}{style}/>", transform_attr(b.transform, self.p()));
    }

    /// Region (x, y, w, h) in the local space of an element whose accumulated transform is `ctm`, covering the
    /// stage plus `pad` local units; `None` when `ctm` cannot be inverted.
    fn local_region(&self, ctm: Affine, pad: f64) -> Option<String> {
        let det = ctm.determinant();
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let r = ctm.inverse().transform_rect_bbox(self.stage).inflate(pad, pad);
        let finite = [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite());
        finite.then(|| format!("x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", num(r.x0, 2), num(r.y0, 2), num(r.width(), 2), num(r.height(), 2)))
    }

    fn group(&mut self, g: &Group, out: &mut String, ctm: Affine, depth: usize) -> Result<()> {
        if !g.transform.is_finite() || g.children.is_empty() {
            return Ok(());
        }
        if g.mask.as_ref().is_some_and(Vec::is_empty) {
            return Ok(());
        }
        let local = ctm * g.transform;
        let alpha_only = filter::pure_alpha(&g.color);
        let ct = (!g.color.is_identity() && alpha_only.is_none()).then_some(&g.color);
        let chain = filter::build(&g.filters, ct, self.p());

        let mut body = String::new();
        self.nodes(&g.children, &mut body, local, depth.saturating_add(1))?;
        if body.is_empty() {
            return Ok(());
        }

        // Outer element: transform, blend, isolation, opacity (SVG applies filter, then mask, then opacity, then
        // blending, so the display-list order mask -> filters -> colour -> blend needs the nesting below).
        let mut outer = transform_attr(g.transform, self.p());
        let mut style = String::new();
        if let Some(css) = blend_css(g.blend) {
            let _ = write!(style, "mix-blend-mode:{css};");
        }
        if g.blend == BlendMode::Layer {
            style.push_str("isolation:isolate;");
        }
        if !style.is_empty() {
            let _ = write!(outer, " style=\"{style}\"");
        }
        if let Some(a) = alpha_only.filter(|a| (a - 1.0).abs() > 1e-6) {
            let _ = write!(outer, " opacity=\"{}\"", num(f64::from(a), 4));
        }
        if self.opts.emit_keys
            && let Some(key) = g.key
        {
            let _ = write!(outer, " data-key=\"{key}\"");
        }

        let filter_id = chain.map(|chain| {
            let head = self.local_region(local, chain.pad).map_or_else(
                || " filterUnits=\"objectBoundingBox\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\"".to_owned(),
                |r| format!(" filterUnits=\"userSpaceOnUse\" {r}"),
            );
            self.define("f", "filter", &format!("{head} color-interpolation-filters=\"sRGB\">\n{}", chain.body))
        });
        let mask_id = match &g.mask {
            Some(mask) => match self.mask_def(mask, local, depth)? {
                Some(id) => Some(id),
                // A mask that draws nothing hides everything.
                None => return Ok(()),
            },
            None => None,
        };

        let mut opened = 0usize;
        if !outer.is_empty() {
            let _ = writeln!(out, "<g{outer}>");
            opened = opened.saturating_add(1);
        }
        if let Some(id) = filter_id {
            let _ = writeln!(out, "<g filter=\"url(#{id})\">");
            opened = opened.saturating_add(1);
        }
        if let Some(id) = mask_id {
            let _ = writeln!(out, "<g mask=\"url(#{id})\">");
            opened = opened.saturating_add(1);
        }
        out.push_str(&body);
        for _ in 0..opened {
            out.push_str("</g>\n");
        }
        Ok(())
    }

    /// Defines a luminance-free (`mask-type: alpha`) mask for `nodes`; `None` when they draw nothing.
    fn mask_def(&mut self, nodes: &[Node], local: Affine, depth: usize) -> Result<Option<String>> {
        let mut body = String::new();
        self.nodes(nodes, &mut body, local, depth.saturating_add(1))?;
        if body.is_empty() {
            return Ok(None);
        }
        let region = self.local_region(local, 0.0).unwrap_or_else(|| "x=\"-100000\" y=\"-100000\" width=\"200000\" height=\"200000\"".to_owned());
        let attrs = format!(" maskUnits=\"userSpaceOnUse\" {region} mask-type=\"alpha\" style=\"mask-type:alpha\">\n{body}");
        Ok(Some(self.define("m", "mask", &attrs)))
    }

    fn mesh(&mut self, mesh: &Mesh, out: &mut String, ctm: Affine, depth: usize) -> Result<()> {
        let m = mesh;
        if !m.transform.is_finite() || m.source.len() != m.target.len() || m.content.is_empty() {
            return Ok(());
        }
        let local = ctm * m.transform;
        let mut content = String::new();
        self.nodes(&m.content, &mut content, local, depth.saturating_add(1))?;
        if content.is_empty() {
            return Ok(());
        }
        let content_id = self.next_id("c");
        let _ = writeln!(self.defs, "<g id=\"{content_id}\">\n{content}</g>");
        let mut tris = String::new();
        for tri in m.triangles.iter().take(MAX_TRIANGLES) {
            self.check(tris.len())?;
            let get = |pts: &[Point], i: u32| usize::try_from(i).ok().and_then(|i| pts.get(i)).copied();
            let [i0, i1, i2] = *tri;
            let (Some(s0), Some(s1), Some(s2)) = (get(&m.source, i0), get(&m.source, i1), get(&m.source, i2)) else { continue };
            let (Some(d0), Some(d1), Some(d2)) = (get(&m.target, i0), get(&m.target, i1), get(&m.target, i2)) else { continue };
            let Some(map) = triangle_affine([s0, s1, s2], [d0, d1, d2]) else { continue };
            let p = self.p();
            let outline = format!(" M{} {} L{} {} L{} {} Z", num(d0.x, p), num(d0.y, p), num(d1.x, p), num(d1.y, p), num(d2.x, p), num(d2.y, p));
            let clip = self.define("t", "clipPath", &format!("><path d=\"{}\"/>", outline.trim()));
            let _ = writeln!(
                tris,
                "<g clip-path=\"url(#{clip})\"><use xlink:href=\"#{content_id}\" transform=\"{}\"/></g>",
                matrix(map, p).unwrap_or_else(|| "matrix(1 0 0 1 0 0)".to_owned())
            );
        }
        if tris.is_empty() {
            return Ok(());
        }
        let wrapper = transform_attr(m.transform, self.p());
        if wrapper.is_empty() {
            out.push_str(&tris);
        } else {
            let _ = writeln!(out, "<g{wrapper}>\n{tris}</g>");
        }
        Ok(())
    }

    /// The stage background rectangle, if the list has one.
    pub(super) fn background(list: &DisplayList, size: Size, precision: u8) -> Option<String> {
        let c = list.background.filter(|c| c.a > 0)?;
        let mut el = format!("<rect width=\"{}\" height=\"{}\" fill=\"{}\"", num(size.width, precision), num(size.height, precision), hex(c));
        if c.a < 255 {
            let _ = write!(el, " fill-opacity=\"{}\"", opacity(c.a));
        }
        el.push_str("/>\n");
        Some(el)
    }
}

fn solid(c: Rgba8, prop: &str) -> Option<String> {
    if c.a == 0 {
        return None;
    }
    let mut s = format!(" {prop}=\"{}\"", hex(c));
    if c.a < 255 {
        let _ = write!(s, " {prop}-opacity=\"{}\"", opacity(c.a));
    }
    Some(s)
}

fn pattern_transform(t: Affine, precision: u8) -> String {
    matrix(t, precision.max(5)).map_or_else(String::new, |m| format!(" patternTransform=\"{m}\""))
}

fn pattern_gradient_transform(t: Affine, precision: u8) -> String {
    matrix(t, precision.max(5)).map_or_else(String::new, |m| format!(" gradientTransform=\"{m}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn triangle_map_sends_vertices() {
        let s = [p(0.0, 0.0), p(10.0, 0.0), p(0.0, 10.0)];
        let d = [p(5.0, 5.0), p(25.0, 5.0), p(5.0, 15.0)];
        let m = triangle_affine(s, d).unwrap();
        for (a, b) in s.iter().zip(&d) {
            let q = m * *a;
            assert!((q.x - b.x).abs() < 1e-9 && (q.y - b.y).abs() < 1e-9);
        }
        assert!(triangle_affine([p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0)], d).is_none());
    }
}

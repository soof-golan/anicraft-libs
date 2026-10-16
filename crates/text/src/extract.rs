//! Copying parley's laid-out lines and glyph runs into the parley-free data of [`TextLayout`].

use std::ops::Range;

use anim_core::{Point, Rect, Rgba8};
use parley::layout::{GlyphRun as ParleyGlyphRun, PositionedLayoutItem};

use crate::layout::{FontFace, Glyph, GlyphRun, Line, ParaLayout, TextLayout};

/// A glyph run of the line being copied, with its glyphs in layout space.
struct Item<'a> {
    run: ParleyGlyphRun<'a, Rgba8>,
    glyphs: Vec<Glyph>,
    /// Glyphs parley placed, including the direction mark's.
    total: usize,
    /// Glyphs of the same parley run in the items before this one.
    skipped: usize,
    /// The item holds every glyph of its parley run on this line, so its text range is the run's.
    whole: bool,
}

/// Appends the lines and glyph runs of `para` to `out`, in layout space.
pub fn copy_lines(para: &ParaLayout, paragraph_index: usize, out: &mut TextLayout) {
    let mut items: Vec<Item<'_>> = Vec::new();
    let mut spaces: Vec<(usize, f32)> = Vec::new();
    for (li, line) in para.layout.lines().enumerate() {
        let m = *line.metrics();
        let shift = f64::from(para.shift(li));
        let baseline = para.origin.y + f64::from(m.baseline) + shift;
        let left = para.origin.x + f64::from(m.inline_min_coord + m.offset);
        // Spaces at the logical end of a line hang outside its box: at the right of a left-to-right line, at the left of a
        // right-to-left one. (Parley's own `trailing_whitespace` is measured at the wrong end for right-to-left lines.)
        let hang = hanging_whitespace(&line, &mut spaces);
        let (x0, x1) = if para.rtl { (left + hang, left + f64::from(m.advance)) } else { (left, left + f64::from(m.advance) - hang) };
        let range = line.text_range();
        let line_index = out.lines.len();
        out.lines.push(Line {
            paragraph: paragraph_index,
            range: para.global(range.start)..para.global(range.end),
            rect: Rect::new(x0, para.origin.y + f64::from(m.block_min_coord) + shift, x1.max(x0), para.origin.y + f64::from(m.block_max_coord) + shift),
            baseline,
            ascent: f64::from(m.baseline - m.content_block_min_coord),
            descent: f64::from(m.content_block_max_coord - m.baseline),
            rtl: para.rtl,
        });

        // Glyph runs of one parley run (split where only the style changes) share its clusters: count glyphs used.
        items.clear();
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
            let mut glyphs: Vec<Glyph> = gr
                .positioned_glyphs()
                .map(|g| Glyph { id: g.id, x: para.origin.x + f64::from(g.x), y: para.origin.y + f64::from(g.y) + shift, advance: f64::from(g.advance) })
                .collect();
            let total = glyphs.len();
            let index = gr.run().index();
            let skipped: usize = items.iter().filter(|i| i.run.run().index() == index).map(|i| i.total).sum();
            // The invisible direction mark in front of the paragraph shapes to a glyph of its own; it is not text.
            if gr.run().text_range().start < para.prefix {
                let mask = prefix_mask(gr.run(), para.prefix);
                let mut position = skipped;
                glyphs.retain(|_| {
                    let keep = !mask.get(position).copied().unwrap_or(false);
                    position = position.saturating_add(1);
                    keep
                });
            }
            if total > 0 {
                for earlier in items.iter_mut().filter(|i| i.run.run().index() == index) {
                    earlier.whole = false;
                }
            }
            items.push(Item { run: gr, glyphs, total, skipped, whole: skipped == 0 });
        }

        let mut extent: Option<(f64, f64)> = None;
        for item in &mut items {
            if item.glyphs.is_empty() && item.total > 0 {
                continue;
            }
            let (run, gr) = (item.run.run(), &item.run);
            let range = if item.whole { run.text_range() } else { run_text_range(run, item.skipped, item.total) };
            let origin = Point::new(para.origin.x + f64::from(gr.offset()), baseline);
            let advance = f64::from(gr.advance());
            extent = Some(extent.map_or((origin.x, origin.x + advance), |(lo, hi)| (lo.min(origin.x), hi.max(origin.x + advance))));
            let underline = gr.style().underline.as_ref().map(|u| {
                let metrics = run.font_metrics();
                let top = baseline - f64::from(u.offset.unwrap_or(metrics.underline_offset));
                let thickness = f64::from(u.size.unwrap_or(metrics.underline_size)).max(0.5);
                Rect::new(origin.x, top, origin.x + advance, top + thickness)
            });
            let synthesis = run.synthesis();
            out.runs.push(GlyphRun {
                font: FontFace::new(run.font().clone(), run.normalized_coords().iter().map(|c| c.to_bits()).collect()),
                size: f64::from(run.font_size()),
                color: gr.style().brush,
                rtl: run.is_rtl(),
                line: line_index,
                range: para.global(range.start)..para.global(range.end),
                glyphs: std::mem::take(&mut item.glyphs),
                origin,
                advance,
                underline,
                embolden: synthesis.embolden(),
                skew_degrees: synthesis.skew().map_or(0.0, f64::from),
            });
        }
        // Justification stretches the glyphs but not the line metrics: take the line's extent from what was placed.
        if let (Some((lo, hi)), Some(l)) = (extent, out.lines.get_mut(line_index)) {
            let (lo, hi) = if para.rtl { (lo + hang, hi) } else { (lo, hi - hang) };
            l.rect.x0 = lo;
            l.rect.x1 = hi.max(lo);
        }
    }
}

/// The width of the spaces at the logical end of `line`. `spaces` is scratch space.
fn hanging_whitespace(line: &parley::layout::Line<'_, Rgba8>, spaces: &mut Vec<(usize, f32)>) -> f64 {
    spaces.clear();
    // The spaces after the last character that is not one.
    let mut last_visible: Option<usize> = None;
    for cluster in line.runs().flat_map(|run| run.clusters()) {
        let start = cluster.text_range().start;
        if cluster.is_space_or_nbsp() {
            spaces.push((start, cluster.advance()));
        } else {
            last_visible = last_visible.max(Some(start));
        }
    }
    spaces.retain(|&(start, _)| Some(start) > last_visible);
    spaces.sort_unstable_by_key(|&(start, _)| std::cmp::Reverse(start));
    spaces.iter().map(|&(_, advance)| f64::from(advance)).sum()
}

/// For each glyph of `run` in visual order: whether it belongs to the paragraph's invisible direction mark.
fn prefix_mask(run: &parley::layout::Run<'_, Rgba8>, prefix: usize) -> Vec<bool> {
    run.visual_clusters()
        .flat_map(|cluster| {
            let is_mark = cluster.text_range().end <= prefix;
            cluster.glyphs().map(move |_| is_mark)
        })
        .collect()
}

/// The text range of the glyphs `skip..skip + count` of a parley run (in visual order): the clusters that produced them.
fn run_text_range(run: &parley::layout::Run<'_, Rgba8>, skip: usize, count: usize) -> Range<usize> {
    let upto = skip.saturating_add(count);
    let mut seen = 0usize;
    let mut lo = usize::MAX;
    let mut hi = 0usize;
    for cluster in run.visual_clusters() {
        let n = cluster.glyphs().count();
        let next = seen.saturating_add(n);
        let overlaps = n > 0 && next > skip && seen < upto;
        // Clusters without glyphs (the tail of a ligature) go with the glyphs next to them.
        let continuation = n == 0 && seen >= skip && seen <= upto;
        if overlaps || continuation {
            let r = cluster.text_range();
            lo = lo.min(r.start);
            hi = hi.max(r.end);
        }
        seen = next;
        if seen > upto {
            break;
        }
    }
    if lo > hi { run.text_range() } else { lo..hi }
}

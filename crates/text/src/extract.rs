//! Copying parley's laid-out lines and glyph runs into the parley-free data of [`TextLayout`].

use std::ops::Range;

use anim_core::{Point, Rect, Rgba8};
use parley::layout::PositionedLayoutItem;

use crate::layout::{FontFace, Glyph, GlyphRun, Line, ParaLayout, TextLayout};

/// Appends the lines and glyph runs of `para` to `out`, in layout space.
pub fn copy_lines(para: &ParaLayout, paragraph_index: usize, out: &mut TextLayout) {
    for (li, line) in para.layout.lines().enumerate() {
        let m = *line.metrics();
        let shift = f64::from(para.shift(li));
        let baseline = para.origin.y + f64::from(m.baseline) + shift;
        let left = para.origin.x + f64::from(m.inline_min_coord + m.offset);
        // Spaces at the logical end of a line hang outside its box: at the right of a left-to-right line, at the left of a
        // right-to-left one. (Parley's own `trailing_whitespace` is measured at the wrong end for right-to-left lines.)
        let hang = hanging_whitespace(&line);
        let (x0, x1) = if para.rtl { (left + hang, left + f64::from(m.advance)) } else { (left, left + f64::from(m.advance) - hang) };
        let range = line.text_range();
        let line_index = out.lines.len();
        out.lines.push(Line {
            paragraph: paragraph_index,
            range: para.global(range.start)..para.global(range.end),
            rect: Rect::new(x0, para.origin.y + f64::from(m.block_min_coord) + shift, x1.max(x0), para.origin.y + f64::from(m.block_max_coord) + shift),
            baseline,
            ascent: f64::from(m.ascent),
            descent: f64::from(m.descent),
            rtl: para.rtl,
        });

        // Glyph runs of one parley run (split where only the style changes) share its clusters: count glyphs used.
        let mut used: Vec<(usize, usize)> = Vec::new();
        let mut extent: Option<(f64, f64)> = None;
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
            let run = gr.run();
            let total = gr.glyphs().count();
            let skipped = if let Some((_, n)) = used.iter_mut().find(|(r, _)| *r == run.index()) {
                let before = *n;
                *n = n.saturating_add(total);
                before
            } else {
                used.push((run.index(), total));
                0
            };
            // The invisible direction mark in front of the paragraph shapes to a glyph of its own; it is not text.
            let mask = prefix_mask(run, para.prefix);
            let glyphs: Vec<Glyph> = gr
                .positioned_glyphs()
                .enumerate()
                .filter(|(i, _)| !mask.get(skipped.saturating_add(*i)).copied().unwrap_or(false))
                .map(|(_, g)| Glyph { id: g.id, x: para.origin.x + f64::from(g.x), y: para.origin.y + f64::from(g.y) + shift, advance: f64::from(g.advance) })
                .collect();
            if glyphs.is_empty() && total > 0 {
                continue;
            }
            let range = run_text_range(run, skipped, total);
            let origin = Point::new(para.origin.x + f64::from(gr.offset()), baseline);
            let advance = f64::from(gr.advance());
            extent = Some(extent.map_or((origin.x, origin.x + advance), |(lo, hi)| (lo.min(origin.x), hi.max(origin.x + advance))));
            let underline = gr.style().underline.as_ref().map(|u| {
                let metrics = run.metrics();
                let top = baseline - f64::from(u.offset.unwrap_or(metrics.underline_offset));
                let thickness = f64::from(u.size.unwrap_or(metrics.underline_size)).max(0.5);
                Rect::new(origin.x, top, origin.x + advance, top + thickness)
            });
            let synthesis = run.synthesis();
            out.runs.push(GlyphRun {
                font: FontFace::new(run.font().clone(), run.normalized_coords().to_vec()),
                size: f64::from(run.font_size()),
                color: gr.style().brush,
                rtl: run.is_rtl(),
                line: line_index,
                range: para.global(range.start)..para.global(range.end),
                glyphs,
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

/// The width of the spaces at the logical end of `line`.
fn hanging_whitespace(line: &parley::layout::Line<'_, Rgba8>) -> f64 {
    let mut clusters: Vec<(usize, f32, bool)> =
        line.runs().flat_map(|run| run.clusters().map(|c| (c.text_range().start, c.advance(), c.is_space_or_nbsp())).collect::<Vec<_>>()).collect();
    clusters.sort_by_key(|c| c.0);
    clusters.iter().rev().take_while(|c| c.2).map(|c| f64::from(c.1)).sum()
}

/// For each glyph of `run` in visual order: whether it belongs to the paragraph's invisible direction mark.
fn prefix_mask(run: &parley::layout::Run<'_, Rgba8>, prefix: usize) -> Vec<bool> {
    if prefix == 0 {
        return Vec::new();
    }
    run.visual_clusters()
        .flat_map(|cluster| {
            let is_mark = cluster.text_range().end <= prefix;
            cluster.glyphs().map(move |_| is_mark)
        })
        .collect()
}

/// The text range a glyph run was shaped from: all of the parley run's range when the glyph run is the whole run,
/// otherwise the clusters that produced its glyphs.
fn run_text_range(run: &parley::layout::Run<'_, Rgba8>, skip: usize, count: usize) -> Range<usize> {
    let all: usize = run.visual_clusters().map(|c| c.glyphs().count()).sum();
    if skip == 0 && count >= all {
        return run.text_range();
    }
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

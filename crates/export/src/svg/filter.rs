//! Display-list filters and colour transforms as SVG filter primitives.

use std::fmt::{Arguments, Write as _};

use anim_core::{ColorTransform, Rgba8};
use anim_display::Filter;

use super::fmt::{hex, num, opacity};

/// Alpha channel only, as black: the shape of the source.
const ALPHA_ONLY: &str = "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0";

/// The primitives of one `<filter>` element and how far (in local units) they can spread pixels.
pub(super) struct FilterChain {
    pub body: String,
    pub pad: f64,
}

/// A colour transform that is just an alpha multiplier can use the cheaper `opacity` attribute.
pub(super) fn pure_alpha(ct: &ColorTransform) -> Option<f32> {
    let one = |v: f32| (v - 1.0).abs() < 1e-6;
    let zero = |v: f32| v.abs() < 1e-6;
    let [mr, mg, mb, ma] = ct.mul;
    let untouched = one(mr) && one(mg) && one(mb) && ct.add.iter().all(|a| zero(*a));
    (untouched && (0.0..=1.0).contains(&ma)).then_some(ma)
}

/// A drop shadow or glow.
struct Shadow {
    color: Rgba8,
    offset: (f64, f64),
    blur: (f64, f64),
    strength: f64,
    inner: bool,
    knockout: bool,
    hide_object: bool,
}

struct Builder {
    body: String,
    n: u32,
    cur: String,
    pad: f64,
    precision: u8,
}

impl Builder {
    fn result(&mut self) -> String {
        self.n = self.n.saturating_add(1);
        format!("r{}", self.n)
    }

    fn line(&mut self, args: Arguments<'_>) {
        let _ = self.body.write_fmt(args);
        self.body.push('\n');
    }

    fn std_dev(&self, sx: f64, sy: f64) -> String {
        format!("{} {}", num(sx, self.precision.max(3)), num(sy, self.precision.max(3)))
    }

    fn blur(&mut self, sx: f64, sy: f64) {
        if sx <= 0.0 && sy <= 0.0 {
            return;
        }
        self.pad = self.pad.max(3.0 * sx.max(sy));
        let (dev, cur, out) = (self.std_dev(sx, sy), self.cur.clone(), self.result());
        self.line(format_args!("<feGaussianBlur in=\"{cur}\" stdDeviation=\"{dev}\" result=\"{out}\"/>"));
        self.cur = out;
    }

    fn color_matrix(&mut self, values: &str) {
        let (cur, out) = (self.cur.clone(), self.result());
        self.line(format_args!("<feColorMatrix in=\"{cur}\" type=\"matrix\" values=\"{values}\" result=\"{out}\"/>"));
        self.cur = out;
    }

    /// A blurred, offset, coloured copy of the source alpha, combined with the source the way Flash does.
    fn shadow(&mut self, s: &Shadow) {
        let source = self.cur.clone();
        self.color_matrix(ALPHA_ONLY);
        let mut shape = self.cur.clone();
        if s.inner {
            let inverted = self.result();
            self.line(format_args!(
                "<feComponentTransfer in=\"{shape}\" result=\"{inverted}\"><feFuncA type=\"linear\" slope=\"-1\" intercept=\"1\"/></feComponentTransfer>"
            ));
            shape = inverted;
        }
        let (sx, sy) = s.blur;
        let (blurred, dev) = (self.result(), self.std_dev(sx, sy));
        self.line(format_args!("<feGaussianBlur in=\"{shape}\" stdDeviation=\"{dev}\" result=\"{blurred}\"/>"));
        self.pad = self.pad.max(3.0f64.mul_add(sx.max(sy), s.offset.0.abs().max(s.offset.1.abs())));
        let moved = self.result();
        let (dx, dy) = (num(s.offset.0, self.precision), num(s.offset.1, self.precision));
        self.line(format_args!("<feOffset in=\"{blurred}\" dx=\"{dx}\" dy=\"{dy}\" result=\"{moved}\"/>"));
        let flood = self.result();
        let (color, alpha) = (hex(s.color), opacity(s.color.a));
        self.line(format_args!("<feFlood flood-color=\"{color}\" flood-opacity=\"{alpha}\" result=\"{flood}\"/>"));
        let mut shadow = self.result();
        self.line(format_args!("<feComposite in=\"{flood}\" in2=\"{moved}\" operator=\"in\" result=\"{shadow}\"/>"));
        if (s.strength - 1.0).abs() > 1e-3 && s.strength.is_finite() {
            let boosted = self.result();
            let slope = num(s.strength.clamp(0.0, 255.0), 3);
            self.line(format_args!(
                "<feComponentTransfer in=\"{shadow}\" result=\"{boosted}\"><feFuncA type=\"linear\" slope=\"{slope}\"/></feComponentTransfer>"
            ));
            shadow = boosted;
        }
        let result = self.result();
        if s.inner {
            let clipped = self.result();
            self.line(format_args!("<feComposite in=\"{shadow}\" in2=\"{source}\" operator=\"in\" result=\"{clipped}\"/>"));
            if s.knockout || s.hide_object {
                self.line(format_args!("<feOffset in=\"{clipped}\" dx=\"0\" dy=\"0\" result=\"{result}\"/>"));
            } else {
                self.line(format_args!("<feMerge result=\"{result}\"><feMergeNode in=\"{source}\"/><feMergeNode in=\"{clipped}\"/></feMerge>"));
            }
        } else if s.hide_object {
            self.line(format_args!("<feOffset in=\"{shadow}\" dx=\"0\" dy=\"0\" result=\"{result}\"/>"));
        } else if s.knockout {
            self.line(format_args!("<feComposite in=\"{shadow}\" in2=\"{source}\" operator=\"out\" result=\"{result}\"/>"));
        } else {
            self.line(format_args!("<feMerge result=\"{result}\"><feMergeNode in=\"{shadow}\"/><feMergeNode in=\"{source}\"/></feMerge>"));
        }
        self.cur = result;
    }
}

/// Builds the filter primitives for `filters` followed by the colour transform `ct` (when not a pure alpha
/// multiplier). Returns `None` when nothing needs a filter element.
pub(super) fn build(filters: &[Filter], ct: Option<&ColorTransform>, precision: u8) -> Option<FilterChain> {
    let mut b = Builder { body: String::new(), n: 0, cur: "SourceGraphic".to_owned(), pad: 0.0, precision };
    for filter in filters {
        match filter {
            Filter::Blur { x, y, quality } => {
                let k = (f64::from((*quality).clamp(1, 15)) / 12.0).sqrt();
                b.blur(x.max(0.0) * k, y.max(0.0) * k);
            }
            Filter::DropShadow { color, angle_deg, distance, blur_x, blur_y, strength, inner, knockout, hide_object } => {
                let angle = angle_deg.to_radians();
                // Three box passes of width w have a standard deviation of about w / 2.
                b.shadow(&Shadow {
                    color: *color,
                    offset: (distance * angle.cos(), distance * angle.sin()),
                    blur: (blur_x.max(0.0) * 0.5, blur_y.max(0.0) * 0.5),
                    strength: *strength,
                    inner: *inner,
                    knockout: *knockout,
                    hide_object: *hide_object,
                });
            }
            Filter::Glow { color, blur_x, blur_y, strength, inner, knockout } => {
                b.shadow(&Shadow {
                    color: *color,
                    offset: (0.0, 0.0),
                    blur: (blur_x.max(0.0) * 0.5, blur_y.max(0.0) * 0.5),
                    strength: *strength,
                    inner: *inner,
                    knockout: *knockout,
                    hide_object: false,
                });
            }
            Filter::ColorMatrix(m) => {
                // The matrix works in 0..=255 units, SVG in 0..=1: only the offset column changes.
                let values: Vec<String> = m
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let v = f64::from(*v);
                        num(if i % 5 == 4 { v / 255.0 } else { v }, 5)
                    })
                    .collect();
                b.color_matrix(&values.join(" "));
            }
        }
    }
    if let Some(ct) = ct {
        let [mr, mg, mb, ma] = ct.mul;
        let [ar, ag, ab, aa] = ct.add;
        let f = |v: f32| num(f64::from(v), 5);
        let values = format!(
            "{} 0 0 0 {} 0 {} 0 0 {} 0 0 {} 0 {} 0 0 0 {} {}",
            f(mr),
            f(ar),
            f(mg),
            f(ag),
            f(mb),
            f(ab),
            f(ma),
            f(aa)
        );
        b.color_matrix(&values);
    }
    if b.body.is_empty() { None } else { Some(FilterChain { body: b.body, pad: b.pad }) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_alpha_is_detected() {
        assert_eq!(pure_alpha(&ColorTransform::alpha(0.5)), Some(0.5));
        assert!(pure_alpha(&ColorTransform::brightness(0.5)).is_none());
    }

    #[test]
    fn empty_chain_is_none() {
        assert!(build(&[], None, 3).is_none());
    }

    #[test]
    fn blur_pads() {
        let chain = build(&[Filter::Blur { x: 8.0, y: 8.0, quality: 3 }], None, 3).unwrap();
        assert!(chain.pad > 5.0);
        assert!(chain.body.contains("feGaussianBlur"));
    }
}

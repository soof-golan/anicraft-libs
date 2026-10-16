//! Icons drawn with painter primitives: no image files, crisp at any scale, themed by one colour.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};
use serde::{Deserialize, Serialize};

/// Every glyph the widgets in this crate can draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Icon {
    // Tools.
    Select,
    Subselect,
    Lasso,
    Pen,
    Text,
    Line,
    Rectangle,
    Oval,
    Pencil,
    Brush,
    PaintBucket,
    Eyedropper,
    Eraser,
    Hand,
    Zoom,
    FreeTransform,
    Bone,
    Camera,
    Pin,
    Mic,
    // Layers and chrome.
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Outline,
    Folder,
    FolderOpen,
    LayerNormal,
    LayerMask,
    LayerGuide,
    Add,
    Remove,
    ChevronRight,
    ChevronDown,
    Onion,
    OnionOutlines,
    EditMultiple,
    AutoKeyframe,
    Search,
    // Transport.
    Play,
    Pause,
    Stop,
    StepBack,
    StepForward,
    ToStart,
    ToEnd,
    Loop,
    Record,
    // Library items.
    Graphic,
    MovieClip,
    Button,
    Bitmap,
    Sound,
    Video,
    Font,
}

struct Canvas<'a> {
    painter: &'a Painter,
    rect: Rect,
    color: Color32,
    width: f32,
}

impl Canvas<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.rect.width().mul_add(x, self.rect.left()), self.rect.height().mul_add(y, self.rect.top()))
    }

    fn stroke(&self) -> Stroke {
        Stroke::new(self.width, self.color)
    }

    fn line(&self, a: (f32, f32), b: (f32, f32)) {
        self.painter.line_segment([self.p(a.0, a.1), self.p(b.0, b.1)], self.stroke());
    }

    fn poly(&self, pts: &[(f32, f32)], fill: bool) {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        if fill {
            self.painter.add(Shape::convex_polygon(points, self.color, Stroke::NONE));
        } else {
            self.painter.add(Shape::closed_line(points, self.stroke()));
        }
    }

    fn open(&self, pts: &[(f32, f32)]) {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::line(points, self.stroke()));
    }

    fn circle(&self, c: (f32, f32), r: f32, fill: bool) {
        let radius = self.rect.width() * r;
        if fill {
            self.painter.circle_filled(self.p(c.0, c.1), radius, self.color);
        } else {
            self.painter.circle_stroke(self.p(c.0, c.1), radius, self.stroke());
        }
    }

    fn rect_shape(&self, min: (f32, f32), max: (f32, f32), fill: bool) {
        let r = Rect::from_min_max(self.p(min.0, min.1), self.p(max.0, max.1));
        if fill {
            self.painter.rect_filled(r, 1.0, self.color);
        } else {
            self.painter.rect_stroke(r, 1.0, self.stroke(), egui::StrokeKind::Middle);
        }
    }
}

/// Paints `icon` inside `rect` (squared and centred) with `color`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let square = Rect::from_center_size(rect.center(), egui::vec2(side, side));
    let c = Canvas { painter, rect: square.shrink(side * 0.12), color, width: (side / 11.0).clamp(1.0, 2.2) };
    if !paint_tool(&c, icon) && !paint_chrome(&c, icon) && !paint_transport(&c, icon) {
        paint_item(&c, icon);
    }
}

fn paint_tool(c: &Canvas<'_>, icon: Icon) -> bool {
    match icon {
        Icon::Select => c.poly(&[(0.15, 0.05), (0.15, 0.85), (0.4, 0.62), (0.62, 0.95), (0.74, 0.88), (0.52, 0.55), (0.82, 0.52)], true),
        Icon::Subselect => {
            c.poly(&[(0.15, 0.05), (0.15, 0.85), (0.4, 0.62), (0.62, 0.95), (0.74, 0.88), (0.52, 0.55), (0.82, 0.52)], false);
        }
        Icon::Lasso => {
            c.open(&[(0.2, 0.3), (0.5, 0.1), (0.85, 0.3), (0.8, 0.55), (0.45, 0.65), (0.2, 0.5), (0.25, 0.3)]);
            c.open(&[(0.45, 0.65), (0.4, 0.85), (0.55, 0.95)]);
        }
        Icon::Pen => {
            c.poly(&[(0.5, 0.05), (0.8, 0.5), (0.6, 0.7), (0.4, 0.7), (0.2, 0.5)], false);
            c.line((0.5, 0.7), (0.5, 0.95));
            c.circle((0.5, 0.42), 0.07, true);
        }
        Icon::Text => {
            c.line((0.2, 0.15), (0.8, 0.15));
            c.line((0.5, 0.15), (0.5, 0.88));
            c.line((0.38, 0.88), (0.62, 0.88));
        }
        Icon::Line => c.line((0.15, 0.85), (0.85, 0.15)),
        Icon::Rectangle => c.rect_shape((0.12, 0.25), (0.88, 0.75), false),
        Icon::Oval => c.circle((0.5, 0.5), 0.38, false),
        Icon::Pencil => {
            c.poly(&[(0.2, 0.8), (0.28, 0.58), (0.72, 0.14), (0.86, 0.28), (0.42, 0.72)], false);
            c.line((0.28, 0.58), (0.42, 0.72));
            c.poly(&[(0.2, 0.8), (0.24, 0.68), (0.32, 0.76)], true);
        }
        Icon::Brush => {
            c.poly(&[(0.7, 0.1), (0.9, 0.3), (0.55, 0.6), (0.4, 0.45)], false);
            c.open(&[(0.4, 0.45), (0.2, 0.55), (0.12, 0.85), (0.4, 0.78), (0.55, 0.6)]);
        }
        Icon::PaintBucket => {
            c.poly(&[(0.2, 0.45), (0.5, 0.15), (0.78, 0.45), (0.5, 0.8)], false);
            c.open(&[(0.5, 0.15), (0.5, 0.05)]);
            c.circle((0.85, 0.72), 0.07, true);
        }
        Icon::Eyedropper => {
            c.line((0.25, 0.75), (0.6, 0.4));
            c.line((0.15, 0.9), (0.28, 0.72));
            c.poly(&[(0.6, 0.25), (0.75, 0.4), (0.9, 0.25), (0.75, 0.1)], true);
        }
        Icon::Eraser => {
            c.poly(&[(0.1, 0.65), (0.5, 0.2), (0.88, 0.5), (0.5, 0.88)], false);
            c.line((0.3, 0.42), (0.7, 0.72));
        }
        Icon::Hand => {
            c.open(&[(0.28, 0.9), (0.15, 0.55), (0.25, 0.5), (0.32, 0.62), (0.32, 0.15)]);
            c.open(&[(0.45, 0.45), (0.45, 0.1), (0.58, 0.1), (0.58, 0.45)]);
            c.open(&[(0.58, 0.2), (0.7, 0.2), (0.7, 0.55), (0.82, 0.4), (0.9, 0.5), (0.75, 0.9), (0.28, 0.9)]);
        }
        Icon::Zoom => {
            c.circle((0.42, 0.42), 0.28, false);
            c.line((0.63, 0.63), (0.9, 0.9));
            c.line((0.3, 0.42), (0.54, 0.42));
            c.line((0.42, 0.3), (0.42, 0.54));
        }
        Icon::FreeTransform => {
            c.rect_shape((0.2, 0.2), (0.8, 0.8), false);
            for (x, y) in [(0.2, 0.2), (0.8, 0.2), (0.2, 0.8), (0.8, 0.8)] {
                c.rect_shape((x - 0.08, y - 0.08), (x + 0.08, y + 0.08), true);
            }
        }
        Icon::Bone => {
            c.line((0.25, 0.75), (0.75, 0.25));
            c.circle((0.2, 0.8), 0.1, false);
            c.circle((0.8, 0.2), 0.1, false);
        }
        Icon::Camera => {
            c.rect_shape((0.1, 0.3), (0.9, 0.82), false);
            c.poly(&[(0.3, 0.3), (0.38, 0.16), (0.62, 0.16), (0.7, 0.3)], false);
            c.circle((0.5, 0.56), 0.15, false);
        }
        Icon::Pin => {
            c.circle((0.5, 0.32), 0.22, false);
            c.line((0.5, 0.54), (0.5, 0.92));
        }
        Icon::Mic => {
            c.rect_shape((0.38, 0.08), (0.62, 0.55), false);
            c.open(&[(0.25, 0.45), (0.25, 0.65), (0.5, 0.78), (0.75, 0.65), (0.75, 0.45)]);
            c.line((0.5, 0.78), (0.5, 0.94));
        }
        _ => return false,
    }
    true
}

fn paint_chrome(c: &Canvas<'_>, icon: Icon) -> bool {
    match icon {
        Icon::Eye | Icon::EyeOff => {
            c.open(&[(0.05, 0.5), (0.3, 0.25), (0.5, 0.2), (0.7, 0.25), (0.95, 0.5), (0.7, 0.75), (0.5, 0.8), (0.3, 0.75), (0.05, 0.5)]);
            c.circle((0.5, 0.5), 0.14, matches!(icon, Icon::Eye));
            if icon == Icon::EyeOff {
                c.line((0.15, 0.9), (0.85, 0.1));
            }
        }
        Icon::Lock => {
            c.rect_shape((0.22, 0.45), (0.78, 0.9), true);
            c.open(&[(0.32, 0.45), (0.32, 0.25), (0.42, 0.1), (0.58, 0.1), (0.68, 0.25), (0.68, 0.45)]);
        }
        Icon::Unlock => {
            c.rect_shape((0.22, 0.45), (0.78, 0.9), false);
            c.open(&[(0.32, 0.45), (0.32, 0.25), (0.42, 0.1), (0.58, 0.1), (0.68, 0.22)]);
        }
        Icon::Outline => c.rect_shape((0.2, 0.2), (0.8, 0.8), false),
        Icon::Folder | Icon::FolderOpen => {
            c.poly(&[(0.08, 0.2), (0.4, 0.2), (0.5, 0.32), (0.92, 0.32), (0.92, 0.82), (0.08, 0.82)], icon == Icon::Folder);
        }
        Icon::LayerNormal => {
            c.poly(&[(0.5, 0.15), (0.92, 0.38), (0.5, 0.6), (0.08, 0.38)], false);
            c.open(&[(0.08, 0.58), (0.5, 0.8), (0.92, 0.58)]);
        }
        Icon::LayerMask => {
            c.rect_shape((0.12, 0.12), (0.88, 0.88), false);
            c.circle((0.5, 0.5), 0.26, true);
        }
        Icon::LayerGuide => {
            c.open(&[(0.1, 0.85), (0.35, 0.3), (0.6, 0.7), (0.9, 0.15)]);
            c.circle((0.9, 0.15), 0.07, true);
        }
        Icon::Add => {
            c.line((0.5, 0.15), (0.5, 0.85));
            c.line((0.15, 0.5), (0.85, 0.5));
        }
        Icon::Remove => c.line((0.15, 0.5), (0.85, 0.5)),
        Icon::ChevronRight => c.open(&[(0.35, 0.2), (0.7, 0.5), (0.35, 0.8)]),
        Icon::ChevronDown => c.open(&[(0.2, 0.35), (0.5, 0.7), (0.8, 0.35)]),
        Icon::Onion => {
            c.circle((0.38, 0.5), 0.26, false);
            c.circle((0.62, 0.5), 0.26, false);
        }
        Icon::OnionOutlines => {
            c.circle((0.38, 0.5), 0.26, false);
            c.circle((0.62, 0.5), 0.26, false);
            c.line((0.5, 0.28), (0.5, 0.72));
        }
        Icon::EditMultiple => {
            c.rect_shape((0.1, 0.18), (0.6, 0.68), false);
            c.rect_shape((0.4, 0.34), (0.9, 0.84), false);
        }
        Icon::AutoKeyframe => {
            c.poly(&[(0.5, 0.12), (0.88, 0.5), (0.5, 0.88), (0.12, 0.5)], false);
            c.circle((0.5, 0.5), 0.1, true);
        }
        Icon::Search => {
            c.circle((0.42, 0.42), 0.28, false);
            c.line((0.63, 0.63), (0.9, 0.9));
        }
        _ => return false,
    }
    true
}

fn paint_transport(c: &Canvas<'_>, icon: Icon) -> bool {
    match icon {
        Icon::Play => c.poly(&[(0.25, 0.12), (0.88, 0.5), (0.25, 0.88)], true),
        Icon::Pause => {
            c.rect_shape((0.22, 0.15), (0.42, 0.85), true);
            c.rect_shape((0.58, 0.15), (0.78, 0.85), true);
        }
        Icon::Stop => c.rect_shape((0.2, 0.2), (0.8, 0.8), true),
        Icon::StepBack => {
            c.rect_shape((0.15, 0.15), (0.27, 0.85), true);
            c.poly(&[(0.85, 0.12), (0.32, 0.5), (0.85, 0.88)], true);
        }
        Icon::StepForward => {
            c.rect_shape((0.73, 0.15), (0.85, 0.85), true);
            c.poly(&[(0.15, 0.12), (0.68, 0.5), (0.15, 0.88)], true);
        }
        Icon::ToStart => {
            c.rect_shape((0.1, 0.15), (0.2, 0.85), true);
            c.poly(&[(0.52, 0.15), (0.22, 0.5), (0.52, 0.85)], true);
            c.poly(&[(0.88, 0.15), (0.58, 0.5), (0.88, 0.85)], true);
        }
        Icon::ToEnd => {
            c.rect_shape((0.8, 0.15), (0.9, 0.85), true);
            c.poly(&[(0.48, 0.15), (0.78, 0.5), (0.48, 0.85)], true);
            c.poly(&[(0.12, 0.15), (0.42, 0.5), (0.12, 0.85)], true);
        }
        Icon::Loop => {
            c.open(&[(0.2, 0.62), (0.2, 0.35), (0.35, 0.25), (0.75, 0.25)]);
            c.open(&[(0.8, 0.38), (0.8, 0.65), (0.65, 0.75), (0.25, 0.75)]);
            c.poly(&[(0.68, 0.12), (0.88, 0.25), (0.68, 0.38)], true);
            c.poly(&[(0.32, 0.62), (0.12, 0.75), (0.32, 0.88)], true);
        }
        Icon::Record => c.circle((0.5, 0.5), 0.3, true),
        _ => return false,
    }
    true
}

fn paint_item(c: &Canvas<'_>, icon: Icon) {
    match icon {
        Icon::Graphic => {
            c.circle((0.5, 0.5), 0.36, false);
            c.circle((0.5, 0.5), 0.1, true);
        }
        Icon::MovieClip => {
            c.rect_shape((0.1, 0.2), (0.9, 0.8), false);
            c.poly(&[(0.42, 0.34), (0.66, 0.5), (0.42, 0.66)], true);
        }
        Icon::Button => {
            c.rect_shape((0.08, 0.3), (0.92, 0.7), false);
            c.line((0.3, 0.5), (0.7, 0.5));
        }
        Icon::Bitmap => {
            c.rect_shape((0.1, 0.15), (0.9, 0.85), false);
            c.open(&[(0.1, 0.75), (0.38, 0.45), (0.58, 0.65), (0.7, 0.52), (0.9, 0.72)]);
            c.circle((0.7, 0.32), 0.07, true);
        }
        Icon::Sound => {
            c.poly(&[(0.1, 0.4), (0.3, 0.4), (0.5, 0.2), (0.5, 0.8), (0.3, 0.6), (0.1, 0.6)], false);
            c.open(&[(0.65, 0.35), (0.75, 0.5), (0.65, 0.65)]);
            c.open(&[(0.75, 0.22), (0.9, 0.5), (0.75, 0.78)]);
        }
        Icon::Video => {
            c.rect_shape((0.08, 0.25), (0.62, 0.75), false);
            c.poly(&[(0.68, 0.45), (0.92, 0.28), (0.92, 0.72), (0.68, 0.55)], false);
        }
        Icon::Font => {
            c.line((0.15, 0.85), (0.5, 0.12));
            c.line((0.5, 0.12), (0.85, 0.85));
            c.line((0.28, 0.6), (0.72, 0.6));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_paints_some_shape() {
        use Icon::*;
        let all = [
            Select,
            Subselect,
            Lasso,
            Pen,
            Text,
            Line,
            Rectangle,
            Oval,
            Pencil,
            Brush,
            PaintBucket,
            Eyedropper,
            Eraser,
            Hand,
            Zoom,
            FreeTransform,
            Bone,
            Camera,
            Pin,
            Mic,
            Eye,
            EyeOff,
            Lock,
            Unlock,
            Outline,
            Folder,
            FolderOpen,
            LayerNormal,
            LayerMask,
            LayerGuide,
            Add,
            Remove,
            ChevronRight,
            ChevronDown,
            Onion,
            Search,
            Play,
            Pause,
            Stop,
            StepBack,
            StepForward,
            ToStart,
            ToEnd,
            Loop,
            Record,
            Graphic,
            MovieClip,
            Button,
            Bitmap,
            Sound,
            Video,
            Font,
        ];
        let ctx = egui::Context::default();
        let shapes_with = |icon: Option<Icon>| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                if let Some(icon) = icon {
                    paint(ui.painter(), rect, icon, Color32::WHITE);
                }
            });
            out.textures_delta.clear();
            out.shapes.len()
        };
        let baseline = shapes_with(None);
        for icon in all {
            assert!(shapes_with(Some(icon)) > baseline, "{icon:?} painted nothing");
        }
    }
}

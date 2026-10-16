//! Fixture `blend_modes`: every blend mode on known colours, expected values computed by hand (see the .expect.md).

use anim_core::{BlendMode, Rgba8};
use anim_display::{Group, Node, Paint};

use crate::common::{Scene, fill, rect_path};
use crate::kit::{assert_rgb, assert_white, finish, save_crop};

const BACKDROP: Rgba8 = Rgba8::opaque(200, 100, 50);
const SOURCE: Rgba8 = Rgba8::opaque(100, 150, 200);

struct Cell {
    mode: BlendMode,
    source: Rgba8,
    centre: [u8; 3],
    tolerance: u8,
}

fn cell(mode: BlendMode, centre: [u8; 3], tolerance: u8) -> Cell {
    Cell { mode, source: SOURCE, centre, tolerance }
}

fn half(mode: BlendMode, centre: [u8; 3]) -> Cell {
    Cell { mode, source: Rgba8::new(100, 150, 200, 128), centre, tolerance: 3 }
}

#[test]
fn blend_modes() {
    use BlendMode as M;
    // Index = position in the 6-wide grid; None = blank cell.
    let cells: Vec<Option<Cell>> = vec![
        Some(cell(M::Normal, [100, 150, 200], 2)),
        Some(cell(M::Multiply, [78, 59, 39], 3)),
        Some(cell(M::Screen, [222, 191, 211], 3)),
        Some(cell(M::Overlay, [188, 118, 78], 3)),
        Some(cell(M::Darken, [100, 100, 50], 2)),
        Some(cell(M::Lighten, [200, 150, 200], 2)),
        Some(cell(M::ColorDodge, [255, 243, 232], 3)),
        Some(cell(M::ColorBurn, [115, 0, 0], 3)),
        Some(cell(M::HardLight, [157, 127, 167], 3)),
        Some(cell(M::SoftLight, [191, 111, 86], 4)),
        Some(cell(M::Difference, [100, 50, 150], 2)),
        Some(cell(M::Exclusion, [143, 132, 172], 3)),
        Some(cell(M::Hue, [64, 139, 214], 4)),
        Some(cell(M::Saturation, [175, 108, 75], 4)),
        Some(cell(M::Color, [84, 134, 184], 4)),
        Some(cell(M::Luminosity, [216, 116, 66], 4)),
        Some(cell(M::Add, [255, 250, 250], 2)),
        Some(cell(M::Subtract, [100, 0, 0], 2)),
        Some(cell(M::Invert, [55, 155, 205], 2)),
        Some(cell(M::Alpha, [200, 100, 50], 2)),
        Some(cell(M::Erase, [255, 255, 255], 2)),
        Some(cell(M::Layer, [100, 150, 200], 2)),
        Some(cell(M::PassThrough, [100, 150, 200], 2)),
        None,
        None,
        Some(half(M::Multiply, [139, 79, 45])),
        Some(half(M::Screen, [211, 146, 131])),
        Some(half(M::Add, [250, 175, 150])),
        Some(half(M::Subtract, [150, 50, 25])),
        Some(half(M::Invert, [127, 128, 128])),
    ];
    let mut scene = Scene::new(180, 150);
    for (index, cell) in cells.iter().enumerate() {
        let Some(cell) = cell else { continue };
        let (ox, oy) = (f64::from(u32::try_from(index % 6).unwrap()) * 30.0, f64::from(u32::try_from(index / 6).unwrap()) * 30.0);
        let backdrop = fill(rect_path(ox, oy, ox + 30.0, oy + 30.0), Paint::Solid(BACKDROP));
        let blended = Node::Group(Group {
            blend: cell.mode,
            children: vec![fill(rect_path(ox + 4.0, oy + 4.0, ox + 26.0, oy + 26.0), Paint::Solid(cell.source))],
            ..Group::default()
        });
        if matches!(cell.mode, BlendMode::Alpha | BlendMode::Erase) {
            scene.add(Node::Group(Group { blend: BlendMode::Layer, children: vec![backdrop, blended], ..Group::default() }));
        } else {
            scene.add(backdrop);
            scene.add(blended);
        }
    }
    let image = scene.render();
    finish("blend_modes", &image);
    save_crop("blend_modes-grid-5x", &image, 0, 0, 180, 150, 5);

    for (index, cell) in cells.iter().enumerate() {
        let (ox, oy) = (u32::try_from(index % 6).unwrap() * 30, u32::try_from(index / 6).unwrap() * 30);
        let Some(cell) = cell else {
            assert_white(&format!("blank cell {index}"), &image, ox + 15, oy + 15);
            continue;
        };
        assert_rgb(&format!("{:?} centre (cell {index})", cell.mode), &image, ox + 15, oy + 15, cell.centre, cell.tolerance);
        // The ring outside the inset source keeps the backdrop, except where the layer modes remove it.
        let ring = match cell.mode {
            BlendMode::Alpha => [255, 255, 255],
            _ => [200, 100, 50],
        };
        assert_rgb(&format!("{:?} ring (cell {index})", cell.mode), &image, ox + 1, oy + 1, ring, 2);
    }
}

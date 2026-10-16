//! Fixture `bitmap_fills`: rotated, repeated, clamped, smoothed and unsmoothed bitmap fills.

use std::collections::BTreeSet;

use anim_core::Affine;
use anim_display::{Image, Paint};

use crate::common::{Scene, fill, rect_path};
use crate::kit::{assert_rgb, finish, save_crop};

fn texel(i: u32, j: u32) -> [u8; 3] {
    [(60 * i + 30) as u8, (60 * j + 30) as u8, 128]
}

fn palette_image() -> Image {
    let mut bytes = Vec::new();
    for j in 0..4 {
        for i in 0..4 {
            let [r, g, b] = texel(i, j);
            bytes.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Image::new(4, 4, bytes).unwrap()
}

fn scene(width: u32, height: u32, matrix: Affine, repeat: bool, smoothing: bool) -> Scene {
    let mut scene = Scene::new(width, height);
    let id = scene.images.insert(palette_image());
    scene.add(fill(rect_path(0.0, 0.0, f64::from(width), f64::from(height)), Paint::Bitmap { image: id, transform: matrix, repeat, smoothing }));
    scene
}

fn off_palette(image: &Image) -> (usize, usize) {
    let palette: BTreeSet<[u8; 4]> = (0..4).flat_map(|j| (0..4).map(move |i| texel(i, j))).map(|[r, g, b]| [r, g, b, 255]).collect();
    let mut seen = BTreeSet::new();
    let mut off = 0;
    for chunk in image.premultiplied_rgba.chunks_exact(4) {
        let px = [chunk[0], chunk[1], chunk[2], chunk[3]];
        if palette.contains(&px) {
            seen.insert(px);
        } else {
            off += 1;
        }
    }
    (off, seen.len())
}

#[test]
fn bitmap_fills() {
    let quarter_turn = Affine::translate((100.0, 20.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale(10.0);
    let a = scene(160, 100, quarter_turn, true, false).render();
    finish("bitmap_fills_a", &a);
    save_crop("bitmap_fills_a-tile-8x", &a, 50, 15, 60, 50, 10);

    // PA: texel centres, in three tiles.
    for (i, j) in [(0, 0), (3, 1), (1, 3), (2, 2)] {
        let (x, y) = (95 - 10 * j, 25 + 10 * i);
        assert_rgb(&format!("A texel ({i}, {j})"), &a, x, y, texel(i, j), 0);
        assert_rgb(&format!("A texel ({i}, {j}) next tile in x"), &a, x - 40, y, texel(i, j), 0);
        assert_rgb(&format!("A texel ({i}, {j}) next tile in y"), &a, x, y + 40, texel(i, j), 0);
        assert_rgb(&format!("A texel ({i}, {j}) tile in x+"), &a, x + 40, y, texel(i, j), 0);
    }
    // PA2.
    assert_eq!(off_palette(&a).0, 0, "PA2 nearest sampling must only produce texel colours");

    // PB.
    let b = scene(160, 100, Affine::translate((80.0, 50.0)) * Affine::rotate(30.0_f64.to_radians()) * Affine::scale(7.0), true, false).render();
    finish("bitmap_fills_b", &b);
    let (off, seen) = off_palette(&b);
    assert_eq!(off, 0, "PB off-palette pixels");
    assert_eq!(seen, 16, "PB all texels visible");

    // PC.
    let c = scene(160, 100, quarter_turn, true, true).render();
    finish("bitmap_fills_c", &c);
    save_crop("bitmap_fills_c-tile-8x", &c, 50, 15, 60, 50, 10);
    assert!(off_palette(&c).0 > 500, "PC smoothing blends texels: {} off-palette", off_palette(&c).0);
    // Interior texels only: a pixel centre lies 0.5 px (0.05 texel) off a texel centre, so it picks up 5 per cent of a
    // neighbour, which differs by 60 per channel (180 for the wrapped neighbour of an edge texel of the repeating tile).
    for (i, j) in [(1, 1), (2, 2), (1, 2)] {
        assert_rgb(&format!("C texel ({i}, {j})"), &c, 95 - 10 * j, 25 + 10 * i, texel(i, j), 8);
    }

    // PD.
    let d = scene(100, 100, Affine::translate((30.0, 30.0)) * Affine::scale(10.0), false, false).render();
    finish("bitmap_fills_d", &d);
    assert_rgb("D top left", &d, 5, 5, texel(0, 0), 0);
    assert_rgb("D bottom right", &d, 95, 95, texel(3, 3), 0);
    assert_rgb("D left edge", &d, 5, 50, texel(0, 2), 0);
    assert_rgb("D top edge", &d, 50, 5, texel(2, 0), 0);
    assert_rgb("D inside", &d, 35, 35, texel(0, 0), 0);
    assert_rgb("D inside 2", &d, 65, 65, texel(3, 3), 0);
    assert!(d.premultiplied_rgba.chunks_exact(4).all(|p| p[3] == 255), "PD fully covered");
}

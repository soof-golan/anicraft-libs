//! Renders a display list stored as JSON to a PNG, for debugging producers and for quick looks.
//!
//! ```text
//! cargo run -p anim-render --example render_json -- scene.json out.png [--scale 2] [--transparent] [--no-aa]
//! cargo run -p anim-render --example render_json -- --demo demo.json     # writes a small scene to start from
//! ```
//!
//! The input is `{ "list": <DisplayList>, "images": <ImageStore> }`: plain serde of the `anim-display` types
//! (`images` may be left out when nothing refers to a bitmap). The render statistics are printed as JSON.

use std::error::Error;
use std::fs::File;
use std::io::BufWriter;

use anim_core::{Affine, BezPath, Rgba8, Size};
use anim_display::{DisplayList, Fill, FillRule, Filter, GRADIENT_EXTENT, Gradient, GradientStop, Group, ImageStore, Interpolation, Node, Paint, Spread};
use anim_render::{Background, RenderOptions, Renderer, straight_rgba};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Input {
    list: DisplayList,
    #[serde(default)]
    images: ImageStore,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut positional = Vec::new();
    let (mut scale, mut transparent, mut antialias, mut demo) = (1.0_f64, false, true, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scale" => scale = args.next().ok_or("--scale needs a number")?.parse()?,
            "--transparent" => transparent = true,
            "--no-aa" => antialias = false,
            "--demo" => demo = true,
            other => positional.push(other.to_owned()),
        }
    }
    if demo {
        let out = positional.first().ok_or("usage: render_json --demo OUT.json")?;
        serde_json::to_writer_pretty(BufWriter::new(File::create(out)?), &demo_input())?;
        return Ok(());
    }
    let (Some(input), Some(output)) = (positional.first(), positional.get(1)) else {
        return Err("usage: render_json SCENE.json OUT.png [--scale N] [--transparent] [--no-aa]".into());
    };
    let Input { list, images } = serde_json::from_str(&std::fs::read_to_string(input)?)?;
    let mut options = RenderOptions::scaled_stage(list.size, scale).with_antialias(antialias);
    if transparent {
        options = options.with_background(Background::Transparent);
    }
    let mut renderer = Renderer::new();
    let image = renderer.render(&list, &images, &options)?;
    anim_testkit::write_png(std::path::Path::new(&output), image.width, image.height, &straight_rgba(&image))?;
    println!("{}", serde_json::to_string(&renderer.stats())?);
    Ok(())
}

fn demo_input() -> Input {
    let mut disc = BezPath::new();
    disc.move_to((60.0, 100.0));
    disc.curve_to((60.0, 60.0), (140.0, 60.0), (140.0, 100.0));
    disc.curve_to((140.0, 140.0), (60.0, 140.0), (60.0, 100.0));
    disc.close_path();
    let mut card = BezPath::new();
    card.move_to((0.0, 0.0));
    card.line_to((200.0, 0.0));
    card.line_to((200.0, 200.0));
    card.line_to((0.0, 200.0));
    card.close_path();
    let sky = Gradient {
        transform: Affine::translate((100.0, 100.0))
            * Affine::rotate(std::f64::consts::FRAC_PI_2)
            * Affine::scale_non_uniform(200.0 / (GRADIENT_EXTENT * 2.0), 1.0),
        stops: vec![GradientStop { offset: 0.0, color: Rgba8::opaque(40, 70, 160) }, GradientStop { offset: 1.0, color: Rgba8::opaque(250, 190, 120) }],
        spread: Spread::Pad,
        interpolation: Interpolation::LinearRgb,
        focal: 0.0,
    };
    let fill = |path: BezPath, paint: Paint| Node::Fill(Fill { transform: Affine::IDENTITY, path, rule: FillRule::NonZero, paint });
    let sun = Group {
        filters: vec![Filter::DropShadow {
            color: Rgba8::new(0, 0, 0, 160),
            angle_deg: 60.0,
            distance: 10.0,
            blur_x: 8.0,
            blur_y: 8.0,
            strength: 1.0,
            inner: false,
            knockout: false,
            hide_object: false,
        }],
        children: vec![fill(disc, Paint::Solid(Rgba8::opaque(255, 220, 80)))],
        ..Group::default()
    };
    let mut list = DisplayList::new(Size::new(200.0, 200.0), Some(Rgba8::WHITE));
    list.nodes = vec![fill(card, Paint::Linear(sky)), Node::Group(sun)];
    Input { list, images: ImageStore::new() }
}

//! Evaluates an `anim-scene` [`Document`](anim_scene::Document) at a frame into an
//! [`anim_display::DisplayList`].
//!
//! ```text
//! Document + scene + frame ──► Evaluator::frame ──► DisplayList ──► anim-render
//! ```
//!
//! The rules, all covered by tests with hand-built documents:
//!
//! - **Layers** are drawn bottom to top (XFL lists the top layer first). Guide layers draw nothing (unless
//!   [`EvalOptions::include_guides`]); the layers they guide are ordinary. Folders become groups. A mask layer clips
//!   the layers whose parent it is, as a [`Group`](anim_display::Group) with a mask. Hidden layers are skipped
//!   unless [`EvalOptions::include_hidden`]; a hidden mask does not mask.
//! - **Keyframes**: frame `N` shows the keyframe whose span covers it.
//! - **Classic tweens** pair the elements of a tweened keyframe with the next keyframe by depth and library item,
//!   split the matrices into translation, rotation, scale and skew and interpolate those (see [`xform`]), honouring
//!   the rotate setting (auto, none, clockwise or counter-clockwise with extra turns) and the scale flag, and
//!   interpolate colour transforms and filters. Easing is Flash's classic ease or a named or custom bezier ease,
//!   chosen per property (see [`ease`]).
//!   A tween on a layer guided by a motion guide layer follows the guide's first stroke at constant speed, turning
//!   the baseline with the path when "orient to path" is set (see [`guide`]).
//! - **Shape tweens** use `anim-morph`, with the keyframe's shape hints. **Motion-object tweens** read what they can
//!   of `motionObjectXML` (see [`motion`]) and otherwise hold the first keyframe.
//! - **Armatures**: a layer with an `anim_scene::Armature` draws its first keyframe's elements moved by the bones'
//!   pose at the frame (symbol armatures) or deformed by them (shape armatures); poses tween between pose keyframes
//!   (see [`armature`]).
//! - **Symbols**: graphic instances are locked to the parent timeline (loop, play once, single frame from
//!   `first_frame`); movie clips show `first_frame` unless [`EvalOptions::advance_movie_clips`]; buttons show Up, or the
//!   state [`EvalOptions::button_state`] gives one instance ([`Evaluator::frame_of_button`] draws a whole button in a
//!   state; a layer without a keyframe at the state's frame shows the state before it). A symbol with 9-slice guides
//!   keeps its corners unscaled when an instance is scaled (see [`scale9`]). Nesting is limited and cycles are cut.
//! - **Bitmaps** become bitmap nodes in an image store the evaluator fills through an [`ImageProvider`]; **text** is
//!   shaped by `anim-text` (right-to-left included) into glyph fills.
//! - **Onion skins**: [`Evaluator::onion`].
//!
//! Coordinates: stage pixels, y down. Top-level element groups carry a [`Group::key`](anim_display::Group::key) from
//! [`element_key`], so a click found by the renderer maps back to a layer and element.

pub mod armature;
pub mod ease;
pub mod guide;
pub mod images;
pub mod motion;
pub mod scale9;
pub mod tween;
pub mod xform;

mod eval;
mod memo;
mod text;

pub use eval::{EvalOptions, Evaluator, element_key, fade, split_key};
pub use images::{ImageProvider, NoDecoder, PngDecoder, decode_png};
pub use text::rich_text;

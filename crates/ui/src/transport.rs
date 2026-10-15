//! The transport bar: play, pause, step, loop, frame rate and a frame field.

use egui::{Ui, vec2};
use serde::{Deserialize, Serialize};

use crate::{icons::Icon, theme::current_palette, widgets::icon_button};

/// What the transport shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransportState {
    pub playing: bool,
    pub looping: bool,
    pub fps: f32,
    /// Current frame, 0-based.
    pub frame: u32,
    pub frame_count: u32,
}

impl Default for TransportState {
    fn default() -> Self {
        Self { playing: false, looping: true, fps: 24.0, frame: 0, frame_count: 1 }
    }
}

/// What the transport reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransportEvent {
    Play,
    Pause,
    GoToStart,
    GoToEnd,
    StepBack,
    StepForward,
    SetLoop(bool),
    SetFps(f32),
    /// The frame field was edited (0-based).
    SetFrame(u32),
}

/// The next frame after `frame` when stepping by `delta` (-1 or 1), wrapping when `looping`, else clamping to the
/// first and last frame.
#[must_use]
pub const fn step_frame(frame: u32, frame_count: u32, delta: i32, looping: bool) -> u32 {
    let last = frame_count.saturating_sub(1);
    if delta >= 0 {
        let next = frame.saturating_add(delta.unsigned_abs());
        if next > last { if looping { 0 } else { last } } else { next }
    } else if let Some(prev) = frame.checked_sub(delta.unsigned_abs()) {
        prev
    } else if looping {
        last
    } else {
        0
    }
}

/// Draws the transport bar in a horizontal strip and returns its events.
pub fn transport_bar(ui: &mut Ui, state: &TransportState) -> Vec<TransportEvent> {
    let p = current_palette(ui.ctx());
    let mut events = Vec::new();
    ui.horizontal(|ui| {
        let size = vec2(26.0, 24.0);
        if icon_button(ui, size, Icon::ToStart, false, true, "First frame (Home)").clicked() {
            events.push(TransportEvent::GoToStart);
        }
        if icon_button(ui, size, Icon::StepBack, false, true, "Previous frame (,)").clicked() {
            events.push(TransportEvent::StepBack);
        }
        let (icon, tip) = if state.playing { (Icon::Pause, "Pause (Enter)") } else { (Icon::Play, "Play (Enter)") };
        if icon_button(ui, size, icon, state.playing, true, tip).clicked() {
            events.push(if state.playing { TransportEvent::Pause } else { TransportEvent::Play });
        }
        if icon_button(ui, size, Icon::StepForward, false, true, "Next frame (.)").clicked() {
            events.push(TransportEvent::StepForward);
        }
        if icon_button(ui, size, Icon::ToEnd, false, true, "Last frame (End)").clicked() {
            events.push(TransportEvent::GoToEnd);
        }
        if icon_button(ui, size, Icon::Loop, state.looping, true, "Loop playback").clicked() {
            events.push(TransportEvent::SetLoop(!state.looping));
        }
        ui.separator();
        ui.label(egui::RichText::new("Frame").size(11.0).color(p.text_weak));
        let mut shown = state.frame.saturating_add(1);
        let last = state.frame_count.max(1);
        if ui.add(egui::DragValue::new(&mut shown).range(1..=last).speed(0.2)).changed() {
            events.push(TransportEvent::SetFrame(shown.saturating_sub(1)));
        }
        ui.label(egui::RichText::new(format!("/ {last}")).size(11.0).color(p.text_weak));
        ui.separator();
        let mut fps = state.fps;
        if ui.add(egui::DragValue::new(&mut fps).range(1.0..=240.0).speed(0.1).max_decimals(2).suffix(" fps")).changed() {
            events.push(TransportEvent::SetFps(fps));
        }
    });
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_wraps_or_clamps() {
        assert_eq!(step_frame(3, 10, 1, true), 4);
        assert_eq!(step_frame(9, 10, 1, true), 0);
        assert_eq!(step_frame(9, 10, 1, false), 9);
        assert_eq!(step_frame(0, 10, -1, true), 9);
        assert_eq!(step_frame(0, 10, -1, false), 0);
        assert_eq!(step_frame(5, 10, -1, false), 4);
        assert_eq!(step_frame(0, 0, 1, true), 0);
    }
}

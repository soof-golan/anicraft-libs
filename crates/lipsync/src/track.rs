//! Offline analysis and viseme-track helpers.

use anim_core::Tick;

use crate::engine::{Config, VisemeKey};
use crate::live::LiveLipSync;
use crate::viseme::{AnimateViseme, Viseme};

/// Analyses a mono recording with the default [`Config`].
///
/// The first key always starts at [`Tick::ZERO`]; the track ends back at `Neutral`. Keys are strictly increasing in
/// time and consecutive keys always differ. A `sample_rate` outside 4 kHz ..= 384 kHz is clamped.
#[must_use]
pub fn analyze(samples: &[f32], sample_rate: u32) -> Vec<VisemeKey> {
    analyze_with(samples, sample_rate, Config::default())
}

/// [`analyze`] with explicit tuning.
#[must_use]
pub fn analyze_with(samples: &[f32], sample_rate: u32, config: Config) -> Vec<VisemeKey> {
    let mut live = LiveLipSync::with_config(sample_rate, config);
    let mut keys = live.push_keys(samples);
    keys.extend(live.finish());
    let mut track = Vec::with_capacity(keys.len().saturating_add(1));
    track.push(VisemeKey { start: Tick::ZERO, viseme: Viseme::Neutral });
    for key in keys {
        match track.last_mut() {
            Some(last) if key.start <= last.start => last.viseme = key.viseme,
            Some(last) if last.viseme == key.viseme => {}
            _ => track.push(key),
        }
    }
    // A replaced key may now equal its predecessor.
    track.dedup_by(|b, a| a.viseme == b.viseme);
    track
}

/// The viseme showing at time `t` in a track sorted by start time (Neutral before the first key).
#[must_use]
pub fn viseme_at(track: &[VisemeKey], t: Tick) -> Viseme {
    let idx = track.partition_point(|k| k.start <= t);
    idx.checked_sub(1).and_then(|i| track.get(i)).map_or(Viseme::Neutral, |k| k.viseme)
}

/// Re-expresses a track in Animate's lip-sync set, merging neighbours that map to the same shape.
#[must_use]
pub fn to_animate_track(track: &[VisemeKey]) -> Vec<(Tick, AnimateViseme)> {
    let mut out: Vec<(Tick, AnimateViseme)> = Vec::with_capacity(track.len());
    for k in track {
        let shape = k.viseme.to_animate();
        if out.last().is_none_or(|(_, s)| *s != shape) {
            out.push((k.start, shape));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viseme_lookup() {
        let track = [VisemeKey { start: Tick::ZERO, viseme: Viseme::Neutral }, VisemeKey { start: Tick::from_seconds_f64(1.0), viseme: Viseme::Ah }];
        assert_eq!(viseme_at(&track, Tick::from_seconds_f64(0.5)), Viseme::Neutral);
        assert_eq!(viseme_at(&track, Tick::from_seconds_f64(1.0)), Viseme::Ah);
        assert_eq!(viseme_at(&track, Tick(-5)), Viseme::Neutral);
        assert_eq!(viseme_at(&[], Tick::ZERO), Viseme::Neutral);
    }

    #[test]
    fn animate_track_merges_shared_shapes() {
        let t = |s| Tick::from_seconds_f64(s);
        let track = [
            VisemeKey { start: t(0.0), viseme: Viseme::Neutral },
            VisemeKey { start: t(1.0), viseme: Viseme::S },
            VisemeKey { start: t(1.1), viseme: Viseme::D },
            VisemeKey { start: t(1.2), viseme: Viseme::Ah },
        ];
        let a = to_animate_track(&track);
        assert_eq!(a.len(), 3);
        assert_eq!(a[1], (t(1.0), AnimateViseme::Etc));
    }

    #[test]
    fn empty_and_tiny_input() {
        assert_eq!(analyze(&[], 44_100), vec![VisemeKey { start: Tick::ZERO, viseme: Viseme::Neutral }]);
        assert_eq!(analyze(&[0.1], 0).len(), 1);
    }
}

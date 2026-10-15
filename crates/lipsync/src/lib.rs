//! Audio to visemes, offline and live, with no machine-learning weights.
//!
//! [`analyze`] turns a mono recording into a track of [`VisemeKey`]s; [`LiveLipSync`] does the same incrementally
//! for microphone input at about 50 ms latency. Both run the same pipeline, so a recording gives the same keys
//! whether it is analysed whole or pushed in chunks of any size (the output is bit-for-bit deterministic).
//!
//! # Method
//!
//! Clean-room, classical speech processing (the textbook source-filter model):
//!
//! 1. **Framing.** The input is averaged down by an integer factor to at most 16 kHz, then cut into 25 ms Hann
//!    windows every 10 ms. Each frame describes the 10 ms hop at the middle of its window.
//! 2. **Voice activity.** Frame RMS (dBFS) is compared with an adaptive threshold: the highest of a fixed floor
//!    (-52 dBFS), the tracked noise floor plus 12 dB, and the decaying speech peak minus 42 dB. Quiet frames are
//!    silence, shown as `Neutral`.
//! 3. **Fricatives.** High zero-crossing rate with a large share of spectral energy above ~4 kHz (FFT) is `S`
//!    (sibilant); a flatter, weaker noise spectrum is `F`.
//! 4. **Stops.** A sudden energy rise out of silence (8 dB) in an unvoiced, non-sibilant frame is a burst: `D`,
//!    held for two frames.
//! 5. **Voicing.** The normalised autocorrelation peak over 70-400 Hz lags separates voiced from unvoiced sound.
//! 6. **Nasals.** Voiced sound with almost all its energy below 700 Hz is a closed-mouth murmur: `M`.
//! 7. **Vowels.** Linear prediction (autocorrelation method, Levinson-Durbin, pre-emphasis 0.97) models the vocal
//!    tract; the peaks of the LPC envelope (read from an FFT of the polynomial) give F1, F2, F3. The frame takes the
//!    nearest vowel prototype in the log (F1, F2) plane: `Ah` (open), `Uh` (mid), `Oh` (rounded), `WOo`
//!    (pursed), `Ee` (spread, two prototypes). A low F3 with mid F2 is `R`; a weak, dark sonorant (6 dB under the
//!    speech peak) near (370, 1150) Hz is `L`. The incumbent viseme gets a 20 % distance bonus (hysteresis).
//! 8. **Smoothing.** A new label must persist for 2 frames (3 to return to Neutral, 1 for a burst), every viseme
//!    stays at least 40 ms, and a change is dated back to the first frame that showed it.
//!
//! Limitations: this is a heuristic. It is robust on clean speech and on microphones with a moderate noise floor; it
//! does not understand words, so voiced consonants such as N, G or Y map to whichever vowel or murmur they resemble.
//!
//! # Viseme sets
//!
//! [`Viseme`] is Character Animator's set; [`AnimateViseme`] is Animate's lip-sync set; the two map onto each other
//! ([`Viseme::to_animate`], [`AnimateViseme::to_character_animator`]).

mod classify;
mod dsp;
mod engine;
mod features;
mod live;
mod num;
mod track;
mod viseme;

pub use engine::{Config, VisemeKey};
pub use live::LiveLipSync;
pub use track::{analyze, analyze_with, to_animate_track, viseme_at};
pub use viseme::{AnimateViseme, Expression, Viseme};

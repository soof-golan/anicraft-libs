//! Frame classification: features to a raw (unsmoothed) viseme.

use crate::features::Features;
use crate::viseme::Viseme;

/// Voicing score at or above which a frame counts as voiced.
pub const VOICED: f32 = 0.45;
/// High-band share at or above which noise is a sibilant (S) rather than a softer fricative (F).
const SIBILANT_SHARE: f32 = 0.55;
/// High-band share at which a voiced frame still counts as a fricative (Z, V).
const VOICED_FRICATIVE_SHARE: f32 = 0.45;
/// Minimum zero-crossing rate of a fricative.
const FRICATIVE_ZCR: f32 = 0.2;
/// Low-band share at or above which voiced sound is a closed-mouth murmur (M).
const MURMUR_LOW_SHARE: f32 = 0.9;
/// Low-band share of a hum with practically no energy above the band (even Ee and Oo keep a few percent up there).
const PURE_HUM_SHARE: f32 = 0.985;
/// A nasal with a visible low F2 must be this far (dB) below the speech peak.
const NASAL_DROP_DB: f32 = 12.0;
/// Energy drop below the running speech peak, in dB, from which a voiced sonorant may be read as L.
const SONORANT_DROP_DB: f32 = 6.0;
/// A current vowel keeps its place unless a rival is this much closer (distance multiplier on the incumbent).
const INCUMBENT_BONUS: f32 = 0.8;

/// A vowel prototype in the (F1, F2) plane.
struct Prototype {
    viseme: Viseme,
    f1: f32,
    f2: f32,
}

const VOWELS: [Prototype; 6] = [
    Prototype { viseme: Viseme::Ah, f1: 790.0, f2: 1250.0 },
    Prototype { viseme: Viseme::Uh, f1: 600.0, f2: 1250.0 },
    Prototype { viseme: Viseme::Oh, f1: 530.0, f2: 900.0 },
    Prototype { viseme: Viseme::WOo, f1: 350.0, f2: 850.0 },
    Prototype { viseme: Viseme::Ee, f1: 400.0, f2: 2150.0 },
    Prototype { viseme: Viseme::Ee, f1: 560.0, f2: 1850.0 },
];

/// The L prototype (a dark, weak sonorant between the vowels).
const L_PROTOTYPE: Prototype = Prototype { viseme: Viseme::L, f1: 370.0, f2: 1150.0 };

/// What the classifier knows besides the features.
#[derive(Clone, Copy, Debug)]
pub struct Context {
    /// The viseme currently shown (for hysteresis).
    pub current: Viseme,
    /// Slowly decaying peak of the frame energy in dBFS.
    pub peak_db: f32,
    /// This frame is a sudden rise out of silence.
    pub onset: bool,
}

/// Classifies a non-silent frame.
#[must_use]
pub fn classify(f: &Features, ctx: Context) -> Viseme {
    let voiced = f.voicing >= VOICED;
    if f.zcr >= FRICATIVE_ZCR {
        if f.high_share >= SIBILANT_SHARE && (!voiced || f.high_share >= VOICED_FRICATIVE_SHARE) {
            return Viseme::S;
        }
        if !voiced && f.low_share < MURMUR_LOW_SHARE && ctx.onset {
            return Viseme::D;
        }
        if !voiced && f.high_share >= 0.2 {
            return Viseme::F;
        }
    }
    if !voiced {
        return if ctx.onset { Viseme::D } else { Viseme::Neutral };
    }
    if f.low_share >= MURMUR_LOW_SHARE && is_murmur(f, ctx) {
        return Viseme::M;
    }
    let (Some(f1), Some(f2)) = (f.formants[0], f.formants[1]) else { return Viseme::Uh };
    if let Some(f3) = f.formants[2]
        && f3 < 2250.0
        && (1000.0..1750.0).contains(&f2)
        && f1 < 650.0
    {
        return Viseme::R;
    }
    nearest_vowel(f1, f2, f.energy_db, ctx)
}

/// A murmur has nothing but a low hum: no second formant at all, or (next to louder speech) a quiet low F1 with a
/// low, weak F2.
fn is_murmur(f: &Features, ctx: Context) -> bool {
    if f.low_share >= PURE_HUM_SHARE {
        return true;
    }
    match (f.formants[0], f.formants[1]) {
        (Some(f1), Some(f2)) => f.energy_db <= ctx.peak_db - NASAL_DROP_DB && f1 < 450.0 && f2 < 1000.0,
        _ => false,
    }
}

fn nearest_vowel(f1: f32, f2: f32, energy_db: f32, ctx: Context) -> Viseme {
    let sonorant_ok = energy_db <= ctx.peak_db - SONORANT_DROP_DB;
    let candidates = VOWELS.iter().chain(sonorant_ok.then_some(&L_PROTOTYPE));
    let mut best = (f32::INFINITY, Viseme::Uh);
    for p in candidates {
        let d1 = (f1 / p.f1).ln() * 1.2;
        let d2 = (f2 / p.f2).ln();
        let mut d = d1.mul_add(d1, d2 * d2);
        if p.viseme == ctx.current {
            d *= INCUMBENT_BONUS;
        }
        if d < best.0 {
            best = (d, p.viseme);
        }
    }
    best.1
}

//! The properties of bitmap, sound and video library items that the Properties dialogs show and change.
//!
//! The model keeps what a reader understood in typed fields and everything else in [`Extra`]. The settings in this
//! module that have no field of their own (a bitmap's "use imported JPEG data", a sound's export settings, where an
//! item was imported from) live in `Extra` attributes, so they survive an XFL round trip with no change to the readers
//! and writers.

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::error::SceneError;
use crate::extra::Extra;
use crate::library::{BitmapCompression, BitmapItem, LibraryItem, SoundItem, VideoItem};

/// `sourceExternalFilepath`: the file an item was imported from.
const SOURCE_PATH: &str = "sourceExternalFilepath";
/// `sourceLastImported`: when it was imported, seconds since the Unix epoch.
const SOURCE_IMPORTED: &str = "sourceLastImported";
/// `externalFileSize`: the size of the source file in bytes.
const SOURCE_SIZE: &str = "externalFileSize";
/// `useImportedJPEGData`: keep the JPEG data of the imported file instead of compressing again.
const USE_IMPORTED_JPEG: &str = "useImportedJPEGData";
const EXPORT_FORMAT: &str = "exportFormat";
const EXPORT_BITS: &str = "exportBits";
const EXPORT_RATE: &str = "exportSampleRate";
const EXPORT_MONO: &str = "exportConvertStereoToMono";
const EXPORT_QUALITY: &str = "exportQuality";

/// Where an item was imported from, when and how large the file was.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Path of the source file as the document stores it.
    pub path: Option<String>,
    /// Seconds since the Unix epoch.
    pub imported: Option<u64>,
    /// Bytes.
    pub size: Option<u64>,
}

/// Reads the import source recorded in `extra`.
#[must_use]
pub fn source_info(extra: &Extra) -> SourceInfo {
    SourceInfo {
        path: extra.attr(SOURCE_PATH).map(str::to_owned),
        imported: extra.attr(SOURCE_IMPORTED).and_then(|v| v.parse().ok()),
        size: extra.attr(SOURCE_SIZE).and_then(|v| v.parse().ok()),
    }
}

/// Records the import source in `extra`.
pub fn set_source_info(extra: &mut Extra, info: &SourceInfo) {
    match &info.path {
        Some(path) => extra.set_attr(SOURCE_PATH, path.as_str()),
        None => drop(extra.remove_attr(SOURCE_PATH)),
    }
    match info.imported {
        Some(secs) => extra.set_attr(SOURCE_IMPORTED, secs.to_string()),
        None => drop(extra.remove_attr(SOURCE_IMPORTED)),
    }
    match info.size {
        Some(bytes) => extra.set_attr(SOURCE_SIZE, bytes.to_string()),
        None => drop(extra.remove_attr(SOURCE_SIZE)),
    }
}

impl LibraryItem {
    /// The import source of a bitmap, sound or video; empty for the other kinds.
    #[must_use]
    pub fn source(&self) -> SourceInfo {
        match self {
            Self::Bitmap(b) => source_info(&b.extra),
            Self::Sound(s) => source_info(&s.extra),
            Self::Video(v) => source_info(&v.extra),
            Self::Symbol(_) | Self::Font(_) | Self::Folder(_) => SourceInfo::default(),
        }
    }

    /// Records the import source of a bitmap, sound or video; the other kinds ignore it.
    pub fn set_source(&mut self, info: &SourceInfo) {
        match self {
            Self::Bitmap(b) => set_source_info(&mut b.extra, info),
            Self::Sound(s) => set_source_info(&mut s.extra, info),
            Self::Video(v) => set_source_info(&mut v.extra, info),
            Self::Symbol(_) | Self::Font(_) | Self::Folder(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Bitmaps
// ---------------------------------------------------------------------------------------------------------------

impl BitmapItem {
    /// "Use imported JPEG data": a photo bitmap keeps the compressed data of the file it came from.
    #[must_use]
    pub fn uses_imported_jpeg_data(&self) -> bool {
        self.extra.attr(USE_IMPORTED_JPEG) == Some("true")
    }

    /// Sets [`BitmapItem::uses_imported_jpeg_data`].
    pub fn set_uses_imported_jpeg_data(&mut self, on: bool) {
        self.extra.set_attr(USE_IMPORTED_JPEG, if on { "true" } else { "false" });
    }
}

/// What the Bitmap Properties dialog changes; `None` leaves a setting as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BitmapProps {
    pub smoothing: Option<bool>,
    pub compression: Option<BitmapCompression>,
    pub use_imported_jpeg_data: Option<bool>,
    /// JPEG quality 0 ..= 100 for a custom quality.
    pub quality: Option<u8>,
}

/// Applies the Bitmap Properties dialog to the bitmap `name`.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `name` is not a bitmap; [`SceneError::InvalidRange`] for a quality above 100.
pub fn set_bitmap_properties(doc: &mut Document, name: &str, props: &BitmapProps) -> Result<(), SceneError> {
    if props.quality.is_some_and(|q| q > 100) {
        return Err(SceneError::InvalidRange("JPEG quality is 0 to 100"));
    }
    let Some(LibraryItem::Bitmap(bitmap)) = doc.library.find_mut(name) else { return Err(SceneError::NoSuchItem(name.to_owned())) };
    if let Some(v) = props.smoothing {
        bitmap.smoothing = v;
    }
    if let Some(v) = props.compression {
        bitmap.compression = v;
    }
    if let Some(v) = props.use_imported_jpeg_data {
        bitmap.set_uses_imported_jpeg_data(v);
    }
    if let Some(v) = props.quality {
        bitmap.quality = Some(v);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------------------------------------------

/// The compression a sound is exported with (Sound Properties > Compression).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SoundCodec {
    /// Use the publish settings of the document.
    #[default]
    Default,
    Adpcm,
    Mp3,
    Raw,
    Speech,
}

impl SoundCodec {
    /// Every choice in the order of the drop-down.
    pub const ALL: [Self; 5] = [Self::Default, Self::Adpcm, Self::Mp3, Self::Raw, Self::Speech];

    /// Lowercase name for commands.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Adpcm => "adpcm",
            Self::Mp3 => "mp3",
            Self::Raw => "raw",
            Self::Speech => "speech",
        }
    }

    /// The label a person reads.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Adpcm => "ADPCM",
            Self::Mp3 => "MP3",
            Self::Raw => "Raw",
            Self::Speech => "Speech",
        }
    }

    /// Parses [`SoundCodec::name`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The value of `exportFormat`: the number SWF gives the codec.
    const fn format_code(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Adpcm => Some("1"),
            Self::Mp3 => Some("2"),
            Self::Raw => Some("3"),
            Self::Speech => Some("5"),
        }
    }

    fn from_format_code(code: &str) -> Self {
        Self::ALL.into_iter().find(|c| c.format_code() == Some(code)).unwrap_or_default()
    }
}

/// The MP3 and Speech preprocessing quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SoundQuality {
    Fast,
    #[default]
    Medium,
    Best,
}

impl SoundQuality {
    /// Lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Medium => "medium",
            Self::Best => "best",
        }
    }

    /// Parses [`SoundQuality::name`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Fast, Self::Medium, Self::Best].into_iter().find(|q| q.name() == name)
    }
}

/// The sample rates Flash exports at, in hertz.
pub const SOUND_RATES: [u32; 4] = [5512, 11_025, 22_050, 44_100];
/// The MP3 bit rates offered, in kilobits per second.
pub const MP3_BIT_RATES: [u32; 12] = [8, 16, 20, 24, 32, 48, 56, 64, 80, 112, 128, 160];
/// The ADPCM sample sizes offered, in bits.
pub const ADPCM_BITS: [u8; 4] = [2, 3, 4, 5];

/// Export settings of one sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundExport {
    pub codec: SoundCodec,
    /// Sample rate in hertz, one of [`SOUND_RATES`] (ADPCM, Raw and Speech).
    pub sample_rate: u32,
    /// ADPCM bits, one of [`ADPCM_BITS`].
    pub adpcm_bits: u8,
    /// MP3 bit rate, one of [`MP3_BIT_RATES`].
    pub mp3_kbps: u32,
    /// Convert stereo to mono (preprocess).
    pub mono: bool,
    pub quality: SoundQuality,
}

impl Default for SoundExport {
    fn default() -> Self {
        Self { codec: SoundCodec::Default, sample_rate: 22_050, adpcm_bits: 4, mp3_kbps: 16, mono: true, quality: SoundQuality::Fast }
    }
}

impl SoundExport {
    /// Checks the settings against the choices the dialog offers.
    ///
    /// # Errors
    /// [`SceneError::InvalidRange`] naming the setting that is not one of the allowed values.
    pub fn validate(&self) -> Result<(), SceneError> {
        if !SOUND_RATES.contains(&self.sample_rate) {
            return Err(SceneError::InvalidRange("sample rate must be 5512, 11025, 22050 or 44100 Hz"));
        }
        if !ADPCM_BITS.contains(&self.adpcm_bits) {
            return Err(SceneError::InvalidRange("ADPCM bits must be 2, 3, 4 or 5"));
        }
        if !MP3_BIT_RATES.contains(&self.mp3_kbps) {
            return Err(SceneError::InvalidRange("MP3 bit rate must be one of 8, 16, 20, 24, 32, 48, 56, 64, 80, 112, 128 or 160 kbps"));
        }
        Ok(())
    }
}

/// What a sound's `format` attribute says: `44kHz 16bit Stereo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundFormat {
    /// Hertz; Animate writes 5, 11, 22 and 44 kHz for the usual rates.
    pub sample_rate: u32,
    pub bits: u32,
    pub channels: u32,
}

/// Parses a sound's `format` text (`22kHz 16bit Mono`), or `None` when it has another shape.
#[must_use]
pub fn parse_sound_format(text: &str) -> Option<SoundFormat> {
    let mut parts = text.split_whitespace();
    let rate = parts.next()?.strip_suffix("kHz")?;
    let bits = parts.next()?.strip_suffix("bit")?;
    let channels = match parts.next()? {
        "Mono" => 1,
        "Stereo" => 2,
        _ => return None,
    };
    // 5.5 kHz is written as 5.
    let khz: f64 = rate.parse().ok()?;
    let hz = match rate {
        "5" => 5512,
        "11" => 11_025,
        "22" => 22_050,
        "44" => 44_100,
        _ => anim_core::num::round_f64_to_u32(khz * 1000.0),
    };
    Some(SoundFormat { sample_rate: hz, bits: bits.parse().ok()?, channels })
}

impl SoundItem {
    /// The sample rate, size and channels of the sound, from its `format` text.
    #[must_use]
    pub fn parsed_format(&self) -> Option<SoundFormat> {
        self.format.as_deref().and_then(parse_sound_format)
    }

    /// The length in seconds, from the sample count and the sample rate.
    #[must_use]
    pub fn duration_seconds(&self) -> Option<f64> {
        let rate = self.parsed_format()?.sample_rate;
        let samples = self.sample_count?;
        (rate > 0).then(|| f64::from(u32::try_from(samples).unwrap_or(u32::MAX)) / f64::from(rate))
    }

    /// The export settings stored with the sound; the defaults when none were stored.
    #[must_use]
    pub fn export_settings(&self) -> SoundExport {
        let mut export = SoundExport::default();
        let Some(code) = self.extra.attr(EXPORT_FORMAT) else { return export };
        export.codec = SoundCodec::from_format_code(code);
        if let Some(rate) = self.extra.attr(EXPORT_RATE).and_then(|v| v.parse().ok()) {
            export.sample_rate = rate;
        }
        if let Some(bits) = self.extra.attr(EXPORT_BITS) {
            let number = bits.split_whitespace().next().and_then(|n| n.parse::<u32>().ok());
            if let Some(n) = number {
                match export.codec {
                    SoundCodec::Adpcm => export.adpcm_bits = u8::try_from(n).unwrap_or(4),
                    SoundCodec::Mp3 => export.mp3_kbps = n,
                    SoundCodec::Default | SoundCodec::Raw | SoundCodec::Speech => {}
                }
            }
            if export.codec == SoundCodec::Mp3 {
                export.mono = bits.contains("Mono");
            }
        }
        if export.codec != SoundCodec::Mp3 {
            export.mono = self.extra.attr(EXPORT_MONO) == Some("true");
        }
        if let Some(q) = self.extra.attr(EXPORT_QUALITY).and_then(SoundQuality::from_name) {
            export.quality = q;
        }
        export
    }

    /// Stores export settings. [`SoundCodec::Default`] removes them all.
    pub fn set_export_settings(&mut self, export: &SoundExport) {
        for name in [EXPORT_FORMAT, EXPORT_BITS, EXPORT_RATE, EXPORT_MONO, EXPORT_QUALITY] {
            self.extra.remove_attr(name);
        }
        let Some(code) = export.codec.format_code() else { return };
        self.extra.set_attr(EXPORT_FORMAT, code);
        match export.codec {
            SoundCodec::Adpcm => self.extra.set_attr(EXPORT_BITS, format!("{} bit", export.adpcm_bits)),
            SoundCodec::Mp3 => self.extra.set_attr(EXPORT_BITS, format!("{} kbps {}", export.mp3_kbps, if export.mono { "Mono" } else { "Stereo" })),
            SoundCodec::Default | SoundCodec::Raw | SoundCodec::Speech => {}
        }
        if export.codec != SoundCodec::Mp3 {
            self.extra.set_attr(EXPORT_RATE, export.sample_rate.to_string());
            self.extra.set_attr(EXPORT_MONO, if export.mono { "true" } else { "false" });
        }
        if matches!(export.codec, SoundCodec::Mp3 | SoundCodec::Speech) {
            self.extra.set_attr(EXPORT_QUALITY, export.quality.name());
        }
    }
}

/// Sets the export settings of the sound `name`.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `name` is not a sound; the errors of [`SoundExport::validate`].
pub fn set_sound_export(doc: &mut Document, name: &str, export: &SoundExport) -> Result<(), SceneError> {
    export.validate()?;
    let Some(LibraryItem::Sound(sound)) = doc.library.find_mut(name) else { return Err(SceneError::NoSuchItem(name.to_owned())) };
    sound.set_export_settings(export);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Video
// ---------------------------------------------------------------------------------------------------------------

impl VideoItem {
    /// Frames per second, when known.
    #[must_use]
    pub const fn frame_rate(&self) -> Option<f64> {
        self.fps
    }

    /// The length in seconds Animate recorded (`length`), when it did.
    #[must_use]
    pub fn duration_seconds(&self) -> Option<f64> {
        self.extra.attr("length").and_then(|v| v.parse().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_info_round_trips_through_extra() {
        let mut extra = Extra::new();
        assert_eq!(source_info(&extra), SourceInfo::default());
        let info = SourceInfo { path: Some("C:\\art\\pic.png".into()), imported: Some(1_700_000_000), size: Some(4096) };
        set_source_info(&mut extra, &info);
        assert_eq!(source_info(&extra), info);
        set_source_info(&mut extra, &SourceInfo::default());
        assert!(extra.is_empty());
    }

    #[test]
    fn bitmap_properties_apply_and_validate() {
        let mut d = Document::default();
        d.library.push(LibraryItem::Bitmap(BitmapItem::new("pic", "i1"))).unwrap();
        let props = BitmapProps { smoothing: Some(false), compression: Some(BitmapCompression::Photo), use_imported_jpeg_data: Some(true), quality: Some(80) };
        set_bitmap_properties(&mut d, "pic", &props).unwrap();
        let b = d.library.find_bitmap("pic").unwrap();
        assert!(!b.smoothing);
        assert_eq!(b.compression, BitmapCompression::Photo);
        assert!(b.uses_imported_jpeg_data());
        assert_eq!(b.quality, Some(80));
        assert!(set_bitmap_properties(&mut d, "pic", &BitmapProps { quality: Some(101), ..BitmapProps::default() }).is_err());
        assert!(set_bitmap_properties(&mut d, "missing", &BitmapProps::default()).is_err());
        d.library.push(LibraryItem::Sound(SoundItem::new("snd", "i2"))).unwrap();
        assert!(set_bitmap_properties(&mut d, "snd", &BitmapProps::default()).is_err(), "a sound is not a bitmap");
        // Unset fields stay.
        set_bitmap_properties(&mut d, "pic", &BitmapProps { smoothing: Some(true), ..BitmapProps::default() }).unwrap();
        let b = d.library.find_bitmap("pic").unwrap();
        assert!(b.smoothing && b.compression == BitmapCompression::Photo);
    }

    #[test]
    fn sound_formats_parse() {
        assert_eq!(parse_sound_format("44kHz 16bit Stereo"), Some(SoundFormat { sample_rate: 44_100, bits: 16, channels: 2 }));
        assert_eq!(parse_sound_format("22kHz 8bit Mono"), Some(SoundFormat { sample_rate: 22_050, bits: 8, channels: 1 }));
        assert_eq!(parse_sound_format("5kHz 16bit Mono").map(|f| f.sample_rate), Some(5512));
        assert_eq!(parse_sound_format("loud"), None);
        assert_eq!(parse_sound_format("44kHz 16bit Quad"), None);
        let mut s = SoundItem::new("s", "i");
        s.format = Some("22kHz 16bit Mono".into());
        s.sample_count = Some(44_100);
        assert!((s.duration_seconds().unwrap() - 2.0).abs() < 1e-9);
        s.format = None;
        assert_eq!(s.duration_seconds(), None);
    }

    #[test]
    fn sound_export_settings_round_trip_through_extra() {
        let mut s = SoundItem::new("s", "i");
        assert_eq!(s.export_settings(), SoundExport::default());
        let adpcm = SoundExport { codec: SoundCodec::Adpcm, sample_rate: 11_025, adpcm_bits: 3, mono: true, ..SoundExport::default() };
        s.set_export_settings(&adpcm);
        assert_eq!(s.export_settings(), adpcm);
        assert_eq!(s.extra.attr("exportFormat"), Some("1"));
        assert_eq!(s.extra.attr("exportBits"), Some("3 bit"));
        let mp3 = SoundExport { codec: SoundCodec::Mp3, mp3_kbps: 128, mono: false, quality: SoundQuality::Best, ..SoundExport::default() };
        s.set_export_settings(&mp3);
        let back = s.export_settings();
        assert_eq!((back.codec, back.mp3_kbps, back.mono, back.quality), (SoundCodec::Mp3, 128, false, SoundQuality::Best));
        assert_eq!(s.extra.attr("exportBits"), Some("128 kbps Stereo"));
        assert_eq!(s.extra.attr("exportSampleRate"), None, "MP3 keeps the sample rate of the source");
        let raw = SoundExport { codec: SoundCodec::Raw, sample_rate: 44_100, mono: false, ..SoundExport::default() };
        s.set_export_settings(&raw);
        assert_eq!(s.export_settings().sample_rate, 44_100);
        s.set_export_settings(&SoundExport::default());
        assert!(s.extra.is_empty(), "the default removes every export setting");
    }

    #[test]
    fn sound_export_is_validated() {
        let mut d = Document::default();
        d.library.push(LibraryItem::Sound(SoundItem::new("s", "i"))).unwrap();
        let bad_rate = SoundExport { sample_rate: 12_345, ..SoundExport::default() };
        assert!(set_sound_export(&mut d, "s", &bad_rate).is_err());
        assert!(set_sound_export(&mut d, "s", &SoundExport { adpcm_bits: 7, ..SoundExport::default() }).is_err());
        assert!(set_sound_export(&mut d, "s", &SoundExport { mp3_kbps: 99, ..SoundExport::default() }).is_err());
        assert!(set_sound_export(&mut d, "nope", &SoundExport::default()).is_err());
        set_sound_export(&mut d, "s", &SoundExport { codec: SoundCodec::Speech, ..SoundExport::default() }).unwrap();
        assert_eq!(d.library.find_sound("s").unwrap().export_settings().codec, SoundCodec::Speech);
    }

    #[test]
    fn codec_names_round_trip() {
        for c in SoundCodec::ALL {
            assert_eq!(SoundCodec::from_name(c.name()), Some(c));
            assert_eq!(SoundCodec::from_format_code(c.format_code().unwrap_or("0")), c);
        }
    }

    #[test]
    fn items_report_their_source() {
        let mut item = LibraryItem::Bitmap(BitmapItem::new("pic", "i"));
        assert_eq!(item.source(), SourceInfo::default());
        let info = SourceInfo { path: Some("pic.png".into()), imported: Some(5), size: None };
        item.set_source(&info);
        assert_eq!(item.source(), info);
        let mut folder = LibraryItem::Folder(crate::FolderItem { name: "f".into(), item_id: "i".into(), is_expanded: true, extra: Extra::new() });
        folder.set_source(&info);
        assert_eq!(folder.source(), SourceInfo::default());
    }

    #[test]
    fn video_reports_its_length() {
        let mut v = VideoItem {
            name: "v".into(),
            item_id: "i".into(),
            video_data_href: None,
            media: None,
            fps: Some(24.0),
            width: Some(10.0),
            height: Some(5.0),
            extra: Extra::new(),
        };
        assert_eq!(v.duration_seconds(), None);
        v.extra.set_attr("length", "2.5");
        assert_eq!(v.duration_seconds(), Some(2.5));
        assert_eq!(v.frame_rate(), Some(24.0));
    }
}

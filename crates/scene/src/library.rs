//! The library: symbols, bitmaps, sounds, fonts, video and folders, addressed by path-like names.

use std::collections::HashMap;

use anim_core::Rect;
use serde::{Deserialize, Serialize};

use crate::element::SymbolKind;
use crate::error::SceneError;
use crate::extra::Extra;
use crate::media::MediaId;
use crate::timeline::Timeline;

/// `ActionScript` linkage of a library item ("Export for `ActionScript`").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Linkage {
    /// `linkageExportForAS`.
    pub export_for_actionscript: bool,
    /// `linkageExportInFirstFrame` (Animate writes it as false only when explicitly unset, so `Option`).
    pub export_in_first_frame: Option<bool>,
    /// `linkageIdentifier`.
    pub identifier: Option<String>,
    /// `linkageClassName`.
    pub class_name: Option<String>,
    /// `linkageBaseClass`.
    pub base_class: Option<String>,
    /// `linkageImportForRS`.
    pub import_for_runtime_sharing: bool,
    /// `linkageURL`.
    pub url: Option<String>,
}

/// A symbol (`DOMSymbolItem`): a reusable timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    /// Library path, `folder/sub/name`.
    pub name: String,
    pub kind: SymbolKind,
    /// `itemID`, for example `5ed4e260-00000b9f`.
    pub item_id: String,
    /// Path of the symbol's file under `LIBRARY/` as the `<Include href>` names it, when it came from a file.
    pub href: Option<String>,
    /// Other attributes of the `<Include>` element (`itemIcon`, `loadImmediate`, `lastModified`, ...).
    pub include_attrs: Vec<(String, String)>,
    pub timeline: Timeline,
    pub linkage: Option<Linkage>,
    /// 9-slice scaling guides in the symbol's own pixels.
    pub scale9_grid: Option<Rect>,
    /// `lastModified`, seconds since the Unix epoch.
    pub last_modified: Option<u64>,
    pub extra: Extra,
}

impl Symbol {
    /// A symbol with an empty timeline named like itself.
    #[must_use]
    pub fn new(name: impl Into<String>, kind: SymbolKind, item_id: impl Into<String>) -> Self {
        let name = name.into();
        let leaf = name.rsplit('/').next().unwrap_or(&name).to_owned();
        Self {
            name,
            kind,
            item_id: item_id.into(),
            href: None,
            include_attrs: Vec::new(),
            timeline: Timeline::new(leaf),
            linkage: None,
            scale9_grid: None,
            last_modified: None,
            extra: Extra::new(),
        }
    }
}

/// How a bitmap is compressed on export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BitmapCompression {
    #[default]
    Lossless,
    /// JPEG (`originalCompressionType="photo"`).
    Photo,
}

impl BitmapCompression {
    /// The XFL `originalCompressionType` value.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::Lossless => "lossless",
            Self::Photo => "photo",
        }
    }

    /// Parses an XFL `originalCompressionType` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "lossless" => Some(Self::Lossless),
            "photo" => Some(Self::Photo),
            _ => None,
        }
    }
}

/// A bitmap (`DOMBitmapItem`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BitmapItem {
    pub name: String,
    pub item_id: String,
    /// `href`: the original file name.
    pub href: Option<String>,
    /// `bitmapDataHRef`: the `bin/*.dat` entry holding the pixels.
    pub bitmap_data_href: Option<String>,
    /// The loaded bytes in the session's [`crate::MediaStore`].
    pub media: Option<MediaId>,
    /// JPEG quality 0 ..= 100 (`quality`).
    pub quality: Option<u8>,
    pub compression: BitmapCompression,
    /// `allowSmoothing`.
    pub smoothing: bool,
    /// Pixel size when known (readers derive it from `frameRight`/`frameBottom` divided by 20, or from the data).
    pub size: Option<[f64; 2]>,
    pub extra: Extra,
}

impl BitmapItem {
    /// A bitmap item without data.
    #[must_use]
    pub fn new(name: impl Into<String>, item_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            item_id: item_id.into(),
            href: None,
            bitmap_data_href: None,
            media: None,
            quality: None,
            compression: BitmapCompression::Lossless,
            smoothing: true,
            size: None,
            extra: Extra::new(),
        }
    }
}

/// A sound (`DOMSoundItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundItem {
    pub name: String,
    pub item_id: String,
    pub href: Option<String>,
    /// `soundDataHRef`: the `bin/*.dat` entry.
    pub sound_data_href: Option<String>,
    /// `format`, for example `44kHz 16bit Stereo`.
    pub format: Option<String>,
    pub media: Option<MediaId>,
    /// `sampleCount`.
    pub sample_count: Option<u64>,
    pub extra: Extra,
}

impl SoundItem {
    /// A sound item without data.
    #[must_use]
    pub fn new(name: impl Into<String>, item_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            item_id: item_id.into(),
            href: None,
            sound_data_href: None,
            format: None,
            media: None,
            sample_count: None,
            extra: Extra::new(),
        }
    }
}

/// An embedded font (`DOMFontItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontItem {
    pub name: String,
    pub item_id: String,
    /// The font face it embeds (`font`).
    pub font: Option<String>,
    pub extra: Extra,
}

/// A video (`DOMVideoItem`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoItem {
    pub name: String,
    pub item_id: String,
    /// `videoDataHRef`.
    pub video_data_href: Option<String>,
    pub media: Option<MediaId>,
    pub fps: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub extra: Extra,
}

/// A library folder (`DOMFolderItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderItem {
    /// Folder path, `parent/child`.
    pub name: String,
    pub item_id: String,
    /// Shown open in the library panel (`isExpanded`).
    pub is_expanded: bool,
    pub extra: Extra,
}

/// One library entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LibraryItem {
    Symbol(Box<Symbol>),
    Bitmap(BitmapItem),
    Sound(SoundItem),
    Font(FontItem),
    Video(VideoItem),
    Folder(FolderItem),
}

impl LibraryItem {
    /// The library path of the item.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Symbol(s) => &s.name,
            Self::Bitmap(b) => &b.name,
            Self::Sound(s) => &s.name,
            Self::Font(f) => &f.name,
            Self::Video(v) => &v.name,
            Self::Folder(f) => &f.name,
        }
    }

    /// Renames the item (callers that must also fix references use [`crate::edit::rename_library_item`]).
    pub fn set_name(&mut self, name: String) {
        match self {
            Self::Symbol(s) => s.name = name,
            Self::Bitmap(b) => b.name = name,
            Self::Sound(s) => s.name = name,
            Self::Font(f) => f.name = name,
            Self::Video(v) => v.name = name,
            Self::Folder(f) => f.name = name,
        }
    }

    /// The `itemID` of the item.
    #[must_use]
    pub fn item_id(&self) -> &str {
        match self {
            Self::Symbol(s) => &s.item_id,
            Self::Bitmap(b) => &b.item_id,
            Self::Sound(s) => &s.item_id,
            Self::Font(f) => &f.item_id,
            Self::Video(v) => &v.item_id,
            Self::Folder(f) => &f.item_id,
        }
    }

    /// Short lowercase kind name for messages and tool output.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Symbol(_) => "symbol",
            Self::Bitmap(_) => "bitmap",
            Self::Sound(_) => "sound",
            Self::Font(_) => "font",
            Self::Video(_) => "video",
            Self::Folder(_) => "folder",
        }
    }

    /// The media blob this item points at, if it has one.
    #[must_use]
    pub const fn media(&self) -> Option<MediaId> {
        match self {
            Self::Bitmap(b) => b.media,
            Self::Sound(s) => s.media,
            Self::Video(v) => v.media,
            Self::Symbol(_) | Self::Font(_) | Self::Folder(_) => None,
        }
    }
}

/// All library items in file order. Names are unique per library (Animate enforces it for same-kind siblings; the
/// model enforces it for everything so a name always resolves to one item).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Library {
    pub items: Vec<LibraryItem>,
}

impl Library {
    /// An empty library.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Position of the item named `name`.
    #[must_use]
    pub fn find_index(&self, name: &str) -> Option<usize> {
        self.items.iter().position(|i| i.name() == name)
    }

    /// The item named `name`.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&LibraryItem> {
        self.items.iter().find(|i| i.name() == name)
    }

    /// Mutable access to the item named `name`.
    pub fn find_mut(&mut self, name: &str) -> Option<&mut LibraryItem> {
        self.items.iter_mut().find(|i| i.name() == name)
    }

    /// The symbol named `name`.
    #[must_use]
    pub fn find_symbol(&self, name: &str) -> Option<&Symbol> {
        self.items.iter().find_map(|i| match i {
            LibraryItem::Symbol(s) if s.name == name => Some(&**s),
            _ => None,
        })
    }

    /// Mutable access to the symbol named `name`.
    pub fn find_symbol_mut(&mut self, name: &str) -> Option<&mut Symbol> {
        self.items.iter_mut().find_map(|i| match i {
            LibraryItem::Symbol(s) if s.name == name => Some(&mut **s),
            _ => None,
        })
    }

    /// The bitmap named `name`.
    #[must_use]
    pub fn find_bitmap(&self, name: &str) -> Option<&BitmapItem> {
        self.items.iter().find_map(|i| match i {
            LibraryItem::Bitmap(b) if b.name == name => Some(b),
            _ => None,
        })
    }

    /// The sound named `name`.
    #[must_use]
    pub fn find_sound(&self, name: &str) -> Option<&SoundItem> {
        self.items.iter().find_map(|i| match i {
            LibraryItem::Sound(s) if s.name == name => Some(s),
            _ => None,
        })
    }

    /// Every symbol.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.items.iter().filter_map(|i| match i {
            LibraryItem::Symbol(s) => Some(&**s),
            _ => None,
        })
    }

    /// Every bitmap.
    pub fn bitmaps(&self) -> impl Iterator<Item = &BitmapItem> {
        self.items.iter().filter_map(|i| match i {
            LibraryItem::Bitmap(b) => Some(b),
            _ => None,
        })
    }

    /// Every sound.
    pub fn sounds(&self) -> impl Iterator<Item = &SoundItem> {
        self.items.iter().filter_map(|i| match i {
            LibraryItem::Sound(s) => Some(s),
            _ => None,
        })
    }

    /// Every embedded font (Document's `fonts`).
    pub fn fonts(&self) -> impl Iterator<Item = &FontItem> {
        self.items.iter().filter_map(|i| match i {
            LibraryItem::Font(f) => Some(f),
            _ => None,
        })
    }

    /// Every library folder (Document's `folders`).
    pub fn folders(&self) -> impl Iterator<Item = &FolderItem> {
        self.items.iter().filter_map(|i| match i {
            LibraryItem::Folder(f) => Some(f),
            _ => None,
        })
    }

    /// Name to position map for many lookups at once (a renderer resolving every instance of a frame).
    #[must_use]
    pub fn index(&self) -> HashMap<&str, usize> {
        let mut map = HashMap::with_capacity(self.items.len());
        for (i, item) in self.items.iter().enumerate() {
            map.entry(item.name()).or_insert(i);
        }
        map
    }

    /// Adds an item.
    ///
    /// # Errors
    /// [`SceneError::NameTaken`] when the name exists, [`SceneError::InvalidName`] when it is empty or has an empty
    /// path segment.
    pub fn push(&mut self, item: LibraryItem) -> Result<(), SceneError> {
        validate_item_name(item.name())?;
        if self.find_index(item.name()).is_some() {
            return Err(SceneError::NameTaken(item.name().to_owned()));
        }
        self.items.push(item);
        Ok(())
    }

    /// Removes and returns the item named `name`.
    pub fn remove(&mut self, name: &str) -> Option<LibraryItem> {
        let pos = self.find_index(name)?;
        Some(self.items.remove(pos))
    }

    /// Names of the folders `name` lives in, outermost first: `a/b/c` gives `a`, `a/b`.
    #[must_use]
    pub fn parent_folder_paths(name: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (i, ch) in name.char_indices() {
            if ch == '/' {
                out.push(name.get(..i).unwrap_or_default().to_owned());
            }
        }
        out
    }

    /// An `itemID` of the form `xxxxxxxx-yyyyyyyy` not used by any item, derived from `seed` so the same edit on the
    /// same document always yields the same id.
    #[must_use]
    pub fn fresh_item_id(&self, seed: &str) -> String {
        let mut counter = u64::try_from(self.items.len()).unwrap_or(0);
        loop {
            let high = MediaId::of(seed.as_bytes()).0.wrapping_add(counter);
            let id = format!("{:08x}-{:08x}", high >> 32, high & 0xffff_ffff);
            if !self.items.iter().any(|i| i.item_id() == id) {
                return id;
            }
            counter = counter.wrapping_add(1);
        }
    }
}

/// Checks a library path: non-empty, no empty segments, no control characters.
///
/// # Errors
/// [`SceneError::InvalidName`].
pub fn validate_item_name(name: &str) -> Result<(), SceneError> {
    if name.is_empty() || name.split('/').any(str::is_empty) || name.chars().any(char::is_control) {
        return Err(SceneError::InvalidName(name.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str) -> LibraryItem {
        LibraryItem::Symbol(Box::new(Symbol::new(name, SymbolKind::Graphic, "id")))
    }

    #[test]
    fn lookup_and_uniqueness() {
        let mut lib = Library::new();
        lib.push(sym("a")).unwrap();
        lib.push(sym("f/b")).unwrap();
        assert_eq!(lib.push(sym("a")), Err(SceneError::NameTaken("a".into())));
        assert!(lib.push(sym("")).is_err());
        assert!(lib.push(sym("x//y")).is_err());
        assert!(lib.find_symbol("f/b").is_some());
        assert!(lib.find_bitmap("a").is_none());
        assert_eq!(lib.symbols().count(), 2);
        assert_eq!(lib.index().get("f/b"), Some(&1));
        assert_eq!(lib.find_symbol("f/b").map(|s| s.timeline.name.as_str()), Some("b"));
        assert!(lib.remove("a").is_some());
        assert!(lib.remove("a").is_none());
    }

    #[test]
    fn folder_paths() {
        assert_eq!(Library::parent_folder_paths("a/b/c"), vec!["a".to_owned(), "a/b".to_owned()]);
        assert!(Library::parent_folder_paths("x").is_empty());
    }

    #[test]
    fn fresh_ids_are_unique_and_deterministic() {
        let mut lib = Library::new();
        let a = lib.fresh_item_id("seed");
        assert_eq!(a, lib.fresh_item_id("seed"));
        let mut item = Symbol::new("s", SymbolKind::Graphic, a.clone());
        item.item_id.clone_from(&a);
        lib.push(LibraryItem::Symbol(Box::new(item))).unwrap();
        assert_ne!(a, lib.fresh_item_id("seed"));
    }

    #[test]
    fn typed_iterators_and_media() {
        let mut lib = Library::new();
        let mut b = BitmapItem::new("pic", "i1");
        b.media = Some(MediaId(5));
        lib.push(LibraryItem::Bitmap(b)).unwrap();
        lib.push(LibraryItem::Sound(SoundItem::new("snd", "i2"))).unwrap();
        lib.push(LibraryItem::Folder(FolderItem { name: "fold".into(), item_id: "i3".into(), is_expanded: true, extra: Extra::new() })).unwrap();
        lib.push(LibraryItem::Font(FontItem { name: "fnt".into(), item_id: "i4".into(), font: None, extra: Extra::new() })).unwrap();
        assert_eq!(lib.bitmaps().count(), 1);
        assert_eq!(lib.sounds().count(), 1);
        assert_eq!(lib.folders().count(), 1);
        assert_eq!(lib.fonts().count(), 1);
        assert_eq!(lib.find("pic").and_then(LibraryItem::media), Some(MediaId(5)));
        assert_eq!(lib.find("fold").map(LibraryItem::kind_name), Some("folder"));
        assert!(lib.find_sound("snd").is_some());
    }
}

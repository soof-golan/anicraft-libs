//! Test-only helpers: a hand-rolled zip builder that can produce the malformed shapes the readers must survive.

#![cfg(test)]
#![allow(dead_code)]

use flate2::Compression;
use flate2::write::DeflateEncoder;
use std::io::Write;

/// How the data descriptor after an entry's data looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Descriptor {
    /// No descriptor: sizes and CRC are in the local header.
    None,
    /// `PK\x07\x08` + CRC + 32-bit sizes.
    Signed,
    /// CRC + 32-bit sizes without the signature.
    Unsigned,
    /// `PK\x07\x08` + CRC + 64-bit sizes (Zip64 entries).
    Signed64,
}

#[derive(Clone, Debug)]
pub struct RawEntry {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub method: u16,
    pub flags: u16,
    pub descriptor: Descriptor,
    /// Overrides the CRC written to the central directory (and local header when there is no descriptor).
    pub crc_override: Option<u32>,
    /// Overrides the uncompressed size written to the central directory.
    pub size_override: Option<u32>,
    pub external_attributes: u32,
}

impl RawEntry {
    pub fn stored(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            method: 0,
            flags: 0x0800,
            descriptor: Descriptor::None,
            crc_override: None,
            size_override: None,
            external_attributes: 0,
        }
    }

    pub fn deflated(name: &str, data: &[u8]) -> Self {
        Self { method: 8, ..Self::stored(name, data) }
    }

    pub fn with_descriptor(mut self, descriptor: Descriptor) -> Self {
        self.descriptor = descriptor;
        if descriptor != Descriptor::None {
            self.flags |= 0x0008;
        }
        self
    }

    pub fn with_flags(mut self, flags: u16) -> Self {
        self.flags |= flags;
        self
    }

    pub fn with_name_bytes(mut self, name: &[u8]) -> Self {
        self.name = name.to_vec();
        self.flags &= !0x0800;
        self
    }

    pub fn directory(name: &str) -> Self {
        Self { external_attributes: 0x10, ..Self::stored(name, b"") }
    }
}

pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// What to build.
#[derive(Clone, Debug)]
pub struct RawZip {
    pub entries: Vec<RawEntry>,
    /// Write the central directory and end record.
    pub central: bool,
    /// Added to the central directory size recorded in the end record (Animate's files need +54).
    pub cd_size_delta: i64,
    /// Bytes put before the archive (a self-extracting stub); offsets stay relative to the archive start.
    pub prefix: Vec<u8>,
    /// Archive comment after the end record.
    pub comment: Vec<u8>,
}

impl RawZip {
    pub fn new(entries: Vec<RawEntry>) -> Self {
        Self { entries, central: true, cd_size_delta: 0, prefix: Vec::new(), comment: Vec::new() }
    }

    pub fn build(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut offsets = Vec::new();
        let mut compressed_sizes = Vec::new();
        for entry in &self.entries {
            let payload = if entry.method == 8 { deflate(&entry.data) } else { entry.data.clone() };
            let crc = crc32fast::hash(&entry.data);
            offsets.push(out.len());
            compressed_sizes.push(payload.len());
            let has_descriptor = entry.descriptor != Descriptor::None;
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20_u16.to_le_bytes());
            out.extend_from_slice(&entry.flags.to_le_bytes());
            out.extend_from_slice(&entry.method.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0x21, 0]);
            let (header_crc, header_compressed, header_size) = if has_descriptor {
                (0, 0, 0)
            } else {
                (
                    entry.crc_override.unwrap_or(crc),
                    u32::try_from(payload.len()).unwrap(),
                    entry.size_override.unwrap_or_else(|| u32::try_from(entry.data.len()).unwrap()),
                )
            };
            out.extend_from_slice(&header_crc.to_le_bytes());
            out.extend_from_slice(&header_compressed.to_le_bytes());
            out.extend_from_slice(&header_size.to_le_bytes());
            out.extend_from_slice(&u16::try_from(entry.name.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&0_u16.to_le_bytes());
            out.extend_from_slice(&entry.name);
            out.extend_from_slice(&payload);
            match entry.descriptor {
                Descriptor::None => {}
                Descriptor::Signed | Descriptor::Unsigned => {
                    if entry.descriptor == Descriptor::Signed {
                        out.extend_from_slice(b"PK\x07\x08");
                    }
                    out.extend_from_slice(&crc.to_le_bytes());
                    out.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
                    out.extend_from_slice(&u32::try_from(entry.data.len()).unwrap().to_le_bytes());
                }
                Descriptor::Signed64 => {
                    out.extend_from_slice(b"PK\x07\x08");
                    out.extend_from_slice(&crc.to_le_bytes());
                    out.extend_from_slice(&u64::try_from(payload.len()).unwrap().to_le_bytes());
                    out.extend_from_slice(&u64::try_from(entry.data.len()).unwrap().to_le_bytes());
                }
            }
        }
        if self.central {
            let directory_start = out.len();
            for (index, entry) in self.entries.iter().enumerate() {
                let crc = entry.crc_override.unwrap_or_else(|| crc32fast::hash(&entry.data));
                out.extend_from_slice(b"PK\x01\x02");
                out.extend_from_slice(&20_u16.to_le_bytes());
                out.extend_from_slice(&20_u16.to_le_bytes());
                out.extend_from_slice(&entry.flags.to_le_bytes());
                out.extend_from_slice(&entry.method.to_le_bytes());
                out.extend_from_slice(&[0, 0, 0x21, 0]);
                out.extend_from_slice(&crc.to_le_bytes());
                out.extend_from_slice(&u32::try_from(*compressed_sizes.get(index).unwrap()).unwrap().to_le_bytes());
                out.extend_from_slice(&entry.size_override.unwrap_or_else(|| u32::try_from(entry.data.len()).unwrap()).to_le_bytes());
                out.extend_from_slice(&u16::try_from(entry.name.len()).unwrap().to_le_bytes());
                out.extend_from_slice(&[0; 2 + 2 + 2 + 2]);
                out.extend_from_slice(&entry.external_attributes.to_le_bytes());
                out.extend_from_slice(&u32::try_from(*offsets.get(index).unwrap()).unwrap().to_le_bytes());
                out.extend_from_slice(&entry.name);
            }
            let directory_size = out.len() - directory_start;
            out.extend_from_slice(b"PK\x05\x06");
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&u16::try_from(self.entries.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&u16::try_from(self.entries.len()).unwrap().to_le_bytes());
            let declared = i64::try_from(directory_size).unwrap() + self.cd_size_delta;
            out.extend_from_slice(&u32::try_from(declared).unwrap().to_le_bytes());
            out.extend_from_slice(&u32::try_from(directory_start).unwrap().to_le_bytes());
            out.extend_from_slice(&u16::try_from(self.comment.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&self.comment);
        }
        let mut with_prefix = self.prefix.clone();
        with_prefix.extend(out);
        with_prefix
    }
}

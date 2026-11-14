//! Minimal ZIP container for modern FLA (XFL). No entries are extracted to disk.
use std::io::{Read, Write};
const LIMIT: usize = 64 * 1024 * 1024;
fn u16_at(b: &[u8], p: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        b.get(p..p + 2).ok_or("ZIP truncado")?.try_into().unwrap(),
    ))
}
fn u32_at(b: &[u8], p: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4).ok_or("ZIP truncado")?.try_into().unwrap(),
    ))
}
fn push16(b: &mut Vec<u8>, v: u16) {
    b.extend(v.to_le_bytes());
}
fn push32(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_le_bytes());
}
#[cfg(test)]
pub fn pack(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let entries: Vec<_> = entries
        .iter()
        .map(|(name, bytes)| (*name, bytes.as_slice()))
        .collect();
    pack_slices(&entries)
}
#[cfg(test)]
pub fn pack_slices(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    write_entries(&mut out, entries, |_| None, None).unwrap();
    out
}

pub(crate) fn write_compressed(
    out: &mut impl std::io::Write,
    entries: &[(&str, &[u8])],
    compression: impl Fn(&str) -> flate2::Compression,
    recovery: Option<&mut RecoveryCompression>,
) -> std::io::Result<()> {
    write_entries(out, entries, |name| Some(compression(name)), recovery)
}

fn write_entries(
    out: &mut impl std::io::Write,
    entries: &[(&str, &[u8])],
    compression: impl Fn(&str) -> Option<flate2::Compression>,
    mut recovery: Option<&mut RecoveryCompression>,
) -> std::io::Result<()> {
    let mut offset = 0u32;
    let mut header = Vec::new();
    let mut central = vec![];
    for (name, data) in entries {
        header.clear();
        let crc = crc32fast::hash(data);
        let size = data.len() as u32;
        let compressed = if data.len() >= 256
            && let Some(level) = compression(name)
        {
            if let Some(cache) = recovery.as_deref_mut().filter(|_| {
                name.starts_with("LIBRETOON/media/") || name.starts_with("LIBRETOON/references/")
            }) {
                cache.compress(crc, data, level)?
            } else {
                deflate(data, level)?.into()
            }
        } else {
            std::sync::Arc::default()
        };
        let (method, payload) = if !compressed.is_empty() && compressed.len() < data.len() {
            (8, compressed.as_ref())
        } else {
            (0, *data)
        };
        let packed_size = payload.len() as u32;
        push32(&mut header, 0x04034b50);
        for n in [20, 0x800, method, 0, 33] {
            push16(&mut header, n);
        }
        push32(&mut header, crc);
        push32(&mut header, packed_size);
        push32(&mut header, size);
        push16(&mut header, name.len() as u16);
        push16(&mut header, 0);
        header.extend(name.as_bytes());
        out.write_all(&header)?;
        out.write_all(payload)?;
        push32(&mut central, 0x02014b50);
        for n in [20, 20, 0x800, method, 0, 33] {
            push16(&mut central, n);
        }
        push32(&mut central, crc);
        push32(&mut central, packed_size);
        push32(&mut central, size);
        push16(&mut central, name.len() as u16);
        for _ in 0..4 {
            push16(&mut central, 0);
        }
        push32(&mut central, 0);
        push32(&mut central, offset);
        central.extend(name.as_bytes());
        offset += header.len() as u32 + packed_size;
    }
    let start = offset;
    header.clear();
    let size = central.len() as u32;
    out.write_all(&central)?;
    push32(&mut header, 0x06054b50);
    push16(&mut header, 0);
    push16(&mut header, 0);
    push16(&mut header, entries.len() as u16);
    push16(&mut header, entries.len() as u16);
    push32(&mut header, size);
    push32(&mut header, start);
    push16(&mut header, 0);
    out.write_all(&header)
}
fn deflate(data: &[u8], level: flate2::Compression) -> std::io::Result<Vec<u8>> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), level);
    encoder.write_all(data)?;
    encoder.finish()
}

struct RecoveryEntry {
    crc: u32,
    level: u32,
    raw: Vec<u8>,
    compressed: std::sync::Arc<[u8]>,
}

#[derive(Default)]
pub(crate) struct RecoveryCompression {
    entries: std::collections::VecDeque<RecoveryEntry>,
    bytes: usize,
    #[cfg(test)]
    hits: usize,
}

impl RecoveryCompression {
    fn compress(
        &mut self,
        crc: u32,
        data: &[u8],
        level: flate2::Compression,
    ) -> std::io::Result<std::sync::Arc<[u8]>> {
        const LIMIT: usize = 48 * 1024 * 1024;
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.crc == crc && entry.level == level.level() && entry.raw == data)
        {
            let entry = self.entries.remove(index).unwrap();
            let result = entry.compressed.clone();
            self.entries.push_back(entry);
            #[cfg(test)]
            {
                self.hits += 1;
            }
            return Ok(result);
        }
        let compressed: std::sync::Arc<[u8]> = deflate(data, level)?.into();
        let bytes = data.len() + compressed.len();
        if bytes <= LIMIT {
            while self.bytes + bytes > LIMIT || self.entries.len() >= 2048 {
                let expired = self.entries.pop_front().unwrap();
                self.bytes -= expired.raw.len() + expired.compressed.len();
            }
            self.bytes += bytes;
            self.entries.push_back(RecoveryEntry {
                crc,
                level: level.level(),
                raw: data.to_vec(),
                compressed: compressed.clone(),
            });
        }
        Ok(compressed)
    }
}

#[cfg(test)]
#[test]
fn recovery_reuses_unchanged_entries_and_writes_a_self_contained_zip() {
    let mut cache = RecoveryCompression::default();
    let name = "LIBRETOON/media/1";
    let original = vec![3u8; 1024];
    let changed = vec![8u8; 1024];
    for (index, data) in [&original, &original, &changed].into_iter().enumerate() {
        let mut archive = Vec::new();
        write_compressed(
            &mut archive,
            &[(name, data.as_slice())],
            |_| flate2::Compression::default(),
            Some(&mut cache),
        )
        .unwrap();
        let decoded = Archive::new(&archive)
            .unwrap()
            .entry_limited(name.as_bytes(), 1024)
            .unwrap()
            .unwrap();
        assert_eq!(&decoded, data);
        assert_eq!(cache.hits, usize::from(index > 0));
    }
}

pub fn document(bytes: &[u8]) -> Result<String, String> {
    xml_text(entry(bytes, b"DOMDocument.xml")?.ok_or("No se encuentra DOMDocument.xml en el FLA")?)
}
pub fn xml_text(data: Vec<u8>) -> Result<String, String> {
    if data.starts_with(&[0xff, 0xfe]) || data.starts_with(&[0xfe, 0xff]) {
        if data.len() % 2 != 0 {
            return Err("XML UTF-16 truncado".into());
        }
        let little = data[0] == 0xff;
        let words = data[2..].chunks_exact(2).map(|c| {
            if little {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        });
        let text = char::decode_utf16(words)
            .collect::<Result<String, _>>()
            .map_err(|e| e.to_string())?;
        Ok(text
            .replacen("encoding=\"UTF-16\"", "encoding=\"UTF-8\"", 1)
            .replacen("encoding=\"utf-16\"", "encoding=\"utf-8\"", 1))
    } else {
        String::from_utf8(data).map_err(|e| e.to_string())
    }
}
/// Central directory is indexed once when importing a library.
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: std::collections::HashMap<Vec<u8>, usize>,
    directory_size_recovered: bool,
}
impl<'a> Archive<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, String> {
        let end=(bytes.len().saturating_sub(65557)..bytes.len().saturating_sub(3)).rev().find(|&i|bytes.get(i..i+4)==Some(&[0x50,0x4b,0x05,0x06])&&u16_at(bytes,i+20).is_ok_and(|len|i+22+len as usize==bytes.len())).ok_or("Este archivo no es un FLA moderno (CS5 o posterior). Los FLA binarios antiguos no están admitidos.")?;
        if u16_at(bytes, end + 4)? != 0 || u16_at(bytes, end + 6)? != 0 {
            return Err("ZIP dividido no admitido".into());
        }
        let count = u16_at(bytes, end + 10)?;
        if count != u16_at(bytes, end + 8)? {
            return Err("Directorio ZIP incoherente".into());
        }
        let mut p = u32_at(bytes, end + 16)? as usize;
        let start = p;
        let declared_size = u32_at(bytes, end + 12)? as usize;
        if start > end {
            return Err("Directorio ZIP fuera de rango".into());
        }
        // Parse the declared number of records against the physical EOCD boundary.
        // Animate 24 files can overstate the directory size by 54 bytes.
        let stop = end;
        let mut entries = std::collections::HashMap::new();
        for _ in 0..count {
            if u32_at(bytes, p)? != 0x02014b50 {
                return Err("Directorio FLA dañado".into());
            }
            let len = u16_at(bytes, p + 28)? as usize;
            let extra = u16_at(bytes, p + 30)? as usize;
            let comment = u16_at(bytes, p + 32)? as usize;
            let name = bytes
                .get(p + 46..p + 46 + len)
                .ok_or("Nombre ZIP truncado")?;
            if entries.insert(name.to_vec(), p).is_some() {
                return Err("Entrada ZIP duplicada".into());
            }
            p = p
                .checked_add(46 + len + extra + comment)
                .filter(|p| *p <= stop)
                .ok_or("Directorio ZIP truncado")?;
        }
        if p != stop {
            return Err("Tamaño de directorio ZIP incoherente".into());
        }
        let actual_size = p - start;
        let directory_size_recovered = declared_size != actual_size;
        if directory_size_recovered
            && !(declared_size == actual_size + 54 && entries.contains_key(b"mimetype".as_slice()))
        {
            return Err("Tamaño de directorio ZIP incoherente".into());
        }
        Ok(Self {
            bytes,
            entries,
            directory_size_recovered,
        })
    }
    pub fn directory_size_recovered(&self) -> bool {
        self.directory_size_recovered
    }
    pub fn names(&self) -> impl Iterator<Item = &[u8]> {
        self.entries.keys().map(Vec::as_slice)
    }
    pub fn entry(&self, wanted: &[u8]) -> Result<Option<Vec<u8>>, String> {
        self.entry_limited(wanted, LIMIT)
    }
    pub(crate) fn entry_limited(
        &self,
        wanted: &[u8],
        limit: usize,
    ) -> Result<Option<Vec<u8>>, String> {
        let Some(&p) = self.entries.get(wanted) else {
            return Ok(None);
        };
        let bytes = self.bytes;
        let name = wanted;
        let flags = u16_at(bytes, p + 8)?;
        let method = u16_at(bytes, p + 10)?;
        let crc = u32_at(bytes, p + 16)?;
        let compressed = u32_at(bytes, p + 20)? as usize;
        let uncompressed = u32_at(bytes, p + 24)? as usize;
        let local = u32_at(bytes, p + 42)? as usize;
        if flags & 1 != 0 || uncompressed > limit || compressed > limit {
            return Err("FLA cifrado o demasiado grande".into());
        }
        if u32_at(bytes, local)? != 0x04034b50 {
            return Err("Cabecera ZIP dañada".into());
        }
        let local_name_len = u16_at(bytes, local + 26)? as usize;
        if u16_at(bytes, local + 6)? != flags
            || u16_at(bytes, local + 8)? != method
            || bytes.get(local + 30..local + 30 + local_name_len) != Some(name)
        {
            return Err("Cabecera local ZIP no coincide con el directorio".into());
        }
        let start =
            local + 30 + u16_at(bytes, local + 26)? as usize + u16_at(bytes, local + 28)? as usize;
        let data = bytes
            .get(start..start + compressed)
            .ok_or("Contenido FLA truncado")?;
        let decoded = match method {
            0 => data.to_vec(),
            8 => {
                let mut out = vec![];
                flate2::read::DeflateDecoder::new(data)
                    .take((limit + 1) as u64)
                    .read_to_end(&mut out)
                    .map_err(|e| e.to_string())?;
                out
            }
            _ => return Err("Compresión FLA no admitida".into()),
        };
        if decoded.len() != uncompressed || crc32fast::hash(&decoded) != crc {
            return Err("DOMDocument.xml dañado".into());
        }
        Ok(Some(decoded))
    }
}
pub fn entry(bytes: &[u8], wanted: &[u8]) -> Result<Option<Vec<u8>>, String> {
    Archive::new(bytes)?.entry(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_round_trip_and_corruption() {
        let b = pack(&[("DOMDocument.xml", b"<DOMDocument/>".to_vec())]);
        assert_eq!(document(&b).unwrap(), "<DOMDocument/>");
        let mut corrupt = b;
        corrupt[45] ^= 1;
        assert!(document(&corrupt).is_err());
    }
}

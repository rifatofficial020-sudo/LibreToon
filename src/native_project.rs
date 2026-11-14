//! Versioned LibreToon project container. No interchange preview is required.
use crate::{Document, fla_zip::Archive};
const DOCUMENT: &str = "LIBRETOON/document.json";
const MANIFEST: &str = "LIBRETOON/manifest.json";
pub const EXTENSION: &str = "ltoon";
pub const LIMIT: usize = 256 * 1024 * 1024;
const MANIFEST_V1: &[u8] = br#"{"format":"LibreToon","version":1}"#;
const MANIFEST_V2: &[u8] = br#"{"format":"LibreToon","version":2}"#;
const LEGACY_DOCUMENT_LIMIT: usize = 64 * 1024 * 1024;
// Switch before the legacy ceiling so there is room for continued editing.
const COMPACT_THRESHOLD: usize = 1024 * 1024;
const COMPACT_DOCUMENT_LIMIT: usize = 128 * 1024 * 1024;

#[derive(serde::Deserialize)]
struct Manifest {
    format: String,
    version: u32,
}

pub fn is_native_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
}

#[cfg(test)]
pub fn encode(doc: &Document) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    write(doc, &mut bytes)?;
    Ok(bytes)
}

pub fn write(doc: &Document, writer: &mut impl std::io::Write) -> Result<(), String> {
    write_with_recovery_cache(doc, writer, None)
}

pub(crate) fn write_with_recovery_cache(
    doc: &Document,
    writer: &mut impl std::io::Write,
    recovery: Option<&mut crate::fla_zip::RecoveryCompression>,
) -> Result<(), String> {
    let mut snapshot = doc.clone();
    crate::raster::prune(&mut snapshot);
    snapshot.validate()?;
    let metadata = without_media_payloads(&snapshot);
    let legacy = if legacy_cels_exceed_threshold(&metadata) {
        None
    } else {
        bounded_json(&metadata, COMPACT_THRESHOLD)?
    };
    let (manifest, native) = match legacy {
        Some(native) => (MANIFEST_V1, native),
        None => {
            let native = cels::compact(|| bounded_json(&metadata, COMPACT_DOCUMENT_LIMIT))?
                .ok_or("Los datos del proyecto superan el límite de 128 MiB incluso sin repetir fotogramas")?;
            (MANIFEST_V2, native)
        }
    };
    write_document(
        &snapshot,
        vec![(MANIFEST, manifest)],
        &native,
        writer,
        recovery,
    )
}

// A lower bound only selects the compact path when the legacy writer would
// certainly hit the threshold. Count repeated cels by identity.
fn legacy_cels_exceed_threshold(doc: &Document) -> bool {
    let mut sizes = std::collections::HashMap::new();
    let mut bytes = 0usize;
    let layers = doc
        .layers
        .iter()
        .chain(
            doc.extras
                .scenes
                .iter()
                .flat_map(|scene| scene.layers.iter()),
        )
        .chain(
            doc.extras
                .symbols
                .iter()
                .flat_map(|symbol| symbol.layers.iter()),
        );
    for frame in layers.flat_map(|layer| layer.frames.iter()) {
        let size = sizes.entry(frame.identity()).or_insert_with(|| {
            frame.iter().fold(0usize, |total, mark| {
                total
                    .saturating_add(40)
                    .saturating_add(mark.points.len().saturating_mul(5))
                    .saturating_add(mark.nodes.len().saturating_mul(30))
            })
        });
        bytes = bytes.saturating_add(*size);
        if bytes > COMPACT_THRESHOLD {
            return true;
        }
    }
    false
}

fn without_media_payloads(doc: &Document) -> Document {
    let mut metadata = doc.clone();
    for asset in &mut metadata.extras.media.assets {
        asset.data = Default::default();
    }
    for reference in &mut metadata.extras.references {
        reference.data = Default::default();
    }
    metadata
}

fn bounded_json(doc: &Document, limit: usize) -> Result<Option<Vec<u8>>, String> {
    struct Output {
        bytes: Vec<u8>,
        limit: usize,
        overflow: bool,
    }
    impl std::io::Write for Output {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if data.len() > self.limit - self.bytes.len() {
                self.overflow = true;
                return Err(std::io::Error::other("Project metadata limit exceeded"));
            }
            self.bytes.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Output {
        bytes: Vec::new(),
        limit,
        overflow: false,
    };
    match serde_json::to_writer(&mut output, doc) {
        Ok(()) => Ok(Some(output.bytes)),
        Err(_) if output.overflow => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn pack(doc: &Document, header: Vec<(&str, &[u8])>) -> Result<Vec<u8>, String> {
    let native = bounded_json(&without_media_payloads(doc), LEGACY_DOCUMENT_LIMIT)?
        .ok_or("Proyecto demasiado grande para el contenedor de proyecto")?;
    let mut bytes = Vec::new();
    write_document(doc, header, &native, &mut bytes, None)?;
    Ok(bytes)
}

fn write_document(
    doc: &Document,
    header: Vec<(&str, &[u8])>,
    native: &[u8],
    writer: &mut impl std::io::Write,
    recovery: Option<&mut crate::fla_zip::RecoveryCompression>,
) -> Result<(), String> {
    let names: Vec<_> = doc
        .extras
        .media
        .assets
        .iter()
        .map(|asset| media_entry(asset.id))
        .collect();
    let mut entries = Vec::new();
    entries.extend(header);
    entries.push((DOCUMENT, native));
    entries.extend(
        names
            .iter()
            .zip(doc.extras.media.assets.iter())
            .map(|(name, asset)| (name.as_str(), asset.data.as_slice())),
    );
    let reference_names: Vec<_> = doc
        .extras
        .references
        .iter()
        .filter(|r| !r.data.is_empty())
        .map(|r| format!("LIBRETOON/references/{}.image", r.id))
        .collect();
    entries.extend(
        reference_names
            .iter()
            .zip(doc.extras.references.iter().filter(|r| !r.data.is_empty()))
            .map(|(name, r)| (name.as_str(), r.data.as_slice())),
    );
    let size = entries.iter().try_fold(22usize, |sum, (name, data)| {
        sum.checked_add(76 + name.len() * 2)?
            .checked_add(data.len())
    });
    if entries.len() > u16::MAX as usize || size.is_none_or(|size| size > LIMIT) {
        return Err("El proyecto supera el límite de 256 MiB".into());
    }
    let raster_names: std::collections::HashSet<_> = names
        .iter()
        .zip(doc.extras.media.assets.iter())
        .filter_map(|(name, asset)| asset.raster_tile.then_some(name.as_str()))
        .collect();
    crate::fla_zip::write_compressed(
        writer,
        &entries,
        |name| {
            if name == DOCUMENT || raster_names.contains(name) {
                flate2::Compression::default()
            } else {
                flate2::Compression::fast()
            }
        },
        recovery,
    )
    .map_err(|error| error.to_string())
}

fn media_entry(id: u64) -> String {
    format!("LIBRETOON/media/{id}")
}

pub fn decode_if_native(bytes: &[u8]) -> Result<Option<Document>, String> {
    let archive = Archive::new(bytes)?;
    let Some(manifest) = archive.entry_limited(MANIFEST.as_bytes(), 4096)? else {
        return Ok(None);
    };
    if archive
        .names()
        .any(|name| name.eq_ignore_ascii_case(b"DOMDocument.xml"))
    {
        return Err("A native LibreToon project cannot contain an XFL preview".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&manifest).map_err(|error| error.to_string())?;
    if manifest.format != "LibreToon" || !matches!(manifest.version, 1 | 2) {
        return Err(format!(
            "Unsupported LibreToon project format or version ({})",
            manifest.version
        ));
    }
    let limit = if manifest.version == 1 {
        LEGACY_DOCUMENT_LIMIT
    } else {
        COMPACT_DOCUMENT_LIMIT
    };
    let document = archive
        .entry_limited(DOCUMENT.as_bytes(), limit)?
        .ok_or("Missing LibreToon document")?;
    restore(&archive, &document).map(Some)
}

pub(crate) fn restore(archive: &Archive<'_>, bytes: &[u8]) -> Result<Document, String> {
    let mut doc: Document = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let mut budget = LIMIT.saturating_sub(bytes.len());
    let mut read = |path: &str, limit: usize| -> Result<crate::shared::SharedVec<u8>, String> {
        let data = archive
            .entry_limited(path.as_bytes(), budget.min(limit))?
            .ok_or_else(|| format!("Missing project resource: {path}"))?;
        budget -= data.len();
        Ok(data.into())
    };
    for asset in &mut doc.extras.media.assets {
        if asset.data.is_empty() {
            asset.data = read(&media_entry(asset.id), 64 * 1024 * 1024)?;
        }
    }
    for reference in &mut doc.extras.references {
        if reference.data.is_empty() && reference.dimensions != [0, 0] {
            reference.data = read(
                &format!("LIBRETOON/references/{}.image", reference.id),
                16 * 1024 * 1024,
            )?;
        }
    }
    doc.validate()?;
    Ok(doc)
}

pub(crate) mod cels {
    use crate::{Mark, shared::SharedVec};
    use serde::{
        Deserialize, Deserializer, Serialize, Serializer,
        de::{self, MapAccess, SeqAccess, Visitor, value},
        ser::SerializeStruct,
    };
    use std::{cell::Cell, collections::HashMap, fmt};

    type Cel = SharedVec<Mark>;
    type Frames = SharedVec<Cel>;

    thread_local! { static COMPACT: Cell<bool> = const { Cell::new(false) }; }

    pub(super) fn compact<T>(f: impl FnOnce() -> T) -> T {
        struct Reset(bool);
        impl Drop for Reset {
            fn drop(&mut self) {
                COMPACT.set(self.0);
            }
        }
        let _reset = Reset(COMPACT.replace(true));
        f()
    }

    pub(crate) fn serialize<S: Serializer>(
        frames: &Frames,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if !COMPACT.get() {
            return frames.serialize(serializer);
        }
        let mut indices = HashMap::new();
        let mut unique = Vec::new();
        let mut exposures = Vec::with_capacity(frames.len());
        let mut previous: Option<(&Cel, usize)> = None;
        for frame in frames {
            let index = *indices.entry(frame.identity()).or_insert_with(|| {
                if let Some((last, index)) = previous
                    && last == frame
                {
                    return index;
                }
                let index = unique.len();
                unique.push(frame);
                index
            });
            exposures.push(index);
            previous = Some((frame, index));
        }
        let mut encoded = serializer.serialize_struct("Cels", 2)?;
        encoded.serialize_field("cels", &unique)?;
        encoded.serialize_field("exposures", &exposures)?;
        encoded.end()
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Frames, D::Error> {
        struct FrameVisitor;

        impl<'de> Visitor<'de> for FrameVisitor {
            type Value = Frames;

            fn expecting(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt.write_str("a frame array or a cel table with exposure indices")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Frames, A::Error> {
                Frames::deserialize(value::SeqAccessDeserializer::new(seq))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Frames, A::Error> {
                #[derive(Deserialize)]
                struct Table {
                    cels: Vec<Cel>,
                    exposures: Vec<usize>,
                }
                let table = Table::deserialize(value::MapAccessDeserializer::new(map))?;
                if table.cels.len() > 10000 || table.exposures.len() > 10000 {
                    return Err(de::Error::custom("Project contains too many exposures"));
                }
                table
                    .exposures
                    .into_iter()
                    .map(|index| {
                        table
                            .cels
                            .get(index)
                            .cloned()
                            .ok_or_else(|| de::Error::custom("Invalid project cel index"))
                    })
                    .collect()
            }
        }
        deserializer.deserialize_any(FrameVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_fla_migrates_to_native_without_xfl_preview() {
        let mut document = crate::fresh_document();
        document.layers[0].name = "Editable art".into();
        document.layers[0].frames[0].push(crate::Mark {
            nodes: vec![
                crate::document::Node::corner([10., 10.]),
                crate::document::Node {
                    position: [40., 20.],
                    in_handle: [30., 15.],
                    out_handle: [50., 15.],
                },
            ]
            .into(),
            ..Default::default()
        });
        let legacy = crate::fla::encode(&document).unwrap();
        let migrated = crate::fla::decode(&legacy).unwrap();
        let native = encode(&migrated).unwrap();
        let archive = Archive::new(&native).unwrap();
        assert!(archive.entry(b"DOMDocument.xml").unwrap().is_none());
        assert_eq!(
            archive.entry(MANIFEST.as_bytes()).unwrap().unwrap(),
            MANIFEST_V1
        );
        let restored = decode_if_native(&native).unwrap().unwrap();
        assert_eq!(
            serde_json::to_vec(&restored).unwrap(),
            serde_json::to_vec(&document).unwrap()
        );
    }

    #[test]
    #[ignore = "manual archive benchmark with a production project fixture"]
    fn benchmark_dbgt_storage() {
        let path =
            std::path::PathBuf::from(std::env::var_os("LIBRETOON_BENCHMARK_PROJECT").unwrap());
        let before = std::fs::metadata(&path).unwrap().len();
        let start = std::time::Instant::now();
        let doc = crate::documents::read_project(&path).unwrap();
        let open = start.elapsed();
        let start = std::time::Instant::now();
        let encoded = encode(&doc).unwrap();
        let save = start.elapsed();
        let mut recovery = crate::fla_zip::RecoveryCompression::default();
        write_with_recovery_cache(&doc, &mut Vec::new(), Some(&mut recovery)).unwrap();
        let start = std::time::Instant::now();
        let mut recovered = Vec::new();
        write_with_recovery_cache(&doc, &mut recovered, Some(&mut recovery)).unwrap();
        let recovery_write = start.elapsed();
        assert!(encoded == recovered);
        let restored = decode_if_native(&encoded).unwrap().unwrap();
        assert_eq!(
            restored.extras.media.assets.len(),
            doc.extras.media.assets.len()
        );
        for (old, new) in doc
            .extras
            .media
            .assets
            .iter()
            .zip(&restored.extras.media.assets)
        {
            assert_eq!(old.id, new.id);
            assert!(old.data == new.data);
        }
        assert_eq!(restored.layers.len(), doc.layers.len());
        for (old, new) in doc.layers.iter().zip(&restored.layers) {
            assert!(old.frames == new.frames);
        }
        eprintln!(
            "{before} -> {} bytes; open and validate {open:?}; serialize and compress {save:?}; recovery with warm resource compression {recovery_write:?}",
            encoded.len()
        );
    }

    #[test]
    fn repeated_cels_compress_losslessly_and_legacy_archives_still_open() {
        let mut doc = crate::fresh_document();
        let mut cel = Vec::new();
        for y in 0..120 {
            cel.push(crate::Mark {
                points: vec![[10., y as f32], [150., y as f32 + 1.]].into(),
                ..Default::default()
            });
        }
        doc.layers[0].frames = vec![cel.into(); 24].into();
        let json = serde_json::to_vec(&doc).unwrap();
        let legacy = crate::fla_zip::pack_slices(&[(MANIFEST, MANIFEST_V1), (DOCUMENT, &json)]);
        let compact = encode(&doc).unwrap();
        assert!(compact.len() * 4 < legacy.len());
        println!(
            "Uncompressed archive: {} bytes; compressed archive: {} bytes",
            legacy.len(),
            compact.len()
        );
        for bytes in [legacy, compact] {
            let restored = decode_if_native(&bytes).unwrap().unwrap();
            assert_eq!(serde_json::to_vec(&restored).unwrap(), json);
        }
    }

    #[test]
    fn large_projects_store_repeated_exposures_once_and_open_losslessly() {
        let mut doc = crate::fresh_document();
        let cel: crate::shared::SharedVec<_> = (0..8_000)
            .map(|i| crate::Mark {
                points: vec![[10., (i % 500) as f32], [150., (i % 500) as f32 + 1.]].into(),
                ..Default::default()
            })
            .collect();
        doc.layers[0].frames = vec![cel; 96].into();
        doc.layers[0].options.keys = Default::default();
        doc.layers[0].init_keys();
        let bytes = encode(&doc).unwrap();
        let archive = Archive::new(&bytes).unwrap();
        assert_eq!(
            archive.entry(MANIFEST.as_bytes()).unwrap().unwrap(),
            MANIFEST_V2
        );
        assert!(archive.entry(DOCUMENT.as_bytes()).unwrap().unwrap().len() < 2 * 1024 * 1024);
        let reopened = decode_if_native(&bytes).unwrap().unwrap();
        let frames = &reopened.layers[0].frames;
        assert_eq!(frames.len(), 96);
        assert!(frames[0] == doc.layers[0].frames[0]);
        assert_eq!(frames[0].identity(), frames[95].identity());
    }

    #[test]
    fn native_open_refuses_unknown_versions_and_missing_document() {
        for manifest in [
            br#"{"format":"LibreToon","version":999}"#.as_slice(),
            MANIFEST_V1,
        ] {
            let bytes = crate::fla_zip::pack_slices(&[(MANIFEST, manifest)]);
            assert!(decode_if_native(&bytes).is_err());
        }
    }
}

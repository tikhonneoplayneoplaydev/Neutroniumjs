//! `.nim` module container — a simple, deterministic archive for Neutronium.js
//! modules.
//!
//! On-disk layout (little-endian, all integers `u32` unless noted):
//!
//! ```text
//! magic      = b"NIM1"                         (4 bytes)
//! flags      = u32                             (reserved, must be 0)
//! manifest_len = u32                           (UTF-8 JSON byte length)
//! manifest   = [manifest_len]u8                (NeutroniumManifest JSON)
//! entry_count = u32
//! entries    = entry_count * Entry
//! Entry {
//!     path_len  = u32
//!     path      = [path_len]u8                 (forward-slash, relative, UTF-8)
//!     data_len  = u32
//!     data      = [data_len]u8
//! }
//! ```
//!
//! There is intentionally no compression: a `.nim` is a container, not a
//! compression format, so the loader can mmap/stream entries without pulling in
//! a codec. Paths are always relative and may not escape the container root
//! (`..` segments are rejected).
//!
//! The design is forward-compatible: unknown `flags` bits cause an error, and
//! unknown manifest fields are ignored by serde.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Container magic number: `"NIM1"`.
pub const MAGIC: &[u8; 4] = b"NIM1";
/// Current container format version (carried in `flags` low bits).
pub const FORMAT_VERSION: u32 = 1;

/// Manifest describing a `.nim` module. This is the only required metadata;
/// extra fields are preserved on read and re-serialized on write.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NeutroniumManifest {
    /// Machine-readable module name, e.g. `std/fs`.
    pub name: String,
    /// Semver-compatible version string.
    pub version: String,
    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Entry point relative to the archive root (e.g. `index.js`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    /// Other modules this one depends on (`name` -> version requirement).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
    /// Any unknown fields are captured so round-tripping is lossless.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl NeutroniumManifest {
    /// Create a minimal manifest with a name and version.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    /// Read and parse a manifest from a JSON reader.
    pub fn from_reader(reader: impl Read) -> Result<Self> {
        serde_json::from_reader(reader).context("parsing manifest JSON")
    }
}

/// An in-memory representation of a `.nim` archive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NimArchive {
    pub manifest: NeutroniumManifest,
    /// File entries keyed by their normalized relative path.
    pub entries: BTreeMap<String, Vec<u8>>,
}

impl NimArchive {
    /// Create an empty archive with the given manifest.
    pub fn new(manifest: NeutroniumManifest) -> Self {
        Self {
            manifest,
            entries: BTreeMap::new(),
        }
    }

    /// Add (or overwrite) a file entry. The path is validated and normalized.
    pub fn add_file(&mut self, path: &str, data: impl Into<Vec<u8>>) -> Result<()> {
        let normalized = normalize_path(path)?;
        self.entries.insert(normalized, data.into());
        Ok(())
    }

    /// Get a file's contents by normalized path.
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        let p = normalize_path(path).ok()?;
        self.entries.get(&p).map(|v| v.as_slice())
    }

    /// Serialize the archive to a writer in deterministic (sorted) order.
    pub fn write_to(&self, writer: &mut impl Write) -> Result<()> {
        writer.write_all(MAGIC)?;
        writer.write_all(&FORMAT_VERSION.to_le_bytes())?;

        let manifest_bytes = serde_json::to_vec_pretty(&self.manifest)
            .context("serializing manifest")?;
        writer.write_all(&(manifest_bytes.len() as u32).to_le_bytes())?;
        writer.write_all(&manifest_bytes)?;

        writer.write_all(&(self.entries.len() as u32).to_le_bytes())?;
        for (path, data) in &self.entries {
            let path_bytes = path.as_bytes();
            writer.write_all(&(path_bytes.len() as u32).to_le_bytes())?;
            writer.write_all(path_bytes)?;
            writer.write_all(&(data.len() as u32).to_le_bytes())?;
            writer.write_all(data)?;
        }
        writer.flush()?;
        Ok(())
    }

    /// Serialize to a freshly allocated `Vec<u8>`.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.write_to(&mut buf)?;
        Ok(buf)
    }

    /// Parse a `.nim` archive from a reader.
    pub fn from_reader(reader: &mut impl Read) -> Result<Self> {
        let mut magic = [0u8; 4];
        reader
            .read_exact(&mut magic)
            .context("reading .nim magic (truncated file?)")?;
        if &magic != MAGIC {
            bail!("not a .nim file: bad magic {magic:?}, expected {MAGIC:?}");
        }

        let flags = read_u32(reader).context("reading flags")?;
        if flags != FORMAT_VERSION {
            bail!(
                "unsupported .nim format version {flags}; this build supports version {FORMAT_VERSION}"
            );
        }

        let manifest_len = read_u32(reader).context("reading manifest length")? as usize;
        let mut manifest_bytes = vec![0u8; manifest_len];
        reader
            .read_exact(&mut manifest_bytes)
            .context("reading manifest")?;
        let manifest: NeutroniumManifest =
            serde_json::from_slice(&manifest_bytes).context("parsing manifest JSON")?;

        let entry_count = read_u32(reader).context("reading entry count")? as usize;
        let mut entries = BTreeMap::new();
        for i in 0..entry_count {
            let path_len = read_u32(reader)
                .with_context(|| format!("reading path length of entry {i}"))? as usize;
            let mut path_bytes = vec![0u8; path_len];
            reader
                .read_exact(&mut path_bytes)
                .with_context(|| format!("reading path of entry {i}"))?;
            let raw_path =
                String::from_utf8(path_bytes).with_context(|| format!("entry {i} path is not UTF-8"))?;
            let path = normalize_path(&raw_path)
                .with_context(|| format!("invalid entry path {raw_path:?} in entry {i}"))?;

            let data_len = read_u32(reader)
                .with_context(|| format!("reading data length of entry {i} ({path})"))? as usize;
            let mut data = vec![0u8; data_len];
            reader
                .read_exact(&mut data)
                .with_context(|| format!("reading data of entry {i} ({path})"))?;

            if entries.insert(path.clone(), data).is_some() {
                bail!("duplicate entry path {path:?} in archive");
            }
        }

        Ok(Self { manifest, entries })
    }

    /// Convenience: parse directly from a byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut cur = std::io::Cursor::new(bytes);
        Self::from_reader(&mut cur)
    }
}

/// Pack a directory on disk into a `.nim` archive, writing the result to
/// `output`. A `manifest.json` found at the directory root is used as the
/// manifest; otherwise `default_manifest` is used.
pub fn pack_directory(dir: &Path, output: &Path, default_manifest: NeutroniumManifest) -> Result<NimArchive> {
    if !dir.is_dir() {
        bail!("pack source {} is not a directory", dir.display());
    }

    let mut archive = NimArchive::new(default_manifest);

    // If a manifest.json exists at the root it wins.
    let manifest_path = dir.join("manifest.json");
    if manifest_path.is_file() {
        let file = std::fs::File::open(&manifest_path)
            .with_context(|| format!("opening {}", manifest_path.display()))?;
        archive.manifest = NeutroniumManifest::from_reader(file)?;
    }

    for entry in walkdir(dir)? {
        let rel = entry
            .strip_prefix(dir)
            .expect("walked entries are always under the root");
        let rel_str = rel
            .to_str()
            .with_context(|| format!("non-UTF-8 path {}", rel.display()))?
            .replace('\\', "/");
        if rel_str == "manifest.json" {
            continue;
        }
        let data = std::fs::read(&entry)
            .with_context(|| format!("reading {}", entry.display()))?;
        archive.add_file(&rel_str, data)?;
    }

    let mut out = std::fs::File::create(output)
        .with_context(|| format!("creating {}", output.display()))?;
    archive.write_to(&mut out)?;
    Ok(archive)
}

/// Unpack a `.nim` archive into `dir`, which is created if it does not exist.
/// Returns the manifest.
pub fn unpack_to(archive: &NimArchive, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating output directory {}", dir.display()))?;

    // Write the manifest alongside the entries so the module is self-describing
    // on disk as well as in the archive.
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = serde_json::to_vec_pretty(&archive.manifest)?;
    std::fs::write(&manifest_path, manifest_bytes)
        .with_context(|| format!("writing {}", manifest_path.display()))?;

    for (path, data) in &archive.entries {
        let full = dir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&full, data).with_context(|| format!("writing {}", full.display()))?;
    }
    Ok(())
}

/// Read and parse a `.nim` archive from a file path, returning the loaded
/// archive. This is the high-level "manifest loader" entry point.
pub fn load_nim(path: &Path) -> Result<NimArchive> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    NimArchive::from_bytes(&bytes)
}

fn read_u32(reader: &mut impl Read) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

/// Normalize a relative path: reject absolute paths, `.`/`..`/empty components,
/// backslashes, and NUL bytes. The result always uses forward slashes.
fn normalize_path(path: &str) -> Result<String> {
    if path.is_empty() {
        bail!("empty path");
    }
    if path.contains('\0') {
        bail!("path contains NUL byte: {path:?}");
    }
    let p = path.replace('\\', "/");
    if p.starts_with('/') {
        bail!("absolute paths are not allowed in a .nim archive: {path:?}");
    }
    let mut parts = Vec::new();
    for component in p.split('/') {
        match component {
            "" | "." => continue,
            ".." => bail!("path escapes archive root: {path:?}"),
            c => parts.push(c),
        }
    }
    if parts.is_empty() {
        bail!("path has no file component: {path:?}");
    }
    Ok(parts.join("/"))
}

/// Minimal recursive directory walk (avoids a `walkdir` dependency).
fn walkdir(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                out.push(path);
            } else {
                // Symlinks etc. are intentionally skipped to keep packs
                // reproducible and free of escapes.
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_archive() -> NimArchive {
        let mut m = NeutroniumManifest::new("std/demo", "1.2.3");
        m.main = Some("index.js".to_string());
        let mut a = NimArchive::new(m);
        a.add_file("index.js", b"print('hi')".to_vec()).unwrap();
        a.add_file("nested/util.js", b"export const x=1;".to_vec()).unwrap();
        a
    }

    #[test]
    fn roundtrip_preserves_manifest_and_entries() {
        let archive = sample_archive();
        let bytes = archive.to_bytes().unwrap();
        assert_eq!(&bytes[..4], MAGIC);
        let parsed = NimArchive::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, archive);
    }

    #[test]
    fn output_is_deterministic() {
        let a = sample_archive();
        let b = a.to_bytes().unwrap();
        let c = a.to_bytes().unwrap();
        assert_eq!(b, c);
    }

    #[test]
    fn rejects_path_traversal() {
        let mut a = NimArchive::new(NeutroniumManifest::new("x", "0.0.1"));
        assert!(a.add_file("../etc/passwd", b"").is_err());
        assert!(a.add_file("/abs", b"").is_err());
        assert!(a.add_file("a/../../b", b"").is_err());
    }

    #[test]
    fn normalizes_backslashes_and_dot_segments() {
        let mut a = NimArchive::new(NeutroniumManifest::new("x", "0.0.1"));
        a.add_file(".\\src\\./lib.js", b"data").unwrap();
        assert!(a.get("src/lib.js").is_some());
    }

    #[test]
    fn rejects_bad_magic_and_version() {
        assert!(NimArchive::from_bytes(b"NOPE....").is_err());
        let mut bad = MAGIC.to_vec();
        bad.extend_from_slice(&99u32.to_le_bytes());
        assert!(NimArchive::from_bytes(&bad).is_err());
    }

    #[test]
    fn detects_truncation() {
        let bytes = sample_archive().to_bytes().unwrap();
        assert!(NimArchive::from_bytes(&bytes[..bytes.len() - 3]).is_err());
    }
}

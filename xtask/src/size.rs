//! The bundle size budget that `cargo xtask build-worker` enforces (NFR-SEC-2, Rust workspace § 9).

/// NFR-SEC-2: the compressed bundle (`index.js` and `index_bg.wasm`) is at most 10 MiB.
pub const MAX_COMPRESSED: u64 = 10 * 1024 * 1024;
/// Cloudflare's limit on the uncompressed Worker: 64 MiB on all plans since 2026-09-04.
pub const MAX_UNCOMPRESSED: u64 = 64 * 1024 * 1024;
/// Above this the build warns: Rust workspace § 4 moves to `opt-level = "z"` past 8 MiB.
pub const WARN_COMPRESSED: u64 = 8 * 1024 * 1024;

/// One bundle file, uncompressed and gzipped.
pub struct FileSize {
    pub name: String,
    pub raw: u64,
    pub gzip: u64,
}

impl FileSize {
    pub fn measure(name: &str, bytes: &[u8]) -> Self {
        FileSize {
            name: name.to_owned(),
            raw: bytes.len() as u64,
            gzip: gzip_len(bytes),
        }
    }
}

/// The gzip size of `bytes` at level 9: miniz_oxide's DEFLATE stream at level 9 plus gzip's
/// 10-byte header and 8-byte trailer.
pub fn gzip_len(bytes: &[u8]) -> u64 {
    miniz_oxide::deflate::compress_to_vec(bytes, 9).len() as u64 + 18
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Within,
    /// Within the budget, but over 8 MiB compressed.
    Warn,
    Over(String),
}

/// Judges the bundle as a whole: the compressed total against NFR-SEC-2, the uncompressed total
/// against Cloudflare's limit.
pub fn size_budget(files: &[FileSize]) -> Verdict {
    let raw: u64 = files.iter().map(|f| f.raw).sum();
    let gzip: u64 = files.iter().map(|f| f.gzip).sum();
    if gzip > MAX_COMPRESSED {
        Verdict::Over(format!(
            "{gzip} bytes compressed exceeds the {MAX_COMPRESSED}-byte budget (NFR-SEC-2)"
        ))
    } else if raw > MAX_UNCOMPRESSED {
        Verdict::Over(format!(
            "{raw} bytes uncompressed exceeds Cloudflare's {MAX_UNCOMPRESSED}-byte limit"
        ))
    } else if gzip > WARN_COMPRESSED {
        Verdict::Warn
    } else {
        Verdict::Within
    }
}

/// The size table `build-worker` prints.
pub fn table(files: &[FileSize]) -> String {
    let mib = |b: u64| b as f64 / (1024.0 * 1024.0);
    let mut out =
        String::from("| File | Bytes | Gzip -9 bytes | Gzip MiB |\n|---|---:|---:|---:|\n");
    for f in files {
        out += &format!(
            "| {} | {} | {} | {:.3} |\n",
            f.name,
            f.raw,
            f.gzip,
            mib(f.gzip)
        );
    }
    let (raw, gzip) = (
        files.iter().map(|f| f.raw).sum::<u64>(),
        files.iter().map(|f| f.gzip).sum::<u64>(),
    );
    out += &format!(
        "| total | {raw} | {gzip} | {:.3} of {:.0} |\n",
        mib(gzip),
        mib(MAX_COMPRESSED)
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(raw: u64, gzip: u64) -> FileSize {
        FileSize {
            name: "f".into(),
            raw,
            gzip,
        }
    }

    /// The compressed bundle may be exactly 10 MiB and not a byte more; the uncompressed bundle at
    /// most 64 MiB; over 8 MiB compressed warns. Sizes add up across the bundle's files.
    ///
    /// Covers: NFR-SEC-2
    #[test]
    fn size_budget() {
        assert_eq!(super::size_budget(&[file(100, 40)]), Verdict::Within);
        assert_eq!(
            super::size_budget(&[file(1, MAX_COMPRESSED)]),
            Verdict::Warn
        );
        assert!(matches!(
            super::size_budget(&[file(1, MAX_COMPRESSED + 1)]),
            Verdict::Over(_)
        ));
        assert!(matches!(
            super::size_budget(&[file(1, MAX_COMPRESSED / 2 + 1), file(1, MAX_COMPRESSED / 2)]),
            Verdict::Over(_)
        ));
        assert_eq!(
            super::size_budget(&[file(MAX_UNCOMPRESSED, WARN_COMPRESSED)]),
            Verdict::Within
        );
        assert!(matches!(
            super::size_budget(&[file(MAX_UNCOMPRESSED + 1, 1)]),
            Verdict::Over(_)
        ));
    }

    /// The measured gzip size is a real DEFLATE stream plus the gzip framing.
    #[test]
    fn gzip_len_matches_the_stream() {
        let data = b"hello hello hello hello hello hello".repeat(100);
        let deflated = miniz_oxide::deflate::compress_to_vec(&data, 9);
        assert_eq!(
            miniz_oxide::inflate::decompress_to_vec(&deflated).unwrap(),
            data
        );
        assert_eq!(gzip_len(&data), deflated.len() as u64 + 18);
        assert!(gzip_len(&data) < data.len() as u64);
    }
}

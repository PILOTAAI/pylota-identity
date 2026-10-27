//! Attachment sniffing and risk ([Inbound › Attachment safety], B10). The sniffed type wins over the
//! declared `Content-Type` and the extension. Container checks read structure only: ZIP central
//! directories (three kinds of entry inflated, 1 MiB each), OLE directory names, the gzip trailer, a
//! PDF byte scan. A risky attachment is never passed to agents or to text extraction.
//!
//! [Inbound › Attachment safety]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use serde::Serialize;

/// Bytes the magic checks look at.
pub const SNIFF_BYTES: usize = 8192;
/// Output cap of each inflated entry.
pub const INFLATE_CAP: usize = 1 << 20;
const BOMB_TOTAL: u64 = 100_000_000;
const BOMB_RATIO: u64 = 100;
const BOMB_ENTRIES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Pdf,
    Archive,
    /// Office documents, which can carry macros: OOXML, ODF, OLE, RTF.
    Document,
    Executable,
    Script,
    Image,
    Calendar,
    Html,
    Text,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Executable,
    Macro,
    ArchiveBomb,
    EncryptedArchive,
    TypeMismatch,
    EncryptedDocument,
}

impl Risk {
    /// Every risk but `encrypted_document` quarantines the message (`risky_attachment`).
    pub fn quarantines(self) -> bool {
        self != Risk::EncryptedDocument
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Assessment {
    /// The sniffed media type.
    pub sniffed_type: &'static str,
    pub family: Family,
    /// The first risk of the table that applies.
    pub risk: Option<Risk>,
}

const EXECUTABLE_EXT: &[&str] = &[
    "exe", "dll", "scr", "com", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf",
    "wsh", "hta", "msi", "msp", "lnk", "jar", "apk", "app", "dmg", "iso", "img", "vhd", "vhdx",
    "reg", "cpl", "pif", "sh", "scf", "url",
];
const MACRO_EXT: &[&str] = &[
    "docm", "dotm", "xlsm", "xltm", "xlam", "pptm", "potm", "ppam", "sldm",
];
const ARCHIVE_EXT: &[&str] = &[
    "zip", "rar", "7z", "gz", "tgz", "tar", "cab", "bz2", "xz", "zst",
];

fn extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty() && ext.len() <= 10).then(|| ext.trim().to_ascii_lowercase())
}

fn ext_family(ext: &str) -> Family {
    match ext {
        "pdf" => Family::Pdf,
        e if ARCHIVE_EXT.contains(&e) => Family::Archive,
        "doc" | "docx" | "dot" | "dotx" | "xls" | "xlsx" | "xlt" | "xltx" | "ppt" | "pptx"
        | "pps" | "ppsx" | "odt" | "ods" | "odp" | "rtf" | "msg" => Family::Document,
        e if MACRO_EXT.contains(&e) => Family::Document,
        e if EXECUTABLE_EXT.contains(&e) => Family::Executable,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "tif" | "tiff" | "heic" | "bmp" => Family::Image,
        "ics" | "ical" | "ifb" => Family::Calendar,
        "htm" | "html" | "xhtml" => Family::Html,
        "txt" | "csv" | "md" | "log" | "json" | "xml" | "eml" | "vcf" => Family::Text,
        _ => Family::Unknown,
    }
}

fn declared_family(ct: &str) -> Family {
    let ct = ct.to_ascii_lowercase();
    match ct.as_str() {
        "application/pdf" => Family::Pdf,
        "application/zip"
        | "application/x-zip-compressed"
        | "application/gzip"
        | "application/x-gzip"
        | "application/x-rar-compressed"
        | "application/vnd.rar"
        | "application/x-7z-compressed"
        | "application/vnd.ms-cab-compressed" => Family::Archive,
        "text/html" | "application/xhtml+xml" => Family::Html,
        "text/calendar" => Family::Calendar,
        "application/msword"
        | "application/vnd.ms-excel"
        | "application/vnd.ms-powerpoint"
        | "application/rtf"
        | "text/rtf"
        | "application/vnd.ms-outlook" => Family::Document,
        c if c.starts_with("application/vnd.openxmlformats-officedocument.")
            || c.starts_with("application/vnd.oasis.opendocument.")
            || c.starts_with("application/vnd.ms-word.")
            || c.starts_with("application/vnd.ms-excel.")
            || c.starts_with("application/vnd.ms-powerpoint.") =>
        {
            Family::Document
        }
        c if c.starts_with("image/") => Family::Image,
        c if c.starts_with("text/") => Family::Text,
        _ => Family::Unknown,
    }
}

// ---- ZIP ----------------------------------------------------------------------------------------

struct ZipEntry {
    name: String,
    method: u16,
    encrypted: bool,
    compressed: u64,
    uncompressed: u64,
    local_offset: u64,
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The central directory, read without inflating anything. `None` when it cannot be read.
fn zip_entries(b: &[u8]) -> Option<Vec<ZipEntry>> {
    let search_from = b.len().saturating_sub(65_557);
    let eocd = (search_from..b.len().saturating_sub(21))
        .rev()
        .find(|&i| b[i..].starts_with(b"PK\x05\x06"))?;
    let mut count = u64::from(le16(b, eocd + 10)?);
    let mut cd_offset = u64::from(le32(b, eocd + 16)?);
    if (count == 0xffff || cd_offset == 0xffff_ffff)
        && eocd >= 20
        && b[eocd - 20..].starts_with(b"PK\x06\x07")
    {
        let z64 = usize::try_from(le64(b, eocd - 12)?).ok()?;
        if b.get(z64..)?.starts_with(b"PK\x06\x06") {
            count = le64(b, z64 + 32)?;
            cd_offset = le64(b, z64 + 48)?;
        }
    }
    let mut pos = usize::try_from(cd_offset).ok()?;
    let mut out = Vec::new();
    for _ in 0..count.min(BOMB_ENTRIES as u64 + 1) {
        if !b.get(pos..)?.starts_with(b"PK\x01\x02") {
            return if out.is_empty() { None } else { Some(out) };
        }
        let flags = le16(b, pos + 8)?;
        let method = le16(b, pos + 10)?;
        let mut compressed = u64::from(le32(b, pos + 20)?);
        let mut uncompressed = u64::from(le32(b, pos + 24)?);
        let name_len = usize::from(le16(b, pos + 28)?);
        let extra_len = usize::from(le16(b, pos + 30)?);
        let comment_len = usize::from(le16(b, pos + 32)?);
        let mut local_offset = u64::from(le32(b, pos + 42)?);
        let name = String::from_utf8_lossy(b.get(pos + 46..pos + 46 + name_len)?).into_owned();
        let extra = b.get(pos + 46 + name_len..pos + 46 + name_len + extra_len)?;
        // ZIP64 extended information: only the fields whose 32-bit value is 0xFFFFFFFF, in order.
        let mut e = 0;
        while e + 4 <= extra.len() {
            let id = le16(extra, e)?;
            let len = usize::from(le16(extra, e + 2)?);
            if id == 0x0001 {
                let mut f = e + 4;
                for v in [&mut uncompressed, &mut compressed, &mut local_offset] {
                    if *v == 0xffff_ffff {
                        *v = le64(extra, f)?;
                        f += 8;
                    }
                }
            }
            e += 4 + len;
        }
        out.push(ZipEntry {
            name,
            method,
            encrypted: flags & 1 != 0,
            compressed,
            uncompressed,
            local_offset,
        });
        pos += 46 + name_len + extra_len + comment_len;
    }
    Some(out)
}

enum Inflate {
    Data(Vec<u8>),
    /// The entry would inflate beyond its cap.
    TooLarge,
    Unreadable,
}

/// Inflates one entry, at most [`INFLATE_CAP`] bytes.
fn inflate(b: &[u8], e: &ZipEntry) -> Inflate {
    if e.uncompressed > INFLATE_CAP as u64 {
        return Inflate::TooLarge;
    }
    let Some(local) = usize::try_from(e.local_offset).ok() else {
        return Inflate::Unreadable;
    };
    let data = (|| {
        if !b.get(local..)?.starts_with(b"PK\x03\x04") {
            return None;
        }
        let start =
            local + 30 + usize::from(le16(b, local + 26)?) + usize::from(le16(b, local + 28)?);
        let len = usize::try_from(e.compressed).ok()?;
        b.get(start..start.checked_add(len)?)
    })();
    let Some(data) = data else {
        return Inflate::Unreadable;
    };
    match e.method {
        0 if data.len() <= INFLATE_CAP => Inflate::Data(data.to_vec()),
        0 => Inflate::TooLarge,
        8 => match miniz_oxide::inflate::decompress_to_vec_with_limit(data, INFLATE_CAP) {
            Ok(v) => Inflate::Data(v),
            Err(err) if err.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput => {
                Inflate::TooLarge
            }
            Err(_) => Inflate::Unreadable,
        },
        _ => Inflate::Unreadable,
    }
}

struct ZipFacts {
    kind: &'static str,
    family: Family,
    executable: bool,
    macro_part: bool,
    bomb: bool,
    encrypted_entry: bool,
    encrypted_odf: bool,
}

fn name_is_executable(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    extension(base).is_some_and(|e| EXECUTABLE_EXT.contains(&e.as_str()))
}

fn name_is_archive(name: &str) -> bool {
    extension(name).is_some_and(|e| ARCHIVE_EXT.contains(&e.as_str()))
}

fn zip_facts(b: &[u8], depth: usize) -> ZipFacts {
    let mut f = ZipFacts {
        kind: "application/zip",
        family: Family::Archive,
        executable: false,
        macro_part: false,
        bomb: false,
        encrypted_entry: false,
        encrypted_odf: false,
    };
    let Some(entries) = zip_entries(b) else {
        return f;
    };
    let total: u64 = entries
        .iter()
        .map(|e| e.uncompressed)
        .fold(0, u64::saturating_add);
    let packed: u64 = entries
        .iter()
        .map(|e| e.compressed)
        .fold(0, u64::saturating_add)
        .max(1);
    f.bomb = entries.len() > BOMB_ENTRIES || total > BOMB_TOTAL || total / packed > BOMB_RATIO;
    f.encrypted_entry = entries.iter().any(|e| e.encrypted);
    f.macro_part = entries.iter().any(|e| {
        e.name
            .rsplit('/')
            .next()
            .is_some_and(|n| n.eq_ignore_ascii_case("vbaProject.bin"))
    });
    f.executable = entries.iter().any(|e| name_is_executable(&e.name));
    let has = |n: &str| entries.iter().find(|e| e.name == n);
    let prefixed = |p: &str| entries.iter().any(|e| e.name.starts_with(p));

    if has("[Content_Types].xml").is_some() {
        f.family = Family::Document;
        f.kind = if prefixed("word/") {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        } else if prefixed("xl/") {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        } else if prefixed("ppt/") {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        } else {
            "application/vnd.openxmlformats-package"
        };
        if let Some(e) = has("[Content_Types].xml") {
            match inflate(b, e) {
                Inflate::Data(xml) => {
                    if String::from_utf8_lossy(&xml).contains("macroEnabled") {
                        f.macro_part = true;
                    }
                }
                Inflate::TooLarge => f.bomb = true,
                Inflate::Unreadable => {}
            }
        }
    } else if has("mimetype").is_some() {
        f.family = Family::Document;
        f.kind = "application/vnd.oasis.opendocument";
        if let Some(e) = has("META-INF/manifest.xml") {
            match inflate(b, e) {
                Inflate::Data(xml) => {
                    f.encrypted_odf = String::from_utf8_lossy(&xml).contains("encryption-data")
                }
                Inflate::TooLarge => f.bomb = true,
                Inflate::Unreadable => {}
            }
        }
    } else if has("AndroidManifest.xml").is_some() {
        f.kind = "application/vnd.android.package-archive";
        f.family = Family::Executable;
        f.executable = true;
    } else if has("META-INF/MANIFEST.MF").is_some() {
        f.kind = "application/java-archive";
        f.family = Family::Executable;
        f.executable = true;
    }

    // One level of nested ZIP for the executable-name check; a third level counts as a bomb.
    for e in entries
        .iter()
        .filter(|e| e.name.to_ascii_lowercase().ends_with(".zip"))
    {
        if depth >= 1 {
            f.bomb = true;
            break;
        }
        match inflate(b, e) {
            Inflate::Data(inner) => {
                let n = zip_facts(&inner, depth + 1);
                f.executable |= n.executable;
                f.bomb |= n.bomb;
                f.encrypted_entry |= n.encrypted_entry;
            }
            Inflate::TooLarge => f.bomb = true,
            Inflate::Unreadable => {}
        }
    }
    if depth >= 1 && entries.iter().any(|e| name_is_archive(&e.name)) {
        f.bomb = true;
    }
    f
}

// ---- OLE ----------------------------------------------------------------------------------------

struct OleFacts {
    kind: &'static str,
    executable: bool,
    macro_part: bool,
    encrypted: bool,
}

/// MSI database root CLSID `000C1084-0000-0000-C000-000000000046`.
const MSI_CLSID: [u8; 16] = [
    0x00, 0x0c, 0x10, 0x84, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46,
];

fn ole_facts(b: &[u8]) -> OleFacts {
    let mut f = OleFacts {
        kind: "application/x-ole-storage",
        executable: false,
        macro_part: false,
        encrypted: false,
    };
    let Ok(cfb) = cfb::CompoundFile::open(std::io::Cursor::new(b)) else {
        return f;
    };
    let root = cfb.root_entry();
    if root.clsid().as_bytes() == &MSI_CLSID {
        f.kind = "application/x-msi";
        f.executable = true;
    }
    let names: Vec<String> = cfb
        .walk()
        .take(100_000)
        .map(|e| e.name().to_owned())
        .collect();
    let has = |n: &str| names.iter().any(|x| x.eq_ignore_ascii_case(n));
    f.macro_part = has("VBA") || has("_VBA_PROJECT") || has("Macros");
    f.encrypted = has("EncryptionInfo") && has("EncryptedPackage");
    if f.kind == "application/x-ole-storage" {
        f.kind = if has("WordDocument") {
            "application/msword"
        } else if has("Workbook") || has("Book") {
            "application/vnd.ms-excel"
        } else if has("PowerPoint Document") {
            "application/vnd.ms-powerpoint"
        } else if names.iter().any(|n| n.starts_with("__substg1.0_")) {
            "application/vnd.ms-outlook"
        } else {
            "application/x-ole-storage"
        };
    }
    f
}

fn pdf_encrypted(b: &[u8]) -> bool {
    b.windows(8)
        .enumerate()
        .any(|(i, w)| w == b"/Encrypt" && b.get(i + 8).is_none_or(|&c| !c.is_ascii_alphanumeric()))
}

fn is_text(head: &[u8]) -> bool {
    match std::str::from_utf8(head) {
        Ok(s) => !s
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{c}')),
        // A multi-byte character cut by the 8 KiB window is fine.
        Err(e) => e.error_len().is_none() && is_text(&head[..e.valid_up_to()]),
    }
}

fn magic(b: &[u8]) -> (&'static str, Family) {
    let starts = |m: &[u8]| b.starts_with(m);
    if starts(b"%PDF-") {
        return ("application/pdf", Family::Pdf);
    }
    if starts(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        return ("application/x-ole-storage", Family::Document);
    }
    if starts(b"MZ") {
        return (
            "application/vnd.microsoft.portable-executable",
            Family::Executable,
        );
    }
    if starts(&[0x7f, b'E', b'L', b'F']) {
        return ("application/x-elf", Family::Executable);
    }
    for m in [
        [0xfe, 0xed, 0xfa, 0xce],
        [0xfe, 0xed, 0xfa, 0xcf],
        [0xce, 0xfa, 0xed, 0xfe],
        [0xcf, 0xfa, 0xed, 0xfe],
        [0xca, 0xfe, 0xba, 0xbe],
    ] {
        if starts(&m) {
            return ("application/x-mach-binary", Family::Executable);
        }
    }
    if starts(b"#!") {
        return ("text/x-shellscript", Family::Script);
    }
    if starts(b"Rar!\x1a\x07") {
        return ("application/vnd.rar", Family::Archive);
    }
    if starts(&[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c]) {
        return ("application/x-7z-compressed", Family::Archive);
    }
    if starts(&[0x1f, 0x8b]) {
        return ("application/gzip", Family::Archive);
    }
    if starts(b"MSCF") {
        return ("application/vnd.ms-cab-compressed", Family::Archive);
    }
    if starts(&[0x4c, 0, 0, 0, 0x01, 0x14, 0x02, 0]) {
        return ("application/x-ms-shortcut", Family::Executable);
    }
    if b.get(0x8001..0x8006) == Some(b"CD001") {
        return ("application/x-iso9660-image", Family::Executable);
    }
    if starts(&[0x78, 0x9f, 0x3e, 0x22]) {
        return ("application/vnd.ms-tnef", Family::Document);
    }
    if starts(b"{\\rtf") {
        return ("application/rtf", Family::Document);
    }
    if starts(b"\x89PNG") {
        return ("image/png", Family::Image);
    }
    if starts(&[0xff, 0xd8, 0xff]) {
        return ("image/jpeg", Family::Image);
    }
    if starts(b"GIF8") {
        return ("image/gif", Family::Image);
    }
    if starts(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        return ("image/webp", Family::Image);
    }
    if starts(b"II*\0") || starts(b"MM\0*") {
        return ("image/tiff", Family::Image);
    }
    if b.get(4..12) == Some(b"ftypheic")
        || b.get(4..12) == Some(b"ftypheix")
        || b.get(4..12) == Some(b"ftypmif1")
    {
        return ("image/heic", Family::Image);
    }
    if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        return ("application/zip", Family::Archive);
    }
    let head = &b[..b.len().min(SNIFF_BYTES)];
    let trimmed = {
        let s = head.strip_prefix(&[0xef, 0xbb, 0xbf][..]).unwrap_or(head);
        let i = s
            .iter()
            .position(|c| !c.is_ascii_whitespace())
            .unwrap_or(s.len());
        &s[i..]
    };
    if trimmed.len() >= 15 && trimmed[..15].eq_ignore_ascii_case(b"BEGIN:VCALENDAR") {
        return ("text/calendar", Family::Calendar);
    }
    let lead = &trimmed[..trimmed.len().min(512)];
    let lower: Vec<u8> = lead.iter().map(u8::to_ascii_lowercase).collect();
    if lower.starts_with(b"<!doctype html") || lower.windows(5).any(|w| w == b"<html") {
        return ("text/html", Family::Html);
    }
    if is_text(head) {
        return ("text/plain", Family::Text);
    }
    ("application/octet-stream", Family::Unknown)
}

/// Sniffs an attachment and classifies its risk.
pub fn assess(data: &[u8], filename: Option<&str>, declared: &str) -> Assessment {
    let (mut kind, mut family) = magic(data);
    let ext = filename.and_then(extension);
    let mut executable = matches!(family, Family::Executable | Family::Script);
    let mut macro_part = false;
    let mut bomb = false;
    let mut encrypted_archive = matches!(
        kind,
        "application/vnd.rar" | "application/x-7z-compressed" | "application/vnd.ms-cab-compressed"
    );
    let mut encrypted_document = false;

    match kind {
        "application/zip" => {
            let z = zip_facts(data, 0);
            kind = z.kind;
            family = z.family;
            executable |= z.executable;
            macro_part |= z.macro_part;
            bomb |= z.bomb;
            encrypted_archive |= z.encrypted_entry;
            encrypted_document |= z.encrypted_odf;
        }
        "application/x-ole-storage" => {
            let o = ole_facts(data);
            kind = o.kind;
            executable |= o.executable;
            macro_part |= o.macro_part;
            encrypted_document |= o.encrypted;
        }
        "application/gzip" => {
            if data.len() >= 18 {
                let isize = u64::from(le32(data, data.len() - 4).unwrap_or(0));
                bomb |= isize / (data.len() as u64).max(1) > BOMB_RATIO;
            }
        }
        "application/pdf" => encrypted_document = pdf_encrypted(data),
        _ => {}
    }
    if ext.as_deref().is_some_and(|e| EXECUTABLE_EXT.contains(&e)) {
        executable = true;
    }
    if ext.as_deref().is_some_and(|e| MACRO_EXT.contains(&e)) {
        macro_part = true;
    }

    let mismatch = matches!(
        family,
        Family::Archive | Family::Html | Family::Script | Family::Document
    ) && {
        let by_ext = ext
            .as_deref()
            .map(ext_family)
            .filter(|f| *f != Family::Unknown);
        let by_type = Some(declared_family(declared)).filter(|f| *f != Family::Unknown);
        let differs = |f: Option<Family>| {
            f.is_some_and(|f| f != family && !(f == Family::Text && family == Family::Script))
        };
        differs(by_ext) || differs(by_type)
    };

    let risk = if executable {
        Some(Risk::Executable)
    } else if macro_part {
        Some(Risk::Macro)
    } else if bomb {
        Some(Risk::ArchiveBomb)
    } else if encrypted_archive {
        Some(Risk::EncryptedArchive)
    } else if mismatch {
        Some(Risk::TypeMismatch)
    } else if encrypted_document {
        Some(Risk::EncryptedDocument)
    } else {
        None
    };
    Assessment {
        sniffed_type: kind,
        family,
        risk,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A ZIP of stored entries (method 0), or deflated ones when `deflate` is set.
    pub(crate) fn zip(entries: &[(&str, &[u8])], deflate: bool, encrypted: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let (method, body) = if deflate {
                (8u16, miniz_oxide::deflate::compress_to_vec(data, 6))
            } else {
                (0u16, data.to_vec())
            };
            let offset = out.len() as u32;
            let flags: u16 = u16::from(encrypted);
            let header = |sig: &[u8], central: bool| {
                let mut h = sig.to_vec();
                if central {
                    h.extend_from_slice(&20u16.to_le_bytes());
                }
                h.extend_from_slice(&20u16.to_le_bytes());
                h.extend_from_slice(&flags.to_le_bytes());
                h.extend_from_slice(&method.to_le_bytes());
                h.extend_from_slice(&[0; 4]); // time, date
                h.extend_from_slice(&[0; 4]); // crc (not checked)
                h.extend_from_slice(&(body.len() as u32).to_le_bytes());
                h.extend_from_slice(&(data.len() as u32).to_le_bytes());
                h.extend_from_slice(&(name.len() as u16).to_le_bytes());
                h.extend_from_slice(&0u16.to_le_bytes());
                if central {
                    h.extend_from_slice(&[0; 6]); // comment len, disk, internal attrs
                    h.extend_from_slice(&[0; 4]); // external attrs
                    h.extend_from_slice(&offset.to_le_bytes());
                }
                h.extend_from_slice(name.as_bytes());
                h
            };
            out.extend(header(b"PK\x03\x04", false));
            out.extend_from_slice(&body);
            central.extend(header(b"PK\x01\x02", true));
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn risk(data: &[u8], name: &str, declared: &str) -> Option<Risk> {
        assess(data, Some(name), declared).risk
    }

    const DOCX_TYPES: &[u8] = br#"<Types><Override ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;

    #[test]
    fn b10_executables() {
        assert_eq!(
            risk(b"MZ\x90\x00", "invoice.pdf", "application/pdf"),
            Some(Risk::Executable)
        );
        assert_eq!(
            risk(b"\x7fELF\x02", "x", "application/octet-stream"),
            Some(Risk::Executable)
        );
        assert_eq!(
            risk(b"\xcf\xfa\xed\xfe", "x", "application/octet-stream"),
            Some(Risk::Executable)
        );
        assert_eq!(
            risk(b"#!/bin/sh\nrm -rf /", "notes.txt", "text/plain"),
            Some(Risk::Executable)
        );
        assert_eq!(
            risk(b"plain text", "run.ps1", "text/plain"),
            Some(Risk::Executable)
        );
        assert_eq!(
            risk(b"plain text", "invoice.pdf.exe", "application/pdf"),
            Some(Risk::Executable)
        );
        assert_eq!(
            risk(
                b"L\0\0\0\x01\x14\x02\0",
                "doc.lnk",
                "application/octet-stream"
            ),
            Some(Risk::Executable)
        );
        let mut iso = vec![0u8; 0x8010];
        iso[0x8001..0x8006].copy_from_slice(b"CD001");
        assert_eq!(
            risk(&iso, "disk.bin", "application/octet-stream"),
            Some(Risk::Executable)
        );
        let zipped = zip(
            &[("readme.txt", b"hi"), ("tools/setup.EXE", b"MZ")],
            true,
            false,
        );
        assert_eq!(
            risk(&zipped, "files.zip", "application/zip"),
            Some(Risk::Executable)
        );
        let nested = zip(
            &[("inner.zip", &zip(&[("run.js", b"x")], false, false))],
            true,
            false,
        );
        assert_eq!(
            risk(&nested, "files.zip", "application/zip"),
            Some(Risk::Executable)
        );
        let jar = zip(&[("META-INF/MANIFEST.MF", b"Main-Class: x")], false, false);
        assert_eq!(
            assess(&jar, Some("lib.zip"), "application/zip").sniffed_type,
            "application/java-archive"
        );
        assert_eq!(
            risk(&jar, "lib.zip", "application/zip"),
            Some(Risk::Executable)
        );
    }

    #[test]
    fn b10_macros() {
        let docm = zip(
            &[
                ("[Content_Types].xml", DOCX_TYPES),
                ("word/document.xml", b"<w/>"),
                ("word/vbaProject.bin", b"x"),
            ],
            true,
            false,
        );
        assert_eq!(
            risk(
                &docm,
                "report.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            Some(Risk::Macro)
        );
        let docx = zip(
            &[
                ("[Content_Types].xml", DOCX_TYPES),
                ("word/document.xml", b"<w/>"),
            ],
            true,
            false,
        );
        let a = assess(
            &docx,
            Some("report.docx"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        assert_eq!(a.risk, None);
        assert_eq!(
            a.sniffed_type,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            risk(&docx, "report.docm", "application/octet-stream"),
            Some(Risk::Macro)
        );
    }

    #[test]
    fn b10_archive_bombs() {
        let big = vec![0u8; 1_000_000];
        let bomb = zip(&[("zeros.bin", &big)], true, false);
        assert_eq!(
            risk(&bomb, "a.zip", "application/zip"),
            Some(Risk::ArchiveBomb)
        );
        let deep = zip(
            &[(
                "l1.zip",
                &zip(
                    &[("l2.zip", &zip(&[("x.txt", b"x")], false, false))],
                    false,
                    false,
                ),
            )],
            false,
            false,
        );
        assert_eq!(
            risk(&deep, "a.zip", "application/zip"),
            Some(Risk::ArchiveBomb)
        );
        let mut gz = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
        gz.extend_from_slice(&[0; 10]);
        gz.extend_from_slice(&(500_000_000u32).to_le_bytes());
        assert_eq!(
            risk(&gz, "a.gz", "application/gzip"),
            Some(Risk::ArchiveBomb)
        );
        let ok = zip(&[("a.txt", b"hello hello hello")], true, false);
        assert_eq!(risk(&ok, "a.zip", "application/zip"), None);
    }

    #[test]
    fn b10_encrypted() {
        let enc = zip(&[("secret.pdf", b"%PDF")], false, true);
        assert_eq!(
            risk(&enc, "a.zip", "application/zip"),
            Some(Risk::EncryptedArchive)
        );
        assert_eq!(
            risk(b"Rar!\x1a\x07\x01\x00", "a.rar", "application/vnd.rar"),
            Some(Risk::EncryptedArchive)
        );
        assert_eq!(
            risk(b"7z\xbc\xaf\x27\x1c", "a.7z", "application/x-7z-compressed"),
            Some(Risk::EncryptedArchive)
        );
        assert_eq!(
            risk(b"MSCF\0\0\0\0", "a.cab", "application/octet-stream"),
            Some(Risk::EncryptedArchive)
        );
        let pdf =
            b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\ntrailer\n<< /Root 1 0 R /Encrypt 5 0 R >>\n%%EOF";
        let r = risk(pdf, "a.pdf", "application/pdf");
        assert_eq!(r, Some(Risk::EncryptedDocument));
        assert!(!r.unwrap().quarantines());
        assert_eq!(
            risk(
                b"%PDF-1.7\n/EncryptMetadata false",
                "a.pdf",
                "application/pdf"
            ),
            None
        );
        let odf = zip(
            &[
                ("mimetype", b"application/vnd.oasis.opendocument.text"),
                ("META-INF/manifest.xml", b"<manifest:encryption-data/>"),
            ],
            false,
            false,
        );
        assert_eq!(
            risk(&odf, "a.odt", "application/vnd.oasis.opendocument.text"),
            Some(Risk::EncryptedDocument)
        );
    }

    fn ole(streams: &[&str], storages: &[&str]) -> Vec<u8> {
        use std::io::Write as _;
        let mut c = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new())).unwrap();
        for s in storages {
            c.create_storage(s).unwrap();
        }
        for s in streams {
            c.create_stream(s).unwrap().write_all(b"x").unwrap();
        }
        c.flush().unwrap();
        c.into_inner().into_inner()
    }

    #[test]
    fn b10_ole() {
        let doc = ole(&["/WordDocument"], &[]);
        let a = assess(&doc, Some("letter.doc"), "application/msword");
        assert_eq!((a.sniffed_type, a.risk), ("application/msword", None));
        let macro_doc = ole(
            &["/WordDocument", "/Macros/VBA/dir"],
            &["/Macros", "/Macros/VBA"],
        );
        assert_eq!(
            risk(&macro_doc, "letter.doc", "application/msword"),
            Some(Risk::Macro)
        );
        let enc = ole(&["/EncryptionInfo", "/EncryptedPackage"], &[]);
        assert_eq!(
            risk(&enc, "report.docx", "application/octet-stream"),
            Some(Risk::EncryptedDocument)
        );
        // An OLE file named as a PDF is a macro-capable document in disguise.
        assert_eq!(
            risk(&doc, "invoice.pdf", "application/pdf"),
            Some(Risk::TypeMismatch)
        );
    }

    #[test]
    fn b10_type_mismatch() {
        let z = zip(&[("a.txt", b"x")], false, false);
        assert_eq!(
            risk(&z, "invoice.pdf", "application/pdf"),
            Some(Risk::TypeMismatch)
        );
        assert_eq!(
            risk(
                b"<html><body>pay</body></html>",
                "invoice.pdf",
                "application/pdf"
            ),
            Some(Risk::TypeMismatch)
        );
        // Harmless: octet-stream, or one image type declared as another.
        assert_eq!(risk(&z, "files.zip", "application/octet-stream"), None);
        assert_eq!(risk(b"\x89PNG\r\n", "photo.jpg", "image/jpeg"), None);
        assert_eq!(risk(b"<!DOCTYPE html><p>x", "page.html", "text/html"), None);
        assert_eq!(
            assess(b"plain words", Some("a.txt"), "text/plain").sniffed_type,
            "text/plain"
        );
        assert_eq!(
            assess(b"\0\x01\x02", None, "application/octet-stream").family,
            Family::Unknown
        );
        assert_eq!(
            assess("caf\u{e9}".repeat(3000).as_bytes(), None, "text/plain").family,
            Family::Text
        );
    }
}

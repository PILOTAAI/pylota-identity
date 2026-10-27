//! TNEF (`winmail.dat`, `application/ms-tnef`) unpacking ([Inbound › Parsing], B6): the attachments a
//! TNEF stream carries ([MS-OXTNEF]). Every length is checked against the input; a malformed stream is
//! an error and the caller keeps `winmail.dat` as an attachment.
//!
//! [Inbound › Parsing]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
//! [MS-OXTNEF]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxtnef/

/// The stream signature, little-endian.
pub const SIGNATURE: u32 = 0x223E_9F78;

const LVL_ATTACHMENT: u8 = 0x02;
const ATT_ATTACH_REND_DATA: u32 = 0x0006_9002;
const ATT_ATTACH_TITLE: u32 = 0x0001_8010;
const ATT_ATTACH_DATA: u32 = 0x0006_800F;
const ATT_ATTACHMENT: u32 = 0x0006_9005;

const PR_ATTACH_LONG_FILENAME: u16 = 0x3707;

/// At most this many attachments are unpacked from one stream.
pub const MAX_FILES: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub struct TnefFile {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TnefError {
    NotTnef,
    Truncated,
    BadProperty,
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], TnefError> {
        let end = self.pos.checked_add(n).ok_or(TnefError::Truncated)?;
        let s = self.b.get(self.pos..end).ok_or(TnefError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, TnefError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, TnefError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, TnefError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn done(&self) -> bool {
        self.pos >= self.b.len()
    }
    /// Skips padding to a multiple of 4 from `start`.
    fn align(&mut self, len: usize) -> Result<(), TnefError> {
        let pad = (4 - len % 4) % 4;
        self.take(pad).map(|_| ())
    }
}

#[derive(Default)]
struct Pending {
    title: Option<String>,
    long_name: Option<String>,
    data: Option<Vec<u8>>,
}

impl Pending {
    fn finish(self, n: usize, out: &mut Vec<TnefFile>) {
        if let Some(data) = self.data {
            let name = self
                .long_name
                .or(self.title)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("attachment-{n}"));
            out.push(TnefFile { name, data });
        }
    }
}

/// Unpacks the attachments of a TNEF stream.
pub fn extract(stream: &[u8]) -> Result<Vec<TnefFile>, TnefError> {
    let mut r = Reader { b: stream, pos: 0 };
    if r.u32().map_err(|_| TnefError::NotTnef)? != SIGNATURE {
        return Err(TnefError::NotTnef);
    }
    r.u16()?; // legacy key
    let mut out = Vec::new();
    let mut current: Option<Pending> = None;
    while !r.done() && out.len() < MAX_FILES {
        let level = r.u8()?;
        let id = r.u32()?;
        let len = r.u32()? as usize;
        let data = r.take(len)?;
        r.u16()?; // checksum: not checked, as other readers do not either
        if level != LVL_ATTACHMENT {
            continue;
        }
        match id {
            ATT_ATTACH_REND_DATA => {
                if let Some(p) = current.take() {
                    p.finish(out.len() + 1, &mut out);
                }
                current = Some(Pending::default());
            }
            ATT_ATTACH_TITLE => {
                current.get_or_insert_with(Pending::default).title = Some(c_string(data));
            }
            ATT_ATTACH_DATA => {
                current.get_or_insert_with(Pending::default).data = Some(data.to_vec());
            }
            ATT_ATTACHMENT => {
                if let Some(name) = long_filename(data)? {
                    current.get_or_insert_with(Pending::default).long_name = Some(name);
                }
            }
            _ => {}
        }
    }
    if let Some(p) = current.take() {
        p.finish(out.len() + 1, &mut out);
    }
    Ok(out)
}

/// An 8-bit string up to its first NUL (Windows-1252 bytes above 0x7F become U+FFFD).
fn c_string(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn utf16_string(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Finds `PR_ATTACH_LONG_FILENAME` in an `attAttachment` MAPI property list.
fn long_filename(b: &[u8]) -> Result<Option<String>, TnefError> {
    let mut r = Reader { b, pos: 0 };
    let count = r.u32()?;
    let mut found = None;
    for _ in 0..count.min(4096) {
        let ptype = r.u16()?;
        let pid = r.u16()?;
        if pid >= 0x8000 {
            r.take(16)?; // GUID
            if r.u32()? == 0 {
                r.u32()?;
            } else {
                let len = r.u32()? as usize;
                r.take(len)?;
                r.align(len)?;
            }
        }
        let multi = ptype & 0x1000 != 0;
        let base = ptype & !0x1000;
        let values = if multi || matches!(base, 0x001E | 0x001F | 0x0102 | 0x000D) {
            r.u32()?.min(4096)
        } else {
            1
        };
        for _ in 0..values {
            match base {
                0x001E | 0x001F | 0x0102 | 0x000D => {
                    let len = r.u32()? as usize;
                    let v = r.take(len)?;
                    r.align(len)?;
                    if pid == PR_ATTACH_LONG_FILENAME && found.is_none() {
                        found = Some(match base {
                            0x001F => utf16_string(v),
                            _ => c_string(v),
                        });
                    }
                }
                0x0001 | 0x0002 | 0x0003 | 0x0004 | 0x000A | 0x000B => {
                    r.take(4)?;
                }
                0x0005 | 0x0006 | 0x0007 | 0x0014 | 0x0040 => {
                    r.take(8)?;
                }
                0x0048 => {
                    r.take(16)?;
                }
                _ => return Err(TnefError::BadProperty),
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn attr(out: &mut Vec<u8>, level: u8, id: u32, data: &[u8]) {
        out.push(level);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        let sum = data.iter().fold(0u16, |s, &b| s.wrapping_add(u16::from(b)));
        out.extend_from_slice(&sum.to_le_bytes());
    }

    /// A TNEF stream with two attachments: one named by its MAPI long filename (Unicode), one by its title.
    pub(crate) fn sample() -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(&SIGNATURE.to_le_bytes());
        s.extend_from_slice(&0x1234u16.to_le_bytes());
        attr(&mut s, 0x01, 0x0008_9006, &[1, 0, 0, 0]); // attTnefVersion
        attr(&mut s, LVL_ATTACHMENT, ATT_ATTACH_REND_DATA, &[0; 14]);
        attr(&mut s, LVL_ATTACHMENT, ATT_ATTACH_TITLE, b"REPORT~1.PDF\0");
        attr(&mut s, LVL_ATTACHMENT, ATT_ATTACH_DATA, b"%PDF-1.7 report");
        let name: Vec<u8> = "quarterly report.pdf\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut props = Vec::new();
        props.extend_from_slice(&2u32.to_le_bytes());
        props.extend_from_slice(&0x0003u16.to_le_bytes()); // PT_LONG
        props.extend_from_slice(&0x0E21u16.to_le_bytes());
        props.extend_from_slice(&7u32.to_le_bytes());
        props.extend_from_slice(&0x001Fu16.to_le_bytes()); // PT_UNICODE
        props.extend_from_slice(&PR_ATTACH_LONG_FILENAME.to_le_bytes());
        props.extend_from_slice(&1u32.to_le_bytes());
        props.extend_from_slice(&(name.len() as u32).to_le_bytes());
        props.extend_from_slice(&name);
        props.extend(std::iter::repeat_n(0, (4 - name.len() % 4) % 4));
        attr(&mut s, LVL_ATTACHMENT, ATT_ATTACHMENT, &props);
        attr(&mut s, LVL_ATTACHMENT, ATT_ATTACH_REND_DATA, &[0; 14]);
        attr(&mut s, LVL_ATTACHMENT, ATT_ATTACH_TITLE, b"notes.txt\0");
        attr(&mut s, LVL_ATTACHMENT, ATT_ATTACH_DATA, b"plain notes");
        s
    }

    #[test]
    fn b6_tnef_unpacked() {
        let files = extract(&sample()).unwrap();
        assert_eq!(
            files,
            vec![
                TnefFile {
                    name: "quarterly report.pdf".into(),
                    data: b"%PDF-1.7 report".to_vec()
                },
                TnefFile {
                    name: "notes.txt".into(),
                    data: b"plain notes".to_vec()
                },
            ]
        );
    }

    #[test]
    fn b6_tnef_malformed() {
        assert_eq!(extract(b"PK\x03\x04"), Err(TnefError::NotTnef));
        assert_eq!(extract(b""), Err(TnefError::NotTnef));
        let s = sample();
        for cut in [7, 20, s.len() - 3] {
            assert_eq!(extract(&s[..cut]), Err(TnefError::Truncated), "{cut}");
        }
        let mut huge = SIGNATURE.to_le_bytes().to_vec();
        huge.extend_from_slice(&[0, 0, 2, 0x0f, 0x80, 6, 0, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(extract(&huge), Err(TnefError::Truncated));
    }
}

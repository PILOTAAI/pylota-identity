//! The content of an opaque S/MIME `signed-data` part (B9: signed-only mail keeps its content). A
//! minimal BER reader walks `ContentInfo` → `SignedData` → `encapContentInfo` → `eContent` (RFC 5652
//! § 3, § 5.1, § 5.2), accepting the indefinite lengths and constructed OCTET STRINGs that mail clients
//! emit. The signature is never verified.

const OID_SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];

/// Nesting allowed inside a constructed OCTET STRING.
const MAX_DEPTH: usize = 16;

#[derive(Clone, Copy)]
struct Tlv<'a> {
    tag: u8,
    constructed: bool,
    /// The contents; for an indefinite length, everything up to the end-of-contents octets.
    body: &'a [u8],
}

/// Reads one TLV from `b`, returning it and the bytes after it.
fn tlv(b: &[u8], depth: usize) -> Option<(Tlv<'_>, &[u8])> {
    let (&first, rest) = b.split_first()?;
    if first & 0x1f == 0x1f {
        return None; // high tag numbers do not occur on this path
    }
    let (&len0, rest) = rest.split_first()?;
    let constructed = first & 0x20 != 0;
    if len0 == 0x80 {
        if !constructed || depth > MAX_DEPTH {
            return None;
        }
        // Indefinite: walk children until 00 00.
        let mut cur = rest;
        loop {
            if cur.starts_with(&[0, 0]) {
                let used = rest.len() - cur.len();
                return Some((
                    Tlv {
                        tag: first,
                        constructed,
                        body: &rest[..used],
                    },
                    &cur[2..],
                ));
            }
            let (_, next) = tlv(cur, depth + 1)?;
            cur = next;
        }
    }
    let (len, rest) = if len0 & 0x80 == 0 {
        (usize::from(len0), rest)
    } else {
        let n = usize::from(len0 & 0x7f);
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let len = rest[..n]
            .iter()
            .fold(0usize, |a, &x| (a << 8) | usize::from(x));
        (len, &rest[n..])
    };
    if rest.len() < len {
        return None;
    }
    Some((
        Tlv {
            tag: first,
            constructed,
            body: &rest[..len],
        },
        &rest[len..],
    ))
}

fn children(body: &[u8], depth: usize) -> Option<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    let mut cur = body;
    while !cur.is_empty() {
        let (t, next) = tlv(cur, depth)?;
        out.push(t);
        cur = next;
    }
    Some(out)
}

fn octets(t: Tlv<'_>, depth: usize, out: &mut Vec<u8>) -> Option<()> {
    if t.tag & 0x1f != 0x04 || depth > MAX_DEPTH {
        return None;
    }
    if t.constructed {
        for c in children(t.body, depth + 1)? {
            octets(c, depth + 1, out)?;
        }
    } else {
        out.extend_from_slice(t.body);
    }
    Some(())
}

/// The encapsulated content of a `signed-data` ContentInfo, or `None` when it is not one or is detached.
pub fn signed_content(der: &[u8]) -> Option<Vec<u8>> {
    let (info, _) = tlv(der, 0)?;
    let info = children(info.body, 0)?;
    let (oid, explicit) = (info.first()?, info.get(1)?);
    if oid.tag != 0x06 || oid.body != OID_SIGNED_DATA || explicit.tag != 0xa0 {
        return None;
    }
    let signed = *children(explicit.body, 0)?.first()?;
    let fields = children(signed.body, 0)?;
    // version, digestAlgorithms, encapContentInfo
    let encap = fields.get(2)?;
    let encap = children(encap.body, 0)?;
    let e_content = encap.get(1)?;
    if e_content.tag != 0xa0 {
        return None;
    }
    let mut out = Vec::new();
    octets(*children(e_content.body, 0)?.first()?, 0, &mut out)?;
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn der(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        if body.len() < 128 {
            v.push(body.len() as u8);
        } else {
            v.push(0x82);
            v.extend_from_slice(&(body.len() as u16).to_be_bytes());
        }
        v.extend_from_slice(body);
        v
    }

    fn indefinite(tag: u8, parts: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![tag, 0x80];
        for p in parts {
            v.extend_from_slice(p);
        }
        v.extend_from_slice(&[0, 0]);
        v
    }

    /// A `signed-data` ContentInfo around `content`, BER with indefinite lengths and a constructed
    /// OCTET STRING split in two, as Outlook writes it.
    pub(crate) fn sample(content: &[u8]) -> Vec<u8> {
        let (a, b) = content.split_at(content.len() / 2);
        let e_content = indefinite(0xa0, &[indefinite(0x24, &[der(0x04, a), der(0x04, b)])]);
        let data_oid = der(
            0x06,
            &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01],
        );
        let encap = indefinite(0x30, &[data_oid, e_content]);
        let signed = indefinite(
            0x30,
            &[der(0x02, &[1]), der(0x31, &[]), encap, der(0x31, &[])],
        );
        indefinite(
            0x30,
            &[der(0x06, OID_SIGNED_DATA), indefinite(0xa0, &[signed])],
        )
    }

    /// Covers: B9
    #[test]
    fn b9_opaque_signed_content() {
        let content = b"Content-Type: text/plain\r\n\r\nSigned body\r\n";
        assert_eq!(
            signed_content(&sample(content)).as_deref(),
            Some(&content[..])
        );
        // Definite lengths too.
        let encap = der(
            0x30,
            &[der(0x06, &[0x2a]), der(0xa0, &der(0x04, b"x"))].concat(),
        );
        let signed = der(0x30, &[der(0x02, &[1]), der(0x31, &[]), encap].concat());
        let info = der(
            0x30,
            &[der(0x06, OID_SIGNED_DATA), der(0xa0, &signed)].concat(),
        );
        assert_eq!(signed_content(&info).as_deref(), Some(&b"x"[..]));
    }

    /// Covers: B9
    #[test]
    fn b9_malformed_is_none() {
        assert_eq!(signed_content(b""), None);
        assert_eq!(signed_content(&[0x30, 0x80]), None);
        assert_eq!(signed_content(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]), None);
        let s = sample(b"abcdef");
        for cut in 0..s.len() {
            assert_eq!(signed_content(&s[..cut]), None, "{cut}");
        }
        // Deep nesting stops.
        let mut deep = [0x24, 0x80].repeat(100);
        deep.extend(std::iter::repeat_n(0, 200));
        assert_eq!(signed_content(&deep), None);
    }
}

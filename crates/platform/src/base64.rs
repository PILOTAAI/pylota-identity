//! Standard base64 (RFC 4648 § 4) decoding for secrets, and base64url (§ 5, unpadded) encoding for the
//! `X-PM-Rpc` header. `platform` may not depend on the `base64` crate (Rust workspace § 2), and this
//! is all it needs.

/// Base64url without padding.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn encode_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | (u32::from(b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

/// Decodes padded or unpadded standard base64; `None` on any other character or a dangling bit group.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim().trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // A single leftover character (6 bits) cannot be a valid encoding; leftover bits must be zero.
    if bits >= 6 || acc & ((1 << bits) - 1) != 0 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode_url};

    #[test]
    fn rfc4648_vectors() {
        for (text, bytes) in [
            ("", &b""[..]),
            ("Zg==", b"f"),
            ("Zm8=", b"fo"),
            ("Zm9v", b"foo"),
            ("Zm9vYg==", b"foob"),
            ("Zm9vYmE=", b"fooba"),
            ("Zm9vYmFy", b"foobar"),
            ("Zm9vYmE", b"fooba"),
        ] {
            assert_eq!(decode(text).as_deref(), Some(bytes), "{text}");
        }
        assert_eq!(encode_url(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode_url(b"fo"), "Zm8");
        assert_eq!(encode_url(&[0xfb, 0xff]), "-_8");
        for bad in ["Z", "Zm9v!", "Zh==", "Zm-_"] {
            assert_eq!(decode(bad), None, "{bad}");
        }
    }
}

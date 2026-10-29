//! A minimal ZIP writer and reader for exports ([Privacy and erasure § 9.2]): stored entries (no
//! compression), each with its CRC-32, written batch by batch; the central directory and the end
//! record close the archive. The reader exists for tests and checks every CRC.
//!
//! [Privacy and erasure § 9.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

/// The CRC-32 (IEEE) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// 1980-01-01 00:00 in MS-DOS form: entries carry no timestamp of their own.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = (1 << 5) | 1;

/// One entry: its local header and data at `offset`, and its central-directory record.
pub fn entry(name: &str, data: &[u8], offset: u64) -> (Vec<u8>, Vec<u8>) {
    let crc = crc32(data);
    let size = data.len() as u32;
    let n = name.as_bytes();
    let mut local = Vec::with_capacity(30 + n.len() + data.len());
    local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    local.extend_from_slice(&20u16.to_le_bytes()); // version needed
    local.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
    local.extend_from_slice(&0u16.to_le_bytes()); // stored
    local.extend_from_slice(&DOS_TIME.to_le_bytes());
    local.extend_from_slice(&DOS_DATE.to_le_bytes());
    local.extend_from_slice(&crc.to_le_bytes());
    local.extend_from_slice(&size.to_le_bytes());
    local.extend_from_slice(&size.to_le_bytes());
    local.extend_from_slice(&(n.len() as u16).to_le_bytes());
    local.extend_from_slice(&0u16.to_le_bytes());
    local.extend_from_slice(n);
    local.extend_from_slice(data);
    let mut cd = Vec::with_capacity(46 + n.len());
    cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    cd.extend_from_slice(&20u16.to_le_bytes()); // made by
    cd.extend_from_slice(&20u16.to_le_bytes()); // needed
    cd.extend_from_slice(&0x0800u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&DOS_TIME.to_le_bytes());
    cd.extend_from_slice(&DOS_DATE.to_le_bytes());
    cd.extend_from_slice(&crc.to_le_bytes());
    cd.extend_from_slice(&size.to_le_bytes());
    cd.extend_from_slice(&size.to_le_bytes());
    cd.extend_from_slice(&(n.len() as u16).to_le_bytes());
    cd.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attributes
    cd.extend_from_slice(&0u32.to_le_bytes()); // external attributes
    cd.extend_from_slice(&(offset as u32).to_le_bytes());
    cd.extend_from_slice(n);
    (local, cd)
}

/// The end of central directory record for `count` entries whose directory (`cd_len` bytes)
/// starts at `cd_offset`.
pub fn end(count: u64, cd_len: u64, cd_offset: u64) -> Vec<u8> {
    let mut e = Vec::with_capacity(22);
    e.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    e.extend_from_slice(&[0; 4]);
    e.extend_from_slice(&(count as u16).to_le_bytes());
    e.extend_from_slice(&(count as u16).to_le_bytes());
    e.extend_from_slice(&(cd_len as u32).to_le_bytes());
    e.extend_from_slice(&(cd_offset as u32).to_le_bytes());
    e.extend_from_slice(&0u16.to_le_bytes());
    e
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Every entry of a stored-only archive, `(name, data)`, in directory order; `None` when the
/// archive is malformed or a CRC does not match.
pub fn read(zip: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let eocd = (0..=zip.len().checked_sub(22)?)
        .rev()
        .find(|&i| u32_at(zip, i) == Some(0x0605_4b50))?;
    let count = usize::from(u16_at(zip, eocd + 10)?);
    let mut p = u32_at(zip, eocd + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(zip, p)? != 0x0201_4b50 {
            return None;
        }
        let crc = u32_at(zip, p + 16)?;
        let size = u32_at(zip, p + 20)? as usize;
        let name_len = usize::from(u16_at(zip, p + 28)?);
        let extra = usize::from(u16_at(zip, p + 30)?) + usize::from(u16_at(zip, p + 32)?);
        let offset = u32_at(zip, p + 42)? as usize;
        let name = String::from_utf8(zip.get(p + 46..p + 46 + name_len)?.to_vec()).ok()?;
        let lname = usize::from(u16_at(zip, offset + 26)?);
        let lextra = usize::from(u16_at(zip, offset + 28)?);
        let start = offset + 30 + lname + lextra;
        let data = zip.get(start..start + size)?.to_vec();
        if crc32(&data) != crc {
            return None;
        }
        out.push((name, data));
        p += 46 + name_len + extra;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_of_known_input() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    /// Entries written in two batches read back with their names and bytes.
    #[test]
    fn round_trip() {
        let (a, cda) = entry("messages.json", b"{\"x\":1}", 0);
        let (b, cdb) = entry(
            "eml/idn_1/m.eml",
            b"Subject: hi\r\n\r\nbody",
            a.len() as u64,
        );
        let mut z = [a, b].concat();
        let cd = [cda, cdb].concat();
        let off = z.len() as u64;
        z.extend_from_slice(&cd);
        z.extend_from_slice(&end(2, cd.len() as u64, off));
        let r = read(&z).unwrap();
        assert_eq!(r[0], ("messages.json".to_owned(), b"{\"x\":1}".to_vec()));
        assert_eq!(r[1].0, "eml/idn_1/m.eml");
        // A flipped byte fails the CRC.
        let mut bad = z.clone();
        bad[31 + "messages.json".len()] ^= 1;
        assert!(read(&bad).is_none());
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! The reader of ZIP packages, the container of 3MF files: entries kept as they are or compressed
//! with DEFLATE, found through the central directory (so packages that write the sizes after the
//! data, as most slicers and CAD programs do, are read too).
//!
//! The reader never panics on a damaged package, checks the CRC-32 of every entry it reads, refuses
//! names that leave the package (`..`, absolute paths, drive letters), encrypted entries and
//! ZIP64 packages by name, and stops at the limits it is given: the number of entries, the size of
//! an entry and the sum of all of them, so a package that claims to hold a mountain is refused
//! before it is unpacked.

use crate::inflate::inflate;
use crate::interchange::crc32;

/// What a package may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLimits {
    /// The most entries (files; folders are not counted).
    pub max_entries: usize,
    /// The most bytes of one entry once unpacked.
    pub max_entry_bytes: usize,
    /// The most bytes of all the entries once unpacked.
    pub max_total_bytes: usize,
}

impl ZipLimits {
    /// The limits that the settings give: the entries are counted as they are and the size of an
    /// entry is given in kibibytes; the whole package may hold four entries of the largest size.
    pub fn new(max_entries: u32, max_entry_kib: u32) -> ZipLimits {
        let entry = max_entry_kib as usize * 1024;
        ZipLimits {
            max_entries: max_entries as usize,
            max_entry_bytes: entry,
            max_total_bytes: entry.saturating_mul(4),
        }
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Result<usize, String> {
    bytes
        .get(at..at.checked_add(2).ok_or("the package is damaged")?)
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
        .ok_or_else(|| "the package ends too soon".to_owned())
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at.checked_add(4).ok_or("the package is damaged")?)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "the package ends too soon".to_owned())
}

/// Whether a name stays inside the package.
fn safe_name(name: &str) -> bool {
    !(name.is_empty()
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains(':')
        || name.contains('\0')
        || name
            .split(['/', '\\'])
            .any(|part| part == ".." || part == "."))
}

/// The entries of a package, with their names and their unpacked bytes, in the order of the
/// directory.
pub fn read_zip(bytes: &[u8], limits: &ZipLimits) -> Result<Vec<(String, Vec<u8>)>, String> {
    // the end of the directory is within the last 64 KiB and 22 bytes
    let from = bytes.len().saturating_sub(22 + 65_535);
    let end = (from..bytes.len().saturating_sub(21))
        .rev()
        .find(|at| bytes[*at..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(|| {
            "the file is not a package: the end of its directory is missing".to_owned()
        })?;
    let count = u16_at(bytes, end + 10)?;
    let dir_size = u32_at(bytes, end + 12)? as usize;
    let dir_at = u32_at(bytes, end + 16)? as usize;
    if count == 0xffff || dir_size == 0xffff_ffff || dir_at == 0xffff_ffff {
        return Err("a package of the ZIP64 kind is not read".to_owned());
    }
    if count > limits.max_entries.saturating_add(count.min(1024)) {
        return Err(format!(
            "the package lists {count} entries and {} are allowed",
            limits.max_entries
        ));
    }
    let mut entries = Vec::new();
    let mut total = 0usize;
    let mut at = dir_at;
    for _ in 0..count {
        if u32_at(bytes, at)? != 0x0201_4b50 {
            return Err("the directory of the package is damaged".to_owned());
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        let crc = u32_at(bytes, at + 16)?;
        let packed = u32_at(bytes, at + 20)? as usize;
        let size = u32_at(bytes, at + 24)? as usize;
        let name_len = u16_at(bytes, at + 28)?;
        let extra_len = u16_at(bytes, at + 30)?;
        let comment_len = u16_at(bytes, at + 32)?;
        let local = u32_at(bytes, at + 42)? as usize;
        let name = bytes
            .get(at + 46..at + 46 + name_len)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .ok_or_else(|| "the package ends too soon".to_owned())?;
        at += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        if !safe_name(&name) {
            return Err(format!("{name}: the name leaves the package"));
        }
        if flags & 1 != 0 {
            return Err(format!("{name}: the entry is encrypted"));
        }
        if size == 0xffff_ffff || packed == 0xffff_ffff {
            return Err("a package of the ZIP64 kind is not read".to_owned());
        }
        if entries.len() >= limits.max_entries {
            return Err(format!(
                "the package holds more than {} entries",
                limits.max_entries
            ));
        }
        if size > limits.max_entry_bytes {
            return Err(format!(
                "{name}: the entry holds {size} bytes and {} are allowed",
                limits.max_entry_bytes
            ));
        }
        total = total.saturating_add(size);
        if total > limits.max_total_bytes {
            return Err(format!(
                "the package holds more than {} bytes in all",
                limits.max_total_bytes
            ));
        }
        if u32_at(bytes, local)? != 0x0403_4b50 {
            return Err(format!("{name}: the entry is not where the directory says"));
        }
        let data_at = local + 30 + u16_at(bytes, local + 26)? + u16_at(bytes, local + 28)?;
        let stored = bytes
            .get(data_at..data_at.saturating_add(packed))
            .ok_or_else(|| format!("{name}: the entry ends too soon"))?;
        let data = match method {
            0 => {
                if packed != size {
                    return Err(format!("{name}: the sizes of a stored entry differ"));
                }
                stored.to_vec()
            }
            8 => inflate(stored, size).map_err(|e| format!("{name}: {e}"))?,
            other => {
                return Err(format!(
                    "{name}: the compression method {other} is not read"
                ))
            }
        };
        if data.len() != size {
            return Err(format!("{name}: the entry does not hold the size it says"));
        }
        if crc32(&data) != crc {
            return Err(format!("{name}: the CRC-32 does not match"));
        }
        entries.push((name, data));
    }
    Ok(entries)
}

/// A package of entries compressed with the method DEFLATE in stored blocks, for the tests and for
/// anyone who wants a package that uses the method the other programs use.
pub fn write_zip_deflate(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in entries {
        let packed = crate::inflate::deflate_stored(data);
        let crc = crc32(data);
        let local = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 8, 0, 0, 0, 0x21, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);
        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&[20, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0x21, 0]);
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        directory.extend_from_slice(&local.to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }
    let dir_at = out.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&dir_at.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> ZipLimits {
        ZipLimits::new(100, 1024)
    }

    #[test]
    fn a_package_of_stored_entries_written_by_the_program_is_read_through_its_directory() {
        let zip = crate::interchange::three_mf(&[]);
        let entries = read_zip(&zip, &limits()).unwrap();
        assert!(entries.iter().any(|(n, _)| n == "3D/3dmodel.model"));
        assert!(entries.iter().any(|(n, _)| n == "[Content_Types].xml"));
    }

    #[test]
    fn entries_compressed_with_deflate_are_unpacked_and_checked() {
        let a = vec![7u8; 100_000];
        let zip = write_zip_deflate(&[("a.bin", &a), ("dir/b.txt", b"hello"), ("empty", b"")]);
        let entries = read_zip(&zip, &limits()).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], ("a.bin".to_owned(), a));
        assert_eq!(entries[1].1, b"hello");
        assert!(entries[2].1.is_empty());
    }

    #[test]
    fn a_real_package_of_another_program_is_read() {
        // made by the zipfile module of Python: two entries, one compressed and one stored, with the
        // sizes in the directory
        let zip = ZIP_FROM_PYTHON;
        let entries = read_zip(&zip, &limits()).unwrap();
        assert_eq!(entries[0].0, "a.txt");
        assert_eq!(entries[0].1, b"abcabcabcabcabcabcabcabcabcabc");
        assert_eq!(entries[1], ("b.txt".to_owned(), b"xyz".to_vec()));
    }

    #[test]
    fn the_limits_are_told_by_name() {
        let big = vec![1u8; 5000];
        let zip = write_zip_deflate(&[("a", &big), ("b", &big)]);
        let small = ZipLimits {
            max_entries: 10,
            max_entry_bytes: 4000,
            max_total_bytes: 100_000,
        };
        assert!(read_zip(&zip, &small).unwrap_err().contains("5000 bytes"));
        let total = ZipLimits {
            max_entries: 10,
            max_entry_bytes: 6000,
            max_total_bytes: 9000,
        };
        assert!(read_zip(&zip, &total).unwrap_err().contains("in all"));
        let few = ZipLimits {
            max_entries: 1,
            max_entry_bytes: 6000,
            max_total_bytes: 100_000,
        };
        assert!(read_zip(&zip, &few).unwrap_err().contains("more than 1"));
    }

    #[test]
    fn a_size_that_lies_is_caught_before_the_memory_is_filled() {
        let mut zip = write_zip_deflate(&[("a", &[3u8; 3000])]);
        // the unpacked size in the directory says 10, the data holds 3000
        let at = zip
            .windows(4)
            .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
            .unwrap();
        zip[at + 24..at + 28].copy_from_slice(&10u32.to_le_bytes());
        let error = read_zip(&zip, &limits()).unwrap_err();
        assert!(error.contains("more than 10 bytes"), "{error}");
    }

    #[test]
    fn names_that_leave_the_package_and_other_unsupported_things_are_refused_by_name() {
        let zip = write_zip_deflate(&[("../x", b"1")]);
        assert!(read_zip(&zip, &limits())
            .unwrap_err()
            .contains("leaves the package"));
        let zip = write_zip_deflate(&[("C:/x", b"1")]);
        assert!(read_zip(&zip, &limits())
            .unwrap_err()
            .contains("leaves the package"));
        let mut zip = write_zip_deflate(&[("a", b"1")]);
        let at = zip
            .windows(4)
            .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
            .unwrap();
        zip[at + 8] = 1;
        assert!(read_zip(&zip, &limits()).unwrap_err().contains("encrypted"));
        zip[at + 8] = 0;
        zip[at + 10] = 12;
        assert!(read_zip(&zip, &limits()).unwrap_err().contains("method 12"));
        assert!(read_zip(b"not a package at all", &limits()).is_err());
        assert!(read_zip(&[], &limits()).is_err());
    }

    #[test]
    fn a_damaged_entry_is_refused_by_its_crc() {
        let mut zip = write_zip_deflate(&[("a", b"hello world")]);
        // flip a byte of the data (after the 30 bytes of the header, the name and 5 of the block)
        zip[30 + 1 + 5] ^= 0x40;
        assert!(read_zip(&zip, &limits()).unwrap_err().contains("CRC-32"));
    }

    #[test]
    fn damaged_packages_never_panic_with_a_fixed_seed() {
        let base = write_zip_deflate(&[("a", &[9u8; 300]), ("b/c", b"text")]);
        let mut seed = 77u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize
        };
        for _ in 0..3000 {
            let mut zip = base.clone();
            for _ in 0..1 + next() % 4 {
                let at = next() % zip.len();
                zip[at] = next() as u8;
            }
            let cut = next() % (zip.len() + 1);
            let _ = read_zip(&zip[..cut], &limits());
            let _ = read_zip(&zip, &limits());
        }
    }

    /// A package made by the zipfile module of Python.
    const ZIP_FROM_PYTHON: [u8; 204] = [
        0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x21, 0x50, 0x81,
        0xfc, 0xb1, 0x48, 0x07, 0x00, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
        0x61, 0x2e, 0x74, 0x78, 0x74, 0x4b, 0x4c, 0x4a, 0x4e, 0xc4, 0x8d, 0x00, 0x50, 0x4b, 0x03,
        0x04, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x21, 0x50, 0x67, 0xba, 0x8e, 0xeb,
        0x03, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x62, 0x2e, 0x74,
        0x78, 0x74, 0x78, 0x79, 0x7a, 0x50, 0x4b, 0x01, 0x02, 0x14, 0x00, 0x14, 0x00, 0x00, 0x00,
        0x08, 0x00, 0x00, 0x00, 0x21, 0x50, 0x81, 0xfc, 0xb1, 0x48, 0x07, 0x00, 0x00, 0x00, 0x1e,
        0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x80, 0x01, 0x00, 0x00, 0x00, 0x00, 0x61, 0x2e, 0x74, 0x78, 0x74, 0x50, 0x4b, 0x01, 0x02,
        0x14, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x21, 0x50, 0x67, 0xba, 0x8e,
        0xeb, 0x03, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x01, 0x2a, 0x00, 0x00, 0x00, 0x62, 0x2e, 0x74,
        0x78, 0x74, 0x50, 0x4b, 0x05, 0x06, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x66,
        0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
}

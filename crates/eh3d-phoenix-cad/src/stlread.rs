// SPDX-License-Identifier: GPL-3.0-or-later
//! The tolerant reader of STL files, for the files that other programs write.
//!
//! It tells a text file from a binary one by the size first (a binary file that starts with the
//! word `solid` in its header is common and is read as binary), reads every `solid` of a text file
//! as a solid of its own with its name, accepts any case and any spacing of the keywords, leaves
//! out the triangles that have a coordinate that is not a finite number, and reads the complete
//! triangles of a binary file that ends too soon. Everything it did not take as it was is told in a
//! warning that has a name, so that nothing is lost in silence. It refuses a file with more
//! triangles than it is given room for before it reserves any memory, and it never panics.

/// What was not as it should be in a file that was read all the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadWarning {
    /// The file has bytes after the last triangle (how many).
    TrailingBytes(usize),
    /// The header says more triangles than the file holds (the ones it says, the ones it holds).
    Truncated(usize, usize),
    /// Triangles with a coordinate that is not a finite number were left out (how many).
    NonFinite(usize),
    /// The file is binary although its header starts with `solid`.
    BinaryWithSolidHeader,
    /// Something of the file is not read (what).
    Ignored(String),
}

impl ReadWarning {
    /// The name of the warning, the same in every language.
    pub fn name(&self) -> &'static str {
        match self {
            ReadWarning::TrailingBytes(_) => "trailing_bytes",
            ReadWarning::Truncated(..) => "truncated",
            ReadWarning::NonFinite(_) => "non_finite",
            ReadWarning::BinaryWithSolidHeader => "binary_with_solid_header",
            ReadWarning::Ignored(_) => "ignored",
        }
    }
}

/// The solids of an STL file, each with its name and its triangles (nine millimetres each), and the
/// warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct StlFile {
    /// The solids in the order of the file; a binary file holds one, called `solid`.
    pub solids: Vec<(String, Vec<[f64; 9]>)>,
    /// What was not as it should be.
    pub warnings: Vec<ReadWarning>,
}

/// The farthest a coordinate may be from the origin, in millimetres (a million kilometres).
const FAR_MM: f64 = 1.0e9;

fn too_far(t: &[f64; 9]) -> bool {
    t.iter().any(|v| v.abs() > FAR_MM)
}

/// Reads the file; more than `max_triangles` triangles is refused.
pub fn read(bytes: &[u8], max_triangles: usize) -> Result<StlFile, String> {
    let mut warnings = Vec::new();
    let binary_size = (bytes.len() >= 84).then(|| {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        (count, 84usize.saturating_add(count.saturating_mul(50)))
    });
    let starts_solid = bytes
        .iter()
        .skip_while(|b| b.is_ascii_whitespace() || **b == 0xEF || **b == 0xBB || **b == 0xBF)
        .take(5)
        .map(u8::to_ascii_lowercase)
        .eq(b"solid".iter().copied());
    let exact_binary = matches!(binary_size, Some((_, size)) if size == bytes.len());
    let looks_text = starts_solid
        && !exact_binary
        && bytes
            .windows(5)
            .take(1 << 20)
            .any(|w| w.eq_ignore_ascii_case(b"facet") || w.eq_ignore_ascii_case(b"endso"));
    if looks_text {
        return read_text(bytes, max_triangles, warnings);
    }
    let Some((count, size)) = binary_size else {
        return Err(if starts_solid {
            "the file is a text file with no facet".to_owned()
        } else {
            "the file is shorter than the header of a binary STL file".to_owned()
        });
    };
    if count > max_triangles {
        return Err(format!(
            "the file holds {count} triangles and {max_triangles} are allowed"
        ));
    }
    if starts_solid {
        warnings.push(ReadWarning::BinaryWithSolidHeader);
    }
    let found = (bytes.len() - 84) / 50;
    let used = if size > bytes.len() {
        if found == 0 {
            return Err(format!(
                "the file says {count} triangles and holds none: it has {} bytes",
                bytes.len()
            ));
        }
        warnings.push(ReadWarning::Truncated(count, found));
        found
    } else {
        if bytes.len() > size {
            warnings.push(ReadWarning::TrailingBytes(bytes.len() - size));
        }
        count
    };
    let float = |at: usize| {
        f64::from(f32::from_le_bytes([
            bytes[at],
            bytes[at + 1],
            bytes[at + 2],
            bytes[at + 3],
        ]))
    };
    let mut triangles = Vec::with_capacity(used);
    let mut dropped = 0;
    for i in 0..used {
        let base = 84 + 50 * i + 12;
        let mut t = [0.0; 9];
        for (k, slot) in t.iter_mut().enumerate() {
            *slot = float(base + 4 * k);
        }
        if t.iter().any(|v| !v.is_finite()) {
            dropped += 1;
        } else if too_far(&t) {
            return Err(format!("triangle {} is too far from the origin", i + 1));
        } else {
            triangles.push(t);
        }
    }
    if dropped > 0 {
        warnings.push(ReadWarning::NonFinite(dropped));
    }
    Ok(StlFile {
        solids: vec![("solid".to_owned(), triangles)],
        warnings,
    })
}

fn read_text(
    bytes: &[u8],
    max_triangles: usize,
    mut warnings: Vec<ReadWarning>,
) -> Result<StlFile, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut solids: Vec<(String, Vec<[f64; 9]>)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut corners: Vec<f64> = Vec::new();
    let mut in_facet = false;
    let mut total = 0usize;
    let mut dropped = 0usize;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim();
        let mut words = line.split_whitespace();
        let Some(first) = words.next() else {
            continue;
        };
        match first.to_ascii_lowercase().as_str() {
            "solid" => {
                let name = line[first.len()..].trim();
                solids.push((
                    if name.is_empty() { "solid" } else { name }.to_owned(),
                    Vec::new(),
                ));
                current = Some(solids.len() - 1);
            }
            "facet" => {
                if in_facet {
                    return Err(format!("line {}: a facet is not closed", n + 1));
                }
                in_facet = true;
                corners.clear();
                if current.is_none() {
                    solids.push(("solid".to_owned(), Vec::new()));
                    current = Some(solids.len() - 1);
                }
            }
            "vertex" => {
                if !in_facet {
                    return Err(format!("line {}: a vertex is outside a facet", n + 1));
                }
                let numbers: Vec<f64> = words
                    .map(|w| {
                        w.parse::<f64>()
                            .map_err(|_| format!("line {}: {w} is not a number", n + 1))
                    })
                    .collect::<Result<_, _>>()?;
                if numbers.len() != 3 {
                    return Err(format!("line {}: a vertex has three numbers", n + 1));
                }
                corners.extend(numbers);
            }
            "endfacet" => {
                if !in_facet {
                    return Err(format!(
                        "line {}: a facet is closed that was not open",
                        n + 1
                    ));
                }
                in_facet = false;
                if corners.len() != 9 {
                    return Err(format!(
                        "line {}: a facet has {} vertices and a triangle has three",
                        n + 1,
                        corners.len() / 3
                    ));
                }
                let mut t = [0.0; 9];
                t.copy_from_slice(&corners);
                if t.iter().any(|v| !v.is_finite()) {
                    dropped += 1;
                } else if too_far(&t) {
                    return Err(format!(
                        "line {}: a vertex is too far from the origin",
                        n + 1
                    ));
                } else {
                    total += 1;
                    if total > max_triangles {
                        return Err(format!(
                            "the file holds more than {max_triangles} triangles"
                        ));
                    }
                    if let Some(at) = current {
                        solids[at].1.push(t);
                    }
                }
            }
            "endsolid" => current = None,
            _ => {}
        }
    }
    if in_facet {
        return Err("the file ends inside a facet".to_owned());
    }
    solids.retain(|(_, t)| !t.is_empty());
    if solids.is_empty() {
        return Err("the text file holds no triangle".to_owned());
    }
    if dropped > 0 {
        warnings.push(ReadWarning::NonFinite(dropped));
    }
    Ok(StlFile { solids, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{stl_ascii, stl_binary, Mesh};

    fn tetra() -> Mesh {
        Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [10.0e6, 0.0, 0.0],
                [0.0, 10.0e6, 0.0],
                [0.0, 0.0, 10.0e6],
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }

    #[test]
    fn a_binary_file_and_a_text_file_of_the_same_solid_give_the_same_triangles() {
        let m = tetra();
        let bin = read(&stl_binary(&[("t", &m)]), 100).unwrap();
        let text = read(stl_ascii(&[("t", &m)]).as_bytes(), 100).unwrap();
        assert_eq!(bin.solids[0].1, text.solids[0].1);
        assert_eq!(bin.solids[0].0, "solid");
        assert_eq!(text.solids[0].0, "t");
        assert!(bin.warnings.is_empty() && text.warnings.is_empty());
    }

    #[test]
    fn a_text_file_with_several_solids_gives_one_solid_each_with_its_name() {
        let m = tetra();
        let text = stl_ascii(&[("first part", &m), ("second", &m)]);
        let file = read(text.as_bytes(), 100).unwrap();
        let names: Vec<&str> = file.solids.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["first_part", "second"]);
        assert_eq!(file.solids[1].1.len(), 4);
    }

    #[test]
    fn keywords_in_capitals_tabs_and_a_byte_order_mark_are_read() {
        let text = "\u{feff}SOLID Cap\r\n\tFACET NORMAL 0 0 1\r\n OUTER LOOP\r\n\tVERTEX 0 0 0\r\n\
                    VERTEX 1e1 0 0\r\nVERTEX\t0 10 0\r\n ENDLOOP\r\nENDFACET\r\nENDSOLID Cap\r\n";
        let file = read(text.as_bytes(), 10).unwrap();
        assert_eq!(file.solids[0].0, "Cap");
        assert_eq!(
            file.solids[0].1,
            vec![[0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0]]
        );
    }

    #[test]
    fn a_binary_file_whose_header_starts_with_solid_is_read_as_binary_and_says_so() {
        let m = tetra();
        let mut bin = stl_binary(&[("t", &m)]);
        bin[..5].copy_from_slice(b"solid");
        // the old reader would take it for text when "facet" were in the header
        bin[10..15].copy_from_slice(b"facet");
        let file = read(&bin, 100).unwrap();
        assert_eq!(file.solids[0].1.len(), 4);
        assert_eq!(file.warnings, vec![ReadWarning::BinaryWithSolidHeader]);
        assert_eq!(file.warnings[0].name(), "binary_with_solid_header");
    }

    #[test]
    fn bytes_after_the_last_triangle_and_a_file_that_ends_too_soon_are_told() {
        let m = tetra();
        let mut bin = stl_binary(&[("t", &m)]);
        let mut more = bin.clone();
        more.extend_from_slice(&[0u8; 7]);
        assert_eq!(
            read(&more, 100).unwrap().warnings,
            vec![ReadWarning::TrailingBytes(7)]
        );
        bin.truncate(84 + 50 * 3 + 20);
        let file = read(&bin, 100).unwrap();
        assert_eq!(file.solids[0].1.len(), 3);
        assert_eq!(file.warnings, vec![ReadWarning::Truncated(4, 3)]);
        // nothing complete is an error
        bin.truncate(84 + 10);
        assert!(read(&bin, 100).unwrap_err().contains("holds none"));
    }

    #[test]
    fn triangles_with_a_coordinate_that_is_not_a_number_are_left_out_and_counted() {
        let m = tetra();
        let mut bin = stl_binary(&[("t", &m)]);
        bin[84 + 12..84 + 16].copy_from_slice(&f32::NAN.to_le_bytes());
        let file = read(&bin, 100).unwrap();
        assert_eq!(file.solids[0].1.len(), 3);
        assert_eq!(file.warnings, vec![ReadWarning::NonFinite(1)]);
        let text = stl_ascii(&[("t", &m)]).replacen("vertex 0.000000", "vertex nan", 1);
        let file = read(text.as_bytes(), 100).unwrap();
        assert_eq!(file.solids[0].1.len(), 3);
        assert_eq!(file.warnings, vec![ReadWarning::NonFinite(1)]);
    }

    #[test]
    fn the_limit_is_checked_before_any_memory_is_reserved() {
        let mut bin = vec![b' '; 80];
        bin.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&bin, 1000).unwrap_err().contains("4294967295"));
        let m = tetra();
        assert!(read(&stl_binary(&[("t", &m)]), 3)
            .unwrap_err()
            .contains("3 are allowed"));
        let text = stl_ascii(&[("t", &m)]);
        assert!(read(text.as_bytes(), 3)
            .unwrap_err()
            .contains("more than 3"));
    }

    #[test]
    fn broken_text_files_are_refused_with_their_line() {
        let ok = "solid a\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid a\n";
        assert!(read(ok.as_bytes(), 10).is_ok());
        let two = ok.replace("vertex 0 1 0\n", "");
        assert!(read(two.as_bytes(), 10).unwrap_err().contains("2 vertices"));
        let word = ok.replace("vertex 1 0 0", "vertex 1 x 0");
        assert!(read(word.as_bytes(), 10).unwrap_err().contains("line 5"));
        let open = ok.replace("endfacet\n", "");
        assert!(read(open.as_bytes(), 10).is_err());
        assert!(read(b"solid empty\nendsolid empty\nfacet", 10).is_err());
        assert!(read(&[0u8; 10], 10).unwrap_err().contains("shorter"));
        let far = ok.replace("vertex 1 0 0", "vertex 1e30 0 0");
        assert!(read(far.as_bytes(), 10).unwrap_err().contains("too far"));
    }

    #[test]
    fn damaged_files_never_panic_with_a_fixed_seed() {
        let m = tetra();
        let bases = [
            stl_binary(&[("t", &m)]),
            stl_ascii(&[("t", &m)]).into_bytes(),
        ];
        let mut seed = 5u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize
        };
        for _ in 0..2000 {
            for base in &bases {
                let mut bytes = base.clone();
                for _ in 0..1 + next() % 4 {
                    let at = next() % bytes.len();
                    bytes[at] = next() as u8;
                }
                let cut = next() % (bytes.len() + 1);
                let _ = read(&bytes[..cut], 1000);
                let _ = read(&bytes, 1000);
            }
        }
    }
}

//! The domain `sketch` of the project file: the sketches of the project.
//!
//! A project keeps its sketches inside its own file, in a domain of its own with its own version,
//! beside the domain `mechanical`. The envelope of the project does not change: the domain is
//! named, has its length written in front of it and is optional, so a program that does not know
//! it opens the project, keeps the domain byte for byte and writes it back as it was. A project
//! that has no sketch has no such domain and is the same file as before.
//!
//! The text is one canonical form: the header `sketch <version>` and then, for each sketch in the
//! order of their names, the line `[sketch]` followed by the text of the sketch file
//! (`phoenix-sketch 1 ...`, see [`super::file`]). The same sketches always give the same bytes and
//! a text that is not in that form is refused with `PHOENIX-CAD-035`, naming the sketch and the
//! line. A domain of a newer version is neither read nor overwritten.
//!
//! The sketch file that sits next to a project (`.phoenixsketch`) is the way in and the way out:
//! it can be opened and put in the project, and a sketch of the project can be written to one.

use super::file::{from_text, to_text};
use super::settings::SketchSettings;
use super::Sketch;
use crate::project::Document;
use crate::CadError;

/// The name of the domain.
pub const SKETCH_DOMAIN: &str = "sketch";

/// The version of the text of the domain that this program writes and reads.
pub const SKETCH_DOMAIN_VERSION: u32 = 1;

const HEADER: &str = "sketch";
const MARK: &str = "[sketch]";

fn bad(why: impl Into<String>) -> CadError {
    CadError::SketchFile(why.into())
}

/// The version in the first line of the domain.
fn header_version(bytes: &[u8]) -> Result<u32, CadError> {
    let first = bytes.split(|b| *b == b'\n').next().unwrap_or(&[]);
    let line =
        std::str::from_utf8(first).map_err(|_| bad("the header of the sketches is not text"))?;
    match line.split_once(' ') {
        Some((HEADER, version)) => {
            let v = version
                .parse::<u32>()
                .ok()
                .filter(|v| v.to_string() == version)
                .ok_or_else(|| bad("the version of the sketches is not a number"))?;
            if v == 0 {
                Err(bad("the version of the sketches is zero"))
            } else {
                Ok(v)
            }
        }
        _ => Err(bad("the sketches do not start with their header")),
    }
}

/// The text of the sketches of a project, the same for the same sketches. Two sketches with the
/// same name, more sketches than the settings allow or a sketch larger than they allow are
/// refused.
pub fn to_bytes(sketches: &[Sketch], settings: &SketchSettings) -> Result<Vec<u8>, CadError> {
    if sketches.len() > settings.max_sketches {
        return Err(bad("there are more sketches than the settings allow"));
    }
    let mut ordered: Vec<&Sketch> = sketches.iter().collect();
    ordered.sort_by(|a, b| a.name().cmp(b.name()));
    for pair in ordered.windows(2) {
        if pair[0].name() == pair[1].name() {
            return Err(bad(format!(
                "the name {} is used by two sketches",
                pair[0].name()
            )));
        }
    }
    let mut out = format!("{HEADER} {SKETCH_DOMAIN_VERSION}\n");
    for sketch in ordered {
        let text = to_text(sketch);
        if text.len() as u64 > settings.max_file_bytes {
            return Err(bad(format!(
                "the sketch {} is larger than the settings allow",
                sketch.name()
            )));
        }
        out.push_str(MARK);
        out.push('\n');
        out.push_str(&text);
    }
    Ok(out.into_bytes())
}

/// Reads the text written by [`to_bytes`]. What is not in that one form, a damaged sketch, two
/// sketches of one name and a text over the limits of the settings are refused with
/// `PHOENIX-CAD-035`; so is a text of a newer version.
pub fn from_bytes(bytes: &[u8], settings: &SketchSettings) -> Result<Vec<Sketch>, CadError> {
    let version = header_version(bytes)?;
    if version > SKETCH_DOMAIN_VERSION {
        return Err(bad("the sketches were written by a newer version"));
    }
    let limit = settings
        .max_file_bytes
        .saturating_mul(settings.max_sketches as u64);
    if bytes.len() as u64 > limit {
        return Err(bad("the sketches are larger than the settings allow"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| bad("the sketches are not text"))?;
    if !text.ends_with('\n') {
        return Err(bad("the sketches do not end with a line end"));
    }
    let mut chunks: Vec<String> = Vec::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if index == 0 {
            continue;
        }
        if line == "[sketch]\n" {
            chunks.push(String::new());
        } else {
            match chunks.last_mut() {
                Some(chunk) => chunk.push_str(line),
                None => {
                    return Err(bad(
                        "the header of the sketches is followed by something else",
                    ))
                }
            }
        }
    }
    if chunks.len() > settings.max_sketches {
        return Err(bad("there are more sketches than the settings allow"));
    }
    let mut out: Vec<Sketch> = Vec::with_capacity(chunks.len());
    for (k, chunk) in chunks.iter().enumerate() {
        let sketch = from_text(chunk).map_err(|e| match e {
            CadError::SketchFile(why) => bad(format!("sketch {}: {why}", k + 1)),
            other => other,
        })?;
        if sketch.elements().len() > settings.max_elements
            || sketch.constraints().len() > settings.max_constraints
        {
            return Err(bad(format!(
                "sketch {} is larger than the settings allow",
                k + 1
            )));
        }
        if to_text(&sketch) != *chunk {
            return Err(bad(format!(
                "sketch {} is not written in the one form",
                k + 1
            )));
        }
        if let Some(last) = out.last() {
            if last.name() >= sketch.name() {
                return Err(bad(format!(
                    "the sketches are not in the order of their names at sketch {}",
                    k + 1
                )));
            }
        }
        out.push(sketch);
    }
    Ok(out)
}

impl Document {
    /// The version of the domain `sketch` as its first line says, or `None` when the file has no
    /// such domain.
    pub fn sketch_version(&self) -> Result<Option<u32>, CadError> {
        match self.get(SKETCH_DOMAIN) {
            Some(domain) => header_version(&domain.data).map(Some),
            None => Ok(None),
        }
    }

    /// The sketches of the project: none when the file has no such domain. A damaged domain, or
    /// one of a newer version, is an error (and the project is then opened read-only so that it
    /// is kept as it is).
    pub fn sketches(&self, settings: &SketchSettings) -> Result<Vec<Sketch>, CadError> {
        match self.get(SKETCH_DOMAIN) {
            Some(domain) => from_bytes(&domain.data, settings),
            None => Ok(Vec::new()),
        }
    }

    /// Writes the sketches in the domain `sketch`, which is optional; with no sketch the domain is
    /// taken away. A domain of a newer version, or one that cannot be read, is not written over:
    /// it is kept as it is and the call is refused.
    pub fn set_sketches(
        &mut self,
        sketches: &[Sketch],
        settings: &SketchSettings,
    ) -> Result<(), CadError> {
        if let Some(version) = self.sketch_version()? {
            if version > SKETCH_DOMAIN_VERSION {
                return Err(bad(
                    "the sketches were written by a newer version and are kept as they are",
                ));
            }
        }
        if sketches.is_empty() {
            if self.get(SKETCH_DOMAIN).is_some() {
                self.remove(SKETCH_DOMAIN)?;
            }
            return Ok(());
        }
        let bytes = to_bytes(sketches, settings)?;
        self.set(SKETCH_DOMAIN, false, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::sketch::SketchPlane;
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn settings() -> SketchSettings {
        SketchSettings::defaults()
    }

    fn plate(name: &str) -> Sketch {
        let mut k = Sketch::new(name, SketchPlane::on_face(Face::Top)).unwrap();
        k.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let c = k.add_point(mm(20), mm(12)).unwrap();
        k.add_circle(c, mm(3)).unwrap();
        k
    }

    #[test]
    fn the_sketches_are_written_in_the_order_of_their_names_and_read_back_equal() {
        let s = settings();
        let (a, b) = (plate("alpha"), plate("beta"));
        let bytes = to_bytes(&[b.clone(), a.clone()], &s).unwrap();
        assert!(bytes.starts_with(b"sketch 1\n[sketch]\nphoenix-sketch 1\nname alpha\n"));
        assert_eq!(bytes, to_bytes(&[a.clone(), b.clone()], &s).unwrap());
        assert_eq!(from_bytes(&bytes, &s).unwrap(), vec![a, b]);
    }

    #[test]
    fn two_sketches_of_one_name_and_too_many_sketches_are_refused() {
        let s = settings();
        let e = to_bytes(&[plate("a"), plate("a")], &s).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-035");
        assert!(e.to_string().contains("two sketches"), "{e}");
        let few = SketchSettings::with_user_config("[sketch]\nmax_sketches = 1\n").unwrap();
        assert!(to_bytes(&[plate("a"), plate("b")], &few).is_err());
        let two = to_bytes(&[plate("a"), plate("b")], &s).unwrap();
        assert!(from_bytes(&two, &few).is_err());
    }

    #[test]
    fn a_text_that_is_not_in_the_one_form_is_refused_with_the_sketch_and_the_line() {
        let s = settings();
        let good = String::from_utf8(to_bytes(&[plate("a"), plate("b")], &s).unwrap()).unwrap();
        let cases: Vec<(String, &str)> = vec![
            (good.replace("sketch 1\n", "sketch 01\n"), "version"),
            (good.replace("sketch 1\n", "sketch 0\n"), "zero"),
            (good.replacen("sketch 1\n", "sketches 1\n", 1), "header"),
            (good.trim_end().to_string(), "line end"),
            (good.replace("[sketch]\n", "[sketch]\n\n"), "sketch 1"),
            (good.replace("name a\n", "# note\nname a\n"), "one form"),
            (good.replace("name b\n", "name a\n"), "order"),
            (good.replace("e1 point", "e9 point"), "sketch 1"),
            (good.replacen("[sketch]\n", "x\n[sketch]\n", 1), "followed"),
            (good.replace("\n", "\r\n"), "version"),
        ];
        for (text, wanted) in cases {
            let e = from_bytes(text.as_bytes(), &s).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-035", "{text:?}");
            assert!(e.to_string().contains(wanted), "{wanted}: {e}");
        }
        assert!(from_bytes(b"sketch 1\n\xff\n", &s).is_err());
        assert!(from_bytes(b"sketch 1\n", &s).unwrap().is_empty());
    }

    #[test]
    fn a_domain_of_a_newer_version_is_neither_read_nor_overwritten() {
        let s = settings();
        let future = b"sketch 2\nsomething \xff that comes later\n".to_vec();
        let mut d = Document::new();
        d.set(SKETCH_DOMAIN, false, future.clone()).unwrap();
        assert_eq!(d.sketch_version().unwrap(), Some(2));
        let e = d.sketches(&s).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-035");
        assert!(e.to_string().contains("newer version"), "{e}");
        assert!(d.set_sketches(&[plate("a")], &s).is_err());
        assert!(d.set_sketches(&[], &s).is_err());
        assert_eq!(d.get(SKETCH_DOMAIN).unwrap().data, future);
        // and it goes through the file as it is
        let read = Document::from_bytes(&d.to_bytes(), 1 << 20).unwrap();
        assert_eq!(read.get(SKETCH_DOMAIN).unwrap().data, future);
    }

    #[test]
    fn a_damaged_domain_is_an_error_and_is_not_written_over() {
        let s = settings();
        let mut d = Document::new();
        d.set(SKETCH_DOMAIN, false, b"hello\n".to_vec()).unwrap();
        assert!(d.sketch_version().is_err());
        assert!(d.sketches(&s).is_err());
        assert!(d.set_sketches(&[plate("a")], &s).is_err());
        assert_eq!(d.get(SKETCH_DOMAIN).unwrap().data, b"hello\n".to_vec());
        // a header that is fine and a sketch that is not: the error says which
        d.set(
            SKETCH_DOMAIN,
            false,
            b"sketch 1\n[sketch]\nphoenix-sketch 1\nname a\nplane top 0 0 0\nnext 2 1\ne1 line e7 e8\n".to_vec(),
        )
        .unwrap();
        assert_eq!(d.sketch_version().unwrap(), Some(1));
        let e = d.sketches(&s).unwrap_err();
        assert!(e.to_string().contains("sketch 1: "), "{e}");
    }

    #[test]
    fn the_domain_sits_in_the_project_beside_the_others_and_comes_out_again() {
        let s = settings();
        let mut d = Document::new();
        d.set("mechanical", true, b"mechanical 1\n".to_vec())
            .unwrap();
        d.set("notes-from-elsewhere", false, vec![1, 2, 255])
            .unwrap();
        assert_eq!(d.sketches(&s).unwrap(), Vec::<Sketch>::new());
        d.set_sketches(&[plate("a")], &s).unwrap();
        assert_eq!(
            d.names(),
            vec!["mechanical", "notes-from-elsewhere", "sketch"]
        );
        assert!(!d.get(SKETCH_DOMAIN).unwrap().critical);
        // the file keeps the length of the domain in front of it
        let bytes = d.to_bytes();
        let text = String::from_utf8_lossy(&bytes).to_string();
        let len = d.get(SKETCH_DOMAIN).unwrap().data.len();
        assert!(text.contains(&format!("domain sketch optional {len}\n")));
        let read = Document::from_bytes(&bytes, 1 << 20).unwrap();
        assert_eq!(read.sketches(&s).unwrap(), vec![plate("a")]);
        assert_eq!(read.to_bytes(), bytes);
        // a program that does not know the domain opens the project and may edit it
        let opened =
            Document::open(&bytes, 1 << 20, &["mechanical", "notes-from-elsewhere"]).unwrap();
        assert!(!opened.read_only);
        // with no sketch left the domain goes away
        d.set_sketches(&[], &s).unwrap();
        assert!(d.get(SKETCH_DOMAIN).is_none());
        d.set_sketches(&[], &s).unwrap();
    }
}

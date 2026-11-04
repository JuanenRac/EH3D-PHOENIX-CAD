//! The envelope of a project file: a format version and a set of named domains.
//!
//! The text is one header line `PHOENIX-CAD-PROJECT <version>` followed by, for each domain in
//! name order, a line `domain <name> <critical|optional> <length>`, exactly `length` bytes of data
//! and a line end. The envelope does not look inside a domain, so a domain that this program does
//! not know is read, kept and written back byte for byte. A domain marked critical changes the
//! meaning of the design: a file with a critical domain that the program does not understand is
//! opened read-only instead of being edited and saved without it.
//!
//! A file of an older version is brought to the current one by a chain of migrations; a file of a
//! newer version is refused. Nothing here says what a domain holds: the mechanical domain of the
//! project format belongs to a contract that is not accepted yet.

use std::collections::BTreeMap;

use crate::CadError;

/// The version this program writes.
pub const FORMAT_VERSION: u32 = 1;

/// The most domains a file may hold.
pub const MAX_DOMAINS: usize = 1000;

/// The longest name of a domain.
pub const MAX_DOMAIN_NAME: usize = 64;

/// A step that brings a document from one version to the next.
pub type Migration = fn(&mut Document);

/// The migrations: the entry `i` brings version `i + 1` to version `i + 2`. There is none while
/// the current version is the first.
pub const MIGRATIONS: &[Migration] = &[];

const MAGIC: &str = "PHOENIX-CAD-PROJECT";

/// One domain of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain {
    /// Whether a program that does not understand it must not edit the project.
    pub critical: bool,
    /// The content, which the envelope never reads.
    pub data: Vec<u8>,
}

/// A project: a version and its domains by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The format version of the content.
    pub version: u32,
    domains: BTreeMap<String, Domain>,
}

/// A document that was opened, and whether it may be edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// The document.
    pub document: Document,
    /// True when a critical domain is not understood.
    pub read_only: bool,
}

fn bad(why: &str) -> CadError {
    CadError::Format(why.to_string())
}

fn valid_domain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_DOMAIN_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

impl Document {
    /// An empty document of the current version.
    pub fn new() -> Document {
        Document {
            version: FORMAT_VERSION,
            domains: BTreeMap::new(),
        }
    }

    /// Sets a domain, replacing the one of that name. The name holds lower-case letters, digits,
    /// hyphens and underscores.
    pub fn set(&mut self, name: &str, critical: bool, data: Vec<u8>) -> Result<(), CadError> {
        if !valid_domain_name(name) {
            return Err(CadError::InvalidName(name.to_string()));
        }
        if !self.domains.contains_key(name) && self.domains.len() >= MAX_DOMAINS {
            return Err(CadError::ValueOutOfRange("domains"));
        }
        self.domains
            .insert(name.to_string(), Domain { critical, data });
        Ok(())
    }

    /// A domain.
    pub fn get(&self, name: &str) -> Option<&Domain> {
        self.domains.get(name)
    }

    /// Removes a domain.
    pub fn remove(&mut self, name: &str) -> Result<Domain, CadError> {
        self.domains
            .remove(name)
            .ok_or_else(|| CadError::UnknownName(name.to_string()))
    }

    /// The names of the domains in order.
    pub fn names(&self) -> Vec<&str> {
        self.domains.keys().map(String::as_str).collect()
    }

    /// The bytes of the file.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = format!("{MAGIC} {}\n", self.version).into_bytes();
        for (name, domain) in &self.domains {
            let kind = if domain.critical {
                "critical"
            } else {
                "optional"
            };
            out.extend(format!("domain {name} {kind} {}\n", domain.data.len()).bytes());
            out.extend(&domain.data);
            out.push(b'\n');
        }
        out
    }

    /// Reads a file of the current version or an older one (which is migrated). A file larger
    /// than `max_bytes`, of a newer version or not well formed is refused.
    pub fn from_bytes(bytes: &[u8], max_bytes: usize) -> Result<Document, CadError> {
        Document::from_bytes_with(bytes, max_bytes, FORMAT_VERSION, MIGRATIONS)
    }

    /// Like [`Document::from_bytes`] for a given current version and chain of migrations.
    pub fn from_bytes_with(
        bytes: &[u8],
        max_bytes: usize,
        current: u32,
        steps: &[Migration],
    ) -> Result<Document, CadError> {
        if bytes.len() > max_bytes {
            return Err(bad("the file is larger than the limit"));
        }
        let mut pos = 0;
        let header = take_line(bytes, &mut pos)?;
        let version = match header.split_once(' ') {
            Some((magic, v)) if magic == MAGIC => v
                .parse::<u32>()
                .map_err(|_| bad("the version is not a number"))?,
            _ => return Err(bad("this is not a project file")),
        };
        if version == 0 {
            return Err(bad("the version is zero"));
        }
        if version > current {
            return Err(bad("the file was written by a newer version"));
        }
        let mut doc = Document {
            version,
            domains: BTreeMap::new(),
        };
        while pos < bytes.len() {
            let line = take_line(bytes, &mut pos)?;
            let parts: Vec<&str> = line.split(' ').collect();
            let (name, critical, len) = match parts.as_slice() {
                ["domain", name, kind, len] => {
                    let critical = match *kind {
                        "critical" => true,
                        "optional" => false,
                        _ => return Err(bad("a domain is neither critical nor optional")),
                    };
                    let len = len
                        .parse::<usize>()
                        .map_err(|_| bad("the length of a domain is not a number"))?;
                    (*name, critical, len)
                }
                _ => return Err(bad("a line is not a domain")),
            };
            if !valid_domain_name(name) {
                return Err(bad("the name of a domain is not valid"));
            }
            if doc.domains.contains_key(name) {
                return Err(bad("a domain is repeated"));
            }
            if doc.domains.len() >= MAX_DOMAINS {
                return Err(bad("too many domains"));
            }
            let end = pos
                .checked_add(len)
                .filter(|e| *e < bytes.len())
                .ok_or_else(|| bad("a domain is cut short"))?;
            if bytes[end] != b'\n' {
                return Err(bad("a domain does not end with a line end"));
            }
            doc.domains.insert(
                name.to_string(),
                Domain {
                    critical,
                    data: bytes[pos..end].to_vec(),
                },
            );
            pos = end + 1;
        }
        migrate_with(&mut doc, steps, current)?;
        Ok(doc)
    }

    /// Opens a file and says whether it may be edited: it may not when a critical domain is not
    /// among the `understood` ones.
    pub fn open(bytes: &[u8], max_bytes: usize, understood: &[&str]) -> Result<Opened, CadError> {
        let document = Document::from_bytes(bytes, max_bytes)?;
        let read_only = document
            .domains
            .iter()
            .any(|(name, d)| d.critical && !understood.contains(&name.as_str()));
        Ok(Opened {
            document,
            read_only,
        })
    }
}

fn take_line<'a>(bytes: &'a [u8], pos: &mut usize) -> Result<&'a str, CadError> {
    let rest = &bytes[*pos..];
    let end = rest
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(|| bad("a line has no end"))?;
    let line = std::str::from_utf8(&rest[..end]).map_err(|_| bad("a line is not text"))?;
    *pos += end + 1;
    Ok(line)
}

/// Brings a document to the version `target` with the chain of migrations `steps`.
pub fn migrate_with(doc: &mut Document, steps: &[Migration], target: u32) -> Result<(), CadError> {
    while doc.version < target {
        let index =
            usize::try_from(doc.version - 1).map_err(|_| bad("the version is too large"))?;
        let step = steps
            .get(index)
            .ok_or_else(|| bad("there is no migration for this version"))?;
        step(doc);
        doc.version += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: usize = 1 << 20;

    fn sample() -> Document {
        let mut d = Document::new();
        d.set("mechanical", true, b"box 100 60 30".to_vec())
            .unwrap();
        d.set("notes", false, b"two\nlines\n".to_vec()).unwrap();
        d.set("empty", false, Vec::new()).unwrap();
        d
    }

    #[test]
    fn the_migrations_match_the_current_version() {
        assert_eq!(MIGRATIONS.len() + 1, FORMAT_VERSION as usize);
    }

    #[test]
    fn a_document_is_written_and_read_back_equal_with_binary_content() {
        let mut d = sample();
        d.set("blob", false, vec![0, 255, b'\n', b'\n', 10, 13])
            .unwrap();
        let bytes = d.to_bytes();
        assert!(bytes.starts_with(b"PHOENIX-CAD-PROJECT 1\n"));
        assert_eq!(Document::from_bytes(&bytes, BIG).unwrap(), d);
        assert_eq!(d.names(), vec!["blob", "empty", "mechanical", "notes"]);
        // the same document is always the same bytes
        assert_eq!(bytes, d.clone().to_bytes());
    }

    #[test]
    fn a_domain_that_is_not_known_is_kept_byte_for_byte() {
        let mut d = sample();
        d.set("from-the-future", false, b"\x01\x02 anything \xff".to_vec())
            .unwrap();
        let bytes = d.to_bytes();
        let mut read = Document::from_bytes(&bytes, BIG).unwrap();
        // the program edits the domain it knows and writes the file again
        read.set("notes", false, b"changed".to_vec()).unwrap();
        let again = Document::from_bytes(&read.to_bytes(), BIG).unwrap();
        assert_eq!(
            again.get("from-the-future"),
            d.get("from-the-future"),
            "an unknown domain survives"
        );
        assert_eq!(again.get("notes").unwrap().data, b"changed");
    }

    #[test]
    fn a_critical_domain_that_is_not_understood_makes_the_project_read_only() {
        let mut d = sample();
        d.set("thermal-model", true, b"x".to_vec()).unwrap();
        let bytes = d.to_bytes();
        let all = ["mechanical", "thermal-model"];
        assert!(!Document::open(&bytes, BIG, &all).unwrap().read_only);
        assert!(
            Document::open(&bytes, BIG, &["mechanical"])
                .unwrap()
                .read_only
        );
        // an optional domain that is not understood does not matter
        let opened = Document::open(&sample().to_bytes(), BIG, &["mechanical"]).unwrap();
        assert!(!opened.read_only);
    }

    #[test]
    fn files_that_are_not_well_formed_are_refused_with_one_code() {
        let good = sample().to_bytes();
        let mut cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"hello\n".to_vec(),
            b"PHOENIX-CAD-PROJECT\n".to_vec(),
            b"PHOENIX-CAD-PROJECT x\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 0\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 2\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 1\nsomething else\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 1\ndomain a sometimes 0\n\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 1\ndomain A optional 0\n\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 1\ndomain a optional 5\nab\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 1\ndomain a optional 2\nabX".to_vec(),
            b"PHOENIX-CAD-PROJECT 1\ndomain a optional 0\n\ndomain a optional 0\n\n".to_vec(),
            b"PHOENIX-CAD-PROJECT 1\ndomain a optional 99999999999999999999\n".to_vec(),
        ];
        // every cut of a good file is refused or is a shorter good file, never a panic
        for n in 0..good.len() {
            cases.push(good[..n].to_vec());
        }
        for case in cases {
            if let Err(e) = Document::from_bytes(&case, BIG) {
                assert_eq!(e.code(), "PHOENIX-CAD-018", "{case:?}");
            }
        }
        for case in [
            &b""[..],
            &b"hello\n"[..],
            &b"PHOENIX-CAD-PROJECT 2\n"[..],
            &b"PHOENIX-CAD-PROJECT 1\ndomain a optional 5\nab\n"[..],
        ] {
            assert_eq!(
                Document::from_bytes(case, BIG).unwrap_err().code(),
                "PHOENIX-CAD-018"
            );
        }
    }

    #[test]
    fn a_file_larger_than_the_limit_is_refused_before_it_is_read() {
        let bytes = sample().to_bytes();
        assert!(Document::from_bytes(&bytes, bytes.len()).is_ok());
        assert_eq!(
            Document::from_bytes(&bytes, bytes.len() - 1)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-018"
        );
    }

    #[test]
    fn domain_names_and_their_number_are_limited() {
        let mut d = Document::new();
        for name in ["", "Upper", "with space", "a.b"] {
            assert_eq!(
                d.set(name, false, Vec::new()).unwrap_err().code(),
                "PHOENIX-CAD-017",
                "{name:?}"
            );
        }
        assert!(d
            .set(&"a".repeat(MAX_DOMAIN_NAME + 1), false, Vec::new())
            .is_err());
        for i in 0..MAX_DOMAINS {
            d.set(&format!("d{i}"), false, Vec::new()).unwrap();
        }
        assert_eq!(
            d.set("one-more", false, Vec::new()).unwrap_err().code(),
            "PHOENIX-CAD-007"
        );
        // replacing one that exists is still allowed
        assert!(d.set("d0", true, vec![1]).is_ok());
        assert!(d.remove("d0").is_ok());
        assert_eq!(d.remove("d0").unwrap_err().code(), "PHOENIX-CAD-014");
    }

    fn rename_mech(doc: &mut Document) {
        if let Ok(d) = doc.remove("mech") {
            doc.set("mechanical", d.critical, d.data).unwrap();
        }
    }

    fn add_marker(doc: &mut Document) {
        doc.set("migrated", false, b"v3".to_vec()).unwrap();
    }

    #[test]
    fn an_older_file_is_brought_to_the_current_version_by_the_chain() {
        let mut old = Document::new();
        old.version = 1;
        old.set("mech", true, b"box".to_vec()).unwrap();
        let bytes = old.to_bytes();
        let steps: &[Migration] = &[rename_mech, add_marker];
        let doc = Document::from_bytes_with(&bytes, BIG, 3, steps).unwrap();
        assert_eq!(doc.version, 3);
        assert_eq!(doc.names(), vec!["mechanical", "migrated"]);
        assert_eq!(doc.get("mechanical").unwrap().data, b"box");
        // a chain that is too short cannot reach the version
        let short: &[Migration] = &[rename_mech];
        assert_eq!(
            Document::from_bytes_with(&bytes, BIG, 3, short)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-018"
        );
        // a file already at the current version is not touched
        let new = Document::from_bytes_with(&doc.to_bytes(), BIG, 3, steps).unwrap();
        assert_eq!(new, doc);
    }
}

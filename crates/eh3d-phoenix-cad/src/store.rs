//! Saving and loading files without losing work.
//!
//! [`save_atomic`] writes the new content to a temporary file next to the target, flushes it to
//! disk and only then puts it in place, so a crash leaves either the old file or the new one and
//! never half of one. The files it replaces are kept as numbered copies (`name.bak1` is the
//! newest), as many as the settings say. [`load_limited`] refuses a file larger than a limit
//! before reading it, because a file from outside is not trusted.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::CadError;

fn io(what: &str, e: &std::io::Error) -> CadError {
    CadError::Io(format!("{what}: {e}"))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// The path of the numbered copy of a file (`1` is the newest).
pub fn backup_path(path: &Path, number: u32) -> PathBuf {
    with_suffix(path, &format!(".bak{number}"))
}

/// Writes `data` to `path` atomically and keeps up to `backups` older copies of the file.
pub fn save_atomic(path: &Path, data: &[u8], backups: u32) -> Result<(), CadError> {
    let tmp = with_suffix(path, ".tmp");
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(io("cannot write the temporary file", &e));
    }
    if backups > 0 && path.exists() {
        let rotate = || -> std::io::Result<()> {
            for n in (1..backups).rev() {
                let from = backup_path(path, n);
                if from.exists() {
                    fs::rename(&from, backup_path(path, n + 1))?;
                }
            }
            fs::copy(path, backup_path(path, 1)).map(|_| ())
        };
        if let Err(e) = rotate() {
            let _ = fs::remove_file(&tmp);
            return Err(io("cannot keep the copies", &e));
        }
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io("cannot put the file in place", &e)
    })
}

/// Reads a whole file, refusing it before reading when it is larger than `max_bytes`.
pub fn load_limited(path: &Path, max_bytes: u64) -> Result<Vec<u8>, CadError> {
    let meta = fs::metadata(path).map_err(|e| io("cannot read the file", &e))?;
    if meta.len() > max_bytes {
        return Err(CadError::Io(format!(
            "the file has {} bytes, more than the {max_bytes} allowed",
            meta.len()
        )));
    }
    fs::read(path).map_err(|e| io("cannot read the file", &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test, removed at the end of the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("phoenix-cad-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_new_file_is_written_whole_and_leaves_no_temporary_file() {
        let dir = Scratch::new("new");
        let path = dir.file("model.cfg");
        save_atomic(&path, b"first", 3).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert!(!with_suffix(&path, ".tmp").exists());
        assert!(!backup_path(&path, 1).exists());
    }

    #[test]
    fn saving_over_a_file_keeps_the_newest_copies_and_no_more() {
        let dir = Scratch::new("rotate");
        let path = dir.file("model.cfg");
        for text in ["one", "two", "three", "four", "five"] {
            save_atomic(&path, text.as_bytes(), 2).unwrap();
        }
        assert_eq!(fs::read(&path).unwrap(), b"five");
        assert_eq!(fs::read(backup_path(&path, 1)).unwrap(), b"four");
        assert_eq!(fs::read(backup_path(&path, 2)).unwrap(), b"three");
        assert!(!backup_path(&path, 3).exists());
        assert!(!with_suffix(&path, ".tmp").exists());
    }

    #[test]
    fn no_copies_are_kept_when_the_setting_is_zero() {
        let dir = Scratch::new("zero");
        let path = dir.file("model.cfg");
        save_atomic(&path, b"one", 0).unwrap();
        save_atomic(&path, b"two", 0).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert!(!backup_path(&path, 1).exists());
    }

    #[test]
    fn a_failed_save_leaves_the_old_file_alone() {
        let dir = Scratch::new("fail");
        let path = dir.file("model.cfg");
        save_atomic(&path, b"safe", 1).unwrap();
        // the folder of the target does not exist, so the temporary file cannot be made
        let missing = dir.file("nowhere").join("model.cfg");
        let err = save_atomic(&missing, b"lost", 1).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-012");
        assert_eq!(fs::read(&path).unwrap(), b"safe");
    }

    #[test]
    fn a_file_larger_than_the_limit_is_refused_before_it_is_read() {
        let dir = Scratch::new("limit");
        let path = dir.file("big.bin");
        save_atomic(&path, &[7u8; 100], 0).unwrap();
        assert_eq!(load_limited(&path, 100).unwrap().len(), 100);
        let err = load_limited(&path, 99).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-012");
        assert!(err.to_string().contains("100 bytes"));
        let missing = load_limited(&dir.file("absent"), 10).unwrap_err();
        assert_eq!(missing.code(), "PHOENIX-CAD-012");
    }
}

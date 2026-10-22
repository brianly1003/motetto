//! Where downloaded models live, and how a file the user already has is adopted.
use crate::catalog::ModelFile;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::Manager;

/// The app-owned folder the downloader fills.
pub fn dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("models")
}

/// Where `file` is usable: in `dir`, else at its adopted path. Only an exact size counts; the
/// hash was checked when it was downloaded or adopted.
pub fn path(dir: &Path, adopted: &BTreeMap<String, String>, file: &ModelFile) -> Option<PathBuf> {
    [
        Some(dir.join(&file.file)),
        adopted.get(&file.id).map(PathBuf::from),
    ]
    .into_iter()
    .flatten()
    .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() == file.bytes))
}

/// The first candidate with exactly the catalog's size and SHA-256, so a model fetched by hand
/// (or by the CLI) isn't downloaded twice.
pub fn adopt(file: &ModelFile, candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() == file.bytes)
                && hash(p).is_some_and(|h| h == file.sha256)
        })
        .cloned()
}

/// Streamed in chunks: models are several GB.
fn hash(path: &Path) -> Option<String> {
    let mut input = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0; 1 << 20];
    loop {
        match input.read(&mut chunk).ok()? {
            0 => return Some(format!("{:x}", hasher.finalize())),
            n => hasher.update(&chunk[..n]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FileKind;

    fn model(bytes: &[u8]) -> ModelFile {
        ModelFile {
            id: "tiny".into(),
            kind: FileKind::Speech,
            file: "tiny.bin".into(),
            label: "Tiny".into(),
            url: "https://huggingface.co/motet/test/resolve/abc/tiny.bin".into(),
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motet-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn path_requires_exact_size() {
        let dir = folder("path");
        let file = model(b"0123456789");
        let none = BTreeMap::new();
        assert_eq!(path(&dir, &none, &file), None);
        std::fs::write(dir.join("tiny.bin"), b"01234").unwrap();
        assert_eq!(path(&dir, &none, &file), None, "short file");
        std::fs::write(dir.join("tiny.bin"), b"0123456789").unwrap();
        assert_eq!(path(&dir, &none, &file), Some(dir.join("tiny.bin")));
        // An adopted path works when the folder has nothing, and a vanished one does not.
        std::fs::remove_file(dir.join("tiny.bin")).unwrap();
        let elsewhere = dir.join("mine.bin");
        std::fs::write(&elsewhere, b"0123456789").unwrap();
        let adopted = BTreeMap::from([("tiny".to_string(), elsewhere.to_string_lossy().into())]);
        assert_eq!(path(&dir, &adopted, &file), Some(elsewhere.clone()));
        std::fs::remove_file(&elsewhere).unwrap();
        assert_eq!(path(&dir, &adopted, &file), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn adopt_checks_hash() {
        let dir = folder("adopt");
        let file = model(b"0123456789");
        let wrong = dir.join("wrong.bin");
        std::fs::write(&wrong, b"9876543210").unwrap();
        let short = dir.join("short.bin");
        std::fs::write(&short, b"0123").unwrap();
        let right = dir.join("right.bin");
        std::fs::write(&right, b"0123456789").unwrap();
        let missing = dir.join("missing.bin");
        assert_eq!(adopt(&file, &[wrong.clone(), short, missing]), None);
        assert_eq!(adopt(&file, &[wrong, right.clone()]), Some(right));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

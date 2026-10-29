//! Presenter scripts (SPEC 14.4): Markdown files in `<library>/Scripts/`, written atomically.
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::UNIX_EPOCH,
};
use tauri::Manager;
use ts_rs::TS;

const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ScriptInfo {
    pub name: String,
    #[ts(type = "number")]
    pub modified_ms: u64,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// Saves wait for each other, so a stale base is always seen.
static WRITES: Mutex<()> = Mutex::new(());

fn dir(root: &Path) -> PathBuf {
    root.join("Scripts")
}

/// A script name the user typed, without `.md`: 1-80 letters, digits, spaces or `-_.()`.
fn name(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let name = raw.strip_suffix(".md").unwrap_or(raw).trim();
    let ok = !name.is_empty()
        && name.chars().count() <= 80
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || " -_.()".contains(c));
    if ok {
        Ok(name.into())
    } else {
        Err("Use a name of up to 80 letters, digits, spaces or - _ . ( ).".into())
    }
}

/// A script name checked like a saved one, for files that live beside it.
pub fn checked_name(raw: &str) -> Result<String, String> {
    name(raw)
}

fn path(root: &Path, raw: &str) -> Result<PathBuf, String> {
    Ok(dir(root).join(format!("{}.md", name(raw)?)))
}

fn modified_ms(path: &Path) -> Result<u64, String> {
    let time = fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?;
    Ok(time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64))
}

fn list(root: &Path) -> Result<Vec<ScriptInfo>, String> {
    let Ok(entries) = fs::read_dir(dir(root)) else {
        return Ok(vec![]);
    };
    let mut scripts = vec![];
    for entry in entries.flatten() {
        let file = entry.path();
        let Some(stem) = file
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".md"))
        else {
            continue;
        };
        if name(stem).as_deref() != Ok(stem) || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        scripts.push(ScriptInfo {
            name: stem.into(),
            modified_ms: modified_ms(&file)?,
            bytes: entry.metadata().map_or(0, |m| m.len()),
        });
    }
    scripts.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(a.name.cmp(&b.name)));
    Ok(scripts)
}

fn read(root: &Path, raw: &str) -> Result<String, String> {
    crate::library::text(&path(root, raw)?)
}

/// Writes `text` and returns the new modified time. `base_modified_ms` is the time the editor
/// loaded (None for a new script); a different time on disk means another window saved it.
fn save(root: &Path, raw: &str, text: &str, base_modified_ms: Option<u64>) -> Result<u64, String> {
    if text.len() > MAX_BYTES {
        return Err("A script must be smaller than 256 KB.".into());
    }
    let target = path(root, raw)?;
    let _writes = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let exists = target.try_exists().map_err(|e| e.to_string())?;
    match (exists, base_modified_ms) {
        (true, None) => return Err("A script with this name already exists.".into()),
        (true, Some(base)) if modified_ms(&target)? != base => {
            return Err("This script changed in another window.".into());
        }
        (false, Some(_)) => return Err("This script was renamed or deleted.".into()),
        _ => {}
    }
    fs::create_dir_all(dir(root)).map_err(|e| format!("Couldn't save the script: {e}"))?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = dir(root).join(format!(
        ".script-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // Refuse a pre-existing temporary path instead of following links.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("Couldn't save the script: {e}"))?;
    let result = file
        .write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary, &target));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Couldn't save the script: {e}"))?;
    modified_ms(&target)
}

fn rename(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let (from, to) = (path(root, from)?, path(root, to)?);
    let _writes = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    if to.try_exists().map_err(|e| e.to_string())? {
        return Err("A script with this name already exists.".into());
    }
    fs::rename(from, to).map_err(|e| format!("Couldn't rename the script: {e}"))
}

/// Deleted scripts move to `Scripts/.deleted/`, so a mistake can be undone from Finder.
fn delete(root: &Path, raw: &str) -> Result<(), String> {
    let source = path(root, raw)?;
    let bin = dir(root).join(".deleted");
    fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let _writes = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    fs::rename(&source, bin.join(format!("{}-{stamp}.md", name(raw)?)))
        .map_err(|e| format!("Couldn't delete the script: {e}"))
}

fn root(app: &tauri::AppHandle) -> PathBuf {
    app.state::<crate::library_commands::Library>().root.clone()
}
#[tauri::command]
pub fn scripts_list(app: tauri::AppHandle) -> Result<Vec<ScriptInfo>, String> {
    list(&root(&app))
}
#[tauri::command]
pub fn script_read(app: tauri::AppHandle, name: String) -> Result<String, String> {
    read(&root(&app), &name)
}
#[tauri::command]
pub fn script_save(
    app: tauri::AppHandle,
    name: String,
    text: String,
    base_modified_ms: Option<u64>,
) -> Result<u64, String> {
    save(&root(&app), &name, &text, base_modified_ms)
}
#[tauri::command]
pub fn script_rename(app: tauri::AppHandle, from: String, to: String) -> Result<(), String> {
    rename(&root(&app), &from, &to)
}
#[tauri::command]
pub fn script_delete(app: tauri::AppHandle, name: String) -> Result<(), String> {
    delete(&root(&app), &name)
}
/// Show in Finder (a script's ••• menu): the Scripts folder, where its `.md` file is.
#[tauri::command]
pub fn script_reveal(app: tauri::AppHandle) -> Result<(), String> {
    crate::settings_system::reveal(&dir(&root(&app)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "motetto-scripts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn exports_bindings() {
        ScriptInfo::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }

    #[test]
    fn rejects_bad_names() {
        for bad in [
            "../x",
            "a/b",
            "",
            "   ",
            ".hidden",
            "a\\b",
            "x:y",
            &"a".repeat(81),
        ] {
            assert!(name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(name(" Team X demo.md ").unwrap(), "Team X demo");
        assert_eq!(name("Q4 (draft)_v2-final").unwrap(), "Q4 (draft)_v2-final");
        assert_eq!(name("Tiếng Việt").unwrap(), "Tiếng Việt");
        assert_eq!(name(&"a".repeat(80)).unwrap().len(), 80);
    }

    #[test]
    fn rejects_over_256_kib() {
        let root = temp_root();
        let text = "a".repeat(MAX_BYTES + 1);
        assert!(
            save(&root, "Big", &text, None)
                .unwrap_err()
                .contains("256 KB")
        );
        assert!(!dir(&root).join("Big.md").exists());
        save(&root, "Fits", &"a".repeat(MAX_BYTES), None).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_is_atomic_and_detects_conflict() {
        let root = temp_root();
        let first = save(&root, "Demo", "# One\nHello.", None).unwrap();
        assert_eq!(read(&root, "Demo").unwrap(), "# One\nHello.");
        assert!(
            save(&root, "Demo", "again", None)
                .unwrap_err()
                .contains("already exists")
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = save(&root, "Demo", "# Two", Some(first)).unwrap();
        assert_eq!(
            save(&root, "Demo", "stale", Some(first)).unwrap_err(),
            "This script changed in another window."
        );
        assert_eq!(read(&root, "Demo").unwrap(), "# Two");
        save(&root, "Demo", "# Three", Some(second)).unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir(&root))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        assert!(
            save(&root, "Gone", "x", Some(1))
                .unwrap_err()
                .contains("renamed or deleted")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_ignores_non_md_and_hidden() {
        let root = temp_root();
        assert!(list(&root).unwrap().is_empty(), "no Scripts folder yet");
        save(&root, "Demo", "abc", None).unwrap();
        fs::write(dir(&root).join("notes.txt"), "x").unwrap();
        fs::write(dir(&root).join(".script-1-1.tmp"), "x").unwrap();
        fs::write(dir(&root).join(".secret.md"), "x").unwrap();
        fs::create_dir_all(dir(&root).join("folder.md")).unwrap();
        let listed = list(&root).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].name.as_str(), listed[0].bytes), ("Demo", 3));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_and_delete_keep_the_text() {
        let root = temp_root();
        save(&root, "Old", "text", None).unwrap();
        save(&root, "Taken", "other", None).unwrap();
        assert!(
            rename(&root, "Old", "Taken")
                .unwrap_err()
                .contains("already exists")
        );
        rename(&root, "Old", "New").unwrap();
        assert_eq!(read(&root, "New").unwrap(), "text");
        delete(&root, "New").unwrap();
        assert!(read(&root, "New").is_err());
        let bin: Vec<_> = fs::read_dir(dir(&root).join(".deleted"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(fs::read_to_string(bin[0].path()).unwrap(), "text");
        assert_eq!(list(&root).unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}

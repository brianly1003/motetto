//! Export MP4…: a screen recording saved as one MP4 where the user chooses. One part is copied;
//! several are joined without re-encoding (`trim::Media::export` over each whole part).
use crate::{library, library_commands::Library, trim::Media};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

/// A title made safe as a file name: no path separators or control characters, no leading dot;
/// `fallback` when nothing is left.
pub(crate) fn clean_title(title: &str, fallback: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_start_matches('.');
    if name.is_empty() {
        fallback.to_string()
    } else {
        name.to_string()
    }
}

/// The file name the save panel proposes: the title without path separators.
fn file_name(title: &str) -> String {
    format!("{}.mp4", clean_title(title, "Screen recording"))
}

/// Writes the whole video beside `dst` first, then renames it over, so a failure never leaves a
/// half-written file or removes one the user chose to replace.
fn export(dir: &Path, dst: &Path, media: &Media) -> Result<(), String> {
    let videos = crate::trim::videos(dir);
    if videos.is_empty() {
        return Err("This recording has no video to export".into());
    }
    let name = dst
        .file_name()
        .ok_or("Choose a file name")?
        .to_string_lossy();
    let tmp = dst.with_file_name(format!(".{name}.partial.mp4"));
    let _ = fs::remove_file(&tmp);
    let written = if let [(_, path)] = videos.as_slice() {
        fs::copy(path, &tmp).map(|_| ()).map_err(|e| e.to_string())
    } else {
        videos
            .iter()
            .map(|(_, path)| Ok((path.clone(), 0, (media.length)(path)?)))
            .collect::<Result<Vec<(PathBuf, u64, u64)>, String>>()
            .and_then(|parts| (media.export)(&parts, &tmp))
    };
    let result = written.and_then(|()| fs::rename(&tmp, dst).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Asks where to save, then exports; false when the save panel was cancelled.
#[tauri::command]
pub async fn export_video(app: tauri::AppHandle, id: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = app.state::<Library>().root.clone();
        let summary = library::summary(&root, &id)?;
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("MP4 video", &["mp4"])
            .set_file_name(file_name(&summary.title))
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let dst = file.into_path().map_err(|e| e.to_string())?;
        let dir = library::folder(&root, &id)?;
        // A trim moves the parts; the same flag keeps the two apart.
        let library = app.state::<Library>();
        if library.busy.swap(true, Ordering::SeqCst) {
            return Err("Wait until recording and processing finish before exporting.".into());
        }
        let result = export(&dir, &dst, &crate::trim::NATIVE);
        library.busy.store(false, Ordering::SeqCst);
        result.map(|()| true)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{ScreenPart, ScreenSourceInfo};

    fn length(path: &Path) -> Result<u64, String> {
        Ok(fs::read_to_string(path).unwrap().len() as u64)
    }
    fn join(parts: &[(PathBuf, u64, u64)], dst: &Path) -> Result<(), String> {
        let text: String = parts
            .iter()
            .map(|(p, from, to)| format!("{}:{from}-{to};", fs::read_to_string(p).unwrap()))
            .collect();
        fs::write(dst, text).map_err(|e| e.to_string())
    }
    const FAKE: Media = Media {
        length,
        export: join,
    };

    #[test]
    fn one_part_is_copied_and_several_are_joined_whole() {
        let dir = std::env::temp_dir().join(format!("motetto-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let part = |file: &str, offset_ms| ScreenPart {
            file: file.into(),
            offset_ms,
            source: ScreenSourceInfo {
                kind: "window".into(),
                name: "Safari".into(),
            },
            while_sharing: false,
            width: 2,
            height: 2,
        };
        fs::write(dir.join("part-0000.screen.mp4"), "aaa").unwrap();
        crate::screen_files::write(&dir, "screen.json", &vec![part("part-0000.screen.mp4", 0)])
            .unwrap();
        let dst = dir.join("out.mp4");
        fs::write(&dst, "old").unwrap();
        export(&dir, &dst, &FAKE).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "aaa");
        fs::write(dir.join("part-0001.screen.mp4"), "bb").unwrap();
        crate::screen_files::write(
            &dir,
            "screen.json",
            &vec![
                part("part-0001.screen.mp4", 3),
                part("part-0000.screen.mp4", 0),
            ],
        )
        .unwrap();
        export(&dir, &dst, &FAKE).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "aaa:0-3;bb:0-2;");
        assert!(!dir.join(".out.mp4.partial.mp4").exists());
        assert_eq!(file_name("A/b: c"), "A-b- c.mp4");
        assert_eq!(file_name(" .. "), "Screen recording.mp4");
        fs::remove_dir_all(dir).unwrap();
    }
}

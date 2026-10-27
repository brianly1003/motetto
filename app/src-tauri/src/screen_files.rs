//! Atomic screen manifests and recoverable video-only deletion within one meeting folder.
use crate::{
    library,
    screen::{Moment, ScreenPart},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn video_name(name: &str) -> bool {
    name.starts_with("part-")
        && name.ends_with(".screen.mp4")
        && Path::new(name).file_name().is_some_and(|n| n == name)
}
pub fn safe_file(dir: &Path, name: &str) -> Option<PathBuf> {
    if Path::new(name).file_name()?.to_str()? != name {
        return None;
    }
    let file = dir.join(name);
    if !file.symlink_metadata().ok()?.file_type().is_file() {
        return None;
    }
    let path = file.canonicalize().ok()?;
    (path.parent()? == dir.canonicalize().ok()?).then_some(path)
}
pub fn read<T: DeserializeOwned + Default>(dir: &Path, name: &str) -> T {
    safe_file(dir, name)
        .and_then(|p| library::text(&p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
pub fn write(dir: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    let target = dir.join(name);
    if target
        .symlink_metadata()
        .is_ok_and(|m| !m.file_type().is_file())
    {
        return Err("Screen metadata is not a regular file".into());
    }
    let temp = dir.join(format!(
        ".{name}.{}.tmp",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temp, target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub fn progress(
    dir: &Path,
    part: &ScreenPart,
    video: &motetto_core::video::VideoOutcome,
    finished: bool,
) -> Result<(), String> {
    let mut parts: Vec<ScreenPart> = read(dir, "screen.json");
    parts.retain(|p| p.file != part.file);
    if !finished || safe_file(dir, &part.file).is_some() {
        let mut part = part.clone();
        part.width = video.width;
        part.height = video.height;
        parts.push(part);
        parts.sort_by_key(|p| p.offset_ms);
    }
    write(dir, "screen.json", &parts)?;
    let mut moments: Vec<Moment> = read(dir, "screen-moments.json");
    for ms in &video.moments_ms {
        let ms = part.offset_ms.saturating_add(*ms);
        if !moments.iter().any(|m| m.ms == ms) {
            moments.push(Moment {
                ms,
                yours: part.while_sharing,
            });
        }
    }
    moments.sort_by_key(|m| m.ms);
    write(dir, "screen-moments.json", &moments)
}

fn files(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = vec![];
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if video_name(&name) || ["screen.json", "screen-moments.json"].contains(&name.as_str()) {
            if safe_file(dir, &name).is_none() {
                return Err("Cannot move linked or invalid video files".into());
            }
            names.push(name);
        }
    }
    Ok(names)
}
fn move_files(from: &Path, to: &Path, names: &[String]) -> Result<(), String> {
    for name in names {
        if to.join(name).symlink_metadata().is_ok() {
            return Err("A video already exists at the destination".into());
        }
    }
    let mut moved: Vec<&String> = vec![];
    for name in names {
        if let Err(e) = fs::rename(from.join(name), to.join(name)) {
            let mut reason = format!("Couldn't move video: {e}");
            for previous in moved.into_iter().rev() {
                if let Err(error) = fs::rename(to.join(previous), from.join(previous)) {
                    reason.push_str(&format!(" Some files remain in {}: {error}", to.display()));
                }
            }
            return Err(reason);
        }
        moved.push(name);
    }
    Ok(())
}
fn recovery(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join("deleted-video");
    if !path.exists() {
        fs::create_dir(&path).map_err(|e| e.to_string())?;
    }
    if path
        .symlink_metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
        || path.canonicalize().map_err(|e| e.to_string())?.parent() != Some(dir)
    {
        return Err("Invalid deleted-video folder".into());
    }
    Ok(path)
}
pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    let dir = library::folder(root, id)?;
    if !root
        .join(id)
        .symlink_metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_dir()
    {
        return Err("Cannot change a linked meeting's video".into());
    }
    let names = files(&dir)?;
    if names.is_empty() {
        return Ok(());
    }
    let recovery = recovery(&dir)?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let target = recovery.join(time.to_string());
    fs::create_dir(&target).map_err(|e| e.to_string())?;
    let result = move_files(&dir, &target, &names);
    if result.is_err() {
        let _ = fs::remove_dir(&target);
    }
    result
}
pub fn restore(root: &Path, id: &str) -> Result<(), String> {
    let dir = library::folder(root, id)?;
    if !root
        .join(id)
        .symlink_metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_dir()
    {
        return Err("Cannot change a linked meeting's video".into());
    }
    let recovery = recovery(&dir)?;
    let mut folders = vec![];
    for entry in fs::read_dir(&recovery).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir()
            && let Ok(time) = entry.file_name().to_string_lossy().parse::<u128>()
        {
            folders.push((time, entry.path()));
        }
    }
    folders.sort_by_key(|p| p.0);
    let source = folders.pop().ok_or("No deleted video to restore")?.1;
    let names = files(&source)?;
    move_files(&source, &dir, &names)?;
    fs::remove_dir(&source).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moments_persist_on_the_meeting_clock_and_partial_parts_are_narrowed() {
        let dir =
            std::env::temp_dir().join(format!("motetto-screen-progress-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let part = ScreenPart {
            file: "part-0001.screen.mp4".into(),
            offset_ms: 72000,
            source: crate::screen::ScreenSourceInfo {
                kind: "display".into(),
                name: "Display".into(),
            },
            while_sharing: true,
            width: 0,
            height: 0,
        };
        let video = motetto_core::video::VideoOutcome {
            path: dir.join(&part.file).to_string_lossy().into_owned(),
            width: 1280,
            height: 720,
            moments_ms: vec![0, 15000],
            lost: false,
            error: None,
        };
        progress(&dir, &part, &video, false).unwrap();
        progress(&dir, &part, &video, false).unwrap();
        let moments: Vec<Moment> = read(&dir, "screen-moments.json");
        assert_eq!(
            moments.iter().map(|m| m.ms).collect::<Vec<_>>(),
            vec![72000, 87000]
        );
        assert!(moments.iter().all(|m| m.yours));
        assert_eq!(read::<Vec<ScreenPart>>(&dir, "screen.json")[0].width, 1280);
        progress(&dir, &part, &video, true).unwrap();
        assert!(read::<Vec<ScreenPart>>(&dir, "screen.json").is_empty());
        fs::write(dir.join(&part.file), b"partial video").unwrap();
        progress(&dir, &part, &video, true).unwrap();
        assert_eq!(read::<Vec<ScreenPart>>(&dir, "screen.json").len(), 1);
        fs::write(dir.join("screen.json"), b"broken").unwrap();
        assert!(read::<Vec<ScreenPart>>(&dir, "screen.json").is_empty());
        assert!(safe_file(&dir, "../secret.screen.mp4").is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn video_delete_restores_bytes_and_refuses_overwrites_and_links() {
        let root =
            std::env::temp_dir().join(format!("motetto-video-delete-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("1")).unwrap();
        let dir = root.join("1");
        fs::write(dir.join("part-0000.screen.mp4"), b"video").unwrap();
        fs::write(dir.join("part-0000.mic.wav"), b"audio").unwrap();
        write(&dir, "screen.json", &vec!["manifest"]).unwrap();
        delete(&root, "1").unwrap();
        assert!(!dir.join("screen.json").exists());
        assert!(dir.join("part-0000.mic.wav").exists());
        fs::write(dir.join("part-0000.screen.mp4"), b"new").unwrap();
        assert!(restore(&root, "1").is_err());
        fs::remove_file(dir.join("part-0000.screen.mp4")).unwrap();
        restore(&root, "1").unwrap();
        assert_eq!(
            fs::read(dir.join("part-0000.screen.mp4")).unwrap(),
            b"video"
        );
        #[cfg(unix)]
        {
            fs::remove_file(dir.join("screen.json")).unwrap();
            std::os::unix::fs::symlink(dir.join("part-0000.mic.wav"), dir.join("screen.json"))
                .unwrap();
            assert!(delete(&root, "1").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}

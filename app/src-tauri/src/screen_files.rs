//! Atomic screen manifests and recoverable video-only deletion within one meeting folder.
use crate::{
    library,
    screen::{Moment, ScreenPart},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn video_name(name: &str) -> bool {
    name.starts_with("part-")
        && name.ends_with(".screen.mp4")
        && Path::new(name).file_name().is_some_and(|n| n == name)
}
/// An imported video plays in the Video tab, but it is also the import's audio source
/// (Transcribe again), so audio retention keeps it, not Delete video or video retention.
pub fn imported_video(name: &str) -> bool {
    matches!(name, "source.mp4" | "source.mov")
}
/// Picture size of the first video track in an MP4 or MOV file, from its `tkhd` box; `None` for
/// audio-only or unreadable files. Reads box headers and the `moov` box (up to 64 MB) only.
pub fn video_size(path: &Path) -> Option<(u32, u32)> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut at = 0;
    while at + 8 <= len {
        file.seek(SeekFrom::Start(at)).ok()?;
        let mut head = [0u8; 16];
        file.read_exact(&mut head[..8]).ok()?;
        let (mut size, mut header) = (u64::from(u32::from_be_bytes(head[..4].try_into().ok()?)), 8);
        if size == 1 {
            file.read_exact(&mut head[8..]).ok()?;
            (size, header) = (u64::from_be_bytes(head[8..].try_into().ok()?), 16);
        } else if size == 0 {
            size = len - at;
        }
        // A crafted size must not wrap around and send the scan backwards.
        let end = at.checked_add(size)?;
        if size < header || end > len {
            return None;
        }
        if &head[4..8] == b"moov" {
            let mut moov = vec![
                0;
                usize::try_from(size - header)
                    .ok()
                    .filter(|n| *n <= 64 << 20)?
            ];
            file.read_exact(&mut moov).ok()?;
            return boxes(&moov)
                .filter(|(kind, _)| kind == b"trak")
                .find_map(|(_, trak)| track_size(trak));
        }
        at = end;
    }
    None
}
fn boxes(mut data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        let size = usize::try_from(u32::from_be_bytes(data.get(..4)?.try_into().ok()?)).ok()?;
        let kind = data.get(4..8)?.try_into().ok()?;
        let body = data.get(8..size)?;
        data = &data[size..];
        Some((kind, body))
    })
}
fn track_size(trak: &[u8]) -> Option<(u32, u32)> {
    let find = |data, name: &[u8; 4]| boxes(data).find(|(kind, _)| kind == name).map(|b| b.1);
    let hdlr = find(find(trak, b"mdia")?, b"hdlr")?;
    if hdlr.get(8..12)? != b"vide" {
        return None;
    }
    // Width and height close the box as 16.16 fixed point, in both box versions.
    let tkhd = find(trak, b"tkhd")?;
    let end = tkhd.len().checked_sub(8)?;
    let half = |at: usize| Some(u32::from_be_bytes(tkhd.get(at..at + 4)?.try_into().ok()?) >> 16);
    let (width, height) = (half(end)?, half(end + 4)?);
    (width > 0 && height > 0).then_some((width, height))
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
    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        [&(body.len() as u32 + 8).to_be_bytes()[..], kind, body].concat()
    }
    fn track(handler: &[u8; 4], width: u32, height: u32) -> Vec<u8> {
        // A version 0 tkhd body is 84 bytes, ending in width and height (16.16).
        let mut tkhd = vec![0u8; 76];
        tkhd.extend((width << 16).to_be_bytes());
        tkhd.extend((height << 16).to_be_bytes());
        let hdlr = [&[0u8; 8][..], handler, &[0u8; 13]].concat();
        atom(
            b"trak",
            &[atom(b"tkhd", &tkhd), atom(b"mdia", &atom(b"hdlr", &hdlr))].concat(),
        )
    }
    #[test]
    fn video_size_reads_the_video_track_wherever_moov_sits() {
        let dir = std::env::temp_dir().join(format!("motetto-video-size-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let file = dir.join("source.mp4");
        let ftyp = atom(b"ftyp", b"isom\0\0\0\0");
        let moov = atom(
            b"moov",
            &[
                track(b"soun", 0, 0),
                track(b"text", 640, 60),
                track(b"vide", 1920, 1080),
            ]
            .concat(),
        );
        fs::write(&file, [&ftyp[..], &moov, &atom(b"mdat", &[1; 32])].concat()).unwrap();
        assert_eq!(video_size(&file), Some((1920, 1080)));
        // A 64-bit mdat before moov, as cameras and screen recorders write it.
        let mut mdat = [&1u32.to_be_bytes()[..], b"mdat", &48u64.to_be_bytes()].concat();
        mdat.extend([0; 32]);
        fs::write(&file, [&ftyp[..], &mdat, &moov].concat()).unwrap();
        assert_eq!(video_size(&file), Some((1920, 1080)));
        let audio = atom(b"moov", &track(b"soun", 0, 0));
        fs::write(&file, [&ftyp[..], &audio].concat()).unwrap();
        assert_eq!(video_size(&file), None);
        // A 64-bit size near u64::MAX ends the scan instead of overflowing.
        let huge = [
            &1u32.to_be_bytes()[..],
            b"mdat",
            &(u64::MAX - 4).to_be_bytes(),
        ]
        .concat();
        fs::write(&file, [&ftyp[..], &huge, &moov].concat()).unwrap();
        assert_eq!(video_size(&file), None);
        fs::write(&file, &moov[..moov.len() - 4]).unwrap();
        assert_eq!(video_size(&file), None);
        assert!(imported_video("source.mov") && !imported_video("../source.mp4"));
        fs::remove_dir_all(dir).unwrap();
    }
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

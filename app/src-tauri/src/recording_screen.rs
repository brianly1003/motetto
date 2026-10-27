//! Picture-part metadata and bounded progress handling outside native capture callbacks.
use crate::{
    controller::Message,
    screen::{self, ScreenPart, ScreenSourceInfo},
    screen_files,
};
use motetto_core::{
    events::{ScreenChoice, ScreenPick, Stop},
    video::{ScreenCapture, VideoOutcome},
};
use std::{path::Path, sync::mpsc, thread::JoinHandle};
use tauri::{Emitter, Manager};

fn part(base: &str, offset_ms: u64, pick: &ScreenPick, while_sharing: bool) -> ScreenPart {
    ScreenPart {
        file: format!(
            "{}.screen.mp4",
            Path::new(base)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ),
        offset_ms,
        source: ScreenSourceInfo {
            kind: match pick.choice {
                ScreenChoice::CallWindow { .. } => "window",
                ScreenChoice::Display { .. } => "display",
            }
            .into(),
            name: pick.name.clone(),
        },
        while_sharing,
        width: 0,
        height: 0,
    }
}

type Prepared = (Option<ScreenCapture>, Option<JoinHandle<()>>);
pub fn prepare(
    app: &tauri::AppHandle,
    results: &mpsc::Sender<Message>,
    id: &str,
    base: &str,
    offset_ms: u64,
    stop: &Stop,
    pick: Option<(&ScreenPick, bool)>,
) -> Result<Prepared, String> {
    let Some((pick, while_sharing)) = pick else {
        screen::set_window(stop, 0);
        return Ok((None, None));
    };
    let dir = Path::new(base)
        .parent()
        .ok_or("Missing recording folder")?
        .to_path_buf();
    let settings = app.state::<crate::settings::Store>().get()?;
    let resolved = screen::resolve(pick, &settings, &format!("{base}.screen.mp4"), &dir);
    let mut capture = match resolved {
        screen::Resolved::Capture(capture) => capture,
        fallback => {
            let lost = matches!(fallback, screen::Resolved::NoWindow);
            let _ = results.send(Message::ScreenEnded {
                id: id.into(),
                part: stop.clone(),
                lost,
                restart: false,
                reason: (!lost)
                    .then(|| "Not enough space for the video. Recording audio only".into()),
            });
            return Ok((None, None));
        }
    };
    screen::set_window(
        stop,
        match capture.target {
            motetto_core::video::ScreenTarget::Window(id) => id,
            _ => 0,
        },
    );
    let part = part(base, offset_ms, pick, while_sharing);
    let initial = VideoOutcome {
        path: capture.path.clone(),
        width: 0,
        height: 0,
        moments_ms: vec![],
        lost: false,
        error: None,
    };
    if let Err(reason) = screen_files::progress(&dir, &part, &initial, false) {
        let _ = results.send(Message::ScreenEnded {
            id: id.into(),
            part: stop.clone(),
            lost: false,
            restart: false,
            reason: Some(reason),
        });
        return Ok((None, None));
    }
    let (tx, rx) = mpsc::sync_channel::<VideoOutcome>(4);
    capture.progress = Some(tx);
    let results = results.clone();
    let id = id.to_string();
    let stop = stop.clone();
    let app = app.clone();
    let thread = std::thread::Builder::new()
        .name("motetto-video-metadata".into())
        .spawn(move || {
            let mut ended = false;
            for video in rx {
                let saved = screen_files::progress(&dir, &part, &video, false);
                if !ended && (video.lost || video.error.is_some() || saved.is_err()) {
                    let _ = results.send(Message::ScreenEnded {
                        id: id.clone(),
                        part: stop.clone(),
                        lost: video.lost && saved.is_ok(),
                        restart: video.width > 0 || saved.is_err(),
                        reason: saved.err().or(video.error.clone()),
                    });
                    ended = true;
                }
            }
            let _ = app.emit("motetto://library-changed", ());
        })
        .map_err(|e| e.to_string())?;
    Ok((Some(capture), Some(thread)))
}

pub fn finish(
    base: &str,
    offset: u64,
    pick: Option<&ScreenPick>,
    sharing: bool,
    video: &VideoOutcome,
    app: &tauri::AppHandle,
) {
    if let (Some(pick), Some(dir)) = (pick, Path::new(base).parent())
        && let Err(reason) =
            screen_files::progress(dir, &part(base, offset, pick, sharing), video, true)
    {
        let _ = app.emit(
            "motetto://notice",
            format!("Couldn't save video details: {reason}"),
        );
    }
}

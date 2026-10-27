//! Pure call detection: which app is on a call, from a snapshot of audio processes and titles.
//! No FFI and no threads; the platform layer builds `Snapshot`s and feeds `Watcher::update`.
//! Detection only decides when to ask, it never starts a recording (SPEC 4.2).

use crate::events::Mode;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Two-signal apps need mic and output this long in a row (SPEC 4.2 step 3).
const TWO_SIGNAL_MS: u64 = 5_000;
/// Single-signal apps (Zoom, Teams) need the mic alone this long (SPEC debounce).
const SINGLE_SIGNAL_MS: u64 = 1_500;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize, ts_rs::TS)]
pub struct AppRule {
    pub app_id: String,
    pub name: String,
    pub mac_ids: Vec<String>,
    pub win_ids: Vec<String>,
    pub title_patterns: Vec<String>,
    pub single_signal: bool,
    pub mode: Mode,
}

/// The bundled call-app list.
pub fn rules() -> Vec<AppRule> {
    // The file is compiled in and a test parses it, so this cannot fail at run time.
    serde_json::from_str(include_str!("apps.json")).expect("apps.json is valid")
}

fn id_matches(id: &str, bundle_id: &str) -> bool {
    // Case-insensitive: Windows image names vary (Zoom.exe, zoom.exe).
    bundle_id.eq_ignore_ascii_case(id)
        || bundle_id
            .get(..id.len())
            .zip(bundle_id.get(id.len()..))
            .is_some_and(|(head, rest)| head.eq_ignore_ascii_case(id) && rest.starts_with('.'))
}

/// The rule for a bundle id (macOS) or image name (Windows): exact, or a helper such as
/// `com.google.Chrome.helper` of a listed id.
pub fn match_app<'a>(rules: &'a [AppRule], bundle_id: &str) -> Option<&'a AppRule> {
    rules.iter().find(|r| {
        r.mac_ids
            .iter()
            .chain(&r.win_ids)
            .any(|id| id_matches(id, bundle_id))
    })
}

/// The rule whose title pattern matches a window title. `^` at the start means "starts with",
/// anything else is a plain substring.
pub fn match_title<'a>(rules: &'a [AppRule], title: &str) -> Option<&'a AppRule> {
    rules
        .iter()
        .find(|r| r.title_patterns.iter().any(|p| title_matches(p, title)))
}

/// Match a title by prefix (`^`) or literal substring, as the call detector does.
pub fn title_matches(pattern: &str, title: &str) -> bool {
    match pattern.strip_prefix('^') {
        Some(prefix) => title.starts_with(prefix),
        None => title.contains(pattern),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioProcess {
    pub pid: u32,
    pub bundle_id: String,
    pub input: bool,
    pub output: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub processes: Vec<AudioProcess>,
    /// (owner bundle id, window title)
    pub titles: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    Started {
        call_id: String,
        app_id: String,
        app: String,
    },
    Ended {
        call_id: String,
    },
    Mic {
        app_id: String,
        active: bool,
    },
}

#[derive(Default)]
struct AppState {
    /// Since when the call signals have held without a break (call not started yet).
    qualifying_since: Option<u64>,
    call_id: Option<String>,
    /// The app the call was announced as; later title changes never rename it.
    call_app_id: String,
    mic_active: bool,
    /// First poll with no mic input while a call is on.
    gone_since: Option<u64>,
}

pub struct Watcher {
    own_pid: u32,
    end_after_ms: u64,
    apps: BTreeMap<String, AppState>,
}

/// Motetto's own capture holds the mic while recording; it is never a call.
fn is_own(own_pid: u32, p: &AudioProcess) -> bool {
    p.pid == own_pid || id_matches("local.motetto.desktop", &p.bundle_id)
}

impl Watcher {
    pub fn new(own_pid: u32, end_after_ms: u64) -> Self {
        Self {
            own_pid,
            end_after_ms,
            apps: BTreeMap::new(),
        }
    }

    /// The app a process counts for, and its state key. A browser process becomes the service
    /// that its own browser's window titles show (Meet, Teams on the web); no matching title
    /// leaves it "browser". The key is the browser family, so one browser holds one call even
    /// while its title appears or disappears.
    fn resolve<'a>(
        rules: &'a [AppRule],
        snapshot: &Snapshot,
        p: &AudioProcess,
    ) -> Option<(String, &'a AppRule)> {
        let rule = match_app(rules, &p.bundle_id)?;
        if rule.app_id != "browser" {
            return Some((rule.app_id.clone(), rule));
        }
        let family = family_of(rule, &p.bundle_id);
        let titled = snapshot
            .titles
            .iter()
            .filter(|(owner, _)| {
                match_app(rules, owner).is_some_and(|r| r.app_id == "browser")
                    && family_of(rule, owner).eq_ignore_ascii_case(&family)
            })
            .find_map(|(_, title)| match_title(rules, title));
        Some((format!("browser:{family}"), titled.unwrap_or(rule)))
    }

    pub fn set_end_after_ms(&mut self, value: u64) {
        self.end_after_ms = value;
    }

    pub fn update(
        &mut self,
        rules: &[AppRule],
        snapshot: &Snapshot,
        now_ms: u64,
    ) -> Vec<WatchEvent> {
        // state key -> (rule, any process holds input, any process plays output)
        let mut seen: BTreeMap<String, (&AppRule, bool, bool)> = BTreeMap::new();
        for p in &snapshot.processes {
            if is_own(self.own_pid, p) {
                continue;
            }
            let Some((key, rule)) = Self::resolve(rules, snapshot, p) else {
                continue;
            };
            let e = seen.entry(key).or_insert((rule, false, false));
            e.1 |= p.input;
            e.2 |= p.output;
        }

        let mut events = Vec::new();
        let ids: Vec<String> = self
            .apps
            .keys()
            .cloned()
            .chain(seen.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for id in ids {
            let now_seen = seen.get(id.as_str()).copied();
            let input = now_seen.is_some_and(|s| s.1);
            let st = self.apps.entry(id.clone()).or_default();

            if let Some(call_id) = st.call_id.clone() {
                if input {
                    st.gone_since = None;
                    if !st.mic_active {
                        st.mic_active = true;
                        events.push(WatchEvent::Mic {
                            app_id: st.call_app_id.clone(),
                            active: true,
                        });
                    }
                } else {
                    if st.mic_active {
                        st.mic_active = false;
                        events.push(WatchEvent::Mic {
                            app_id: st.call_app_id.clone(),
                            active: false,
                        });
                    }
                    let gone = *st.gone_since.get_or_insert(now_ms);
                    if now_ms.saturating_sub(gone) >= self.end_after_ms {
                        events.push(WatchEvent::Ended { call_id });
                        self.apps.remove(&id);
                    }
                }
                continue;
            }

            let Some((rule, _, output)) = now_seen else {
                self.apps.remove(&id);
                continue;
            };
            // Teams on the web inherits single_signal from the Teams row, so a
            // browser with a Teams-titled window asks after 1.5 s of mic alone. Split the
            // row if live tests show false asks.
            let (qualifies, need) = if rule.single_signal {
                (input, SINGLE_SIGNAL_MS)
            } else {
                (input && output, TWO_SIGNAL_MS)
            };
            if !qualifies {
                st.qualifying_since = None;
                continue;
            }
            let since = *st.qualifying_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= need {
                let call_id = format!("{}-{now_ms}", rule.app_id);
                st.call_id = Some(call_id.clone());
                st.call_app_id = rule.app_id.clone();
                st.mic_active = true;
                events.push(WatchEvent::Started {
                    call_id,
                    app_id: rule.app_id.clone(),
                    app: rule.name.clone(),
                });
            }
        }
        events
    }
}

/// What this computer's audio processes and browser window titles look like now.
pub fn snapshot() -> Result<Snapshot, String> {
    #[cfg(target_os = "macos")]
    {
        use crate::platform::macos_calls;
        let browsers = rules()
            .into_iter()
            .find(|r| r.app_id == "browser")
            .map(|r| r.mac_ids)
            .unwrap_or_default();
        // Zoom and Teams need only the processes, so a title failure leaves titles empty.
        Ok(Snapshot {
            processes: macos_calls::processes()?,
            titles: macos_calls::browser_titles(&browsers).unwrap_or_default(),
        })
    }
    // WASAPI sessions (SPEC 4.2 Windows column) when Windows acceptance resumes
    #[cfg(not(target_os = "macos"))]
    Ok(Snapshot::default())
}

/// The browser a bundle id belongs to: the listed id it matches (so a helper joins its
/// browser). Safari's web-content helper is a separate bundle, so it maps to Safari.
fn family_of(rule: &AppRule, bundle_id: &str) -> String {
    if id_matches("com.apple.WebKit.WebContent", bundle_id) {
        return "com.apple.Safari".into();
    }
    rule.mac_ids
        .iter()
        .chain(&rule.win_ids)
        .find(|id| id_matches(id, bundle_id))
        .map(|id| id.to_ascii_lowercase())
        .unwrap_or_else(|| bundle_id.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(bundle: &str, input: bool, output: bool) -> Snapshot {
        Snapshot {
            processes: vec![AudioProcess {
                pid: 10,
                bundle_id: bundle.into(),
                input,
                output,
            }],
            titles: vec![],
        }
    }

    fn started(ev: &[WatchEvent]) -> usize {
        ev.iter()
            .filter(|e| matches!(e, WatchEvent::Started { .. }))
            .count()
    }

    #[test]
    fn helper_bundles_resolve_to_their_app() {
        let r = rules();
        let id = |b| match_app(&r, b).map(|x| x.app_id.as_str());
        assert_eq!(id("com.google.Chrome.helper"), Some("browser"));
        assert_eq!(id("com.microsoft.teams2"), Some("teams"));
        assert_eq!(id("com.microsoft.teams2.helper"), Some("teams"));
        assert_eq!(id("com.apple.Music"), None);
        assert_eq!(id("com.google.Chromeish"), None);
    }

    #[test]
    fn meet_title_turns_a_browser_into_meet() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let mut snap = proc("com.google.Chrome.helper", true, true);
        snap.titles = vec![("com.google.Chrome".into(), "Meet - abc-defg-hij".into())];
        assert!(w.update(&r, &snap, 0).is_empty());
        let ev = w.update(&r, &snap, 5_000);
        assert_eq!(
            ev,
            vec![WatchEvent::Started {
                call_id: "meet-5000".into(),
                app_id: "meet".into(),
                app: "Google Meet".into()
            }]
        );
    }

    #[test]
    fn untitled_browser_stays_a_browser() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let snap = proc("com.google.Chrome.helper", true, true);
        w.update(&r, &snap, 0);
        let ev = w.update(&r, &snap, 5_000);
        assert!(matches!(&ev[0], WatchEvent::Started { app_id, app, .. }
            if app_id == "browser" && app == "Browser"));
    }

    #[test]
    fn single_signal_app_starts_after_one_and_a_half_seconds() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let snap = proc("us.zoom.xos", true, false);
        assert!(w.update(&r, &snap, 0).is_empty());
        assert_eq!(started(&w.update(&r, &snap, 2_000)), 1);
    }

    #[test]
    fn voice_message_without_playback_starts_nothing() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let mic_only = proc("net.whatsapp.WhatsApp", true, false);
        for t in (0..=10_000).step_by(2_000) {
            assert!(w.update(&r, &mic_only, t).is_empty());
        }
        let mut w = Watcher::new(1, 30_000);
        let both = proc("net.whatsapp.WhatsApp", true, true);
        for t in (0..=4_000).step_by(2_000) {
            assert!(w.update(&r, &both, t).is_empty());
        }
    }

    #[test]
    fn brief_mic_drop_keeps_the_same_call() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let on = proc("us.zoom.xos", true, true);
        let off = Snapshot::default();
        w.update(&r, &on, 0);
        let first = w.update(&r, &on, 2_000);
        assert_eq!(started(&first), 1);
        assert_eq!(
            w.update(&r, &off, 10_000),
            vec![WatchEvent::Mic {
                app_id: "zoom".into(),
                active: false
            }]
        );
        assert!(w.update(&r, &off, 18_000).is_empty());
        assert_eq!(
            w.update(&r, &on, 20_000),
            vec![WatchEvent::Mic {
                app_id: "zoom".into(),
                active: true
            }]
        );
        // Still the same call afterwards: nothing new, no second Started.
        assert!(w.update(&r, &on, 22_000).is_empty());
    }

    #[test]
    fn call_ends_after_the_mic_is_gone_long_enough() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let on = proc("us.zoom.xos", true, true);
        let off = Snapshot::default();
        w.update(&r, &on, 0);
        let call_id = match &w.update(&r, &on, 2_000)[0] {
            WatchEvent::Started { call_id, .. } => call_id.clone(),
            e => panic!("{e:?}"),
        };
        w.update(&r, &off, 10_000);
        assert!(w.update(&r, &off, 38_000).is_empty());
        assert_eq!(
            w.update(&r, &off, 40_000),
            vec![WatchEvent::Ended {
                call_id: call_id.clone()
            }]
        );
        assert!(w.update(&r, &off, 42_000).is_empty());
        assert!(w.update(&r, &off, 60_000).is_empty());
    }

    #[test]
    fn motetto_own_process_is_never_a_call() {
        let r = rules();
        let mut w = Watcher::new(42, 30_000);
        let snap = Snapshot {
            processes: vec![AudioProcess {
                pid: 42,
                bundle_id: "local.motetto.desktop".into(),
                input: true,
                output: true,
            }],
            titles: vec![],
        };
        for t in (0..=10_000).step_by(2_000) {
            assert!(w.update(&r, &snap, t).is_empty());
        }
    }

    #[test]
    fn every_bundled_rule_parses_and_defaults_are_ask_or_off() {
        let r = rules();
        assert!(!r.is_empty());
        let get = |id: &str| r.iter().find(|x| x.app_id == id).unwrap();
        assert!(get("zoom").single_signal);
        assert_eq!(get("zoom").mode, Mode::Ask);
        assert_eq!(get("slack").mode, Mode::Off);
        assert_eq!(get("webex").mode, Mode::Off);
        assert_eq!(get("facetime").mode, Mode::Off);
        assert_eq!(get("browser").name, "Browser");
        assert!(get("meet").mac_ids.is_empty());
    }

    #[test]
    fn title_patterns_use_prefix_and_substring() {
        let r = rules();
        let id = |t| match_title(&r, t).map(|x| x.app_id.as_str());
        assert_eq!(id("Meet - abc"), Some("meet"));
        assert_eq!(id("abc - Google Meet"), Some("meet"));
        assert_eq!(id("Say Meet - x"), None);
        assert_eq!(id("Chat | Microsoft Teams"), Some("teams"));
    }

    fn chrome(titles: &[&str]) -> Snapshot {
        let mut s = proc("com.google.Chrome.helper", true, true);
        s.titles = titles
            .iter()
            .map(|t| ("com.google.Chrome".to_string(), t.to_string()))
            .collect();
        s
    }

    #[test]
    fn browser_then_meet_title_is_one_call() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let mut all = Vec::new();
        all.extend(w.update(&r, &chrome(&[]), 0));
        all.extend(w.update(&r, &chrome(&[]), 5_000));
        for t in [7_000, 9_000, 20_000] {
            all.extend(w.update(&r, &chrome(&["Meet - abc-defg-hij"]), t));
        }
        assert_eq!(started(&all), 1);
        let id = match &all[0] {
            WatchEvent::Started { call_id, .. } => call_id.clone(),
            e => panic!("{e:?}"),
        };
        let off = Snapshot::default();
        w.update(&r, &off, 30_000);
        assert_eq!(
            w.update(&r, &off, 60_000),
            vec![WatchEvent::Ended { call_id: id }]
        );
    }

    #[test]
    fn meet_then_lost_title_is_one_call() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let mut all = Vec::new();
        all.extend(w.update(&r, &chrome(&["Meet - abc-defg-hij"]), 0));
        all.extend(w.update(&r, &chrome(&["Meet - abc-defg-hij"]), 5_000));
        for t in [7_000, 9_000, 20_000] {
            all.extend(w.update(&r, &chrome(&["Inbox"]), t));
        }
        assert_eq!(started(&all), 1);
        assert!(
            matches!(&all[0], WatchEvent::Started { app_id, call_id, .. }
            if app_id == "meet" && call_id == "meet-5000")
        );
        assert_eq!(all.len(), 1);
        let off = Snapshot::default();
        w.update(&r, &off, 30_000);
        assert_eq!(
            w.update(&r, &off, 60_000),
            vec![WatchEvent::Ended {
                call_id: "meet-5000".into()
            }]
        );
    }

    #[test]
    fn title_from_another_browser_does_not_resolve() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let mut snap = proc("com.google.Chrome.helper", true, true);
        snap.titles = vec![("com.apple.Safari".into(), "Meet - abc-defg-hij".into())];
        w.update(&r, &snap, 0);
        let ev = w.update(&r, &snap, 5_000);
        assert!(matches!(&ev[0], WatchEvent::Started { app_id, .. } if app_id == "browser"));
    }

    #[test]
    fn single_signal_boundary_is_exact() {
        let r = rules();
        let mut w = Watcher::new(1, 30_000);
        let snap = proc("us.zoom.xos", true, false);
        assert!(w.update(&r, &snap, 0).is_empty());
        assert!(w.update(&r, &snap, 1_499).is_empty());
        assert_eq!(started(&w.update(&r, &snap, 1_500)), 1);
    }

    #[test]
    fn windows_image_names_match_case_insensitively() {
        let r = rules();
        assert_eq!(
            match_app(&r, "zoom.exe").map(|x| x.app_id.as_str()),
            Some("zoom")
        );
        assert_eq!(
            match_app(&r, "ZOOM.EXE").map(|x| x.app_id.as_str()),
            Some("zoom")
        );
    }
}

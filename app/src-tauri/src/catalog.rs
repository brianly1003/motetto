//! The pinned list of downloadable models and the three tiers built from them.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use ts_rs::TS;

pub use motetto_core::events::{SpeedResult, TierId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum FileKind {
    Speech,
    Vad,
    Notes,
    /// The speaker-diarization pair: segmentation, then voice embeddings.
    Speakers,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ModelFile {
    pub id: String,
    pub kind: FileKind,
    pub file: String,
    pub label: String,
    pub url: String,
    #[ts(type = "number")]
    pub bytes: u64,
    pub sha256: String,
    /// Filled in for `model_catalog` only: the file is installed (exact size on disk).
    #[serde(default)]
    pub on_disk: bool,
}

/// `speech` and `notes` are `ModelFile` ids; the suits strings are shown as-is.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct Tier {
    pub id: TierId,
    pub speech: String,
    pub notes: String,
    pub suits_mac: String,
    pub suits_pc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct Catalog {
    pub files: Vec<ModelFile>,
    pub tiers: Vec<Tier>,
    pub vad: String,
    /// Segmentation then embedding model: every tier downloads both, after the notes model.
    pub speakers: Vec<String>,
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../models.json")).expect("models.json is valid")
    })
}

impl Catalog {
    pub fn file(&self, id: &str) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.id == id)
    }
    pub fn tier(&self, id: TierId) -> &Tier {
        self.tiers
            .iter()
            .find(|t| t.id == id)
            .expect("every tier is in models.json")
    }
    /// Speech + notes + voice detection + speakers: what a tier downloads in total.
    pub fn tier_bytes(&self, id: TierId) -> u64 {
        self.tier_files(id)
            .filter_map(|f| self.file(f))
            .map(|f| f.bytes)
            .sum()
    }
    /// Every file a tier needs, in download order after the speed test: speech, voice detection,
    /// notes, speakers.
    pub fn tier_files(&self, id: TierId) -> impl Iterator<Item = &String> {
        let tier = self.tier(id);
        [&tier.speech, &self.vad, &tier.notes]
            .into_iter()
            .chain(&self.speakers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_hex(s: &str, len: usize) -> bool {
        s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    #[test]
    fn catalog_is_pinned_and_complete() {
        let c = catalog();
        let mut ids: Vec<&str> = c.files.iter().map(|f| f.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), c.files.len(), "ids are unique");
        assert_eq!(c.files.len(), 9);
        for f in &c.files {
            assert!(f.url.starts_with("https://huggingface.co/"), "{}", f.id);
            assert!(f.url.ends_with(&format!("/{}", f.file)), "{}", f.id);
            let (_, rest) = f.url.split_once("/resolve/").expect("pinned resolve url");
            assert!(
                is_hex(&rest[..40], 40) && rest[40..].starts_with('/'),
                "{}",
                f.id
            );
            assert!(is_hex(&f.sha256, 64), "{}", f.id);
            assert!(f.bytes > 0, "{}", f.id);
        }
        assert_eq!(c.tiers.len(), 3);
        for t in &c.tiers {
            assert_eq!(c.file(&t.speech).unwrap().kind, FileKind::Speech);
            assert_eq!(c.file(&t.notes).unwrap().kind, FileKind::Notes);
        }
        assert_eq!(c.file(&c.vad).unwrap().kind, FileKind::Vad);
        assert_eq!(c.speakers.len(), 2);
        for id in &c.speakers {
            assert_eq!(c.file(id).unwrap().kind, FileKind::Speakers);
        }
        assert_eq!(
            c.tier_bytes(TierId::Best),
            1_081_140_203 + 5_027_783_488 + 885_098 + 5_992_913 + 28_281_164
        );
    }
}

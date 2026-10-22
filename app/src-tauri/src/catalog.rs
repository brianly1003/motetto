//! The pinned list of downloadable models and the three tiers built from them.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum TierId {
    Best,
    Balanced,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum FileKind {
    Speech,
    Vad,
    Notes,
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
}

/// One speed test: `rtf` is processing time over audio time, so below 1.0 keeps up with a call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SpeedResult {
    pub tier: TierId,
    pub rtf: f32,
    pub keeps_up: bool,
    pub measured_at: String,
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
    /// Speech + notes + voice detection: what a tier downloads in total.
    pub fn tier_bytes(&self, id: TierId) -> u64 {
        let tier = self.tier(id);
        [&tier.speech, &tier.notes, &self.vad]
            .iter()
            .filter_map(|f| self.file(f))
            .map(|f| f.bytes)
            .sum()
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
        assert_eq!(c.files.len(), 7);
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
        assert_eq!(
            c.tier_bytes(TierId::Best),
            1_081_140_203 + 5_027_783_488 + 885_098
        );
    }
}

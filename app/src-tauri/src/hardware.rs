//! This computer's chip and memory, and the tier offered first for it (SPEC 5.1).
use crate::catalog::TierId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Hardware {
    pub chip: String,
    pub memory_gb: u32,
    pub suggested: TierId,
    /// "M1 Max, 32 GB"
    pub label: String,
}

/// macOS column of SPEC 5.1: 16 GB or more starts at Best, less at Balanced. Light is only
/// offered by a slow speed test.
pub fn suggested_tier(memory_gb: u32) -> TierId {
    if memory_gb >= 16 {
        TierId::Best
    } else {
        TierId::Balanced
    }
}

fn describe(chip: &str, memory_gb: u32) -> Hardware {
    let chip = chip.trim().trim_start_matches("Apple ").to_string();
    let label = match (chip.is_empty(), memory_gb) {
        (false, 0) => chip.clone(),
        (true, 0) => "This computer".into(),
        (true, gb) => format!("{gb} GB"),
        (false, gb) => format!("{chip}, {gb} GB"),
    };
    Hardware {
        chip,
        memory_gb,
        suggested: suggested_tier(memory_gb),
        label,
    }
}

pub fn read() -> Hardware {
    describe(
        &sysctl("machdep.cpu.brand_string").unwrap_or_default(),
        memory_gb().map_or(0, |gb| u32::try_from(gb).unwrap_or(u32::MAX)),
    )
}

/// Physical memory in whole GB from `sysctl hw.memsize`; None where it can't be read. Windows
/// picks its tier from the GPU (SPEC), which isn't built yet.
pub fn memory_gb() -> Option<u64> {
    sysctl("hw.memsize")?.parse::<u64>().ok().map(|b| b >> 30)
}

#[cfg(target_os = "macos")]
fn sysctl(key: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", key])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
#[cfg(not(target_os = "macos"))]
fn sysctl(_key: &str) -> Option<String> {
    None
}

#[tauri::command]
pub async fn hardware_info() -> Result<Hardware, String> {
    tauri::async_runtime::spawn_blocking(read)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suggested_tier_by_memory() {
        assert_eq!(suggested_tier(32), TierId::Best);
        assert_eq!(suggested_tier(16), TierId::Best);
        assert_eq!(suggested_tier(15), TierId::Balanced);
        assert_eq!(suggested_tier(8), TierId::Balanced);
        assert_eq!(describe("Apple M1 Max", 32).label, "M1 Max, 32 GB");
        assert_eq!(describe("", 0).suggested, TierId::Balanced);
        Hardware::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }
}

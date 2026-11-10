//! Platform capture backends and their shared packet-to-WAV rules.

#[cfg(any(target_os = "windows", test))]
mod audio;
#[cfg(target_os = "macos")]
pub(crate) mod macos_calls;
#[cfg(target_os = "macos")]
pub(crate) mod macos_mics;
#[cfg(target_os = "macos")]
pub(crate) mod macos_trim;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub(crate) mod macos_video;

/// Samples one 32-bit float WAV track may hold. hound keeps the data size in a `u32` and, in a
/// release build, wraps silently past 4 GiB: the header would then claim a few bytes and every
/// reader would lose the rest. A 1 MiB margin covers the header. The session starts a new part
/// long before this (`session::PART_MAX_MS`); this stop only guards rates above 96 kHz.
pub(crate) const WAV_MAX_SAMPLES: u32 = (u32::MAX - (1 << 20)) / 4;

/// Err once a track holding `written` samples has no room for one more.
pub(crate) fn wav_room(written: u32) -> Result<(), &'static str> {
    if written >= WAV_MAX_SAMPLES {
        Err("this part reached the 4 GB size limit of a WAV file; the audio so far is kept")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_track_stops_before_its_wav_size_wraps() {
        assert!(super::wav_room(0).is_ok());
        assert!(super::wav_room(super::WAV_MAX_SAMPLES - 1).is_ok());
        assert!(super::wav_room(super::WAV_MAX_SAMPLES).is_err());
        // The full data size, plus the header, still fits a u32.
        assert!(u64::from(super::WAV_MAX_SAMPLES) * 4 + 1024 < u64::from(u32::MAX));
        // Three hours at 96 kHz fit, so the session's part rotation always comes first.
        assert!(3 * 3600 * 96_000 < u64::from(super::WAV_MAX_SAMPLES));
    }
}

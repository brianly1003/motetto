//! HTTP inference against a caller-managed whisper-server on IPv4 loopback only.

use serde::Deserialize;
use std::{error::Error, io::Cursor, time::Duration};

/// A timestamped speech segment on the meeting timeline.
#[derive(Debug, Deserialize, PartialEq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Reuses connections to a local sidecar. Process startup belongs to the app adapter.
pub struct Client {
    agent: ureq::Agent,
    url: String,
}

impl Client {
    /// Only a port is accepted, so settings cannot redirect audio off the machine.
    pub fn new(port: u16) -> Result<Self, Box<dyn Error>> {
        if port == 0 {
            return Err("whisper-server port must be nonzero".into());
        }
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        Ok(Self {
            agent,
            url: format!("http://127.0.0.1:{port}/inference"),
        })
    }

    /// Send 16 kHz mono samples; offset is added locally, never used to skip slice audio.
    pub fn transcribe(
        &self,
        audio: &[f32],
        offset_ms: u64,
        language: Option<&str>,
        prompt: Option<&str>,
    ) -> Result<Vec<Segment>, Box<dyn Error>> {
        if audio.is_empty() || audio.iter().any(|v| !v.is_finite()) {
            return Err("speech input must contain finite audio samples".into());
        }
        let mut wav = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut wav,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 16_000,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )?;
            for &sample in audio {
                writer.write_sample(sample)?;
            }
            writer.finalize()?;
        }
        let boundary = "motetto-local-audio-boundary";
        let mut body = Vec::new();
        for (name, value) in [
            ("response_format", "verbose_json"),
            ("language", language.unwrap_or("auto")),
            ("prompt", prompt.unwrap_or("")),
            ("beam_size", "1"),
            ("best_of", "1"),
            ("translate", "false"),
            ("offset_t", "0"),
        ] {
            if value.contains(boundary) {
                return Err("speech parameter contains multipart delimiter".into());
            }
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        }
        let wav = wav.into_inner();
        if wav
            .windows(boundary.len())
            .any(|v| v == boundary.as_bytes())
        {
            return Err("audio contains multipart delimiter".into());
        }
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"slice.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
        body.extend_from_slice(&wav);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let mut response = self
            .agent
            .post(&self.url)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .send(body.as_slice())
            .map_err(|e| {
                format!("local whisper-server failed: {e}; check the sidecar and its model")
            })?;
        if !response.status().is_success() {
            return Err(format!("local whisper-server returned {}", response.status()).into());
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_vec()?;
        parse(&bytes, offset_ms)
    }
}

fn parse(bytes: &[u8], offset: u64) -> Result<Vec<Segment>, Box<dyn Error>> {
    #[derive(Deserialize)]
    struct Reply {
        segments: Vec<Part>,
    }
    #[derive(Deserialize)]
    struct Part {
        start: f64,
        end: f64,
        text: String,
    }
    let reply: Reply = serde_json::from_slice(bytes)?;
    reply
        .segments
        .into_iter()
        .filter(|s| !s.text.trim().is_empty() && !crate::echo::credit(&s.text))
        .map(|s| {
            if !s.start.is_finite()
                || !s.end.is_finite()
                || s.start < 0.0
                || s.end < s.start
                || s.end * 1000.0 >= u64::MAX as f64
            {
                return Err("invalid whisper-server segment times".into());
            }
            Ok(Segment {
                start_ms: offset
                    .checked_add((s.start * 1000.0).round() as u64)
                    .ok_or("segment timestamp overflow")?,
                end_ms: offset
                    .checked_add((s.end * 1000.0).round() as u64)
                    .ok_or("segment timestamp overflow")?,
                text: s.text.trim().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;

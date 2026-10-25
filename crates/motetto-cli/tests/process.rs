//! Exercise subprocess settings without recording or contacting a model server.
#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::{Arc, Mutex};

/// A stub of Ollama's local API: answers every generate request with one line of transcript and
/// keeps each request's JSON body.
fn stub_ollama() -> (u16, Arc<Mutex<Vec<serde_json::Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies = Arc::new(Mutex::new(vec![]));
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for mut socket in listener.incoming().flatten() {
            let mut request = vec![];
            let mut buf = [0u8; 4096];
            // Headers, then Content-Length bytes of JSON.
            let body = loop {
                let n = socket.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break None;
                }
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request).into_owned();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|n| n.trim().parse().ok())
                        .unwrap_or(0);
                    if body.len() >= length {
                        break Some(body.to_string());
                    }
                }
            };
            if let Some(body) = body {
                seen.lock()
                    .unwrap()
                    .push(serde_json::from_str(&body).unwrap());
            }
            let line = "{\"response\":\"[00:00] Say \\\"go\\\"\\n\",\"done\":true}\n";
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{line}\r\n0\r\n\r\n",
                line.len()
            );
        }
    });
    (port, bodies)
}

#[test]
fn local_host_and_empty_vad_survive_conflicting_shell_settings() {
    let dir = std::env::temp_dir().join(format!(
        "motetto-process-{}-{}",
        std::process::id(),
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    // The notes model is reached through Ollama's API on 127.0.0.1; only its port can be moved,
    // and only from the global .env.
    let (port, bodies) = stub_ollama();
    fs::create_dir_all(dir.join(".config/motetto")).unwrap();
    fs::write(
        dir.join(".config/motetto/.env"),
        format!("OLLAMA_PORT={port}\n"),
    )
    .unwrap();
    for (name, script) in [
        ("ffmpeg", "#!/bin/sh\nexit 99\n"),
        (
            "whisper-cli",
            "#!/bin/sh\nprintf '%s\n' \"$@\" > whisper-args\nprintf '0,1000,\" Say \"\"go\"\"\"\n' > call.csv\n",
        ),
        (
            "ollama",
            &format!(
                "#!/bin/sh\n[ \"$OLLAMA_HOST\" = http://127.0.0.1:{port} ] || exit 22\nprintf '%s\n' \"$1\" >> ollama-calls\nif [ \"$1\" = list ]; then\n  printf 'NAME ID SIZE\ntest:latest abc 1GB\n'\nelse\n  cat >/dev/null\n  printf '[00:00] Say \"go\"\n'\nfi\n"
            ),
        ),
    ] {
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut wav = hound::WavWriter::create(
        dir.join("call.wav"),
        hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    wav.write_sample(0_i16).unwrap();
    wav.finalize().unwrap();
    for (vad, expected) in [
        ("WHISPER_VAD_MODEL=\n", None),
        ("", None),
        ("WHISPER_VAD_MODEL=enabled.bin\n", Some("enabled.bin")),
    ] {
        fs::write(
            dir.join(".env"),
            format!("WHISPER_MODEL=fake.bin\n{vad}OLLAMA_MODEL=test\nFIX_MODEL=test\nSUMMARY_CHUNK_CHARS=24000\n"),
        ).unwrap();
        fs::write(dir.join("ollama-calls"), "").unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_motetto"));
        command
            .args(["run", "--file", "call.wav"])
            .current_dir(&dir)
            .env("HOME", &dir)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .env("OLLAMA_HOST", "http://192.0.2.1:11434")
            .env_remove("WHISPER_VAD_MODEL");
        if vad == "WHISPER_VAD_MODEL=\n" {
            command.env("WHISPER_VAD_MODEL", "stale-model.bin");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(dir.join("call.txt")).unwrap(),
            "[00:00] Say \"go\"\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("call.fixed.txt")).unwrap(),
            "[00:00] Say \"go\"\n"
        );
        // The CLI only lists models; the correction and the notes go to the API, each with the
        // fixed context.
        assert_eq!(
            fs::read_to_string(dir.join("ollama-calls")).unwrap(),
            "list\nlist\n"
        );
        let calls: Vec<_> = bodies.lock().unwrap().drain(..).collect();
        assert_eq!(calls.len(), 2);
        assert!(
            calls
                .iter()
                .all(|c| c["model"] == "test" && c["options"]["num_ctx"] == 32768)
        );
        let args = fs::read_to_string(dir.join("whisper-args")).unwrap();
        match expected {
            Some(model) => assert!(args.contains(&format!("--vad\n-vm\n{model}\n"))),
            None => assert!(!args.lines().any(|a| a == "--vad" || a == "-vm")),
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

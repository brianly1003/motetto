//! Notes model calls to the local Ollama server on IPv4 loopback only, streamed, so the context
//! size, the answer cap and Stop are Motetto's: `ollama run` sets none of them, and Ollama.app's
//! own Context length (256k on the owner's Mac) gave qwen3:4b 43 GB on a 32 GB Mac (2026-10-25).
//!
//! A plain TCP stream rather than an HTTP library: Stop has to end a call while the model is still
//! reading the prompt (minutes on a long call, with nothing streamed back), and closing the socket
//! is what makes Ollama stop.

use std::error::Error;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

/// Ollama's port on 127.0.0.1: 11434, or `OLLAMA_PORT` (global `.env` or shell only). Only the
/// port can change, so the transcript never leaves this computer.
pub fn port() -> u16 {
    crate::config::var("OLLAMA_PORT")
        .and_then(|p| p.trim().parse().ok())
        .filter(|&p| p != 0)
        .unwrap_or(11434)
}

/// `OLLAMA_HOST` for the `ollama` CLI (model listing), pinned to the same local server.
pub fn host() -> String {
    format!("http://127.0.0.1:{}", port())
}
/// One context for every model (owner 2026-10-25): the largest prompt seen, 16,186 tokens, plus
/// `MAX_ANSWER_TOKENS` fits, a model is never reloaded between the calls of one meeting, and it
/// can't take a model's own maximum (262,144 for qwen3:4b).
pub const CONTEXT_TOKENS: u32 = 32_768;
/// Notes for a 35-minute call ran about 5,000 tokens; a looping part ran 13,357 before this cap.
pub const MAX_ANSWER_TOKENS: u32 = 8_192;
/// Long enough to stay loaded between the calls of one meeting, short enough to give the memory
/// back soon after.
const KEEP_ALIVE: &str = "1m";

/// What the model wrote, and whether `keep` stopped it before it finished.
pub struct Answer {
    pub text: String,
    pub cut: bool,
}

/// Streams `prompt` through `model`. `keep(answer so far)` returning false ends the call (a model
/// that has started looping); Stop (`tools::cancel`) ends it with `ErrorKind::Interrupted`.
pub fn generate(
    model: &str,
    prompt: &str,
    keep: impl FnMut(&str) -> bool,
) -> Result<Answer, Box<dyn Error>> {
    generate_at(&format!("127.0.0.1:{}", port()), model, prompt, keep)
}

fn generate_at(
    address: &str,
    model: &str,
    prompt: &str,
    mut keep: impl FnMut(&str) -> bool,
) -> Result<Answer, Box<dyn Error>> {
    crate::tools::check()?;
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": true,
        "think": false,
        "keep_alive": KEEP_ALIVE,
        "options": { "num_ctx": CONTEXT_TOKENS, "num_predict": MAX_ANSWER_TOKENS },
    })
    .to_string();
    let address: SocketAddr = address.parse()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|e| format!("cannot reach local Ollama; start Ollama and retry: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    write!(
        stream,
        "POST /api/generate HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut reply = Reply::default();
    let mut chunk = [0u8; 8192];
    loop {
        if crate::tools::cancelled() {
            let _ = stream.shutdown(Shutdown::Both);
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled").into());
        }
        let n = match stream.read(&mut chunk) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let done = n == 0 || reply.take(&chunk[..n])?;
        if !keep(&reply.text) {
            // Closing the connection is what stops Ollama generating.
            let _ = stream.shutdown(Shutdown::Both);
            return Ok(Answer {
                text: reply.text,
                cut: true,
            });
        }
        if done {
            reply.finish()?;
            return Ok(Answer {
                text: reply.text,
                cut: false,
            });
        }
    }
}

/// The HTTP response read so far: headers, then a chunked body of one JSON object per line.
#[derive(Default)]
struct Reply {
    raw: Vec<u8>,
    headers_done: bool,
    chunked: bool,
    body: Vec<u8>,
    text: String,
    done: bool,
}

impl Reply {
    /// Adds bytes from the socket; true once the model said it is done.
    fn take(&mut self, bytes: &[u8]) -> Result<bool, Box<dyn Error>> {
        self.raw.extend_from_slice(bytes);
        if !self.headers_done {
            let Some(end) = self.raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                return Ok(false);
            };
            let head = String::from_utf8_lossy(&self.raw[..end]).to_lowercase();
            let status = head.split_whitespace().nth(1).unwrap_or("");
            self.chunked = head.contains("transfer-encoding: chunked");
            self.raw.drain(..end + 4);
            self.headers_done = true;
            if status != "200" {
                return Err(format!("local Ollama returned {status}").into());
            }
        }
        if self.chunked {
            // size in hex, CRLF, data, CRLF; a size of 0 ends the body.
            while let Some(line) = self.raw.windows(2).position(|w| w == b"\r\n") {
                let size = String::from_utf8_lossy(&self.raw[..line]);
                let size = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)?;
                if self.raw.len() < line + 2 + size + 2 {
                    break;
                }
                self.body
                    .extend_from_slice(&self.raw[line + 2..line + 2 + size]);
                self.raw.drain(..line + 2 + size + 2);
                if size == 0 {
                    self.done = true;
                }
            }
        } else {
            self.body.append(&mut self.raw);
        }
        while let Some(end) = self.body.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.body.drain(..=end).collect();
            self.line(&line)?;
        }
        Ok(self.done)
    }

    /// One streamed object: `{"response": "...", "done": false}`, or `{"error": "..."}`.
    fn line(&mut self, line: &[u8]) -> Result<(), Box<dyn Error>> {
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        let value: serde_json::Value = serde_json::from_slice(line)?;
        if let Some(error) = value.get("error").and_then(|e| e.as_str()) {
            return Err(format!("local Ollama: {error}").into());
        }
        if let Some(part) = value.get("response").and_then(|r| r.as_str()) {
            self.text.push_str(part);
        }
        if value.get("done").and_then(|d| d.as_bool()) == Some(true) {
            self.done = true;
        }
        Ok(())
    }

    /// The connection closed: whatever is left is the last line.
    fn finish(&mut self) -> Result<(), Box<dyn Error>> {
        let rest = std::mem::take(&mut self.body);
        self.line(&rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A stub Ollama on a free loopback port: it records the request and streams `chunks`.
    fn stub(chunks: Vec<&'static str>) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let served = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            let mut request = vec![];
            let mut buf = [0u8; 4096];
            while let Ok(n @ 1..) = socket.read(&mut buf) {
                request.extend_from_slice(&buf[..n]);
            }
            let _ = socket.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n",
            );
            for line in chunks {
                // Split each line over two chunks, as a socket may.
                let (a, b) = line.split_at(line.len() / 2);
                for part in [a, b] {
                    let _ = write!(socket, "{:x}\r\n{part}\r\n", part.len());
                    let _ = socket.flush();
                }
            }
            let _ = socket.write_all(b"0\r\n\r\n");
            String::from_utf8_lossy(&request).into_owned()
        });
        (address, served)
    }

    #[test]
    fn streams_the_answer_with_a_fixed_context_and_cap() {
        let (address, served) = stub(vec![
            "{\"response\":\"### Action \",\"done\":false}\n",
            "{\"response\":\"items\\n- None\",\"done\":false}\n",
            "{\"response\":\"\",\"done\":true,\"done_reason\":\"stop\"}\n",
        ]);
        let answer = generate_at(&address, "qwen3:4b", "Notes, please", |_| true).unwrap();
        assert_eq!(answer.text, "### Action items\n- None");
        assert!(!answer.cut);
        let request = served.join().unwrap();
        assert!(request.starts_with("POST /api/generate HTTP/1.1\r\n"));
        let body: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["model"], "qwen3:4b");
        assert_eq!(body["options"]["num_ctx"], CONTEXT_TOKENS);
        assert_eq!(body["options"]["num_predict"], MAX_ANSWER_TOKENS);
        assert_eq!(body["think"], false);
    }

    #[test]
    fn a_looping_answer_is_cut_and_an_error_is_reported() {
        let (address, served) = stub(vec![
            "{\"response\":\"again\\n\",\"done\":false}\n",
            "{\"response\":\"again\\n\",\"done\":false}\n",
            "{\"response\":\"again\\n\",\"done\":false}\n",
        ]);
        let answer = generate_at(&address, "m", "p", |so_far| {
            so_far.matches("again").count() < 2
        })
        .unwrap();
        // Checked after each read from the socket, which may hold several lines.
        assert!(answer.cut);
        assert!(answer.text.starts_with("again\nagain\n"));
        served.join().unwrap();

        let (address, served) = stub(vec!["{\"error\":\"model 'm' not found\"}\n"]);
        let error = generate_at(&address, "m", "p", |_| true).err().unwrap();
        assert!(error.to_string().contains("not found"));
        served.join().unwrap();
    }
}

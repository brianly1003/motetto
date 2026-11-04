<div align="center">

<img src="app/src-tauri/icons/icon.svg" width="96" alt="Motetto logo" />

# Motetto

**Meeting notes that never leave your computer.**

Record a call from Zoom, Teams, Meet or any other app. Get a timestamped transcript that knows
who spoke, and notes where every point links back to the moment it was said. All of it is made on
your own Mac by local models. No account, no upload, no subscription.

![Platform](https://img.shields.io/badge/platform-macOS%2015%2B%20%C2%B7%20Windows%20(preview)-555)
![Status](https://img.shields.io/badge/status-early%20development-orange)
![Local only](https://img.shields.io/badge/cloud-none-2e7d32)
![License](https://img.shields.io/badge/license-MIT%20%C2%B7%20MPL--2.0%20(core)-blue)

[Issues](https://github.com/brianly1003/motetto/issues)

<!-- Demo: add a 20-30 s GIF (or MP4 dragged into GitHub's editor) here, with a link to the full video. -->

</div>

---

<details>
<summary>Contents</summary>

- [What it is](#what-it-is)
- [Why Motetto](#why-motetto)
- [Features](#features)
- [Desktop app](#desktop-app)
- [Status](#status)
- [Quick start (CLI, macOS)](#quick-start-cli-macos)
- [How it works](#how-it-works)
- [Privacy](#privacy)
- [For developers](#for-developers)
- [Contributing](#contributing)
- [License](#license)
- [Credits](#credits)

</details>

## What it is

Motetto records a call from Teams, Meet, Zoom or any other app as two tracks: your microphone
(**You**) and the computer's sound (**Them**). Each track is transcribed with
[whisper.cpp](https://github.com/ggml-org/whisper.cpp), the lines are merged by time, and a
local model in [Ollama](https://ollama.com) writes the notes: action items with owners, a
short overview and one section per topic, each point linked to the moment it was said.

It comes as a desktop app for macOS (tray, floating recording pill, live captions, meeting
library) and as a command-line tool. Windows is in preview.

## Why Motetto

- **Your calls stay yours.** Audio, transcripts and notes are files in a folder on your disk.
  Nothing is sent anywhere, and there is no telemetry.
- **No bill.** Open models on your own hardware; nothing to subscribe to.
- **Who said it.** Two tracks means "You" and "Them" come from the audio itself, not from a
  guess. Optional local speaker models split the other side into Speaker 2, 3…
- **Made for mixed-language calls.** Tested most on Vietnamese and English, including calls
  that switch between them. Notes are written in the call's language (Vietnamese, Chinese and
  Japanese are recognised from the text) unless you pick one.
- **Notes you can check.** Every action item and decision ends with the `[mm:ss]` it came from;
  click it to hear that moment again.
- **Plain files.** Markdown notes and a `[mm:ss] You: …` transcript you can read, grep, version
  or paste anywhere.

## Features

| | |
|---|---|
| **Record any call** | System audio and microphone on separate tracks (ScreenCaptureKit on macOS, WASAPI on Windows). |
| **Import a file** | WAV, MP3, M4A/AAC, MP4 (Zoom recordings), FLAC, Ogg; Opus and WebM through ffmpeg if installed. |
| **Accurate timing** | Silero VAD for true line start times; fixes for lines whisper joins across long silences or starts too early. |
| **Notes with sources** | Action items for everyone on the call, overview, topics; every point ends with the `[mm:ss]` it came from, and points citing a time nobody spoke are dropped. |
| **Your spellings** | Product names and people (`TERMS`) bias the transcript and fix the notes' spelling. |
| **Long calls** | Long transcripts are summarized in parts and merged. About 8 minutes of processing per hour of call on an M1 Max. |
| **Optional extras** | Speaker labels (sherpa-onnx), a local correction pass for misheard words (`FIX_MODEL`). |
| **Desktop app** | See below. |

## Desktop app

| | |
|---|---|
| **Asks, never assumes** | When Zoom, Teams, Meet or another call app takes the microphone, Motetto asks "Record this call?". Nothing is recorded until you say yes, and a recording can always be stopped. |
| **During the call** | A small floating pill with the timer, live captions, and My notes to jot points that go into the notes. |
| **Screen too, if you want** | Choose Audio + screen to keep the video, then rewatch it in the meeting's Video tab. Imported videos get the same tab. |
| **Edit in place** | Fix a transcript line or the notes directly; lock a meeting so nothing changes it by accident. |
| **Read in** | Read a finished meeting's notes in another language without writing them again. |
| **Script prompter** | A teleprompter strip for talks and demos, hidden from screen shares by default. |
| **Library** | Search, playback, Recently deleted (kept 30 days). |
| **First run** | Picks the right model size for your Mac with a 10-second speed test and downloads it; no Homebrew or terminal needed. |

## Status

Early and moving fast. Be specific about what you rely on:

- **Works:** the CLI on macOS 15+ (record, import, transcribe, notes), checked by an end-to-end
  script with synthetic audio on every change, and the desktop app on macOS, used daily by its author.
- **In progress:** signed and notarized installers, Windows desktop app.
- **Not yet verified:** Windows capture on real hardware, and live desktop recording on real
  calls (owner tests are pending).
- **Known issues:** see [GitHub issues](https://github.com/brianly1003/motetto/issues), for
  example a call that opens in one language and switches to another can lose speech ([#1](https://github.com/brianly1003/motetto/issues/1)).

There is no release build yet; build from source. Desktop app on macOS:

```sh
scripts/build-sidecars.sh                       # whisper-server and llama-server, once
cd ui && npm ci && cd ../app/src-tauri && ../../ui/node_modules/.bin/tauri build --bundles app
```

## Quick start (CLI, macOS)

You need macOS 15 or later, [Rust](https://rustup.rs), [Homebrew](https://brew.sh) and about
7 GB of free disk for the models.

```sh
brew install whisper-cpp ollama
ollama serve &                     # or open the Ollama app
ollama pull qwen3:8b

mkdir -p ~/.cache/whisper && cd ~/.cache/whisper
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin
curl -LO https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin
cd -

git clone https://github.com/brianly1003/motetto && cd motetto
cargo install --path crates/motetto-cli --locked
mkdir -p ~/.config/motetto && cp .env.example ~/.config/motetto/.env   # then edit the paths
```

```sh
motetto record                                          # record until Enter; notes land in ./recordings
motetto run --file call.m4a                             # any audio or video file
motetto run -f call.mic.wav -f call.system.wav          # redo a recording with You/Them labels
```

Give the app you run it from (Terminal, iTerm, VS Code…) **Screen & System Audio Recording**
and **Microphone** permission in System Settings › Privacy & Security, then quit and reopen it.
Use headphones so your mic doesn't pick up the other side.

The keys in `~/.config/motetto/.env` (every key is listed in `.env.example`):

```sh
WHISPER_MODEL=$HOME/.cache/whisper/ggml-large-v3-q5_0.bin       # required ($HOME, not ~)
WHISPER_VAD_MODEL=$HOME/.cache/whisper/ggml-silero-v5.1.2.bin   # strongly recommended
OLLAMA_MODEL=qwen3:8b                                           # required, must be installed
# TERMS=Pusher Beams, FCM            # names the models can't know; global .env only
# FIX_MODEL=gemma3:27b               # optional correction pass, slow
```

## How it works

```
 mic ─────► you.wav ──┐                                  ┌──► transcript.txt
                      ├─► 16 kHz ─► whisper.cpp + VAD ──►│      [mm:ss] You: …
 system ──► them.wav ─┘           (one track at a time)  └──► Ollama ──► notes.md
```

1. **Capture** both sides to separate WAV files at the device's own rate.
2. **Decode** each to 16 kHz mono in Rust (no ffmpeg needed for common formats).
3. **Transcribe** each track with whisper.cpp and Silero VAD, then repair timing and language
   slips whisper is known to make.
4. **Merge** the lines by start time into one transcript.
5. **Summarize** with a local Ollama model into Markdown notes, grounded in the transcript's timestamps.

## Privacy

- Audio, transcripts and notes are written only to your disk (`./recordings` for the CLI,
  `~/Documents/Motetto` for the app).
- No cloud speech or language APIs. Ollama is pinned to `127.0.0.1`; cloud models are refused,
  and a model that isn't installed is never downloaded on its own.
- The network is used only to download models you choose.
- No telemetry, analytics or crash reporting.
- Every recording is announced and can be stopped. The desktop app asks before recording a call
  and never records silently.

**Consent:** recording other people may require their permission where you live. Telling them,
and getting it, is your responsibility.

## For developers

```
crates/motetto-core   pipeline stages: capture, decode, transcribe, summarize, session state
crates/motetto-cli    the `motetto` command
app/src-tauri         desktop app (Tauri, Rust)
ui/                   desktop interface (Next.js static export)
```

Before every commit:

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo test -p motetto-app
```

## Contributing

Bug reports with steps to reproduce are the most useful thing right now. Please don't attach
real call recordings or transcripts; a synthetic reproduction (for example with macOS `say`)
is enough and keeps other people's words private. Pull requests: keep changes small, match the
surrounding style, and run the checks above. By contributing you agree that your contribution is
licensed under MPL-2.0 for `crates/motetto-core` and under the MIT License everywhere else.

## License

Motetto is released under the [MIT License](LICENSE), except the core pipeline in
`crates/motetto-core`, which is under the [Mozilla Public License 2.0](crates/motetto-core/LICENSE).
You can use the core in any project, open or closed; if you distribute changed versions of its
files, publish those files' source under MPL-2.0.

## Credits

Motetto stands on [whisper.cpp](https://github.com/ggml-org/whisper.cpp),
[Silero VAD](https://github.com/snakers4/silero-vad), [Ollama](https://ollama.com),
[sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), [Symphonia](https://github.com/pdeljanov/Symphonia),
[Rubato](https://github.com/HEnquist/rubato) and [Tauri](https://tauri.app).

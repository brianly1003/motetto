fn main() {
    stub_missing_programs();
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}

/// `bundle.externalBin` files must exist for the crate to compile. Debug builds (cargo test,
/// clippy) get a text stub for any program `scripts/build-sidecars.sh` has not produced yet, so
/// they need no 90 MB build; release builds fail instead, so a stub never ships by accident.
fn stub_missing_programs() {
    let dir = std::path::Path::new("binaries");
    let triple = std::env::var("TARGET").unwrap();
    let ext = if triple.contains("windows") {
        ".exe"
    } else {
        ""
    };
    for name in [
        "whisper-server",
        "whisper-cli",
        "whisper-vad-speech-segments",
        "llama-server",
    ] {
        let path = dir.join(format!("{name}-{triple}{ext}"));
        if path.exists() {
            continue;
        }
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!(
                "{} is missing; run scripts/build-sidecars.sh",
                path.display()
            );
        }
        println!(
            "cargo:warning={} is a stub; run scripts/build-sidecars.sh before bundling",
            path.display()
        );
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&path, "stub: run scripts/build-sidecars.sh\n").unwrap();
    }
}

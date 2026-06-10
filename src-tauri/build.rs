use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown-target".to_string());
    println!("cargo:rustc-env=ENJA_TARGET_TRIPLE={target}");

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    prepare_apple_speech_helper(&target);
    prepare_screen_context_helper(&target);
    tauri_build::build()
}

fn prepare_apple_speech_helper(target: &str) {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let bin_dir = manifest_dir.join("bin");
    let helper_path = bin_dir.join(format!("enja-speech-helper-{target}"));
    let source_path = manifest_dir.join("speech-helper/main.swift");
    println!("cargo:rerun-if-changed={}", source_path.display());

    if let Err(err) = fs::create_dir_all(&bin_dir) {
        eprintln!("[enja] failed to create helper bin dir: {err}");
        return;
    }

    if !target.contains("apple-darwin") {
        write_unsupported_helper(
            &helper_path,
            "Apple SpeechAnalyzer is only available on macOS.",
        );
        return;
    }

    let Some(sdk_version) = command_stdout("xcrun", &["--show-sdk-version"]) else {
        write_unsupported_helper(&helper_path, "xcrun is unavailable.");
        return;
    };
    let Some(sdk_path) = command_stdout("xcrun", &["--show-sdk-path"]) else {
        write_unsupported_helper(&helper_path, "macOS SDK path is unavailable.");
        return;
    };
    let sdk_major = sdk_version
        .trim()
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    if sdk_major < 26 {
        write_unsupported_helper(
            &helper_path,
            "macOS 26 SDK is required to build SpeechAnalyzer support.",
        );
        return;
    }

    let Some(swiftc) = command_stdout("xcrun", &["--find", "swiftc"]) else {
        write_unsupported_helper(&helper_path, "swiftc is unavailable.");
        return;
    };
    let Some(swift_target) = swift_target(target) else {
        write_unsupported_helper(
            &helper_path,
            "Unsupported Apple target for SpeechAnalyzer helper.",
        );
        return;
    };
    let module_cache = manifest_dir.join("target/swift-module-cache");
    let _ = fs::create_dir_all(&module_cache);

    let output = Command::new(swiftc.trim())
        .args([
            "-target",
            swift_target,
            "-sdk",
            sdk_path.trim(),
            "-O",
            "-parse-as-library",
            "-module-cache-path",
        ])
        .arg(&module_cache)
        .args([
            "-framework",
            "Speech",
            "-framework",
            "AVFoundation",
            "-framework",
            "CoreMedia",
            "-o",
        ])
        .arg(&helper_path)
        .arg(&source_path)
        .output();
    match output {
        Ok(output) if output.status.success() => mark_executable(&helper_path),
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = format!("{} {}", stdout.trim(), stderr.trim())
                .trim()
                .to_string();
            write_unsupported_helper(
                &helper_path,
                &format!(
                    "swiftc failed while building SpeechAnalyzer helper: {}{}",
                    output.status,
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!(": {detail}")
                    }
                ),
            );
        }
        Err(err) => {
            write_unsupported_helper(
                &helper_path,
                &format!("swiftc could not be executed: {err}"),
            );
        }
    }
}

fn prepare_screen_context_helper(target: &str) {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let bin_dir = manifest_dir.join("bin");
    let helper_path = bin_dir.join(format!("enja-screen-context-helper-{target}"));
    let source_path = manifest_dir.join("screen-context-helper/main.swift");
    println!("cargo:rerun-if-changed={}", source_path.display());

    if let Err(err) = fs::create_dir_all(&bin_dir) {
        eprintln!("[enja] failed to create screen context helper bin dir: {err}");
        return;
    }

    if !target.contains("apple-darwin") {
        write_unsupported_helper(
            &helper_path,
            "Screen context OCR is only available on macOS.",
        );
        return;
    }

    let Some(sdk_path) = command_stdout("xcrun", &["--show-sdk-path"]) else {
        write_unsupported_helper(&helper_path, "macOS SDK path is unavailable.");
        return;
    };
    let Some(swiftc) = command_stdout("xcrun", &["--find", "swiftc"]) else {
        write_unsupported_helper(&helper_path, "swiftc is unavailable.");
        return;
    };
    let Some(swift_target) = screen_context_swift_target(target) else {
        write_unsupported_helper(
            &helper_path,
            "Unsupported Apple target for screen context helper.",
        );
        return;
    };
    let module_cache = manifest_dir.join("target/swift-module-cache");
    let _ = fs::create_dir_all(&module_cache);

    let output = Command::new(swiftc.trim())
        .args([
            "-target",
            swift_target,
            "-sdk",
            sdk_path.trim(),
            "-O",
            "-parse-as-library",
            "-module-cache-path",
        ])
        .arg(&module_cache)
        .args([
            "-framework",
            "AppKit",
            "-framework",
            "CoreGraphics",
            "-framework",
            "Foundation",
            "-framework",
            "Vision",
            "-o",
        ])
        .arg(&helper_path)
        .arg(&source_path)
        .output();
    match output {
        Ok(output) if output.status.success() => mark_executable(&helper_path),
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = format!("{} {}", stdout.trim(), stderr.trim())
                .trim()
                .to_string();
            write_unsupported_helper(
                &helper_path,
                &format!(
                    "swiftc failed while building screen context helper: {}{}",
                    output.status,
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!(": {detail}")
                    }
                ),
            );
        }
        Err(err) => {
            write_unsupported_helper(
                &helper_path,
                &format!("swiftc could not be executed: {err}"),
            );
        }
    }
}

fn swift_target(target: &str) -> Option<&'static str> {
    if target.starts_with("aarch64-") {
        Some("arm64-apple-macosx26.0")
    } else if target.starts_with("x86_64-") {
        Some("x86_64-apple-macosx26.0")
    } else {
        None
    }
}

fn screen_context_swift_target(target: &str) -> Option<&'static str> {
    if target.starts_with("aarch64-") {
        Some("arm64-apple-macosx13.0")
    } else if target.starts_with("x86_64-") {
        Some("x86_64-apple-macosx13.0")
    } else {
        None
    }
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn write_unsupported_helper(path: &Path, reason: &str) {
    let escaped_reason = reason
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\'', "'\"'\"'");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' '{{\"ok\":true,\"status\":\"unsupported\",\"supported\":false,\"authorization\":\"unknown\",\"reason\":\"{escaped_reason}\"}}'\n"
    );
    if let Err(err) = fs::write(path, script) {
        eprintln!("[enja] failed to write unsupported SpeechAnalyzer helper: {err}");
        return;
    }
    mark_executable(path);
}

fn mark_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = fs::set_permissions(path, permissions);
        }
    }
}

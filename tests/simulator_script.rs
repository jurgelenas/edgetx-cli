use assert_cmd::Command;
use std::io::Write;
use tempfile::NamedTempFile;

/// End-to-end test: run the CLI binary with a Lua test script.
///
/// Ignored by default because it requires network access (to download the
/// radio catalog and WASM firmware) and takes significant time.
///
/// Run explicitly with: cargo test --test simulator_script -- --ignored
#[test]
#[ignore]
fn test_simulator_lua_script() {
    let screenshot_dir = tempfile::tempdir().expect("create temp dir");
    let script_screenshot = screenshot_dir.path().join("script-output.png");
    let screenshot_path = screenshot_dir.path().join("final-output.png");

    // Write a Lua test script to a tempfile
    let mut script_file = NamedTempFile::new().expect("create temp script file");
    write!(
        script_file,
        "wait(3)\nkey.press(KEY.SYS)\nwait(1)\nscreenshot({:?})\n",
        script_screenshot.to_str().unwrap(),
    )
    .expect("write script");

    let mut cmd = Command::cargo_bin("edgetx-cli").expect("find binary");
    cmd.args([
        "dev",
        "simulator",
        "--radio",
        "Radiomaster TX16S",
        "--headless",
        "--script",
        script_file.path().to_str().unwrap(),
        "--timeout",
        "30s",
        "--screenshot",
        screenshot_path.to_str().unwrap(),
    ]);

    let output = cmd.output().expect("run command");
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify both screenshots were created and are valid PNGs
    for path in [&script_screenshot, &screenshot_path] {
        assert!(path.exists(), "screenshot {} should exist", path.display());
        let data = std::fs::read(path).expect("read screenshot");
        assert!(data.len() > 8, "screenshot should not be empty");
        // PNG magic bytes
        assert_eq!(&data[..4], &[0x89, b'P', b'N', b'G']);
    }
}

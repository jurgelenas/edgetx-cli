use assert_cmd::Command;
use std::path::Path;

/// End-to-end tests for Lua bytecode compilation.
///
/// Ignored by default because they download the EdgeTX Lua compiler WASM module
/// on first run.
///
/// Run explicitly with: cargo test --test luac_compile -- --ignored
const SCRIPT: &str = r#"
local M = {}
function M.run(event)
  local total = 0
  for i = 1, 10 do total = total + i end
  lcd.drawText(1, 1, "total: " .. total, 0)
  return 0
end
return { run = M.run, init = function() end }
"#;

fn cli() -> Command {
    Command::cargo_bin("edgetx-cli").expect("find binary")
}

/// Check the bytecode header the radio validates before loading a script.
///
/// EdgeTX patches `ldump.c` to write `size_t` as an `int`, so every size byte is
/// 4 -- a stock 64-bit `luac` would emit 8 and the radio would reject the file.
fn assert_edgetx_bytecode(data: &[u8]) {
    assert!(data.len() > 18, "bytecode is too short to hold a header");
    assert_eq!(&data[..4], b"\x1bLua", "missing Lua signature");
    assert_eq!(data[4], 0x53, "expected Lua 5.3 bytecode");
    assert_eq!(data[5], 0, "unexpected bytecode format");
    assert_eq!(&data[6..12], b"\x19\x93\r\n\x1a\n", "corrupt header data");
    assert_eq!(
        &data[12..17],
        &[4, 4, 4, 4, 4],
        "sizes must all be 4 bytes: int, size_t-as-int, Instruction, lua_Integer, lua_Number"
    );
}

#[test]
#[ignore]
fn compiles_a_script_to_radio_compatible_bytecode() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let script = dir.path().join("main.lua");
    std::fs::write(&script, SCRIPT).expect("write script");

    cli()
        .args(["dev", "luac", script.to_str().unwrap()])
        .assert()
        .success();

    let luac = dir.path().join("main.luac");
    assert!(luac.exists(), "the compiler wrote no output");
    assert_edgetx_bytecode(&std::fs::read(&luac).expect("read bytecode"));
}

#[test]
#[ignore]
fn honours_the_output_path_and_debug_info() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let script = dir.path().join("main.lua");
    std::fs::write(&script, SCRIPT).expect("write script");

    let stripped = dir.path().join("stripped.luac");
    let with_debug = dir.path().join("debug.luac");

    cli()
        .args([
            "dev",
            "luac",
            script.to_str().unwrap(),
            "-o",
            stripped.to_str().unwrap(),
        ])
        .assert()
        .success();

    cli()
        .args([
            "dev",
            "luac",
            script.to_str().unwrap(),
            "-o",
            with_debug.to_str().unwrap(),
            "--keep-debug",
        ])
        .assert()
        .success();

    let stripped_len = std::fs::metadata(&stripped).unwrap().len();
    let debug_len = std::fs::metadata(&with_debug).unwrap().len();
    assert!(
        stripped_len < debug_len,
        "stripping should shrink the bytecode: {stripped_len} vs {debug_len}"
    );
}

#[test]
#[ignore]
fn reports_syntax_errors_without_writing_output() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let script = dir.path().join("broken.lua");
    std::fs::write(&script, "local x = 1\nif x then\n  y ==== 2\nend\n").expect("write script");

    let output = cli()
        .args(["dev", "luac", script.to_str().unwrap()])
        .output()
        .expect("run command");

    assert!(!output.status.success(), "a broken script must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("syntax error"), "got: {stderr}");
    assert!(
        stderr.contains(":3:"),
        "the error should name the line: {stderr}"
    );
    assert!(!dir.path().join("broken.luac").exists());
}

/// A package with two scripts, which also exercises reusing one compiler across
/// several compiles (the module's output buffer is reused between calls).
fn write_package(dir: &Path) {
    std::fs::create_dir_all(dir.join("SCRIPTS/TOOLS/MyTool")).unwrap();
    std::fs::create_dir_all(dir.join("SCRIPTS/FUNCTIONS")).unwrap();
    std::fs::write(
        dir.join("edgetx.yml"),
        "package:\n  id: github.com/test/mytool\n  description: Build test\n\
         tools:\n  - name: MyTool\n    path: SCRIPTS/TOOLS/MyTool\n\
         functions:\n  - name: Logger\n    path: SCRIPTS/FUNCTIONS/logger.lua\n",
    )
    .unwrap();
    std::fs::write(dir.join("SCRIPTS/TOOLS/MyTool/main.lua"), SCRIPT).unwrap();
    std::fs::write(
        dir.join("SCRIPTS/FUNCTIONS/logger.lua"),
        "return { run = function() return 0 end }",
    )
    .unwrap();
}

#[test]
#[ignore]
fn builds_an_installable_package_with_bytecode() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    let out = dir.path().join("dist");
    let sd = dir.path().join("sdcard");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&sd).unwrap();
    write_package(&src);

    cli()
        .args([
            "dev",
            "build",
            "--src-dir",
            src.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(
        out.join("edgetx.yml").exists(),
        "a package build keeps its manifest"
    );
    for rel in [
        "SCRIPTS/TOOLS/MyTool/main.lua",
        "SCRIPTS/TOOLS/MyTool/main.luac",
        "SCRIPTS/FUNCTIONS/logger.lua",
        "SCRIPTS/FUNCTIONS/logger.luac",
    ] {
        assert!(out.join(rel).exists(), "{rel} missing from the build");
    }
    assert_edgetx_bytecode(&std::fs::read(out.join("SCRIPTS/TOOLS/MyTool/main.luac")).unwrap());

    // The build output installs as a package, bytecode included.
    cli()
        .args([
            "pkg",
            "install",
            "--dir",
            sd.to_str().unwrap(),
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(sd.join("SCRIPTS/TOOLS/MyTool/main.luac").exists());
    assert!(sd.join("SCRIPTS/FUNCTIONS/logger.luac").exists());
}

#[test]
#[ignore]
fn compresses_to_an_sd_card_archive() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    let out = dir.path().join("artifacts");
    std::fs::create_dir_all(&src).unwrap();
    write_package(&src);

    cli()
        .args([
            "dev",
            "build",
            "--src-dir",
            src.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--compress",
            "--name",
            "mytool-v1.2.3",
        ])
        .assert()
        .success();

    let archive = out.join("mytool-v1.2.3.zip");
    assert!(archive.exists(), "no archive was produced");
    assert!(
        !out.join("mytool-v1.2.3").exists(),
        "the staging directory should be gone once zipped"
    );

    let file = std::fs::File::open(&archive).expect("open archive");
    let mut zip = zip::ZipArchive::new(file).expect("read archive");
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    for rel in [
        "SCRIPTS/TOOLS/MyTool/main.lua",
        "SCRIPTS/TOOLS/MyTool/main.luac",
        "SCRIPTS/FUNCTIONS/logger.luac",
    ] {
        assert!(
            names.iter().any(|n| n == rel),
            "{rel} missing from {names:?}"
        );
    }
    assert!(
        !names.iter().any(|n| n.ends_with("edgetx.yml")),
        "an SD card archive carries no manifest: {names:?}"
    );

    // Refuse to clobber an existing archive.
    cli()
        .args([
            "dev",
            "build",
            "--src-dir",
            src.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--compress",
            "--name",
            "mytool-v1.2.3",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore]
fn installs_with_pre_compiled_bytecode() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    let sd = dir.path().join("sdcard");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&sd).unwrap();
    write_package(&src);

    cli()
        .args([
            "pkg",
            "install",
            "--dir",
            sd.to_str().unwrap(),
            "--pre-compile",
            src.to_str().unwrap(),
        ])
        .assert()
        .success();

    let luac = sd.join("SCRIPTS/TOOLS/MyTool/main.luac");
    assert!(luac.exists(), "no bytecode was installed");
    assert_edgetx_bytecode(&std::fs::read(&luac).unwrap());

    // The radio reloads the source whenever it looks newer than the bytecode.
    let lua_time = std::fs::metadata(sd.join("SCRIPTS/TOOLS/MyTool/main.lua"))
        .unwrap()
        .modified()
        .unwrap();
    let luac_time = std::fs::metadata(&luac).unwrap().modified().unwrap();
    assert!(
        luac_time >= lua_time,
        "the bytecode looks stale on the card"
    );

    // Removing the package takes the generated bytecode with it.
    cli()
        .args([
            "pkg",
            "remove",
            "--dir",
            sd.to_str().unwrap(),
            "github.com/test/mytool",
        ])
        .assert()
        .success();

    assert!(!luac.exists(), "the bytecode outlived its package");
    assert!(!sd.join("SCRIPTS/TOOLS/MyTool/main.lua").exists());
}

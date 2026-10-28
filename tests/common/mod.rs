//! Helpers shared by the integration tests

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

pub fn run(cmd: &mut Command) -> Output {
    // Neither the caller's warning levels nor its target directory may leak into the fixtures
    let out = cmd
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{cmd:?} failed:\n{stderr}");
    assert!(!stderr.contains("panicked"), "{cmd:?} panicked:\n{stderr}");
    out
}

pub fn cargo(root: &Path, args: &[&str]) -> Output {
    run(Command::new("cargo").args(args).current_dir(root))
}

pub fn minify(root: &Path, args: &[&str]) -> Output {
    run(Command::new(env!("CARGO_BIN_EXE_cargo-minify"))
        .args([
            "minify",
            "--apply",
            "--allow-dirty",
            "--allow-staged",
            "--allow-no-vcs",
            "--color=never",
        ])
        .args(args)
        .current_dir(root))
}

use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=PANDA_BUILD_VERSION");
    watch_git_metadata();

    let version = env::var("PANDA_BUILD_VERSION")
        .ok()
        .filter(|version| !version.trim().is_empty())
        .or_else(git_describe)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=PANDA_BUILD_VERSION={version}");
}

fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args([
            "describe", "--tags", "--always", "--dirty", "--match", "v[0-9]*",
        ])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|version| !version.is_empty())
}

fn watch_git_metadata() {
    let Some(git_dir) = git_path(["rev-parse", "--git-dir"]) else {
        return;
    };
    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("packed-refs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("refs/tags").display()
    );

    let head = git_dir.join("HEAD");
    let Ok(head_contents) = fs::read_to_string(&head) else {
        return;
    };
    let Some(reference) = head_contents.strip_prefix("ref: ") else {
        return;
    };
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join(reference.trim()).display()
    );
}

fn git_path<const N: usize>(args: [&str; N]) -> Option<PathBuf> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

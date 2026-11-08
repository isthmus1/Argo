//! Scratch directories for tests, under cargo's target directory (`<target>/tmp`, where cargo
//! puts integration tests' `CARGO_TARGET_TMPDIR`) rather than the system temp dir, which is often
//! a small tmpfs; `cargo clean` removes them. Tests don't delete them themselves.

use std::path::PathBuf;

/// A new, empty directory named after `name` and this test process.
pub fn dir(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    // `<target>/debug/deps/<test binary>`
    let target = exe.ancestors().nth(3).expect("inside a target directory");
    let dir = target
        .join("tmp")
        .join(format!("argo-server-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

//! With the `dev` feature Bevy is a shared library. The executables find it and Rust's standard
//! library through their rpath, so `target/debug/void-app` runs without `LD_LIBRARY_PATH`.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_DEV").is_none() {
        return;
    }
    let rustc = std::env::var("RUSTC").expect("cargo sets RUSTC for build scripts");
    let out = Command::new(&rustc)
        .args(["--print", "target-libdir"])
        .output()
        .unwrap_or_else(|e| panic!("running {rustc} --print target-libdir: {e}"));
    assert!(
        out.status.success(),
        "{rustc} --print target-libdir failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let std_dir = String::from_utf8(out.stdout).expect("target-libdir is UTF-8");
    let std_dir = std_dir.trim();
    // Binaries sit in target/<profile>/ (and a copy in deps/), examples in target/<profile>/examples/;
    // libbevy_dylib is in target/<profile>/deps/.
    for arg in ["$ORIGIN/deps", "$ORIGIN", std_dir] {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{arg}");
    }
    for arg in ["$ORIGIN/../deps", std_dir] {
        println!("cargo:rustc-link-arg-examples=-Wl,-rpath,{arg}");
    }
}

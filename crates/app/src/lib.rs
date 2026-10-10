// The `dev` feature links Bevy as a shared library, which a shipped executable must not depend on.
#[cfg(all(feature = "dev", not(debug_assertions)))]
compile_error!(
    "the default `dev` feature links Bevy as a shared library and is for debug builds only. \
     Add --no-default-features to every optimised build of void-app: cargo build/run/test/clippy \
     --release -p void-app --no-default-features, cargo bench -p void-app --no-default-features"
);

// Tracy's C client is linked into whichever crate pulls in tracy-client first: with `dev` that is
// the Bevy shared library, which does not export it, so our own zones fail to link.
#[cfg(all(feature = "dev", feature = "profiling"))]
compile_error!(
    "the `profiling` feature needs Bevy linked statically. Build it with --no-default-features: \
     cargo build -p void-app --no-default-features --features profiling --target-dir target/profiling"
);

pub mod air;
pub mod fleet_game;
pub mod flight;
pub mod map;
pub mod navball;
pub mod overlay;
pub mod scenery;
pub mod tiles;

pub mod solar_mesh;
pub mod world_scenery;

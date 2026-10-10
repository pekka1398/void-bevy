// The `dev` feature links Bevy as a shared library, which a shipped executable must not depend on.
#[cfg(all(feature = "dev", not(debug_assertions)))]
compile_error!("release builds link Bevy statically: cargo build --release -p void-app --no-default-features");

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

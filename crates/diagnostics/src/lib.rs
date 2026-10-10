//! Tracy zones and plots for our own code, beside the ones Bevy's `trace_tracy` gives every system,
//! render stage and GPU pass. With the `tracy` feature (void-app's `profiling`) a `zone!` is a
//! Tracy zone until the end of the enclosing block; without it, `zone!` and `plot!` compile to
//! nothing, so the normal build pays nothing. See guides/profiling.md.
//!
//! A zone needs Tracy's client running, which Bevy's `LogPlugin` starts; a zone hit before that
//! (or in a program without it) panics rather than silently dropping the measurement.

#[cfg(feature = "tracy")]
#[doc(hidden)]
pub use tracy_client;

/// `zone!("name");` times the rest of the enclosing block. Zones that should be siblings go in
/// their own blocks; a later `zone!` in the same block nests inside the earlier one.
#[cfg(feature = "tracy")]
#[macro_export]
macro_rules! zone {
    ($name:literal) => {
        let _zone = $crate::tracy_client::span!($name);
    };
}
/// `zone!("name");` times the rest of the enclosing block. Zones that should be siblings go in
/// their own blocks; a later `zone!` in the same block nests inside the earlier one.
#[cfg(not(feature = "tracy"))]
#[macro_export]
macro_rules! zone {
    ($name:literal) => {};
}

/// `plot!("name", value)` adds a point to a Tracy plot. The value is not evaluated without Tracy.
#[cfg(feature = "tracy")]
#[macro_export]
macro_rules! plot {
    ($name:literal, $value:expr) => {
        $crate::tracy_client::plot!($name, $value)
    };
}
/// `plot!("name", value)` adds a point to a Tracy plot. The value is not evaluated without Tracy.
#[cfg(not(feature = "tracy"))]
#[macro_export]
macro_rules! plot {
    ($name:literal, $value:expr) => {};
}

/// Starts Tracy's client at program start, so zones outside Bevy's app (`--verify`) have one too.
#[cfg(feature = "tracy")]
pub fn start() {
    tracy_client::Client::start();
}
/// Starts Tracy's client at program start, so zones outside Bevy's app (`--verify`) have one too.
#[cfg(not(feature = "tracy"))]
pub fn start() {}

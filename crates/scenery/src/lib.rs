//! The sky's CPU side: the atmosphere and its tables, the cloud field's weather and
//! noise volumes, the star field and the orbit view. The shaders that use them live with the Bevy
//! app.

pub mod atmosphere;
pub mod atmosphere_scene;
pub mod clouds;
pub mod orbit_view;
pub mod stars;
pub mod tables;

pub use atmosphere::{AtmosphereParams, Rgb, earth_like_atmosphere};
pub use orbit_view::{OrbitView, Pose};
pub use stars::{DEFAULT_STARS, STAR_DISTANCE, StarFieldOptions, generate_stars};

pub mod solar;

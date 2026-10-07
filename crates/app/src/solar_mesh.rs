//! Ring geometry is visual only, body fixed in the equatorial plane.
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
pub fn rings(ring: &void_scenery::solar::RingRecipe) -> Mesh {
    ring.validate();
    let mut positions = vec![];
    let mut colors = vec![];
    let mut indices = vec![];
    const SEGMENTS: usize = 256;
    const RADIAL: usize = 96;
    for j in 0..=RADIAL {
        let t = j as f64 / RADIAL as f64;
        let r = ring.inner_radius + (ring.outer_radius - ring.inner_radius) * t;
        let band = 0.65 + 0.35 * (t * 140.0).sin().abs();
        let gap = if (0.59..0.64).contains(&t) { 0.06 } else { 1.0 };
        for i in 0..=SEGMENTS {
            let angle = i as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
            positions.push([(r * angle.cos()) as f32, (r * angle.sin()) as f32, 0.0]);
            colors.push([
                ring.color[0] * band as f32,
                ring.color[1] * band as f32,
                ring.color[2] * band as f32,
                ring.opacity * gap * band as f32,
            ]);
            if j < RADIAL && i < SEGMENTS {
                let a = (j * (SEGMENTS + 1) + i) as u32;
                let b = a + (SEGMENTS + 1) as u32;
                indices.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

//! The acceptance overlays: what the physics is actually touching, drawn on top of what is being
//! rendered. They are the only way to accept contact work by eye — a rocket resting on terrain looks right whether or not the collider under it is the mesh
//! being drawn, and the overlay is what tells them apart.
//!
//! The collision terrain is read back out of Rapier rather than rebuilt here, so a mismatch between
//! the drawn tile and the collided one shows up as two sets of lines instead of being hidden by
//! drawing the same source twice.

use std::collections::{HashMap, HashSet};

use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use glam::DVec3;
use void_landing::{ContactWorld, PlanetFrame};

/// One tile's collision mesh as lines, parented to nothing and positioned relative to the camera.
#[derive(Component)]
pub struct ColliderLine;

/// The live collision-terrain line meshes, one per loaded tile, keyed by the tile's body-fixed
/// origin. Tiles come and go as the craft moves, so this follows them rather than being rebuilt.
#[derive(Resource)]
pub struct ColliderLines {
    lines: HashMap<[u64; 3], Entity>,
    pub material: Handle<StandardMaterial>,
}

impl ColliderLines {
    pub fn new(material: Handle<StandardMaterial>) -> Self {
        Self {
            lines: HashMap::new(),
            material,
        }
    }

    pub fn count(&self) -> usize {
        self.lines.len()
    }

    /// Bring the lines in step with the worlds' loaded collision tiles and move them to `eye`.
    /// `show` false keeps the meshes that exist but hides them, so toggling the overlay back on is
    /// immediate rather than a stall while every tile is rebuilt.
    /// `F` is the caller's own filter on the line entities, because a scene that also moves tiles
    /// or parts has to keep those queries disjoint; it must still select only `ColliderLine`.
    pub fn sync<F: bevy::ecs::query::QueryFilter>(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        transforms: &mut Query<(&mut Transform, &mut Visibility), F>,
        worlds: &[&ContactWorld<PlanetFrame>],
        eye: DVec3,
        show: bool,
    ) {
        let key = |o: DVec3| [o.x.to_bits(), o.y.to_bits(), o.z.to_bits()];
        let mut live: HashMap<[u64; 3], DVec3> = HashMap::new();
        let mut new = Vec::new();
        for world in worlds {
            for tile in world.terrain_colliders() {
                let k = key(tile.origin);
                live.insert(k, tile.origin);
                if show && !self.lines.contains_key(&k) {
                    new.push((k, world.terrain_collider_mesh(tile)));
                }
            }
        }
        self.lines.retain(|k, entity| {
            let keep = live.contains_key(k);
            if !keep {
                commands.entity(*entity).despawn();
            }
            keep
        });
        for (k, (vertices, triangles)) in new {
            let mesh = Mesh::new(PrimitiveTopology::LineList, Default::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vertices)
                .with_inserted_indices(Indices::U32(unique_edges(&triangles)));
            let entity = commands
                .spawn((
                    ColliderLine,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(self.material.clone()),
                    Transform::from_translation((live[&k] - eye).as_vec3()),
                ))
                .id();
            self.lines.insert(k, entity);
        }
        let visibility = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for (k, entity) in &self.lines {
            if let Ok((mut transform, mut v)) = transforms.get_mut(*entity) {
                transform.translation = (live[k] - eye).as_vec3();
                v.set_if_neq(visibility);
            }
        }
    }
}

/// Each triangle edge once, as line-list indices.
pub fn unique_edges(triangles: &[[u32; 3]]) -> Vec<u32> {
    let mut seen = HashSet::new();
    let mut edges = Vec::new();
    for [a, b, c] in triangles {
        for (p, q) in [(*a, *b), (*b, *c), (*c, *a)] {
            let edge = (p.min(q), p.max(q));
            if seen.insert(edge) {
                edges.extend([edge.0, edge.1]);
            }
        }
    }
    edges
}

/// The overlay switches a scene offers, in the order of the function keys that toggle them. Keeping
/// them in one place is what stops the game and the examples drifting into different keys for the same
/// switch, which is how an acceptance pass ends up looking at the wrong thing.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugView {
    /// F2: white triangle edges of the drawn terrain.
    pub wire: bool,
    /// F3: red tile boundaries, which is where the LOD seams are.
    pub bounds: bool,
    /// F4: green colliders — Rapier's terrain triangles and the parts' collider shapes.
    pub colliders: bool,
    /// F5: draw the terrain at all (off for profiling, or to see the colliders on their own: LOD
    /// selection and tile builds keep running either way).
    pub terrain: bool,
}

impl Default for DebugView {
    /// Terrain on, overlays off, which is the scene as it is meant to be looked at.
    fn default() -> Self {
        Self {
            wire: false,
            bounds: false,
            colliders: false,
            terrain: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_triangle_fan_shares_its_edges() {
        // Two triangles over a shared edge: five edges, not six.
        let edges = unique_edges(&[[0, 1, 2], [0, 2, 3]]);
        assert_eq!(edges.len(), 5 * 2);
        let mut pairs: Vec<_> = edges.chunks(2).map(|c| (c[0], c[1])).collect();
        pairs.sort_unstable();
        assert_eq!(pairs, [(0, 1), (0, 2), (0, 3), (1, 2), (2, 3)]);
    }
}

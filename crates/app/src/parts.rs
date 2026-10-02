//! Bevy meshes for the landing crate's collider shapes, so a rocket is drawn as it collides.

use bevy::prelude::*;
use glam::{DQuat, DVec3};
use void_landing::{BodyShape, Piece, SimpleShape};

/// One piece of a part drawn from its collider (for the collider overlay).
#[derive(Component)]
pub struct ColliderShape;

/// Bevy meshes for a compound collider: cylinders, cones and boxes on Y, as Rapier's.
pub fn spawn_shape(
    parent: &mut ChildSpawnerCommands,
    shape: &BodyShape,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
) {
    let pieces: Vec<Piece> = match shape {
        BodyShape::Compound(pieces) => pieces.clone(),
        BodyShape::Simple(s) => vec![Piece {
            shape: *s,
            position: DVec3::ZERO,
            rotation: None,
            mass: None,
        }],
    };
    for piece in pieces {
        let mesh = match piece.shape {
            SimpleShape::Cylinder {
                radius,
                half_height,
            } => meshes.add(Cylinder::new(radius as f32, 2.0 * half_height as f32)),
            SimpleShape::Cone {
                radius,
                half_height,
            } => meshes.add(Cone {
                radius: radius as f32,
                height: 2.0 * half_height as f32,
            }),
            SimpleShape::Box { half_extents: h } => meshes.add(Cuboid::new(
                2.0 * h.x as f32,
                2.0 * h.y as f32,
                2.0 * h.z as f32,
            )),
            SimpleShape::Ball { radius } => meshes.add(Sphere::new(radius as f32)),
        };
        let rotation = piece.rotation.unwrap_or(DQuat::IDENTITY).as_quat();
        parent.spawn((
            ColliderShape,
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(piece.position.as_vec3()).with_rotation(rotation),
        ));
    }
}

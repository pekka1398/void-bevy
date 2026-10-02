//! Bevy meshes from the actual TS PartVisual scene recipes. These are render-only details;
//! the owning assembly runtime still creates its own simplified cylinder/cone colliders.
use bevy::gizmos::config::GizmoConfigGroup;
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SceneLines;
#[derive(Component)]
pub struct PartMesh {
    pub id: String,
    pub local: Transform,
}
#[derive(Component)]
pub struct Flame {
    pub id: String,
    pub local: Transform,
}
pub struct RenderPiece {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub local: Transform,
    pub flame: bool,
}
#[derive(Resource)]
pub struct RenderAssets {
    pub parts: HashMap<String, Vec<RenderPiece>>,
    pub outlines: HashMap<String, Vec<Vec3>>,
    pub green: Handle<StandardMaterial>,
    pub ball: Handle<Mesh>,
    pub center: Handle<StandardMaterial>,
}
#[derive(Deserialize)]
struct VisualDefinition {
    id: String,
    meshes: Vec<MeshSpec>,
    selection: Vec<[f32; 3]>,
}
#[derive(Deserialize)]
struct MeshSpec {
    geometry: GeometrySpec,
    position: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    flame: bool,
    material: MaterialSpec,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum GeometrySpec {
    Cone {
        radius: f32,
        height: f32,
        segments: u32,
    },
    Cylinder {
        top: f32,
        bottom: f32,
        height: f32,
        segments: u32,
    },
    Box {
        size: [f32; 3],
    },
}
#[derive(Deserialize)]
struct MaterialSpec {
    color: [f32; 3],
    metalness: f32,
    roughness: f32,
    unlit: bool,
    opacity: f32,
}
impl GeometrySpec {
    fn mesh(&self) -> Mesh {
        match *self {
            Self::Cone {
                radius,
                height,
                segments,
            } => Cone { radius, height }.mesh().resolution(segments).build(),
            Self::Cylinder {
                top,
                bottom,
                height,
                segments,
            } => ConicalFrustum {
                radius_top: top,
                radius_bottom: bottom,
                height,
            }
            .mesh()
            .resolution(segments)
            .build(),
            Self::Box { size } => Cuboid::new(size[0], size[1], size[2]).into(),
        }
    }
}
impl RenderAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let definitions: Vec<VisualDefinition> =
            serde_json::from_str(include_str!("../data/visuals.json"))
                .expect("invalid TS assembly render data");
        let mut parts = HashMap::new();
        let mut outlines = HashMap::new();
        for d in definitions {
            assert_eq!(d.selection.len() % 2, 0, "outline is a line list");
            let pieces = d
                .meshes
                .into_iter()
                .map(|p| {
                    let m = p.material;
                    RenderPiece {
                        mesh: meshes.add(p.geometry.mesh()),
                        material: materials.add(StandardMaterial {
                            base_color: Color::srgba(m.color[0], m.color[1], m.color[2], m.opacity),
                            metallic: m.metalness,
                            perceptual_roughness: m.roughness,
                            unlit: m.unlit,
                            alpha_mode: if m.opacity < 1.0 {
                                AlphaMode::Blend
                            } else {
                                AlphaMode::Opaque
                            },
                            ..default()
                        }),
                        local: Transform {
                            translation: Vec3::from_array(p.position),
                            rotation: Quat::from_array(p.rotation),
                            scale: Vec3::from_array(p.scale),
                        },
                        flame: p.flame,
                    }
                })
                .collect();
            assert!(
                parts.insert(d.id.clone(), pieces).is_none(),
                "duplicate render definition"
            );
            outlines.insert(
                d.id,
                d.selection.into_iter().map(Vec3::from_array).collect(),
            );
        }
        for d in void_assembly::catalog() {
            assert!(
                parts.contains_key(&d.id),
                "missing authored render model {}",
                d.id
            );
        }
        Self {
            parts,
            outlines,
            green: materials.add(StandardMaterial {
                base_color: Color::srgb_u8(145, 217, 195),
                unlit: true,
                ..default()
            }),
            ball: meshes.add(Sphere::new(0.14)),
            center: materials.add(StandardMaterial {
                base_color: Color::srgb_u8(231, 188, 103),
                unlit: true,
                ..default()
            }),
        }
    }
}

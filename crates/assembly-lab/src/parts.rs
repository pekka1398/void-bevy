//! Bevy meshes from the parts' visual recipes (`data/visuals.json`). These are render-only
//! details; the flight runtime creates its own simplified colliders.
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
                .expect("invalid assembly render data");
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
            if d.box_size_meters.is_some() {
                // Explicit cuboid dimensions are the authored geometry for both drawing and contact.
                let size = void_assembly::part_box_size(d).as_vec3();
                let color =
                    Color::Srgba(Srgba::hex(&d.color).expect("invalid authored part color"));
                parts.insert(
                    d.id.clone(),
                    vec![RenderPiece {
                        mesh: meshes.add(Cuboid::new(size.x, size.y, size.z)),
                        material: materials.add(StandardMaterial {
                            base_color: color,
                            ..default()
                        }),
                        local: Transform::IDENTITY,
                        flame: false,
                    }],
                );
                let h = size / 2.0;
                let mut lines = Vec::new();
                for axis in 0..3 {
                    let b = (axis + 1) % 3;
                    let c = (axis + 2) % 3;
                    for sb in [-1.0, 1.0] {
                        for sc in [-1.0, 1.0] {
                            let mut point = h;
                            point[b] *= sb;
                            point[c] *= sc;
                            point[axis] = -h[axis];
                            lines.push(point);
                            point[axis] = h[axis];
                            lines.push(point);
                        }
                    }
                }
                outlines.insert(d.id.clone(), lines);
            }
            if d.modules
                .iter()
                .any(|m| matches!(m, void_assembly::Module::Crew { .. }))
            {
                // The physical suit uses one box; visible body pieces stay within that envelope.
                let white = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.87, 0.88, 0.84),
                    perceptual_roughness: 0.8,
                    ..default()
                });
                let dark = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.08, 0.10, 0.12),
                    ..default()
                });
                let visor = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.29, 0.19, 0.04),
                    metallic: 0.8,
                    perceptual_roughness: 0.25,
                    ..default()
                });
                let mut pieces = Vec::new();
                let mut box_piece =
                    |size: Vec3, position: Vec3, material: Handle<StandardMaterial>| {
                        pieces.push(RenderPiece {
                            mesh: meshes.add(Cuboid::new(size.x, size.y, size.z)),
                            material,
                            local: Transform::from_translation(position),
                            flame: false,
                        });
                    };
                box_piece(
                    Vec3::new(0.29, 0.65, 0.27),
                    Vec3::new(0.0, 0.12, 0.0),
                    white.clone(),
                );
                box_piece(
                    Vec3::new(0.26, 0.37, 0.27),
                    Vec3::new(0.0, 0.63, 0.0),
                    white.clone(),
                );
                box_piece(
                    Vec3::new(0.22, 0.19, 0.02),
                    Vec3::new(0.0, 0.66, 0.145),
                    visor,
                );
                box_piece(
                    Vec3::new(0.25, 0.45, 0.04),
                    Vec3::new(0.0, 0.13, -0.15),
                    dark.clone(),
                );
                for side in [-1.0, 1.0] {
                    box_piece(
                        Vec3::new(0.075, 0.62, 0.19),
                        Vec3::new(side * 0.18, 0.09, 0.0),
                        white.clone(),
                    );
                    box_piece(
                        Vec3::new(0.11, 0.55, 0.20),
                        Vec3::new(side * 0.09, -0.50, 0.0),
                        white.clone(),
                    );
                    box_piece(
                        Vec3::new(0.13, 0.12, 0.29),
                        Vec3::new(side * 0.09, -0.82, 0.015),
                        dark.clone(),
                    );
                }
                parts.insert(d.id.clone(), pieces);
            }
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

//! Drawing `void_view`'s map: bodies' orbits and trajectories as gizmo lines at the map weight's
//! opacity, and the labels (bodies, the vessel, apsides) as clickable UI markers. A label
//! overlapping a higher-priority one keeps only its dot.
//!
//! Positions come relative to the camera in the ecliptic; `render` turns such a vector into the
//! caller's render axes.

use bevy::prelude::*;
use glam::DVec3;
use void_orbit::CelestialBody;
use void_view::{LabelKind, MapLabel};

pub const PATH_COLOR: &str = "#4fc8ff";
pub const VESSEL_COLOR: &str = "#7dffb0";
fn body_label_font_size(body: &CelestialBody) -> f32 {
    if body.parent_index.is_none() {
        18.0
    } else if matches!(
        body.id.rsplit('/').next().unwrap(),
        "cinder" | "vesper" | "aurelia" | "ares" | "velvet" | "halo" | "azure" | "abyss"
    ) {
        18.0
    } else {
        11.0
    }
}

/// A CSS hex colour; invalid input is logged and shown in diagnostic magenta.
pub fn color(hex: &str) -> Color {
    match Srgba::hex(hex.trim_start_matches('#')) {
        Ok(color) => Color::from(color),
        Err(error) => {
            error!("invalid CSS hex colour {hex:?}: {error}; showing magenta");
            Color::srgb(1.0, 0.0, 1.0)
        }
    }
}

/// A map label: what it names, its slot (0 or 1 for the apsides) and its text child.
#[derive(Component)]
pub struct MapMarker {
    pub kind: LabelKind,
    pub slot: usize,
    pub text: Entity,
    pub font_size: f32,
}

/// One marker per body, one for the vessel and two for apsides, hidden until placed.
pub fn spawn_map_labels(commands: &mut Commands, bodies: &[CelestialBody]) {
    let mut kinds: Vec<(LabelKind, usize, String, Color, f32)> = bodies
        .iter()
        .map(|b| {
            let kind = if b.parent_index.is_none() && b.index == 0 {
                LabelKind::Star
            } else {
                LabelKind::Body(b.index)
            };
            (
                kind,
                0,
                b.name.clone(),
                color(&b.color),
                body_label_font_size(b),
            )
        })
        .collect();
    kinds.push((
        LabelKind::Vessel,
        0,
        "Vessel".into(),
        color(VESSEL_COLOR),
        12.0,
    ));
    for slot in 0..2 {
        kinds.push((
            LabelKind::Apsis,
            slot,
            String::new(),
            color(PATH_COLOR),
            12.0,
        ));
    }
    for (kind, slot, name, dot, font_size) in kinds {
        let text = commands
            .spawn((
                Text::new(name),
                TextFont {
                    font_size: FontSize::Px(font_size),
                    ..default()
                },
                TextShadow {
                    offset: Vec2::ONE,
                    color: Color::BLACK.with_alpha(0.8),
                },
            ))
            .id();
        let marker = commands
            .spawn((
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .with_children(|m| {
                m.spawn((
                    Node {
                        width: px(6),
                        height: px(6),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(dot),
                ));
            })
            .id();
        commands.entity(marker).add_child(text);
        commands.entity(marker).insert(MapMarker {
            kind,
            slot,
            text,
            font_size,
        });
    }
}

/// Place `labels` (highest priority first, from `void_view::map_labels`) on screen.
#[allow(clippy::type_complexity)]
pub fn place_map_labels(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    markers: &mut Query<(&MapMarker, &mut Node, &mut Visibility, &ComputedNode)>,
    texts: &mut Query<(&mut Text, &mut Visibility), Without<MapMarker>>,
    labels: &[MapLabel],
    map_weight: f64,
    render: &dyn Fn(DVec3) -> Vec3,
) {
    let mut shown: Vec<(Vec2, Vec2)> = Vec::new();
    let mut placed: Vec<(LabelKind, usize, Vec2, bool, String)> = Vec::new();
    let mut apsis_slot = 0;
    let size = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
    for label in labels {
        let slot = if label.kind == LabelKind::Apsis {
            apsis_slot += 1;
            apsis_slot - 1
        } else {
            0
        };
        let world = render(label.relative);
        // In front of the camera, and not far off screen.
        let ahead = camera_transform.forward().dot(world) > 0.0;
        let Ok(at) = camera.world_to_viewport(camera_transform, world) else {
            continue;
        };
        let on_screen = at.x > -0.1 * size.x
            && at.x < 1.1 * size.x
            && at.y > -0.1 * size.y
            && at.y < 1.1 * size.y;
        if map_weight <= 0.0 || !ahead || !on_screen {
            continue;
        }
        let extent = markers
            .iter()
            .find(|(m, ..)| m.kind == label.kind && m.slot == slot)
            .map_or(Vec2::new(60.0, 14.0), |(marker, .., computed)| {
                let measured = computed.unrounded_size() * computed.inverse_scale_factor();
                // Before the first UI layout, keep the expected line height for placement.
                Vec2::new(measured.x, measured.y.max(marker.font_size * 1.2))
            });
        let crowded = shown.iter().any(|(p, w)| {
            (p.y - at.y).abs() < (w.y + extent.y) * 0.5
                && if at.x >= p.x {
                    at.x - p.x < w.x
                } else {
                    p.x - at.x < extent.x
                }
        });
        if !crowded {
            shown.push((at, extent));
        }
        placed.push((label.kind, slot, at, crowded, label.text.clone()));
    }
    for (marker, mut node, mut visibility, computed) in markers.iter_mut() {
        let found = placed
            .iter()
            .find(|(kind, slot, ..)| *kind == marker.kind && *slot == marker.slot);
        let Some((_, _, at, crowded, text)) = found else {
            *visibility = Visibility::Hidden;
            continue;
        };
        // A suppressed label must not leave an invisible button above the visible star.
        *visibility = if *crowded {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        // Rounded layout bounds depend on the fractional position of both edges.
        // Feeding them back into centering can alternate the anchor by a pixel.
        let inverse_scale = computed.inverse_scale_factor();
        let height = (computed.unrounded_size().y * inverse_scale).max(marker.font_size * 1.2);
        let anchor = Vec2::new(at.x - 3.0, at.y - height * 0.5);
        let anchor = (anchor / inverse_scale).round() * inverse_scale;
        node.left = px(anchor.x);
        node.top = px(anchor.y);
        if let Ok((mut t, mut v)) = texts.get_mut(marker.text) {
            if t.0 != *text {
                t.0.clone_from(text);
            }
            *v = if *crowded {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}

/// Labels take clicks once the map is half in. Returns whether the pointer is over a label (so a
/// press there never starts a camera drag) and the label clicked this frame, if any.
pub fn label_click(
    markers: &Query<(&Interaction, &MapMarker)>,
    buttons: &ButtonInput<MouseButton>,
    map_weight: f64,
) -> (bool, Option<LabelKind>) {
    let mut over = false;
    let mut clicked = None;
    for (interaction, marker) in markers {
        if *interaction == Interaction::None || map_weight <= 0.5 {
            continue;
        }
        over = true;
        if *interaction == Interaction::Pressed && buttons.just_pressed(MouseButton::Left) {
            clicked = Some(marker.kind);
        }
    }
    (over, clicked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_anchor_stays_fixed_when_rounded_layout_bounds_change() {
        use bevy::camera::{CameraProjection, ComputedCameraValues, RenderTargetInfo};
        let mut app = App::new();
        let camera = Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1280, 720),
                    scale_factor: 1.0,
                }),
                clip_from_view: PerspectiveProjection {
                    aspect_ratio: 1280.0 / 720.0,
                    ..default()
                }
                .get_clip_from_view(),
                ..default()
            },
            ..default()
        };
        let label = MapLabel {
            kind: LabelKind::Star,
            text: "Sol".into(),
            relative: DVec3::new(0.0, 0.0, -100.0),
            color: "#ffd27a".into(),
            priority: 1.0,
        };
        app.add_systems(
            Update,
            move |mut markers: Query<(&MapMarker, &mut Node, &mut Visibility, &ComputedNode)>,
                  mut texts: Query<(&mut Text, &mut Visibility), Without<MapMarker>>| {
                place_map_labels(
                    &camera,
                    &GlobalTransform::IDENTITY,
                    &mut markers,
                    &mut texts,
                    std::slice::from_ref(&label),
                    1.0,
                    &|p| p.as_vec3(),
                );
            },
        );
        let text = app
            .world_mut()
            .spawn((Text::new("Sol"), Visibility::Inherited))
            .id();
        let marker = app
            .world_mut()
            .spawn((
                MapMarker {
                    kind: LabelKind::Star,
                    slot: 0,
                    text,
                    font_size: 22.0,
                },
                Node::default(),
                Visibility::Inherited,
                ComputedNode::default(),
            ))
            .id();
        for scale in [1.0_f32, 1.25, 2.0] {
            let mut previous = None;
            for rounded_height in [(26.4 * scale).floor(), (26.4 * scale).ceil()].repeat(3) {
                *app.world_mut().get_mut::<ComputedNode>(marker).unwrap() = ComputedNode {
                    size: Vec2::new(100.0 * scale, rounded_height),
                    unrounded_size: Vec2::new(100.0, 26.4) * scale,
                    inverse_scale_factor: scale.recip(),
                    ..default()
                };
                app.update();
                let node = app.world().get::<Node>(marker).unwrap();
                let position = (node.left, node.top);
                if let Some(previous) = previous {
                    assert_eq!(position, previous);
                }
                previous = Some(position);
            }
        }
    }
}

//! Drawing `void_view`'s map: bodies' orbits and trajectories as gizmo lines at the map weight's
//! opacity, and the labels (bodies, the vessel, apsides) as clickable UI markers. A label
//! overlapping a higher-priority one keeps only its dot, as lab/view's.
//!
//! Positions come relative to the camera in the ecliptic; `render` turns such a vector into the
//! caller's render axes.

use bevy::prelude::*;
use glam::DVec3;
use void_orbit::CelestialBody;
use void_view::{LabelKind, MapFrame, MapLabel, MapOrbits, MapPath, frame_to_ecliptic};

pub const PATH_COLOR: &str = "#4fc8ff";
pub const PLAN_COLOR: &str = "#ffca66";
pub const VESSEL_COLOR: &str = "#7dffb0";
fn body_label_font_size(body: &CelestialBody) -> f32 {
    if body.parent_index.is_none() {
        22.0
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

/// Bodies' orbits and the given paths, at opacity `alpha`.
pub fn draw_map_lines(
    gizmos: &mut Gizmos,
    bodies: &[CelestialBody],
    orbits: &MapOrbits,
    paths: &[(&MapPath, Color)],
    frame: &MapFrame,
    alpha: f32,
    render: &dyn Fn(DVec3) -> Vec3,
) {
    if alpha <= 0.0 {
        return;
    }
    for body in bodies {
        let Some(placement) = orbits.placement(bodies, body.index, frame) else {
            continue;
        };
        let shape = &orbits.shapes[body.index];
        let at = |p: DVec3| {
            let e = match &placement.axes {
                Some(axes) => frame_to_ecliptic(axes, p),
                None => p,
            };
            render(placement.anchor + e)
        };
        let points = shape.points.iter().map(|&p| at(p));
        let c = color(&body.color).with_alpha(alpha);
        if shape.closed {
            gizmos.linestrip(points.chain(std::iter::once(at(shape.points[0]))), c);
        } else {
            gizmos.linestrip(points, c);
        }
    }
    for (path, c) in paths {
        if path.visible {
            gizmos.linestrip(path.points.iter().map(|&p| render(p)), c.with_alpha(alpha));
        }
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
                let measured = computed.size() * computed.inverse_scale_factor();
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
        node.left = px(at.x - 3.0);
        node.top = px(at.y
            - (computed.size().y * computed.inverse_scale_factor()).max(marker.font_size * 1.2)
                * 0.5);
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

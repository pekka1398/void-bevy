//! `void_navball`'s ball as a Bevy UI image: the painter draws into the image every frame, and its
//! heading and pitch labels are text nodes over it (white over a black shadow, as the lab's).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::Val2;
use void_navball::{NavballInput, NavballPainter, NavballReadout};

/// Labels per ball: twelve headings and four pitches at most.
const LABELS: usize = 16;

/// A ball: its painter, its image and its label pool.
#[derive(Component)]
pub struct Navball {
    pub painter: NavballPainter,
    image: Handle<Image>,
    labels: Vec<Entity>,
}

/// Marks a navball label.
#[derive(Component)]
pub struct NavballLabel;

/// Spawn a ball of `diameter` logical pixels at the window's `pixel_ratio`; the returned entity
/// is the image node, for the caller to place in its layout.
pub fn spawn_navball(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    diameter: f64,
    pixel_ratio: f64,
) -> Entity {
    let painter = NavballPainter::new(diameter, pixel_ratio);
    let image = images.add(Image::new_fill(
        Extent3d {
            width: painter.size as u32,
            height: painter.size as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let node = commands
        .spawn((
            ImageNode::new(image.clone()),
            Node {
                width: px(diameter as f32),
                height: px(diameter as f32),
                ..default()
            },
        ))
        .id();
    let labels = (0..LABELS)
        .map(|_| {
            commands
                .spawn((
                    NavballLabel,
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    }
                    .with_font_weight(FontWeight::SEMIBOLD),
                    TextShadow {
                        offset: Vec2::ONE,
                        color: Color::srgba(0.0, 0.0, 0.0, 170.0 / 255.0),
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    UiTransform::from_translation(Val2::percent(-50, -50)),
                    Visibility::Hidden,
                    ChildOf(node),
                ))
                .id()
        })
        .collect();
    commands.entity(node).insert(Navball {
        painter,
        image,
        labels,
    });
    node
}

/// Draw the ball for `input` and place its labels.
#[allow(clippy::type_complexity)]
pub fn draw_navball(
    ball: &mut Navball,
    input: &NavballInput,
    images: &mut Assets<Image>,
    labels: &mut Query<(&mut Text, &mut Node, &mut TextColor, &mut Visibility), With<NavballLabel>>,
) -> NavballReadout {
    let reading = ball.painter.draw(input);
    if let Some(mut image) = images.get_mut(&ball.image) {
        image.data = Some(ball.painter.rgba.clone());
    }
    for (k, &entity) in ball.labels.iter().enumerate() {
        let Ok((mut text, mut node, mut color, mut visibility)) = labels.get_mut(entity) else {
            continue;
        };
        match ball.painter.labels.get(k) {
            Some(label) => {
                text.0.clone_from(&label.text);
                node.left = px(label.x as f32);
                node.top = px(label.y as f32);
                color.0 = Color::srgba(1.0, 1.0, 1.0, label.alpha as f32);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    reading
}

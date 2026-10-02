//! Line plots in screen pixels, as the lab's `Plots.ts`: each axis has its unit, bounds always
//! include zero, and nothing smooths away extremes. Lines are 2D gizmos on the overlay camera;
//! numbers and labels come from a pool of UI text nodes.

use bevy::ecs::query::QueryFilter;
use bevy::prelude::*;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct PlotLines;

/// A text label of the pool.
#[derive(Component)]
pub struct PlotLabel;

pub struct Series {
    pub label: &'static str,
    pub color: Color,
    pub points: Vec<(f64, f64)>,
    /// Plotted against the right axis.
    pub right: bool,
}

pub struct Plot {
    pub title: String,
    pub series: Vec<Series>,
    pub x_label: &'static str,
    pub left_label: &'static str,
    pub right_label: &'static str,
}

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Text to place this frame: content, top-left-origin pixel position of its baseline-ish middle,
/// colour, size and alignment.
pub struct Label {
    pub text: String,
    pub at: Vec2,
    pub color: Color,
    pub size: f32,
    pub align: Align,
}

const GRID: Color = Color::srgb(
    0x25 as f32 / 255.0,
    0x34 as f32 / 255.0,
    0x3e as f32 / 255.0,
);
const TICK: Color = Color::srgb(
    0x93 as f32 / 255.0,
    0xa6 as f32 / 255.0,
    0xb4 as f32 / 255.0,
);
const AXIS: Color = Color::srgb(
    0xac as f32 / 255.0,
    0xbf as f32 / 255.0,
    0xcb as f32 / 255.0,
);
const TITLE: Color = Color::srgb(
    0xd6 as f32 / 255.0,
    0xe2 as f32 / 255.0,
    0xea as f32 / 255.0,
);

/// The lab's plot padding: room for the left numbers, the right ones if any, the legend and the
/// x numbers.
pub fn padding(right_axis: bool) -> (f32, f32, f32, f32) {
    (54.0, if right_axis { 56.0 } else { 18.0 }, 30.0, 32.0)
}

fn number(n: f64) -> String {
    if n.abs() >= 1000.0 {
        format!("{n:.0}")
    } else if n.abs() >= 10.0 {
        format!("{n:.1}")
    } else {
        format!("{n:.2}")
    }
}

fn bounds<'a>(points: impl Iterator<Item = &'a (f64, f64)>) -> (f64, f64) {
    let (mut lo, mut hi) = (0.0_f64, 0.0_f64);
    for &(_, y) in points {
        lo = lo.min(y);
        hi = hi.max(y);
    }
    if hi - lo < 1e-6 {
        hi = lo + 1.0;
    }
    let margin = (hi - lo) * 0.08;
    (lo - margin, hi + margin)
}

/// Draw `plot` in `rect` (top-left origin, logical pixels, as `Rect { min, max }`) of a
/// `window` sized window; push its text to `labels`.
pub fn draw(
    plot: &Plot,
    rect: Rect,
    window: Vec2,
    gizmos: &mut Gizmos<PlotLines>,
    labels: &mut Vec<Label>,
) {
    let screen = |p: Vec2| Vec2::new(p.x - window.x / 2.0, window.y / 2.0 - p.y);
    let text = |labels: &mut Vec<Label>, text: String, at: Vec2, color: Color, align: Align| {
        labels.push(Label {
            text,
            at,
            color,
            size: 10.0,
            align,
        });
    };
    labels.push(Label {
        text: plot.title.clone(),
        at: rect.min + Vec2::new(0.0, -10.0),
        color: TITLE,
        size: 12.0,
        align: Align::Left,
    });
    let has_right = !plot.right_label.is_empty();
    let (pl, pr, pt, pb) = padding(has_right);
    let (width, height) = (rect.width(), rect.height());
    let (w, h) = (width - pl - pr, height - pt - pb);
    let all = || plot.series.iter().flat_map(|s| s.points.iter());
    if all().next().is_none() {
        return;
    }
    let mut x_min = all().fold(f64::INFINITY, |m, p| m.min(p.0));
    let mut x_max = all().fold(f64::NEG_INFINITY, |m, p| m.max(p.0));
    if x_max == x_min {
        x_max = x_min + 1.0;
    }
    if !x_min.is_finite() {
        x_min = 0.0;
    }
    let lb = bounds(
        plot.series
            .iter()
            .filter(|s| !s.right)
            .flat_map(|s| s.points.iter()),
    );
    let rb = bounds(
        plot.series
            .iter()
            .filter(|s| s.right)
            .flat_map(|s| s.points.iter()),
    );
    let origin = rect.min;
    let x_pixel = |x: f64| origin.x + pl + ((x - x_min) / (x_max - x_min)) as f32 * w;
    let y_pixel =
        |y: f64, b: (f64, f64)| origin.y + pt + h * (1.0 - ((y - b.0) / (b.1 - b.0)) as f32);
    for i in 0..=4 {
        let y = origin.y + pt + i as f32 * h / 4.0;
        gizmos.line_2d(
            screen(Vec2::new(origin.x + pl, y)),
            screen(Vec2::new(origin.x + pl + w, y)),
            GRID,
        );
        let f = i as f64 / 4.0;
        text(
            labels,
            number(lb.1 - (lb.1 - lb.0) * f),
            Vec2::new(origin.x + pl - 7.0, y),
            TICK,
            Align::Right,
        );
        if has_right {
            text(
                labels,
                number(rb.1 - (rb.1 - rb.0) * f),
                Vec2::new(origin.x + pl + w + 7.0, y),
                TICK,
                Align::Left,
            );
        }
        let x = x_min + (x_max - x_min) * f;
        text(
            labels,
            number(x),
            Vec2::new(x_pixel(x), origin.y + height - 15.0),
            TICK,
            Align::Center,
        );
    }
    text(
        labels,
        plot.left_label.into(),
        origin + Vec2::new(8.0, 14.0),
        AXIS,
        Align::Left,
    );
    if has_right {
        text(
            labels,
            plot.right_label.into(),
            origin + Vec2::new(width - 8.0, 14.0),
            AXIS,
            Align::Right,
        );
    }
    text(
        labels,
        plot.x_label.into(),
        origin + Vec2::new(width - pr, height - 2.0),
        AXIS,
        Align::Right,
    );
    let mut legend_x = origin.x + pl;
    for s in &plot.series {
        let b = if s.right { rb } else { lb };
        gizmos.linestrip_2d(
            s.points
                .iter()
                .map(|&(x, y)| screen(Vec2::new(x_pixel(x), y_pixel(y, b)))),
            s.color,
        );
        text(
            labels,
            s.label.into(),
            Vec2::new(legend_x, origin.y + 14.0),
            s.color,
            Align::Left,
        );
        legend_x += s.label.len() as f32 * 6.0 + 18.0;
    }
}

/// Lay `labels` out on the pool's text nodes and hide the rest. The default font is monospaced,
/// so a label's width is its length times 0.6 of its size.
pub fn place_labels<F: QueryFilter>(
    labels: &[Label],
    pool: &mut Query<
        (
            &mut Text,
            &mut TextFont,
            &mut TextColor,
            &mut Node,
            &mut Visibility,
        ),
        F,
    >,
) {
    let mut nodes = pool.iter_mut();
    for label in labels {
        let Some((mut text, mut font, mut color, mut node, mut visibility)) = nodes.next() else {
            break;
        };
        let width = label.text.chars().count() as f32 * label.size * 0.6;
        let left = match label.align {
            Align::Left => label.at.x,
            Align::Center => label.at.x - width / 2.0,
            Align::Right => label.at.x - width,
        };
        if text.0 != label.text {
            text.0.clone_from(&label.text);
        }
        font.font_size = FontSize::Px(label.size);
        color.0 = label.color;
        node.left = px(left);
        node.top = px(label.at.y - label.size * 0.65);
        *visibility = Visibility::Inherited;
    }
    for (.., mut visibility) in nodes {
        *visibility = Visibility::Hidden;
    }
}

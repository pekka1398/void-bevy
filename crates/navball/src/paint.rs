//! The navball painted into a pixel buffer: sky and ground per pixel, then the grid, markers,
//! reticle and rim stroked with antialiased coverage and blended source-over. Lengths are CSS
//! pixels times `pixel_ratio`.

use glam::DVec3;

use crate::{
    MARKER_MIN_SPEED, NavballInput, NavballReadout, dot, heading_pitch, horizon_direction, length,
    navball_basis, scale, to_ball,
};

const SKY: [f64; 3] = [58.0, 128.0, 214.0];
const GROUND: [f64; 3] = [150.0, 96.0, 48.0];
const PROGRADE: [f64; 4] = [242.0, 226.0, 74.0, 1.0];
const RETICLE: [f64; 4] = [255.0, 161.0, 26.0, 1.0];
const SHADOW: [f64; 4] = [0.0, 0.0, 0.0, 170.0 / 255.0];
const RIM: [f64; 4] = [26.0, 32.0, 48.0, 1.0];

/// A heading or pitch label: centre in CSS pixels from the ball's top-left corner, and its
/// opacity. Meant to be drawn in 600 10 px monospace, white over a black shadow 1 px down-right.
#[derive(Clone, Debug, PartialEq)]
pub struct NavballLabel {
    pub x: f64,
    pub y: f64,
    pub text: String,
    pub alpha: f64,
}

enum Shape {
    Segment(DVec3, DVec3),
    Circle(DVec3, f64),
}

pub struct NavballPainter {
    /// Square side in device pixels.
    pub size: usize,
    pub pixel_ratio: f64,
    /// Straight (not premultiplied) RGBA, row 0 at the top.
    pub rgba: Vec<u8>,
    pub labels: Vec<NavballLabel>,
    /// Working colour, linear 0–255 with alpha 0–1.
    color: Vec<[f64; 4]>,
    coverage: Vec<f32>,
}

impl NavballPainter {
    pub fn new(diameter_css_pixels: f64, pixel_ratio: f64) -> Self {
        assert!(
            diameter_css_pixels > 0.0 && pixel_ratio > 0.0,
            "navball: bad size {diameter_css_pixels} at ratio {pixel_ratio}"
        );
        let size = (diameter_css_pixels * pixel_ratio).round() as usize;
        Self {
            size,
            pixel_ratio,
            rgba: vec![0; size * size * 4],
            labels: Vec::new(),
            color: vec![[0.0; 4]; size * size],
            coverage: vec![0.0; size * size],
        }
    }

    /// The ball's diameter in CSS pixels.
    pub fn diameter(&self) -> f64 {
        self.size as f64 / self.pixel_ratio
    }

    pub fn draw(&mut self, input: &NavballInput) -> NavballReadout {
        let basis = navball_basis(input);
        self.fill(&basis);
        self.labels.clear();
        let radius = self.size as f64 / 2.0 / self.pixel_ratio - 1.0;
        self.grid(&basis, radius);
        let speed = length(input.velocity);
        if speed >= MARKER_MIN_SPEED {
            let direction = scale(input.velocity, 1.0 / speed);
            self.marker(to_ball(&basis, direction), radius, false);
            self.marker(to_ball(&basis, scale(direction, -1.0)), radius, true);
        }
        self.reticle(radius);
        self.stroke(&[Shape::Circle(DVec3::ZERO, radius)], 2.0, RIM);
        for (out, c) in self.rgba.as_chunks_mut::<4>().0.iter_mut().zip(&self.color) {
            let byte = |v: f64| v.round_ties_even().clamp(0.0, 255.0) as u8;
            *out = [byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3] * 255.0)];
        }
        let (heading, pitch) = heading_pitch(&basis, basis.nose);
        NavballReadout {
            heading,
            pitch,
            speed,
        }
    }

    /// Sky above the horizon, ground below, darkened toward the rim.
    fn fill(&mut self, basis: &crate::NavballBasis) {
        let size = self.size;
        let half = size as f64 / 2.0;
        let r = half - self.pixel_ratio;
        // The local vertical in screen axes: a pixel's height above the horizon is its ball point
        // along it.
        let (ux, uy, uz) = (
            dot(basis.up, basis.right),
            dot(basis.up, basis.top),
            dot(basis.up, basis.nose),
        );
        for py in 0..size {
            let sy = (half - py as f64 - 0.5) / r;
            for px in 0..size {
                let sx = (px as f64 + 0.5 - half) / r;
                let rr = sx * sx + sy * sy;
                let i = py * size + px;
                let edge = ((1.0 - rr.sqrt()) * r + 0.5).clamp(0.0, 1.0);
                if edge <= 0.0 {
                    self.color[i] = [0.0; 4];
                    continue;
                }
                let sz = (1.0 - rr).max(0.0).sqrt();
                let height = sx * ux + sy * uy + sz * uz;
                // Half a pixel of blend across the horizon; the horizon line covers it.
                let t = (height * r + 0.5).clamp(0.0, 1.0);
                let shade = 0.5 + 0.5 * sz;
                // The canvas stores bytes: the fill is rounded before anything is drawn over it.
                let channel = |k: usize| {
                    ((GROUND[k] + (SKY[k] - GROUND[k]) * t) * shade)
                        .round_ties_even()
                        .clamp(0.0, 255.0)
                };
                self.color[i] = [
                    channel(0),
                    channel(1),
                    channel(2),
                    (255.0 * edge).round_ties_even().clamp(0.0, 255.0) / 255.0,
                ];
            }
        }
    }

    fn grid(&mut self, basis: &crate::NavballBasis, radius: f64) {
        let polyline = |points: &[DVec3]| -> Vec<Shape> {
            points
                .windows(2)
                .filter(|w| w[0].z >= 0.0 && w[1].z >= 0.0)
                .map(|w| {
                    Shape::Segment(
                        DVec3::new(w[0].x * radius, -w[0].y * radius, 0.0),
                        DVec3::new(w[1].x * radius, -w[1].y * radius, 0.0),
                    )
                })
                .collect()
        };
        // Pitch circles every 10 degrees, brighter every 30; the horizon white and thick.
        for pitch in (-80..=80).step_by(10) {
            let points: Vec<DVec3> = (0..=120)
                .map(|k| {
                    to_ball(
                        basis,
                        horizon_direction(basis, 3.0 * k as f64, pitch as f64),
                    )
                })
                .collect();
            let (width, alpha) = match pitch {
                0 => (2.0, 1.0),
                p if p % 30 == 0 => (1.0, 0.55),
                _ => (1.0, 0.22),
            };
            self.stroke(&polyline(&points), width, [255.0, 255.0, 255.0, alpha]);
        }
        // Heading meridians every 30 degrees, from pole to pole.
        for heading in (0..360).step_by(30) {
            let points: Vec<DVec3> = (0..=60)
                .map(|k| {
                    to_ball(
                        basis,
                        horizon_direction(basis, heading as f64, -90.0 + 3.0 * k as f64),
                    )
                })
                .collect();
            let alpha = if heading % 90 == 0 { 0.6 } else { 0.3 };
            self.stroke(&polyline(&points), 1.0, [255.0, 255.0, 255.0, alpha]);
        }
        let centre = self.diameter() / 2.0;
        let mut label = |direction: DVec3, text: String| {
            let p = to_ball(basis, direction);
            // Labels near the rim are squashed and crowded; only the front of the ball is labelled.
            if p.z < 0.35 {
                return;
            }
            self.labels.push(NavballLabel {
                x: centre + p.x * radius,
                y: centre - p.y * radius,
                text,
                alpha: ((p.z - 0.35) / 0.2).min(1.0),
            });
        };
        for heading in (0..360).step_by(30) {
            let text = match heading {
                0 => "N".to_string(),
                90 => "E".to_string(),
                180 => "S".to_string(),
                270 => "W".to_string(),
                h => h.to_string(),
            };
            label(horizon_direction(basis, heading as f64, 4.0), text);
        }
        // Pitch numbers ride on the meridian under the nose, so they stay near the middle.
        let (nose_heading, _) = heading_pitch(basis, basis.nose);
        for pitch in [-60, -30, 30, 60] {
            label(
                horizon_direction(basis, nose_heading + 12.0, pitch as f64),
                pitch.to_string(),
            );
        }
    }

    /// Prograde: a circle with three ticks. Retrograde: a circle with a cross.
    fn marker(&mut self, p: DVec3, radius: f64, retrograde: bool) {
        if p.z < 0.0 {
            return;
        }
        let (x, y, r) = (p.x * radius, -p.y * radius, 6.0);
        let at = |x: f64, y: f64| DVec3::new(x, y, 0.0);
        let mut shape = vec![Shape::Circle(at(x, y), r)];
        if retrograde {
            let d = r * 0.7;
            shape.push(Shape::Segment(at(x - d, y - d), at(x + d, y + d)));
            shape.push(Shape::Segment(at(x + d, y - d), at(x - d, y + d)));
        } else {
            shape.push(Shape::Segment(at(x, y - r), at(x, y - r - 5.0)));
            shape.push(Shape::Segment(at(x - r, y), at(x - r - 5.0, y)));
            shape.push(Shape::Segment(at(x + r, y), at(x + r + 5.0, y)));
            shape.push(Shape::Circle(at(x, y), 1.5));
        }
        self.stroke(&shape, 3.5, SHADOW);
        self.stroke(&shape, 1.8, PROGRADE);
    }

    /// The fixed vessel mark at the centre: wings and a chevron.
    fn reticle(&mut self, radius: f64) {
        let w = radius * 0.32;
        let at = |x: f64, y: f64| DVec3::new(x, y, 0.0);
        let points = [
            at(-w, 0.0),
            at(-w * 0.35, 0.0),
            at(0.0, w * 0.3),
            at(w * 0.35, 0.0),
            at(w, 0.0),
        ];
        let mut shape: Vec<Shape> = points
            .windows(2)
            .map(|p| Shape::Segment(p[0], p[1]))
            .collect();
        shape.push(Shape::Segment(at(0.0, -3.0), at(0.0, 3.0)));
        self.stroke(&shape, 5.0, SHADOW);
        self.stroke(&shape, 2.5, RETICLE);
    }

    /// One canvas `stroke()`: the path's coverage (the union of its pieces, round joins and caps)
    /// blended once, source-over. Coordinates are CSS pixels from the centre, y down.
    fn stroke(&mut self, shapes: &[Shape], width: f64, color: [f64; 4]) {
        let (k, half) = (self.pixel_ratio, self.size as f64 / 2.0);
        let h = width * k / 2.0;
        let size = self.size as i64;
        let (mut x0, mut y0, mut x1, mut y1) = (size, size, -1_i64, -1_i64);
        for shape in shapes {
            let (lo, hi) = match *shape {
                Shape::Segment(a, b) => (a.min(b) * k, a.max(b) * k),
                Shape::Circle(c, r) => ((c - r) * k, (c + r) * k),
            };
            let bx0 = ((lo.x + half - h - 1.0).floor() as i64).max(0);
            let by0 = ((lo.y + half - h - 1.0).floor() as i64).max(0);
            let bx1 = ((hi.x + half + h + 1.0).ceil() as i64).min(size - 1);
            let by1 = ((hi.y + half + h + 1.0).ceil() as i64).min(size - 1);
            for py in by0..=by1 {
                let y = py as f64 + 0.5 - half;
                for px in bx0..=bx1 {
                    let x = px as f64 + 0.5 - half;
                    let d = match *shape {
                        Shape::Segment(a, b) => segment_distance(x, y, a * k, b * k),
                        Shape::Circle(c, r) => {
                            (((x - c.x * k).powi(2) + (y - c.y * k).powi(2)).sqrt() - r * k).abs()
                        }
                    };
                    let cover = (h + 0.5 - d).clamp(0.0, 1.0).min(2.0 * h) as f32;
                    let i = (py * size + px) as usize;
                    if cover > self.coverage[i] {
                        self.coverage[i] = cover;
                    }
                }
            }
            (x0, y0, x1, y1) = (x0.min(bx0), y0.min(by0), x1.max(bx1), y1.max(by1));
        }
        for py in y0.max(0)..=y1 {
            for px in x0.max(0)..=x1 {
                let i = (py * size + px) as usize;
                let a = self.coverage[i] as f64 * color[3];
                self.coverage[i] = 0.0;
                if a <= 0.0 {
                    continue;
                }
                let d = self.color[i];
                let out = a + d[3] * (1.0 - a);
                let mix = |s: f64, t: f64| (s * a + t * d[3] * (1.0 - a)) / out;
                self.color[i] = [
                    mix(color[0], d[0]),
                    mix(color[1], d[1]),
                    mix(color[2], d[2]),
                    out,
                ];
            }
        }
    }
}

fn segment_distance(x: f64, y: f64, a: DVec3, b: DVec3) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((x - a.x) * dx + (y - a.y) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((x - a.x - t * dx).powi(2) + (y - a.y - t * dy).powi(2)).sqrt()
}

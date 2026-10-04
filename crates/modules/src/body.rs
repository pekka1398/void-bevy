//! Every part's body in the air: a cylinder or cone along the part's axis whose ends are exposed
//! where no neighbour covers them.
use glam::DVec3;
use std::f64::consts::{PI, TAU};
use void_aero::{AeroElement, AeroShape, BodyAero};
use void_assembly::{PartGraph, Shape};

/// The end area at `node` that no neighbour among `members` covers; a separated stage covers
/// nothing.
fn exposed(graph: &PartGraph, members: &[String], id: &str, node: &str) -> f64 {
    let radius = graph.part(id).definition.radius;
    let mut covered = 0.0_f64;
    for link in graph.connections() {
        let other = if link.a == id && link.node_a == node {
            Some(&link.b)
        } else if link.b == id && link.node_b == node {
            Some(&link.a)
        } else {
            None
        };
        if let Some(other) = other
            && members.contains(other)
        {
            covered = covered.max(graph.part(other).definition.radius);
        }
    }
    PI * (radius.powi(2) - covered.powi(2)).max(0.0)
}

/// `part`'s body, placed relative to its vessel's centre of mass `centre` in the parts frame.
pub fn element(graph: &PartGraph, members: &[String], part: &str, centre: DVec3) -> AeroElement {
    let p = graph.part(part);
    let d = p.definition;
    AeroElement {
        id: part.into(),
        point: p.pose.position - centre,
        shape: AeroShape::Body(BodyAero {
            axis: p.pose.rotation * DVec3::Y,
            front_area: exposed(graph, members, part, "top"),
            rear_area: exposed(graph, members, part, "bottom"),
            side_area: 2.0 * d.radius * d.height,
            wet_area: TAU * d.radius * d.height,
            length_meters: d.height,
            front_cd: if d.shape == Shape::Cone { 0.25 } else { 0.6 },
            rear_cd: 0.8,
            side_cd: 1.1,
        }),
    }
}

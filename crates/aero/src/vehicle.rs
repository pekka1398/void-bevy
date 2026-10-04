//! Test vehicles, as the lab's `Vehicle.ts`: part geometry, mass, fuel, aerodynamic and thermal
//! set-up, and force limits. The A-01 aircraft and C-01 capsule are authored here; the rocket is
//! read from the assembly lab's demo craft.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use glam::{DQuat, DVec3};
use void_assembly::{CompiledCraft, compile, demo_craft};

use crate::{
    AeroElement, AeroShape, BodyAero, ControlSurface, DEG, DiskShield, ThermalSpec, ThermalState,
    WingAero, finite, finite_vec, length, positive, rotate, thermal_state, validate_rotation,
    validate_shape, validate_thermal,
};

/// Drawing and collider shape. Cylinders and cones lie along local +y, as Three's and Rapier's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartShape {
    Box { size: DVec3 },
    Cylinder { radius: f64, length: f64 },
    Cone { radius: f64, length: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PartAero {
    /// Where the element acts, in part axes.
    pub point: DVec3,
    pub shape: AeroShape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub id: String,
    pub position: DVec3,
    pub rotation: DQuat,
    pub shape: PartShape,
    /// 0xRRGGBB.
    pub color: u32,
    pub dry_mass_kg: f64,
    pub fuel_kg: f64,
    pub aero: Option<PartAero>,
    pub thermal: ThermalSpec,
    /// The face that takes the flow, in part axes; heating scales with how squarely it faces it.
    pub heating_normal: Option<DVec3>,
    /// A shield disk of this radius on the heating normal's face.
    pub shield_radius: Option<f64>,
    pub max_force_n: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Rocket,
    Airbreathing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub part_id: String,
    pub thrust_newtons: f64,
    pub isp_seconds: f64,
    /// Thrust direction in part axes.
    pub direction: DVec3,
    pub tank_ids: Vec<String>,
    pub kind: EngineKind,
    /// An air-breathing engine gives no thrust below this ambient pressure.
    pub minimum_pressure_pa: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThermalLink {
    pub a: String,
    pub b: String,
    pub conductance_wk: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    pub position: DVec3,
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub id: String,
    pub name: String,
    pub parts: Vec<Part>,
    pub engines: Vec<Engine>,
    pub thermal_links: Vec<ThermalLink>,
    pub wheels: Vec<Wheel>,
}

impl Vehicle {
    pub fn part_index(&self, id: &str) -> usize {
        self.parts
            .iter()
            .position(|p| p.id == id)
            .unwrap_or_else(|| panic!("Missing part state {id}"))
    }
}

/// What a flight uses up, per part in the vehicle's part order.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleResources {
    pub fuel: Vec<f64>,
    pub thermal: Vec<ThermalState>,
}

impl VehicleResources {
    pub fn fuel_kg(&self) -> f64 {
        self.fuel.iter().fold(0.0, |a, b| a + b)
    }
}

pub fn resources(vehicle: &Vehicle) -> VehicleResources {
    validate_vehicle(vehicle);
    VehicleResources {
        fuel: vehicle.parts.iter().map(|p| p.fuel_kg).collect(),
        thermal: vehicle
            .parts
            .iter()
            .map(|p| thermal_state(&p.thermal, 288.15))
            .collect(),
    }
}

pub fn part_mass(vehicle: &Vehicle, index: usize, state: &VehicleResources) -> f64 {
    let part = &vehicle.parts[index];
    let (f, thermal) = (state.fuel[index], &state.thermal[index]);
    finite(f, "remaining fuel");
    finite(thermal.ablator_kg, "remaining ablator");
    assert!(
        !(f < 0.0
            || f > part.fuel_kg
            || thermal.ablator_kg < 0.0
            || thermal.ablator_kg > part.thermal.ablator.map_or(0.0, |a| a.mass_kg)),
        "Invalid resource mass {}",
        part.id
    );
    part.dry_mass_kg + f + thermal.ablator_kg
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    pub mass: f64,
    pub center: DVec3,
    /// Diagonal inertia about the centre of mass, vehicle axes.
    pub inertia: DVec3,
}

/// Mass, centre of mass and a diagonal inertia (an approximation for the capsule integrator;
/// Rapier computes the aircraft's full compound inertia itself).
pub fn mass_properties(vehicle: &Vehicle, state: &VehicleResources) -> MassProperties {
    let (mut mass, mut sum) = (0.0, DVec3::ZERO);
    for (i, part) in vehicle.parts.iter().enumerate() {
        let m = part_mass(vehicle, i, state);
        mass += m;
        sum += part.position * m;
    }
    let center = sum * (1.0 / positive(mass, "vehicle mass"));
    let mut inertia = DVec3::ZERO;
    for (i, part) in vehicle.parts.iter().enumerate() {
        let m = part_mass(vehicle, i, state);
        let d = part.position - center;
        let r = part.rotation;
        let axes = [
            rotate(r, DVec3::X),
            rotate(r, DVec3::Y),
            rotate(r, DVec3::Z),
        ];
        let principal = match part.shape {
            PartShape::Box { size } => [
                m * (size.y * size.y + size.z * size.z) / 12.0,
                m * (size.x * size.x + size.z * size.z) / 12.0,
                m * (size.x * size.x + size.y * size.y) / 12.0,
            ],
            PartShape::Cylinder { radius, length } | PartShape::Cone { radius, length } => {
                let side = m * (3.0 * (radius * radius) + length * length) / 12.0;
                [side, m * (radius * radius) / 2.0, side]
            }
        };
        let along = |c: fn(DVec3) -> f64| {
            axes.iter()
                .zip(principal)
                .fold(0.0, |a, (v, p)| a + c(*v) * c(*v) * p)
        };
        inertia += DVec3::new(
            along(|v| v.x) + m * (d.y * d.y + d.z * d.z),
            along(|v| v.y) + m * (d.x * d.x + d.z * d.z),
            along(|v| v.z) + m * (d.x * d.x + d.y * d.y),
        );
    }
    MassProperties {
        mass,
        center,
        inertia,
    }
}

/// The vehicle's aerodynamic elements in vehicle axes, relative to `center`.
pub fn aero_elements(vehicle: &Vehicle, center: DVec3) -> Vec<AeroElement> {
    vehicle
        .parts
        .iter()
        .filter_map(|p| {
            let aero = p.aero.as_ref()?;
            let shape = match &aero.shape {
                AeroShape::Body(s) => AeroShape::Body(BodyAero {
                    axis: rotate(p.rotation, s.axis),
                    ..s.clone()
                }),
                AeroShape::Wing(s) => AeroShape::Wing(WingAero {
                    chord: rotate(p.rotation, s.chord),
                    normal: rotate(p.rotation, s.normal),
                    ..s.clone()
                }),
            };
            Some(AeroElement {
                id: p.id.clone(),
                point: p.position + rotate(p.rotation, aero.point) - center,
                shape,
            })
        })
        .collect()
}

/// The shield disks, in vehicle axes.
pub fn shield_disks(vehicle: &Vehicle) -> Vec<DiskShield> {
    vehicle
        .parts
        .iter()
        .filter_map(|p| match (p.shield_radius, p.heating_normal) {
            (Some(radius_meters), Some(normal)) => Some(DiskShield {
                id: p.id.clone(),
                point: p.position,
                normal: rotate(p.rotation, normal),
                radius_meters,
            }),
            _ => None,
        })
        .collect()
}

pub fn validate_vehicle(vehicle: &Vehicle) {
    assert!(!vehicle.parts.is_empty(), "Empty aerodynamic vehicle");
    let mut ids = std::collections::HashSet::new();
    for p in &vehicle.parts {
        assert!(ids.insert(&p.id), "Duplicate aerodynamic part {}", p.id);
        positive(p.dry_mass_kg, &format!("{} dry mass", p.id));
        finite(p.fuel_kg, "fuel");
        assert!(p.fuel_kg >= 0.0, "Negative fuel");
        finite_vec(p.position, "part position");
        validate_rotation(p.rotation);
        if let Some(aero) = &p.aero {
            finite_vec(aero.point, "aero point");
            validate_shape(&aero.shape);
        }
        if let Some(n) = p.heating_normal {
            finite_vec(n, "heating normal");
            assert!(
                (length(n) - 1.0).abs() <= 1e-8,
                "Heating normal must be unit"
            );
        }
        if let Some(r) = p.shield_radius {
            positive(r, "shield radius");
            assert!(p.heating_normal.is_some(), "Shield needs a normal");
        }
        validate_thermal(&p.thermal);
        positive(p.max_force_n, "part force limit");
        match p.shape {
            PartShape::Box { size } => {
                for v in size.to_array() {
                    positive(v, "box size");
                }
            }
            PartShape::Cylinder { radius, length } | PartShape::Cone { radius, length } => {
                positive(radius, "radius");
                positive(length, "length");
            }
        }
    }
    let has = |id: &String| vehicle.parts.iter().any(|p| &p.id == id);
    for e in &vehicle.engines {
        assert!(
            has(&e.part_id) && !e.tank_ids.is_empty() && e.tank_ids.iter().all(has),
            "Engine resource graph invalid"
        );
        positive(e.thrust_newtons, "engine thrust");
        positive(e.isp_seconds, "engine Isp");
        finite_vec(e.direction, "engine direction");
        assert!(
            (length(e.direction) - 1.0).abs() <= 1e-8,
            "Engine direction must be unit"
        );
        finite(e.minimum_pressure_pa, "engine pressure");
        assert!(e.minimum_pressure_pa >= 0.0, "Negative engine pressure");
    }
    for link in &vehicle.thermal_links {
        assert!(
            has(&link.a) && has(&link.b) && link.a != link.b,
            "Invalid thermal connection"
        );
        finite(link.conductance_wk, "heat link");
        assert!(link.conductance_wk >= 0.0, "Negative heat link");
    }
    for wheel in &vehicle.wheels {
        finite_vec(wheel.position, "wheel position");
        positive(wheel.radius, "wheel radius");
    }
}

/// The lab's default part skin; `edit` applies the per-part overrides.
fn skin(mass: f64, edit: impl FnOnce(&mut ThermalSpec)) -> ThermalSpec {
    let mut spec = ThermalSpec {
        skin_capacity_jk: mass * 120.0,
        core_capacity_jk: mass * 780.0,
        conductance_wk: 12.0,
        radiating_area: 2.0,
        heating_area: 1.0,
        convection_area: 1.0,
        emissivity: 0.8,
        nose_radius: 0.2,
        heating_factor: 0.15,
        max_skin_k: 700.0,
        max_core_k: 500.0,
        ablator: None,
    };
    edit(&mut spec);
    spec
}

const FORWARD: DVec3 = DVec3::Z;
const UP: DVec3 = DVec3::Y;
/// Turns a cylinder's +y axis to +z (forward).
const CYLINDER_FORWARD: DQuat = DQuat::from_xyzw(FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2);

fn wing(
    area: f64,
    aspect_ratio: f64,
    control: ControlSurface,
    sign: f64,
    incidence: f64,
) -> AeroShape {
    AeroShape::Wing(WingAero {
        chord: FORWARD,
        normal: if control == ControlSurface::Rudder {
            DVec3::X
        } else {
            UP
        },
        area,
        aspect_ratio,
        chord_meters: 1.45,
        sweep_radians: 5.0 * DEG,
        incidence_radians: incidence * DEG,
        zero_lift_radians: 0.0,
        stall_radians: 15.0 * DEG,
        cd0: 0.018,
        efficiency: 0.82,
        pitching_moment: 0.0,
        control,
        control_sign: sign,
        max_deflection_radians: 18.0 * DEG,
    })
}

/// A-01: a light aircraft with an air-breathing engine and fixed rolling gear. Vehicle axes: +z
/// forward, +y up, +x toward the right wing.
pub fn aircraft() -> Vehicle {
    let fuselage = Part {
        id: "fuselage".into(),
        position: DVec3::ZERO,
        rotation: CYLINDER_FORWARD,
        shape: PartShape::Cylinder {
            radius: 0.48,
            length: 6.8,
        },
        color: 0xd1dbde,
        dry_mass_kg: 570.0,
        fuel_kg: 180.0,
        aero: Some(PartAero {
            point: DVec3::new(0.0, -0.35, 0.0),
            shape: AeroShape::Body(BodyAero {
                axis: UP,
                front_area: 0.72,
                rear_area: 0.72,
                side_area: 5.8,
                wet_area: 19.0,
                length_meters: 6.8,
                front_cd: 0.18,
                rear_cd: 0.4,
                side_cd: 1.05,
            }),
        }),
        thermal: skin(570.0, |s| {
            s.radiating_area = 19.0;
            s.convection_area = 12.0;
            s.heating_area = 3.0;
        }),
        heating_normal: None,
        shield_radius: None,
        max_force_n: 80_000.0,
    };
    let surface = |id: &str, position: DVec3, size: DVec3, mass: f64, aero: AeroShape| Part {
        id: id.into(),
        position,
        rotation: DQuat::IDENTITY,
        shape: PartShape::Box { size },
        color: if id == "fin" { 0x4a8c99 } else { 0x70a9ab },
        dry_mass_kg: mass,
        fuel_kg: 0.0,
        aero: Some(PartAero {
            point: DVec3::ZERO,
            shape: aero,
        }),
        thermal: skin(mass, |s| {
            s.radiating_area = size.x * size.z * 2.0;
            s.heating_area = 0.2;
            s.heating_factor = 0.12;
            s.nose_radius = 0.035;
            s.convection_area = 1.0;
        }),
        heating_normal: None,
        shield_radius: None,
        max_force_n: if id.starts_with("wing") {
            45_000.0
        } else {
            15_000.0
        },
    };
    let main_wing = DVec3::new(4.8, 0.14, 1.65);
    Vehicle {
        id: "aircraft".into(),
        name: "A-01 test aircraft".into(),
        parts: vec![
            fuselage,
            surface(
                "wing-left",
                DVec3::new(-2.6, 0.1, 0.0),
                main_wing,
                65.0,
                wing(7.92, 6.2, ControlSurface::Aileron, 1.0, 3.0),
            ),
            surface(
                "wing-right",
                DVec3::new(2.6, 0.1, 0.0),
                main_wing,
                65.0,
                wing(7.92, 6.2, ControlSurface::Aileron, -1.0, 3.0),
            ),
            surface(
                "tail",
                DVec3::new(0.0, 0.28, -2.8),
                DVec3::new(3.2, 0.1, 0.9),
                35.0,
                wing(2.88, 3.6, ControlSurface::Elevator, -1.0, 1.3),
            ),
            surface(
                "fin",
                DVec3::new(0.0, 0.9, -2.6),
                DVec3::new(0.1, 1.6, 1.05),
                25.0,
                wing(1.68, 1.5, ControlSurface::Rudder, -1.0, 0.0),
            ),
        ],
        engines: vec![Engine {
            part_id: "fuselage".into(),
            thrust_newtons: 3600.0,
            isp_seconds: 1800.0,
            direction: UP,
            tank_ids: vec!["fuselage".into()],
            kind: EngineKind::Airbreathing,
            minimum_pressure_pa: 8000.0,
        }],
        thermal_links: vec![],
        wheels: vec![
            Wheel {
                position: DVec3::new(0.0, -0.8, 2.1),
                radius: 0.23,
            },
            Wheel {
                position: DVec3::new(-1.1, -0.8, -0.45),
                radius: 0.28,
            },
            Wheel {
                position: DVec3::new(1.1, -0.8, -0.45),
                radius: 0.28,
            },
        ],
    }
}

/// The assembly lab's craft as bodies: its real part poses, masses and fuel, with the end faces
/// joined to another part hidden from the flow. No engines: it is for the wind tunnel.
pub fn assembly_rocket(compiled: &CompiledCraft) -> Vehicle {
    let parts = compiled
        .parts
        .iter()
        .map(|p| {
            let d = p.definition;
            let area = PI * (d.radius * d.radius);
            let occupied = |node: &str| {
                compiled.connections.iter().any(|c| {
                    c.a == p.instance.id && c.node_a == node
                        || c.b == p.instance.id && c.node_b == node
                })
            };
            let cone = d.shape == void_assembly::Shape::Cone;
            let shape = match d.shape {
                void_assembly::Shape::Cone => PartShape::Cone {
                    radius: d.radius,
                    length: d.height,
                },
                void_assembly::Shape::Cylinder => PartShape::Cylinder {
                    radius: d.radius,
                    length: d.height,
                },
                void_assembly::Shape::Box => PartShape::Box {
                    size: DVec3::new(2.0 * d.radius, d.height, 2.0 * d.radius),
                },
            };
            Part {
                id: p.instance.id.clone(),
                position: p.pose.position,
                rotation: p.pose.rotation,
                shape,
                color: u32::from_str_radix(&d.color[1..], 16)
                    .unwrap_or_else(|_| panic!("part colour {}", d.color)),
                dry_mass_kg: d.dry_mass_kg,
                fuel_kg: p.instance.resource_mass(),
                aero: Some(PartAero {
                    point: DVec3::ZERO,
                    shape: AeroShape::Body(BodyAero {
                        axis: UP,
                        front_area: if occupied("top") { 0.0 } else { area },
                        rear_area: if occupied("bottom") { 0.0 } else { area },
                        side_area: 2.0 * d.radius * d.height,
                        wet_area: 2.0 * PI * d.radius * d.height,
                        length_meters: d.height,
                        front_cd: if cone { 0.25 } else { 0.6 },
                        rear_cd: 0.8,
                        side_cd: 1.1,
                    }),
                }),
                thermal: skin(d.dry_mass_kg, |s| {
                    s.radiating_area = 2.0 * PI * d.radius * d.height;
                    s.nose_radius = 0.3;
                }),
                heating_normal: None,
                shield_radius: None,
                max_force_n: 200_000.0,
            }
        })
        .collect();
    Vehicle {
        id: "rocket".into(),
        name: "Assembly two-stage rocket".into(),
        parts,
        engines: vec![],
        thermal_links: vec![],
        wheels: vec![],
    }
}

/// `assembly_rocket` of the assembly lab's demo craft.
pub fn demo_rocket() -> Vehicle {
    assembly_rocket(&compile(&demo_craft()).expect("assembly demo craft compiles"))
}

/// C-01: a heat shield ahead of a conical pod, flying +z first. Without protection the shield
/// has no ablator, conducts heat to the pod and fails sooner.
pub fn capsule(protected_shield: bool, ablator_mass: f64) -> Vehicle {
    let shield_area = PI * (1.25 * 1.25);
    let shield = Part {
        id: "shield".into(),
        position: DVec3::new(0.0, 0.0, 0.35),
        rotation: CYLINDER_FORWARD,
        shape: PartShape::Cylinder {
            radius: 1.25,
            length: 0.15,
        },
        color: 0x957c60,
        dry_mass_kg: 120.0,
        fuel_kg: 0.0,
        aero: Some(PartAero {
            point: DVec3::new(0.0, -0.9, 0.0),
            shape: AeroShape::Body(BodyAero {
                axis: UP,
                front_area: shield_area,
                rear_area: shield_area,
                side_area: 2.5,
                wet_area: 7.0,
                length_meters: 2.5,
                front_cd: 1.2,
                rear_cd: 0.85,
                side_cd: 1.0,
            }),
        }),
        thermal: skin(120.0, |s| {
            s.skin_capacity_jk = 30_000.0;
            s.core_capacity_jk = 150_000.0;
            s.conductance_wk = if protected_shield { 1.5 } else { 200.0 };
            s.radiating_area = 4.9;
            s.heating_area = 4.9;
            s.convection_area = 4.9;
            s.nose_radius = 1.25;
            s.heating_factor = 1.0;
            s.max_skin_k = if protected_shield { 2400.0 } else { 850.0 };
            s.max_core_k = 550.0;
            s.ablator = protected_shield.then_some(crate::Ablator {
                mass_kg: ablator_mass,
                activation_k: 1100.0,
                latent_j_kg: 12e6,
            });
        }),
        heating_normal: Some(UP),
        shield_radius: protected_shield.then_some(1.25),
        max_force_n: 650_000.0,
    };
    // Cone geometry points along local +y. Its wide base must face the leading shield (+z), with
    // the narrow end extending aft, so the pod's local +y points toward craft −z.
    let pod = Part {
        id: "pod".into(),
        position: DVec3::new(0.0, 0.0, -0.35),
        rotation: DQuat::from_xyzw(-FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2),
        shape: PartShape::Cone {
            radius: 1.08,
            length: 1.15,
        },
        color: 0xd6dce0,
        dry_mass_kg: 1250.0,
        fuel_kg: 0.0,
        aero: None,
        thermal: skin(1250.0, |s| {
            s.radiating_area = 7.0;
            s.convection_area = 2.0;
            s.heating_area = 2.0;
            s.heating_factor = 0.5;
            s.nose_radius = 0.7;
            s.max_skin_k = 750.0;
            s.max_core_k = 420.0;
        }),
        heating_normal: None,
        shield_radius: None,
        max_force_n: 800_000.0,
    };
    Vehicle {
        id: "capsule".into(),
        name: if protected_shield {
            "C-01 ablative capsule"
        } else {
            "C-01 unprotected"
        }
        .into(),
        parts: vec![shield, pod],
        engines: vec![],
        thermal_links: vec![ThermalLink {
            a: "shield".into(),
            b: "pod".into(),
            conductance_wk: 0.8,
        }],
        wheels: vec![],
    }
}

/// The lab's default capsule: protected, 130 kg of ablator.
pub const DEFAULT_ABLATOR_KG: f64 = 130.0;

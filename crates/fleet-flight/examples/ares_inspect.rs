//! Diagnose the actual saved main camera against the authoritative Ares surface.
use glam::DVec3;
use void_fleet_flight::checkpoint::FlightCheckpoint;
use void_terrain::TerrainConfig;
fn main() {
    let path = std::env::args().nth(1).expect("checkpoint path");
    let sim = FlightCheckpoint::read(&path).restore();
    let body = sim.world.body_index("ares");
    let sample = sim.presentation.sample(&sim);
    let frames = sim.fleet.frames();
    let into = frames.transform(sim.fleet.origin_frame(), sim.fleet.body_frames(body).1);
    let eye = into.apply_point(sample.eye);
    let focus = into.apply_point(sample.focus);
    let terrain = &sim.terrains[&body];
    let TerrainConfig::Ares(o) = terrain.config() else {
        panic!("needs Ares")
    };
    let (center, along, across) = o.canyon_frame();
    println!(
        "eye body-fixed {eye}; focus {focus}; focus direction {}; range {}m; canyon centre {center}; angle {}rad",
        focus.normalize(),
        (eye - focus).length(),
        focus.normalize().angle_between(center)
    );
    println!(
        "sun body-fixed {}",
        frames
            .transform(
                sim.fleet.body_frames(sim.world.body_index("sol")).0,
                sim.fleet.body_frames(body).1
            )
            .apply_point(DVec3::ZERO)
            .normalize()
    );
    println!(
        "canyon profile across wall (offset km, full height m, 1km-cell height m, linear albedo):"
    );
    for km in [-100., -75., -50., -25., 0., 25., 50., 75., 100.] {
        let d = (center + across * (km * 1000. / terrain.radius_meters)).normalize();
        println!(
            "{km:6.1} {:10.2} {:10.2} {:?}",
            terrain.height(d),
            terrain.sample(d, 1000.).0,
            terrain.sample(d, terrain.finest_cell_meters()).1
        );
    }
    println!("along {along}; across {across}");
    if let Some(flag) = std::env::args().nth(2) {
        assert_eq!(flag, "--polar-checkpoint");
        let output = std::env::args()
            .nth(3)
            .expect("polar checkpoint output path");
        let direction = frames
            .transform(sim.fleet.body_frames(body).1, sim.fleet.origin_frame())
            .apply_direction(DVec3::NEG_Z);
        let mut session = void_fleet_flight::session::FlightSession::load_checkpoint(path);
        session.execute(void_fleet_flight::session::Action::View {
            command: void_fleet_flight::presentation::ViewCommand::BodyPreset {
                body,
                direction,
                distance: terrain.radius_meters * 2.4,
            },
        });
        session.save_checkpoint(output);
    }
}

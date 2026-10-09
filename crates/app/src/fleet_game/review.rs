//! Explicit desktop render review; uses the real UI systems and authored world.
use super::*;
#[derive(Resource)]
pub(super) struct UiReview {
    panel: String,
    path: String,
    size: Option<(u32, u32)>,
    craft: Option<String>,
    opened: bool,
    captured: bool,
    elapsed: f64,
}
pub(super) fn configure(app: &mut App, main_game: bool) {
    if let Some(panel) = argument("--ui-review") {
        assert!(main_game, "UI review requires main game");
        assert!(
            ["hud", "menu", "workshop", "navigation"].contains(&panel.as_str()),
            "unknown --ui-review panel"
        );
        let path = argument("--ui-review-screenshot")
            .expect("--ui-review requires --ui-review-screenshot PATH");
        let scale =
            argument("--ui-review-scale").map_or(1.0, |s| s.parse::<f32>().expect("review scale"));
        assert!(
            [0.8, 1.0, 1.25, 1.5].contains(&scale),
            "supported review scales: 0.8, 1, 1.25, 1.5"
        );
        app.insert_resource(UiScale(scale));
        let craft = argument("--ui-review-craft");
        if let Some(name) = &craft {
            assert!(
                panel == "workshop" && ["rocket", "rover", "aircraft"].contains(&name.as_str()),
                "--ui-review-craft requires workshop and rocket|rover|aircraft"
            );
        }
        let size = argument("--ui-review-size").map(|s| {
            let (w, h) = s.split_once('x').expect("--ui-review-size WIDTHxHEIGHT");
            let dimensions = (
                w.parse::<u32>().expect("review width"),
                h.parse::<u32>().expect("review height"),
            );
            assert!(
                dimensions.0 > 0 && dimensions.1 > 0,
                "review size must be nonzero"
            );
            dimensions
        });
        app.insert_resource(UiReview {
            panel,
            path,
            size,
            craft,
            opened: false,
            captured: false,
            elapsed: 0.,
        });
        app.add_systems(Update, review.before(menus::update));
    }
}
#[allow(clippy::too_many_arguments)]
fn review(
    mut commands: Commands,
    mut state: ResMut<UiReview>,
    mut lab: NonSendMut<Lab>,
    mut menu: ResMut<menus::MenuState>,
    mut workshop: ResMut<workshop::Workshop>,
    mut cockpit: ResMut<completion::Cockpit>,
    time: Res<Time>,
    mut windows: Query<&mut Window>,
    mut exit: MessageWriter<AppExit>,
) {
    if !state.opened {
        if let Some((w, h)) = state.size {
            for mut window in &mut windows {
                window.resolution.set(w as f32, h as f32);
            }
        }
        lab.paused = true;
        match state.panel.as_str() {
            "menu" => menu.blocking = true,
            "workshop" => {
                if let Some(name) = &state.craft {
                    workshop
                        .review_preset(name)
                        .expect("review craft compilation");
                }
                workshop.toggle(&mut lab);
            }
            "navigation" => cockpit.open_navigation(),
            _ => {}
        }
        state.opened = true;
    }
    state.elapsed += time.delta_secs_f64();
    if state.elapsed >= 4. && !state.captured {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        let path = std::path::Path::new(&state.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("review screenshot directory");
        }
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(state.path.clone()));
        state.captured = true;
    }
    if state.elapsed >= 7. {
        exit.write(AppExit::Success);
    }
}

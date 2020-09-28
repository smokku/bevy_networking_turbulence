use bevy_app::{App, ScheduleRunnerPlugin};
use std::time::Duration;

use bevy_networking_turbulence::NetworkingPlugin;

fn main() {
    App::build()
        .add_plugin(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        // The NetworkingPlugin
        .add_plugin(NetworkingPlugin)
        .run();
}

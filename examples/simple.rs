use bevy_app::{App, ScheduleRunnerPlugin};
use bevy_ecs::prelude::*;
use std::time::Duration;

use bevy_networking_turbulence::{NetworkResource, NetworkingPlugin};

fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .expect("A logger was already initialized");

    App::build()
        .add_plugin(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        // The NetworkingPlugin
        .add_plugin(NetworkingPlugin)
        // Our networking
        .add_startup_system(startup.system())
        .run();
}

fn startup(net: ResMut<NetworkResource>) {
    let args = parse_args();
    if args.is_server {
        start_server(net);
    } else {
        start_client(net);
    }
}

fn start_server(mut net: ResMut<NetworkResource>) {
    log::info!("Starting server")
}

fn start_client(mut net: ResMut<NetworkResource>) {
    log::info!("Starting client")
}

struct Args {
    pub is_server: bool,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        panic!("Need to select to run as either a server (--server) or a client (--client).");
    }

    let connection_type = &args[1];

    let is_server = match connection_type.as_str() {
        "--server" | "-s" => true,
        "--client" | "-c" => false,
        _ => panic!("Need to select to run as either a server (--server) or a client (--client)."),
    };

    Args { is_server }
}

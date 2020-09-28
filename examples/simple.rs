use bevy::{
    app::{App, ScheduleRunnerPlugin},
    ecs::prelude::*,
};

use std::{net::SocketAddr, time::Duration};

use bevy_networking_turbulence::{NetworkResource, NetworkingPlugin};
use naia_server_socket::find_my_ip_address;

const SERVER_PORT: u16 = 14191;

fn main() {
    simple_logger::SimpleLogger::from_env()
        .init()
        .expect("A logger was already initialized");

    App::build()
        // minimal plugins necessary for timers + headless loop
        .add_plugin(bevy::type_registry::TypeRegistryPlugin::default())
        .add_plugin(bevy::core::CorePlugin)
        .add_plugin(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        // The NetworkingPlugin
        .add_plugin(NetworkingPlugin)
        // Our networking
        .add_startup_system(startup.system())
        .run();
}

fn startup(mut net: ResMut<NetworkResource>) {
    let ip_address = find_my_ip_address().expect("can't find ip address");
    let socket_address = SocketAddr::new(ip_address, SERVER_PORT);

    let args = parse_args();
    if args.is_server {
        log::info!("Starting server");
        net.listen(socket_address);
    } else {
        log::info!("Starting client");
        net.connect(socket_address);
    }
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

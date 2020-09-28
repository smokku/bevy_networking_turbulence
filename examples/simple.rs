use bevy::{
    app::{App, EventReader, Events, ScheduleRunnerPlugin},
    core::Time,
    ecs::prelude::*,
};

use futures_lite::future;
use std::{net::SocketAddr, time::Duration};

use bevy_networking_turbulence::{NetworkResource, NetworkingPlugin};
use naia_client_socket::Packet as ClientPacket;
use naia_server_socket::{find_my_ip_address, Packet as ServerPacket};

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
        .add_resource(parse_args())
        .add_startup_system(startup.system())
        .add_system(send_packets.system())
        .init_resource::<PacketReader>()
        .add_system(handle_packets.system())
        .run();
}

fn startup(mut net: ResMut<NetworkResource>, args: Res<Args>) {
    let ip_address = find_my_ip_address().expect("can't find ip address");
    let socket_address = SocketAddr::new(ip_address, SERVER_PORT);

    if args.is_server {
        log::info!("Starting server");
        net.listen(socket_address);
    } else {
        log::info!("Starting client");
        net.connect(socket_address);
    }
}

fn send_packets(mut net: ResMut<NetworkResource>, time: Res<Time>, args: Res<Args>) {
    if !args.is_server {
        if (time.seconds_since_startup * 60.) as i64 % 60 == 0 {
            log::info!("PING");
            match net.clients[0]
                .sender
                .send(ClientPacket::new("PING".to_string().into_bytes()))
            {
                Ok(()) => {}
                Err(error) => {
                    log::info!("PING send error: {}", error);
                }
            }
        }
    }
}

#[derive(Default)]
struct PacketReader {
    server_events: EventReader<ServerPacket>,
    client_events: EventReader<ClientPacket>,
}

fn handle_packets(
    mut net: ResMut<NetworkResource>,
    time: Res<Time>,
    mut state: ResMut<PacketReader>,
    server_events: Res<Events<ServerPacket>>,
    client_events: Res<Events<ClientPacket>>,
) {
    for packet in state.server_events.iter(&server_events) {
        log::info!(
            "Server got packet: {}",
            String::from_utf8_lossy(packet.payload())
        );
        let message = format!(
            "PONG @ {} to [{}]",
            time.seconds_since_startup,
            packet.address()
        );
        let payload = message.clone().into_bytes();
        match future::block_on(
            net.servers[0]
                .sender
                .send(ServerPacket::new(packet.address(), payload)),
        ) {
            Ok(()) => {
                log::info!("Sent PONG: {}", message);
            }
            Err(error) => {
                log::info!("PONG send error: {}", error);
            }
        }
    }
    for packet in state.client_events.iter(&client_events) {
        log::info!(
            "Client got packet: {}",
            String::from_utf8_lossy(packet.payload())
        );
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

use bevy::{
    app::{App, EventReader, Events, ScheduleRunnerPlugin},
    core::Time,
    ecs::prelude::*,
};
use bevy_networking_turbulence::{NetworkResource, NetworkingPlugin, Packet};

use std::{net::SocketAddr, time::Duration};

mod utils;
use utils::*;

const SERVER_PORT: u16 = 14191;

fn main() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Debug).expect("cannot initialize console_log");
        }
        else {
            simple_logger::SimpleLogger::from_env()
            .init()
            .expect("A logger was already initialized");
        }
    }

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
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            // FIXME: set this address to your local machine
            let mut socket_address: SocketAddr = "192.168.1.20:0".parse().unwrap();
            socket_address.set_port(SERVER_PORT);
        } else {
            let ip_address =
                bevy_networking_turbulence::find_my_ip_address().expect("can't find ip address");
            let socket_address = SocketAddr::new(ip_address, SERVER_PORT);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    if args.is_server {
        log::info!("Starting server");
        net.listen(socket_address);
    }
    if !args.is_server {
        log::info!("Starting client");
        net.connect(socket_address);
    }
}

fn send_packets(net: ResMut<NetworkResource>, time: Res<Time>, args: Res<Args>) {
    if !args.is_server {
        if (time.seconds_since_startup * 60.) as i64 % 60 == 0 {
            log::info!("PING");
            match net.connections.lock().unwrap()[0]
                .send(Packet::copy_from_slice("PING".to_string().as_bytes()))
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
    packet_events: EventReader<Packet>,
}

fn handle_packets(
    net: ResMut<NetworkResource>,
    time: Res<Time>,
    mut state: ResMut<PacketReader>,
    packet_events: Res<Events<Packet>>,
) {
    for packet in state.packet_events.iter(&packet_events) {
        let message = String::from_utf8_lossy(packet);
        log::info!("Got packet: {}", message);
        if message == "PING" {
            let message = format!("PONG @ {}", time.seconds_since_startup);
            match net.connections.lock().unwrap()[0]
                .send(Packet::copy_from_slice(message.as_bytes()))
            {
                Ok(()) => {
                    log::info!("Sent PONG: {}", message);
                }
                Err(error) => {
                    log::info!("PONG send error: {}", error);
                }
            }
        }
    }
}

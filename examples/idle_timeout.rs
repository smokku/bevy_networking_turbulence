use bevy::{app::ScheduleRunnerSettings, core::FixedTimestep, log::LogPlugin, prelude::*};

use bevy_networking_turbulence::{NetworkEvent, NetworkResource, NetworkingPlugin, Packet};

use std::{net::SocketAddr, time::Duration};

mod utils;
use utils::{parse_idle_timeout_args, IdleTimeoutArgs as Args};

const SERVER_PORT: u16 = 14191;

#[derive(Debug, Default)]
struct PingPongCounter {
    ping_reservoir: usize,
    pong_reservoir: usize,
}

fn main() {
    let args = parse_idle_timeout_args();
    info!("{:?}", args);

    let net_plugin = NetworkingPlugin {
        idle_timeout_ms: args.idle_timeout_ms,
        auto_heartbeat_ms: args.auto_heartbeat_ms,
        ..Default::default()
    };

    let ppc = PingPongCounter {
        ping_reservoir: args.pings,
        pong_reservoir: args.pongs,
    };

    let mut app = App::new();
    app
        // minimal plugins necessary for timers + headless loop
        .insert_resource(ScheduleRunnerSettings::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(ppc)
        .add_plugins(MinimalPlugins)
        .add_plugin(LogPlugin)
        // The NetworkingPlugin
        .add_plugin(net_plugin)
        // Our networking
        .insert_resource(args)
        .add_startup_system(startup.system())
        .add_system(send_pongs.system())
        .add_stage_after(
            CoreStage::Update,
            "ping_sending_stage",
            SystemStage::single(send_pings.system()).with_run_criteria(FixedTimestep::step(1.0)),
        );
    app.run();
}

fn startup(mut net: ResMut<NetworkResource>, args: Res<Args>) {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            // set the following address to your server address (i.e. local machine)
            // and remove compile_error! line
            let mut server_address: SocketAddr = "192.168.1.1:0".parse().unwrap();
            compile_error!("You need to set server_address.");
            server_address.set_port(SERVER_PORT);
        } else {
            let ip_address =
                bevy_networking_turbulence::find_my_ip_address().expect("can't find ip address");
            let server_address = SocketAddr::new(ip_address, SERVER_PORT);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    if args.is_server {
        info!("Starting server");
        net.listen(server_address, None, None);
    }
    if !args.is_server {
        info!("Starting client");
        net.connect(server_address);
    }
}

fn send_pings(mut net: ResMut<NetworkResource>, mut ppc: ResMut<PingPongCounter>) {
    if ppc.ping_reservoir == 0 {
        return;
    }

    ppc.ping_reservoir -= 1;
    net.broadcast(Packet::from("PING"));

    if ppc.ping_reservoir == 0 {
        info!("(No more pings left to send)");
    }
}

fn send_pongs(
    mut net: ResMut<NetworkResource>,
    mut ppc: ResMut<PingPongCounter>,
    mut reader: EventReader<NetworkEvent>,
) {
    for event in reader.iter() {
        match event {
            NetworkEvent::Packet(handle, packet) => {
                let message = String::from_utf8_lossy(packet);
                info!("Got packet on [{}]: {}", handle, message);
                if message == "PING" {
                    if ppc.pong_reservoir > 0 {
                        ppc.pong_reservoir -= 1;
                        match net.send(*handle, Packet::from("PONG")) {
                            Ok(()) => info!("Sent PONG"),
                            Err(error) => warn!("PONG send error: {}", error),
                        }
                    } else {
                        info!("No pongs left to send.");
                    }
                }
            }
            other => {
                info!("Other event: {:?}", other);
            }
        }
    }
}

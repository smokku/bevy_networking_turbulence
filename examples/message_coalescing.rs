use bevy::{
    app::{App, ScheduleRunnerSettings, AppExit, EventWriter, CoreStage},
    core::Time,
    ecs::prelude::*,
    MinimalPlugins,
};

use serde::{Deserialize, Serialize};

use bevy_networking_turbulence::{
    ConnectionChannelsBuilder, MessageChannelMode, MessageChannelSettings,
    NetworkResource, NetworkingPlugin, MessageFlushingStrategy,
};

use std::{net::SocketAddr, time::Duration};

mod utils;
use utils::*;

const SERVER_PORT: u16 = 14191;
const NUM_PINGS: usize = 100;

#[derive(Debug, Default)]
struct PingPongCounter {
    pings_sent: usize,
    pings_seen: usize,
    pongs_sent: usize,
    pongs_seen: usize,
}

type TTL = Option<f64>;

type Ticks = usize;

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

    let args = parse_args();
    let mut net_plugin = NetworkingPlugin::default();
    if args.manual_flush {
        net_plugin.message_flushing_strategy = MessageFlushingStrategy::Never;
    }

    let mut app = App::build();
    app
        // minimal plugins necessary for timers + headless loop
        .insert_resource(ScheduleRunnerSettings::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource::<Ticks>(0)
        .insert_resource::<TTL>(None)
        .insert_resource(PingPongCounter::default())
        .add_plugins(MinimalPlugins)
        // The NetworkingPlugin
        .add_plugin(net_plugin)
        // Our networking
        .insert_resource(args)
        .add_startup_system(startup.system())
        .add_startup_system(setup_channels.system())
        .add_system(tick.system())
        .add_system(send_messages.system())
        .add_system(handle_messages.system())
        .add_system(ttl_system.system())
        ;
    if parse_args().manual_flush {
        app.add_system_to_stage(CoreStage::PostUpdate, flush_channels.system());
    }
    app.run();
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum NetMsg {
    Ping(usize),
    Pong(usize),
}

const NETMSG_SETTINGS: MessageChannelSettings = MessageChannelSettings {
    channel: 0,
    channel_mode: MessageChannelMode::Unreliable,   
    // The buffer size for the mpsc channel of messages that transports messages of this type to /
    // from the network task.
    message_buffer_size: NUM_PINGS,
    // The buffer size for the mpsc channel of packets for this message type that transports
    // packets to / from the packet multiplexer.
    packet_buffer_size: 10,
};

fn setup_channels(mut net: ResMut<NetworkResource>) {
    net.set_channels_builder(|builder: &mut ConnectionChannelsBuilder| {
        builder.register::<NetMsg>(NETMSG_SETTINGS).unwrap();
    });
}

fn tick(mut ticks: ResMut<Ticks>) {
    *ticks += 1;
}

fn flush_channels(mut net: ResMut<NetworkResource>) {
    for (_handle, connection) in net.connections.iter_mut() {
        let channels = connection.channels().unwrap();
        channels.flush::<NetMsg>();
    }
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
        log::info!("Starting server");
        net.listen(server_address, None, None);
    }
    if !args.is_server {
        log::info!("Starting client");
        net.connect(server_address);
    }
}

fn ttl_system(mut ttl: ResMut<TTL>, mut exit: EventWriter<AppExit>, time: Res<Time>, net: Res<NetworkResource>, ppc: Res<PingPongCounter>, args: Res<Args>) {
    match *ttl {
        None => {},
        Some(secs) => {
            let new_secs = secs - time.delta_seconds_f64();
            if new_secs <= 0.0 {
                // dump some stats and exit
                log::info!("Final stats, is_server: {:?}, flushing mode: {}", args.is_server, 
                    if args.manual_flush {"--manual-flush"} else {"--auto-flush"});
                log::info!("{:?}", *ppc);
                for (handle, connection) in net.connections.iter() {
                    log::info!("{:?} [h:{}]", connection.stats(), handle);
                }
                log::info!("Exiting.");
                exit.send(AppExit);
                return;
            } else {
                *ttl = Some(new_secs);
            }
        }
    }
}

fn send_messages(
    mut net: ResMut<NetworkResource>, 
    mut ppc: ResMut<PingPongCounter>, 
    args: Res<Args>, 
    mut ttl: ResMut<TTL>,
    ticks: Res<Ticks>,
) {
    if args.is_server {
        // client sends pings, server replies with pongs in handle_messages.
        return;
    }
    // send 10 pings per tick
    for _ in 0..10 {
        if ppc.pings_sent < NUM_PINGS {
            ppc.pings_sent += 1;
            let msg = NetMsg::Ping(ppc.pings_sent);
            log::info!("[t:{}] Sending ping {}", *ticks, ppc.pings_sent);
            net.broadcast_message(msg);
        } else if ppc.pings_sent == NUM_PINGS && ttl.is_none() {
            // shutdown after short delay, to finish receiving in-flight pongs
            *ttl = Some(1.0);
            return;
        }
    }
}

fn handle_messages(
    mut net: ResMut<NetworkResource>,
    mut ppc: ResMut<PingPongCounter>,
    mut ttl: ResMut<TTL>,
    ticks: Res<Ticks>,
) {
    let mut to_send = Vec::new();
    for (handle, connection) in net.connections.iter_mut() {
        let channels = connection.channels().unwrap();
        while let Some(netmsg) = channels.recv::<NetMsg>() {
            match netmsg {
                NetMsg::Ping(i) => {
                    ppc.pings_seen += 1;
                    log::info!("[t:{}] Sending pong {}", *ticks, i);
                    to_send.push((*handle, NetMsg::Pong(i)));
                    // seen our first ping, so schedule a shutdown
                    if ttl.is_none() {
                        *ttl = Some(3.0);
                    }
                },
                NetMsg::Pong(i) => {
                    ppc.pongs_seen += 1;
                    log::info!("[t:{}] Got pong {}", *ticks, i);
                }
            }
        }
    }
    for (handle, msg) in to_send {
        ppc.pongs_sent += 1;
        net.send_message(handle, msg).unwrap();
    }
}

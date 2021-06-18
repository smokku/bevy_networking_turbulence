/*
- every frame server broadcasts position of all balls
  via unreliable channel server->client
- client sends movement commands to server
  via reliable channel client->server
*/

use bevy::{
    app::{ScheduleRunnerPlugin, ScheduleRunnerSettings},
    prelude::*,
    render::camera::WindowOrigin
};
use bevy_networking_turbulence::{
    ConnectionChannelsBuilder, MessageChannelMode, MessageChannelSettings, NetworkEvent,
    NetworkResource, NetworkingPlugin, ReliableChannelSettings, MessageFlushingStrategy,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, time::Duration};

mod utils;
use utils::*;

const SERVER_PORT: u16 = 14192;
const BOARD_WIDTH: u32 = 1000;
const BOARD_HEIGHT: u32 = 1000;

fn main() {
    simple_logger::SimpleLogger::from_env()
        .init()
        .expect("A logger was already initialized");

    App::build().add_plugin(BallsExample).run();
}

struct Pawn {
    controller: u32,
}
struct Ball {
    velocity: Vec3,
}

struct BallsExample;

impl Plugin for BallsExample {
    fn build(&self, app: &mut AppBuilder) {
        let args = parse_args();
        if args.is_server {
            // Server
            app.insert_resource(ScheduleRunnerSettings::run_loop(Duration::from_secs_f64(
                1.0 / 60.0,
            )))
            .add_plugins(MinimalPlugins)
            .add_plugin(ScheduleRunnerPlugin::default())
            .add_startup_system(server_setup.system())
            .add_system(ball_movement_system.system())
            .insert_resource(NetworkBroadcast { frame: 0 })
            .add_system_to_stage(CoreStage::PreUpdate, handle_messages_server.system())
            .add_system_to_stage(CoreStage::PostUpdate, network_broadcast_system.system())
        } else {
            // Client
            app.insert_resource(WindowDescriptor {
                width: BOARD_WIDTH as f32,
                height: BOARD_HEIGHT as f32,
                ..Default::default()
            })
            .add_plugins(DefaultPlugins)
            .insert_resource(ClearColor(Color::rgb(0.3, 0.3, 0.3)))
            .add_startup_system(client_setup.system())
            .add_system_to_stage(CoreStage::PreUpdate, handle_messages_client.system())
            .insert_resource(ServerIds::default())
            .add_system(ball_control_system.system())
        }
        .insert_resource(args)
        // disable auto flushing of messages on send - we send from our own flush_channels system at the
        // end of each tick.
        .add_plugin(NetworkingPlugin{message_flushing_strategy: MessageFlushingStrategy::Never, ..Default::default()})
        .add_startup_system(network_setup.system())
        .add_system(handle_packets.system())
        .add_system_to_stage(CoreStage::PostUpdate, flush_channels.system());
    }
}

fn ball_movement_system(time: Res<Time>, mut ball_query: Query<(&Ball, &mut Transform)>) {
    for (ball, mut transform) in ball_query.iter_mut() {
        let mut translation = transform.translation + (ball.velocity * time.delta_seconds());
        let mut x = translation.x as i32 % BOARD_WIDTH as i32;
        let mut y = translation.y as i32 % BOARD_HEIGHT as i32;
        if x < 0 {
            x += BOARD_WIDTH as i32;
        }
        if y < 0 {
            y += BOARD_HEIGHT as i32;
        }
        translation.x = x as f32;
        translation.y = y as f32;
        transform.translation = translation;
    }
}

fn ball_control_system(mut net: ResMut<NetworkResource>, keyboard_input: Res<Input<KeyCode>>) {
    if keyboard_input.pressed(KeyCode::Left) {
        net.broadcast_message(ClientMessage::Direction(Direction::Left));
    }

    if keyboard_input.pressed(KeyCode::Right) {
        net.broadcast_message(ClientMessage::Direction(Direction::Right));
    }
}

fn server_setup(mut net: ResMut<NetworkResource>) {
    let ip_address =
        bevy_networking_turbulence::find_my_ip_address().expect("can't find ip address");
    let socket_address = SocketAddr::new(ip_address, SERVER_PORT);
    log::info!("Starting server");
    net.listen(socket_address, None, None);
}

fn client_setup(mut commands: Commands, mut net: ResMut<NetworkResource>) {
    let mut camera = OrthographicCameraBundle::new_2d();
    camera.orthographic_projection.window_origin = WindowOrigin::BottomLeft;
    commands.spawn_bundle(camera);

    let ip_address =
        bevy_networking_turbulence::find_my_ip_address().expect("can't find ip address");
    let socket_address = SocketAddr::new(ip_address, SERVER_PORT);
    log::info!("Starting client");
    net.connect(socket_address);
}

fn flush_channels(mut net: ResMut<NetworkResource>) {
    for (_handle, connection) in net.connections.iter_mut() {
        let channels = connection.channels().unwrap();
        channels.flush::<ClientMessage>();
        channels.flush::<GameStateMessage>();
    }
}

fn network_setup(mut net: ResMut<NetworkResource>) {
    net.set_channels_builder(|builder: &mut ConnectionChannelsBuilder| {
        builder
            .register::<ClientMessage>(CLIENT_STATE_MESSAGE_SETTINGS)
            .unwrap();
        builder
            .register::<GameStateMessage>(GAME_STATE_MESSAGE_SETTINGS)
            .unwrap();
    });
}

#[derive(Default)]
struct NetworkBroadcast {
    frame: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum Direction {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum ClientMessage {
    Hello(String),
    Direction(Direction),
}

const CLIENT_STATE_MESSAGE_SETTINGS: MessageChannelSettings = MessageChannelSettings {
    channel: 0,
    channel_mode: MessageChannelMode::Reliable {
        reliability_settings: ReliableChannelSettings {
            bandwidth: 4096,
            recv_window_size: 1024,
            send_window_size: 1024,
            burst_bandwidth: 1024,
            init_send: 512,
            wakeup_time: Duration::from_millis(100),
            initial_rtt: Duration::from_millis(200),
            max_rtt: Duration::from_secs(2),
            rtt_update_factor: 0.1,
            rtt_resend_factor: 1.5,
        },
        max_message_len: 1024,
    },
    message_buffer_size: 8,
    packet_buffer_size: 8,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct GameStateMessage {
    frame: u32,
    balls: Vec<(u32, Vec3, Vec3)>,
}

const GAME_STATE_MESSAGE_SETTINGS: MessageChannelSettings = MessageChannelSettings {
    channel: 1,
    channel_mode: MessageChannelMode::Unreliable,
    message_buffer_size: 8,
    packet_buffer_size: 8,
};

fn network_broadcast_system(
    mut state: ResMut<NetworkBroadcast>,
    mut net: ResMut<NetworkResource>,
    ball_query: Query<(Entity, &Ball, &Transform)>,
) {
    let mut message = GameStateMessage {
        frame: state.frame,
        balls: Vec::new(),
    };
    state.frame += 1;

    for (entity, ball, transform) in ball_query.iter() {
        message
            .balls
            .push((entity.id(), ball.velocity, transform.translation));
    }

    net.broadcast_message(message);
}

fn handle_packets(
    mut commands: Commands,
    mut net: ResMut<NetworkResource>,
    args: Res<Args>,
    mut network_events: EventReader<NetworkEvent>,
) {
    for event in network_events.iter() {
        match event {
            NetworkEvent::Connected(handle) => match net.connections.get_mut(handle) {
                Some(connection) => {
                    match connection.remote_address() {
                        Some(remote_address) => {
                            log::debug!(
                                "Incoming connection on [{}] from [{}]",
                                handle,
                                remote_address
                            );

                            // New client connected - spawn a ball
                            let mut rng = rand::thread_rng();
                            let vel_x = rng.gen_range(-0.5, 0.5);
                            let vel_y = rng.gen_range(-0.5, 0.5);
                            let pos_x = rng.gen_range(0, BOARD_WIDTH) as f32;
                            let pos_y = rng.gen_range(0, BOARD_HEIGHT) as f32;
                            log::info!("Spawning {}x{} {}/{}", pos_x, pos_y, vel_x, vel_y);
                            commands.spawn_bundle((
                                Ball {
                                    velocity: 400.0 * Vec3::new(vel_x, vel_y, 0.0).normalize(),
                                },
                                Pawn {
                                    controller: *handle,
                                },
                                Transform::from_translation(Vec3::new(pos_x, pos_y, 1.0)),
                            ));
                        }
                        None => {
                            log::debug!("Connected on [{}]", handle);
                        }
                    }

                    if !args.is_server {
                        log::debug!("Sending Hello on [{}]", handle);
                        match net.send_message(*handle, ClientMessage::Hello("test".to_string())) {
                            Ok(msg) => match msg {
                                Some(msg) => {
                                    log::error!("Unable to send Hello: {:?}", msg);
                                }
                                None => {}
                            },
                            Err(err) => {
                                log::error!("Unable to send Hello: {:?}", err);
                            }
                        };
                    }
                }
                None => panic!("Got packet for non-existing connection [{}]", handle),
            },
            _ => {}
        }
    }
}

fn handle_messages_server(mut net: ResMut<NetworkResource>, mut balls: Query<(&mut Ball, &Pawn)>) {
    for (handle, connection) in net.connections.iter_mut() {
        let channels = connection.channels().unwrap();
        while let Some(client_message) = channels.recv::<ClientMessage>() {
            log::debug!(
                "ClientMessage received on [{}]: {:?}",
                handle,
                client_message
            );
            match client_message {
                ClientMessage::Hello(id) => {
                    log::info!("Client [{}] connected on [{}]", id, handle);
                    // TODO: store client id?
                }
                ClientMessage::Direction(dir) => {
                    let mut angle: f32 = 0.03;
                    if dir == Direction::Right {
                        angle *= -1.0;
                    }
                    for (mut ball, pawn) in balls.iter_mut() {
                        if pawn.controller == *handle {
                            ball.velocity = Quat::from_rotation_z(angle) * ball.velocity;
                        }
                    }
                }
            }
        }

        while let Some(_state_message) = channels.recv::<GameStateMessage>() {
            log::error!("GameStateMessage received on [{}]", handle);
        }
    }
}

type ServerIds = HashMap<u32, (u32, u32)>;

fn handle_messages_client(
    mut commands: Commands,
    mut net: ResMut<NetworkResource>,
    mut server_ids: ResMut<ServerIds>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut balls: Query<(Entity, &mut Ball, &mut Transform)>,
) {
    for (handle, connection) in net.connections.iter_mut() {
        let channels = connection.channels().unwrap();
        while let Some(_client_message) = channels.recv::<ClientMessage>() {
            log::error!("ClientMessage received on [{}]", handle);
        }

        // it is possible that many state updates came at the same time - spawn once
        let mut to_spawn: HashMap<u32, (u32, Vec3, Vec3)> = HashMap::new();

        while let Some(mut state_message) = channels.recv::<GameStateMessage>() {
            let message_frame = state_message.frame;
            log::info!(
                "GameStateMessage received on [{}]: {:?}",
                handle,
                state_message
            );

            // update all balls
            for (entity, mut ball, mut transform) in balls.iter_mut() {
                let server_id_entry = server_ids.get_mut(&entity.id()).unwrap();
                let (server_id, update_frame) = *server_id_entry;

                if let Some(index) = state_message
                    .balls
                    .iter()
                    .position(|&update| update.0 == server_id)
                {
                    let (_id, velocity, translation) = state_message.balls.remove(index);

                    if update_frame > message_frame {
                        continue;
                    }
                    server_id_entry.1 = message_frame;

                    ball.velocity = velocity;
                    transform.translation = translation;
                } else {
                    // TODO: despawn disconnected balls
                }
            }
            // create new balls
            for (id, velocity, translation) in state_message.balls.drain(..) {
                if let Some((frame, _velocity, _translation)) = to_spawn.get(&id) {
                    if *frame > message_frame {
                        continue;
                    }
                };
                to_spawn.insert(id, (message_frame, velocity, translation));
            }
        }

        for (id, (frame, velocity, translation)) in to_spawn.iter() {
            log::info!("Spawning {} @{}", id, frame);
            let entity = commands
                .spawn_bundle(SpriteBundle {
                    material: materials.add(
                        Color::rgb(0.8 - (*id as f32 / 5.0), 0.2, 0.2 + (*id as f32 / 5.0)).into(),
                    ),
                    transform: Transform::from_translation(*translation),
                    sprite: Sprite::new(Vec2::new(30.0, 30.0)),
                    ..Default::default()
                })
                .insert(Ball {
                    velocity: *velocity,
                })
                .insert(Pawn { controller: *id })
                .id();
            server_ids.insert(entity.id(), (*id, *frame));
        }
    }
}

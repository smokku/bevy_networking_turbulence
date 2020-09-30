/*
- every frame server broadcasts position of all balls
  via unreliable channel server->client
- client sends movement commands to server
  via reliable channel client->server
*/

use bevy::{
    app::{stage, App, EventReader, Events, ScheduleRunnerPlugin},
    core::CorePlugin,
    prelude::*,
    render::pass::ClearColor,
    sprite::collide_aabb::{collide, Collision},
    type_registry::TypeRegistryPlugin,
};
use bevy_networking_turbulence::{NetworkResource, NetworkingPlugin, Packet};
use std::{net::SocketAddr, time::Duration};

mod utils;
use utils::*;

const SERVER_PORT: u16 = 14192;
const BOARD_WIDTH: u32 = 1000;
const BOARD_HEIGHT: u32 = 1000;

fn main() {
    App::build().add_plugin(Example).run();
}

struct Pawn {
    controller: u32,
}

struct Ball {
    velocity: Vec3,
}

struct Example;

impl Plugin for Example {
    fn build(&self, app: &mut AppBuilder) {
        let args = parse_args();
        if args.is_server {
            // Server
            app.add_plugin(TypeRegistryPlugin::default())
                .add_plugin(CorePlugin)
                .add_plugin(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                    1.0 / 60.0,
                )))
                .add_startup_system(server_setup.system())
                .add_system(ball_movement_system.system())
                .add_resource(NetworkBroadcast { frame: 0 })
                .add_system_to_stage(stage::POST_UPDATE, network_broadcast_system.system())
        } else {
            // Client
            app.add_resource(WindowDescriptor {
                width: BOARD_WIDTH,
                height: BOARD_HEIGHT,
                ..Default::default()
            })
            .add_default_plugins()
            .add_resource(ClearColor(Color::rgb(0.3, 0.3, 0.3)))
            .add_startup_system(client_setup.system())
        }
        .add_resource(args)
        .add_plugin(NetworkingPlugin);
    }
}

fn server_setup(mut commands: Commands, mut net: ResMut<NetworkResource>) {
    commands
        .spawn((Pawn { controller: 1 },))
        .with(Ball {
            velocity: 400.0 * Vec3::new(0.5, -0.5, 0.0).normalize(),
        })
        .with(Transform::from_translation(Vec3::new(0.0, -50.0, 1.0)));

    // TODO: net. register BallTransformMessage
}

fn client_setup(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.spawn(Camera2dComponents::default());
    // .spawn(SpriteComponents {
    //     material: materials.add(Color::rgb(0.8, 0.2, 0.2).into()),
    //     transform: Transform::from_translation(Vec3::new(0.0, -50.0, 1.0)),
    //     sprite: Sprite::new(Vec2::new(30.0, 30.0)),
    //     ..Default::default()
    // })
    // .with(Ball {
    //     velocity: 400.0 * Vec3::new(0.5, -0.5, 0.0).normalize(),
    // });
}

fn ball_movement_system(time: Res<Time>, mut ball_query: Query<(&Ball, &mut Transform)>) {
    // clamp the timestep to stop the ball from escaping when the game starts
    let delta_seconds = f32::min(0.2, time.delta_seconds);

    for (ball, mut transform) in &mut ball_query.iter() {
        transform.translate(ball.velocity * delta_seconds);
    }
}

struct NetworkBroadcast {
    frame: u32,
}

fn network_broadcast_system(
    mut state: ResMut<NetworkBroadcast>,
    mut ball_query: Query<(&Ball, &mut Transform)>,
) {
    for (ball, mut transform) in &mut ball_query.iter() {
        // TODO: build BallTransformMessage
    }
}

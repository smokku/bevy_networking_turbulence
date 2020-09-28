use bevy_app::{AppBuilder, Events, Plugin};
use bevy_ecs::prelude::*;
use bevy_tasks::{IoTaskPool, TaskPool};

use async_compat::Compat;
use std::net::SocketAddr;

use naia_server_socket::{
    LinkConditionerConfig, NaiaServerSocketError, Packet, ServerSocket, ServerSocketTrait,
};

pub struct NetworkingPlugin;

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut AppBuilder) {
        let task_pool = app
            .resources()
            .get::<IoTaskPool>()
            .expect("IoTaskPool resource not found")
            .0
            .clone();

        app.add_resource(NetworkResource::new(task_pool))
            .add_event::<Packet>()
            .add_system(receive_packets.system());
    }
}

pub struct NetworkResource {
    task_pool: TaskPool,
    server_socket: Option<Box<dyn ServerSocketTrait>>,
}

impl NetworkResource {
    fn new(task_pool: TaskPool) -> Self {
        NetworkResource {
            task_pool: task_pool,
            server_socket: None,
        }
    }
    pub fn listen(&mut self, socket_address: SocketAddr) {
        let server_socket = smol::block_on(Compat::new(ServerSocket::listen(socket_address)));
        self.server_socket =
            Some(server_socket.with_link_conditioner(&LinkConditionerConfig::good_condition()));
    }
}

fn receive_packets(mut net: ResMut<NetworkResource>, mut packet_events: ResMut<Events<Packet>>) {
    if let Some(server_socket) = &net.server_socket {
        // if net.receive_future.is_none() {
        //     let fut = server_socket.receive();
        //     net.receive_future = Some(fut);
        // };
        // match (&mut net.receive_future.unwrap()).now_or_never() {
        //     None => {}
        // }

        // packet_events.send(packet);
    }
}

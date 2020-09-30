use bevy::{
    app::{AppBuilder, Events, Plugin},
    ecs::prelude::*,
    tasks::{IoTaskPool, TaskPool},
};

#[cfg(not(target_arch = "wasm32"))]
use async_compat::Compat;
#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::net::SocketAddr;

use naia_client_socket::{ClientSocket, LinkConditionerConfig};
#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{Packet as ServerPacket, ServerSocket};

#[cfg(not(target_arch = "wasm32"))]
pub use naia_server_socket::find_my_ip_address;

mod channels;
mod transport;
pub use transport::Packet;

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
            .add_event::<transport::Packet>()
            .add_system(receive_packets.system());
    }
}

#[allow(dead_code)]
pub struct NetworkResource {
    task_pool: TaskPool,
    pub connections: Vec<Box<dyn transport::Connection>>,
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for NetworkResource {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for NetworkResource {}

impl NetworkResource {
    fn new(task_pool: TaskPool) -> Self {
        NetworkResource {
            task_pool,
            connections: Vec::new(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn listen(&mut self, socket_address: SocketAddr) {
        let (packet_tx, packet_rx): (Sender<ServerPacket>, Receiver<ServerPacket>) = unbounded();

        let mut server_socket =
            futures_lite::future::block_on(Compat::new(ServerSocket::listen(socket_address)))
                .with_link_conditioner(&LinkConditionerConfig::good_condition());
        let sender = server_socket.get_sender();

        let receiver_task = self.task_pool.spawn(Compat::new(async move {
            loop {
                match server_socket.receive().await {
                    Ok(packet) => {
                        let address = packet.address();
                        let message = String::from_utf8_lossy(packet.payload());
                        log::info!("Server recv <- {}: {}", address, message);
                        match packet_tx.send(packet) {
                            Ok(()) => {}
                            Err(error) => {
                                log::info!("Server Send Error: {}", error);
                            }
                        }
                    }
                    Err(error) => {
                        log::info!("Server Receive Error: {}", error);
                    }
                }
            }
        }));

        self.connections
            .push(Box::new(transport::ServerConnection::new(
                receiver_task,
                packet_rx,
                sender,
            )));
    }

    pub fn connect(&mut self, socket_address: SocketAddr) {
        let mut client_socket = ClientSocket::connect(socket_address)
            .with_link_conditioner(&LinkConditionerConfig::good_condition());
        let sender = client_socket.get_sender();

        self.connections
            .push(Box::new(transport::ClientConnection::new(
                client_socket,
                sender,
            )));
    }
}

pub fn receive_packets(
    mut net: ResMut<NetworkResource>,
    mut packet_events: ResMut<Events<transport::Packet>>,
) {
    for connection in net.connections.iter_mut() {
        while let Some(result) = connection.receive() {
            match result {
                Ok(packet) => {
                    let message = String::from_utf8_lossy(packet.payload());
                    log::info!("Received: {}", message);
                    packet_events.send(packet);
                }
                Err(error) => {
                    log::info!("Receive Error: {}", error);
                    // FIXME:error_events.send(error);
                }
            }
        }
    }
}

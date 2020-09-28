use bevy::{
    app::{AppBuilder, Events, Plugin},
    ecs::prelude::*,
    tasks::{IoTaskPool, Task, TaskPool},
};

use async_compat::Compat;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::net::SocketAddr;

use naia_server_socket::{LinkConditionerConfig, Packet, ServerSocket};

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
    servers: Vec<NetworkServer>,
}

#[allow(dead_code)]
struct NetworkServer {
    receiver_task: Task<()>,
    packet_rx: Receiver<Packet>,
}

impl NetworkResource {
    fn new(task_pool: TaskPool) -> Self {
        NetworkResource {
            task_pool,
            servers: Vec::new(),
        }
    }

    pub fn listen(&mut self, socket_address: SocketAddr) {
        let (packet_tx, packet_rx): (Sender<Packet>, Receiver<Packet>) = unbounded();

        let receiver_task = self.task_pool.spawn(Compat::new(async move {
            let mut server_socket = ServerSocket::listen(socket_address)
                .await
                .with_link_conditioner(&LinkConditionerConfig::good_condition());

            loop {
                log::info!("Server loop");
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

        self.servers.push(NetworkServer {
            receiver_task,
            packet_rx,
        });
    }
}

fn receive_packets(net: ResMut<NetworkResource>, mut packet_events: ResMut<Events<Packet>>) {
    for server in net.servers.iter() {
        while let Ok(packet) = server.packet_rx.try_recv() {
            packet_events.send(packet);
        }
    }
}

use bevy::{
    app::{AppBuilder, Events, Plugin},
    ecs::prelude::*,
    tasks::{IoTaskPool, Task, TaskPool},
};

use async_compat::Compat;
use crossbeam_channel::{unbounded, Receiver, Sender};
use futures_lite::future;
use std::net::SocketAddr;

use naia_client_socket::{
    ClientSocket, ClientSocketTrait, MessageSender as ClientSender, Packet as ClientPacket,
};
use naia_server_socket::{
    LinkConditionerConfig, MessageSender as ServerSender, Packet as ServerPacket, ServerSocket,
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
            .add_event::<ServerPacket>()
            .add_event::<ClientPacket>()
            .add_system(receive_packets.system());
    }
}

pub struct NetworkResource {
    task_pool: TaskPool,
    pub servers: Vec<NetworkServer>,
    pub clients: Vec<NetworkClient>,
}

#[allow(dead_code)]
pub struct NetworkServer {
    receiver_task: Task<()>,
    packet_rx: Receiver<ServerPacket>,
    pub sender: ServerSender,
}

#[allow(dead_code)]
pub struct NetworkClient {
    socket: Box<dyn ClientSocketTrait>,
    pub sender: ClientSender,
}

impl NetworkResource {
    fn new(task_pool: TaskPool) -> Self {
        NetworkResource {
            task_pool,
            servers: Vec::new(),
            clients: Vec::new(),
        }
    }

    pub fn listen(&mut self, socket_address: SocketAddr) {
        let (packet_tx, packet_rx): (Sender<ServerPacket>, Receiver<ServerPacket>) = unbounded();

        let mut server_socket = future::block_on(Compat::new(ServerSocket::listen(socket_address)))
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

        self.servers.push(NetworkServer {
            receiver_task,
            packet_rx,
            sender,
        });
    }

    pub fn connect(&mut self, socket_address: SocketAddr) {
        let mut client_socket = ClientSocket::connect(socket_address)
            // .with_link_conditioner(&LinkConditionerConfig::good_condition())
            ;
        let mut message_sender = client_socket.get_sender();
        log::info!("Connect send");
        message_sender
            .send(ClientPacket::new("ping".to_string().into_bytes()))
            .unwrap();
        let sender = client_socket.get_sender();

        self.clients.push(NetworkClient {
            socket: client_socket,
            sender,
        })
    }
}

fn receive_packets(
    mut net: ResMut<NetworkResource>,
    mut server_packet_events: ResMut<Events<ServerPacket>>,
    mut client_packet_events: ResMut<Events<ClientPacket>>,
) {
    for server in net.servers.iter() {
        while let Ok(packet) = server.packet_rx.try_recv() {
            server_packet_events.send(packet);
        }
    }
    for client in net.clients.iter_mut() {
        match client.socket.receive() {
            Ok(event) => {
                match event {
                    Some(packet) => {
                        let message = String::from_utf8_lossy(packet.payload());
                        log::info!("Client recv: {}", message);
                        client_packet_events.send(packet);
                    }
                    None => {
                        //log::info!("Client non-event");
                    }
                }
            }
            Err(err) => {
                log::info!("Client Error: {}", err);
            }
        }
    }
}

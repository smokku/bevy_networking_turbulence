use bevy::{
    app::{AppBuilder, Events, Plugin},
    ecs::prelude::*,
    tasks::{IoTaskPool, TaskPool},
};

#[cfg(not(target_arch = "wasm32"))]
use async_compat::Compat;
#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::{
    collections::HashMap,
    error::Error,
    fmt::Debug,
    net::SocketAddr,
    sync::{atomic, Arc, Mutex, RwLock},
};

use naia_client_socket::{ClientSocket, LinkConditionerConfig};
#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{MessageSender as ServerSender, Packet as ServerPacket, ServerSocket};

#[cfg(not(target_arch = "wasm32"))]
pub use naia_server_socket::find_my_ip_address;

pub use turbulence::message_channels::{MessageChannelMode, MessageChannelSettings};
use turbulence::{
    buffer::BufferPacketPool,
    message_channels::{
        ChannelAlreadyRegistered, ChannelMessage, MessageChannels, MessageChannelsBuilder,
    },
    packet::PacketPool,
    packet_multiplexer::{MuxPacket, MuxPacketPool, PacketMultiplexer},
    reliable_channel,
};

mod transport;
pub use transport::{Connection, Packet};

pub type ConnectionHandle = u32;

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
            .add_event::<NetworkEvent>()
            .add_system(receive_packets.system());
    }
}

pub struct NetworkResource {
    task_pool: TaskPool,

    pending_connections: Arc<Mutex<Vec<Box<dyn Connection>>>>,
    connection_sequence: atomic::AtomicU32,
    pub connections: HashMap<ConnectionHandle, Box<dyn Connection>>,

    #[cfg(not(target_arch = "wasm32"))]
    listeners: Vec<ServerListener>,
    #[cfg(not(target_arch = "wasm32"))]
    server_channels: Arc<RwLock<HashMap<SocketAddr, Sender<Packet>>>>,
}

struct ServerListener {
    receiver_task: bevy::tasks::Task<()>,
    sender: ServerSender,
    socket_address: SocketAddr,
}

#[derive(Debug)]
pub enum NetworkEvent {
    Connected(ConnectionHandle),
    Disconnected(ConnectionHandle),
    Packet(ConnectionHandle, Packet),
    // SendError(NetworkError),
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for NetworkResource {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for NetworkResource {}

impl NetworkResource {
    fn new(task_pool: TaskPool) -> Self {
        NetworkResource {
            task_pool,
            connections: HashMap::new(),
            connection_sequence: atomic::AtomicU32::new(0),
            pending_connections: Arc::new(Mutex::new(Vec::new())),
            listeners: Vec::new(),
            server_channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn listen(&mut self, socket_address: SocketAddr) {
        let (packet_tx, packet_rx): (Sender<ServerPacket>, Receiver<ServerPacket>) = unbounded();

        let mut server_socket =
            futures_lite::future::block_on(Compat::new(ServerSocket::listen(socket_address)))
                .with_link_conditioner(&LinkConditionerConfig::good_condition());
        let sender = server_socket.get_sender();
        let server_channels = self.server_channels.clone();
        let pending_connections = self.pending_connections.clone();

        let receiver_task = self.task_pool.spawn(Compat::new(async move {
            loop {
                match server_socket.receive().await {
                    Ok(packet) => {
                        let address = packet.address();
                        let message = String::from_utf8_lossy(packet.payload());
                        log::debug!("Server recv <- {}: {}", address, message);

                        match server_channels.write() {
                            Ok(mut server_channels) => {
                                if !server_channels.contains_key(&address) {
                                    let (packet_tx, packet_rx): (Sender<Packet>, Receiver<Packet>) =
                                        unbounded();
                                    pending_connections.lock().unwrap().push(Box::new(
                                        transport::ServerConnection::new(
                                            packet_rx,
                                            server_socket.get_sender(),
                                            address,
                                        ),
                                    ));
                                    server_channels.insert(address, packet_tx);
                                }
                            }
                            Err(err) => {
                                log::error!("Error locking server channels: {}", err);
                            }
                        }

                        match server_channels
                            .read()
                            .unwrap()
                            .get(&address)
                            .unwrap()
                            .send(Packet::copy_from_slice(packet.payload()))
                        {
                            Ok(()) => {}
                            Err(error) => {
                                log::error!("Server Send Error: {}", error);
                            }
                        }
                    }
                    Err(error) => {
                        log::error!("Server Receive Error: {}", error);
                    }
                }
            }
        }));

        self.listeners.push(ServerListener {
                receiver_task,
                sender,
            socket_address,
        });
    }

    pub fn connect(&mut self, socket_address: SocketAddr) {
        let mut client_socket = ClientSocket::connect(socket_address)
            .with_link_conditioner(&LinkConditionerConfig::good_condition());
        let sender = client_socket.get_sender();

        self.pending_connections
            .lock()
            .unwrap()
            .push(Box::new(transport::ClientConnection::new(
                client_socket,
                sender,
            )));
    }

    pub fn send(
        &mut self,
        handle: ConnectionHandle,
        payload: Packet,
    ) -> Result<(), Box<dyn Error + Send>> {
        match self.connections.get_mut(&handle) {
            Some(connection) => connection.send(payload),
            None => Err(Box::new(std::io::Error::new(
                // FIXME: move to enum Error
                std::io::ErrorKind::NotFound,
                "No such connection",
            ))),
        }
}

pub fn receive_packets(
    mut net: ResMut<NetworkResource>,
    mut network_events: ResMut<Events<NetworkEvent>>,
) {
    let connections: Vec<Box<dyn Connection>> =
        net.pending_connections.lock().unwrap().drain(..).collect();
    for connection in connections {
        let handle: ConnectionHandle = net
            .connection_sequence
            .fetch_add(1, atomic::Ordering::Relaxed);
        net.connections.insert(handle, connection);
        network_events.send(NetworkEvent::Connected(handle));
    }

    for (handle, connection) in net.connections.iter_mut() {
        while let Some(result) = connection.receive() {
            match result {
                Ok(packet) => {
                    let message = String::from_utf8_lossy(&packet);
                    log::debug!("Received on [{}] RAW: {}", handle, message);
                    network_events.send(NetworkEvent::Packet(*handle, packet));
                }
                Err(error) => {
                    log::error!("Receive Error: {}", error);
                    // FIXME:error_events.send(error);
                }
            }
        }
    }
}

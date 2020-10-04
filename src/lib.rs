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

use turbulence::{
    buffer::{BufferPacket, BufferPacketPool},
    message_channels::{
        ChannelAlreadyRegistered, ChannelMessage, MessageChannels, MessageChannelsBuilder,
    },
    packet::{Packet as PoolPacket, PacketPool, MAX_PACKET_LEN},
    packet_multiplexer::{MuxPacket, MuxPacketPool, PacketMultiplexer},
    reliable_channel,
};
pub use turbulence::{
    message_channels::{MessageChannelMode, MessageChannelSettings},
    reliable_channel::Settings as ReliableChannelSettings,
};

mod channels;
mod transport;
use self::channels::{SimpleBufferPool, TaskPoolRuntime};
pub use transport::{Connection, ConnectionChannelsBuilder, Packet};

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

    runtime: TaskPoolRuntime,
    packet_pool: MuxPacketPool<BufferPacketPool<SimpleBufferPool>>,
    channels_builder_fn: Option<Box<dyn Fn(&mut ConnectionChannelsBuilder) + Send + Sync>>,
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
    // Error(NetworkError),
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for NetworkResource {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for NetworkResource {}

impl NetworkResource {
    fn new(task_pool: TaskPool) -> Self {
        let runtime = TaskPoolRuntime::new(task_pool.clone());
        let packet_pool =
            MuxPacketPool::new(BufferPacketPool::new(SimpleBufferPool(MAX_PACKET_LEN)));

        NetworkResource {
            task_pool,
            connections: HashMap::new(),
            connection_sequence: atomic::AtomicU32::new(0),
            pending_connections: Arc::new(Mutex::new(Vec::new())),
            listeners: Vec::new(),
            server_channels: Arc::new(RwLock::new(HashMap::new())),
            runtime,
            packet_pool,
            channels_builder_fn: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn listen(&mut self, socket_address: SocketAddr) {
        let mut server_socket =
            futures_lite::future::block_on(Compat::new(ServerSocket::listen(socket_address)))
                .with_link_conditioner(&LinkConditionerConfig::good_condition());
        let sender = server_socket.get_sender();
        let server_channels = self.server_channels.clone();
        let pending_connections = self.pending_connections.clone();
        let task_pool = self.task_pool.clone();

        let receiver_task = self.task_pool.spawn(Compat::new(async move {
            loop {
                match server_socket.receive().await {
                    Ok(packet) => {
                        let address = packet.address();
                        let message = String::from_utf8_lossy(packet.payload());
                        log::debug!(
                            "Server recv <- {}:{}: {}",
                            address,
                            packet.payload().len(),
                            message
                        );

                        match server_channels.write() {
                            Ok(mut server_channels) => {
                                if !server_channels.contains_key(&address) {
                                    let (packet_tx, packet_rx): (Sender<Packet>, Receiver<Packet>) =
                                        unbounded();
                                    pending_connections.lock().unwrap().push(Box::new(
                                        transport::ServerConnection::new(
                                            task_pool.clone(),
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
                self.task_pool.clone(),
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

    pub fn broadcast(&mut self, payload: Packet) {
        for (_handle, connection) in self.connections.iter_mut() {
            connection.send(payload.clone()).unwrap();
        }
    }

    pub fn set_channels_builder<F>(&mut self, builder: F)
    where
        F: Fn(&mut ConnectionChannelsBuilder) + Send + Sync + 'static,
    {
        self.channels_builder_fn = Some(Box::new(builder));
    }

    pub fn send_message<M: ChannelMessage + Debug + Clone>(
        &mut self,
        handle: ConnectionHandle,
        message: M,
    ) -> Result<Option<M>, Box<dyn Error + Send>> {
        match self.connections.get_mut(&handle) {
            Some(connection) => {
                let channels = connection.channels().unwrap();
                let unsent = channels.send(message);
                channels.flush::<M>();
                Ok(unsent)
            }
            None => Err(Box::new(std::io::Error::new(
                // FIXME: move to enum Error
                std::io::ErrorKind::NotFound,
                "No such connection",
            ))),
        }
    }

    pub fn broadcast_message<M: ChannelMessage + Debug + Clone>(&mut self, message: M) {
        // log::info!("Broadcast:\n{:?}", message);
        print!(".");
        use std::io::Write;
        std::io::stdout().flush().unwrap();
        for (handle, connection) in self.connections.iter_mut() {
            let channels = connection.channels().unwrap();
            let result = channels.send(message.clone());
            channels.flush::<M>();
            match result {
                Some(_) => {
                    log::error!("Failed broadcast to [{}]", handle);
                }
                None => {}
            }
        }
    }

    pub fn recv_message<M: ChannelMessage + Debug + Clone>(
        &mut self,
        handle: ConnectionHandle,
    ) -> Option<M> {
        match self.connections.get_mut(&handle) {
            Some(connection) => {
                let channels = connection.channels().unwrap();
                channels.recv()
            }
            None => None,
        }
    }
}

pub fn receive_packets(
    mut net: ResMut<NetworkResource>,
    mut network_events: ResMut<Events<NetworkEvent>>,
) {
    let pending_connections: Vec<Box<dyn Connection>> =
        net.pending_connections.lock().unwrap().drain(..).collect();
    for mut conn in pending_connections {
        let handle: ConnectionHandle = net
            .connection_sequence
            .fetch_add(1, atomic::Ordering::Relaxed);
        if let Some(channels_builder_fn) = net.channels_builder_fn.as_ref() {
            conn.build_channels(
                channels_builder_fn,
                net.runtime.clone(),
                net.packet_pool.clone(),
            );
        }
        net.connections.insert(handle, conn);
        network_events.send(NetworkEvent::Connected(handle));
    }

    let packet_pool = net.packet_pool.clone();
    for (handle, connection) in net.connections.iter_mut() {
        while let Some(result) = connection.receive() {
            match result {
                Ok(packet) => {
                    let message = String::from_utf8_lossy(&packet);
                    log::debug!("Received on [{}] {} RAW: {}", handle, packet.len(), message);
                    if let Some(channels_rx) = connection.channels_rx() {
                        log::debug!("Processing as message");
                        let mut pool_packet = packet_pool.acquire();
                        pool_packet.resize(packet.len(), 0);
                        pool_packet[..].copy_from_slice(&*packet);
                        match channels_rx.try_send(pool_packet) {
                            Ok(()) => {
                                print!("!");
                            }
                            Err(err) => {
                                log::error!("Channel Incoming Error: {}", err);
                                // FIXME:error_events.send(error);
                            }
                        }
                    } else {
                        log::debug!("Processing as packet");
                        network_events.send(NetworkEvent::Packet(*handle, packet));
                    }
                }
                Err(err) => {
                    log::error!("Receive Error: {}", err);
                    // FIXME:error_events.send(error);
                }
            }
        }
    }
}

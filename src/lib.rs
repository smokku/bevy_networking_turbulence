use bevy::{
    app::{AppBuilder, Events, Plugin, CoreStage},
    ecs::prelude::*,
    tasks::{IoTaskPool, TaskPool, Task},
    core::FixedTimestep,
};

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::{unbounded, Receiver, Sender, SendError as CrossbeamSendError};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::RwLock;
use std::{
    collections::HashMap,
    error::Error,
    fmt::Debug,
    net::SocketAddr,
    sync::{atomic, Arc, Mutex},
};

use naia_client_socket::ClientSocket;
#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{MessageSender as ServerSender, ServerSocket};

pub use naia_client_socket::LinkConditionerConfig;
#[cfg(not(target_arch = "wasm32"))]
pub use naia_server_socket::find_my_ip_address;

use turbulence::{
    buffer::BufferPacketPool,
    message_channels::ChannelMessage,
    packet::{Packet as PoolPacket, PacketPool, MAX_PACKET_LEN},
    packet_multiplexer::{IncomingTrySendError, MuxPacketPool},
};
pub use turbulence::{
    message_channels::{MessageChannelMode, MessageChannelSettings},
    reliable_channel::Settings as ReliableChannelSettings,
};

mod channels;
mod transport;
use self::{
    channels::{SimpleBufferPool, TaskPoolRuntime},
    transport::MultiplexedPacket,
};
pub use transport::{Connection, ConnectionChannelsBuilder, Packet};

pub type ConnectionHandle = u32;

#[derive(Debug, Hash, PartialEq, Eq, Clone, StageLabel)]
struct SendHeartbeatsStage;

#[derive(Default)]
pub struct NetworkingPlugin {
    pub link_conditioner: Option<LinkConditionerConfig>,
    pub message_flushing_strategy: MessageFlushingStrategy,
    /// Disconnect if no packets received in this number of milliseconds
    pub idle_timeout_ms: Option<usize>,
    /// Should we automatically send heartbeat packets if no other packets have been sent?
    /// these are sent silently, and discarded, so you won't see them in your bevy systems.
    /// if auto_heartbeat_ms elapses, and we haven't sent anything else in that time, we send one.
    pub auto_heartbeat_ms: Option<usize>,
    /// FixedTimestep for the `heartbeats_and_timeouts` system which checks for idle connections
    /// and sends heartbeats. Does not need to be every frame.
    ///
    /// The `heartbeats_and_timeouts` system is only added if `idle_timeout_ms` or `auto_heartbeat_ms` are specified.
    ///
    /// Default if None: 0.5 secs
    pub heartbeats_and_timeouts_timestep_in_seconds: Option<f64>,
}

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut AppBuilder) {
        let task_pool = app
            .world()
            .get_resource::<IoTaskPool>()
            .expect("`IoTaskPool` resource not found.")
            .0
            .clone();

        app.insert_resource(NetworkResource::new(
            task_pool,
            self.link_conditioner.clone(),
            self.message_flushing_strategy,
            self.idle_timeout_ms,
            self.auto_heartbeat_ms,
        ))
        .add_event::<NetworkEvent>()
        .add_system(receive_packets.system());
        if self.idle_timeout_ms.is_some() || self.auto_heartbeat_ms.is_some() {
            // heartbeats and timeouts checking/sending only runs infrequently:
            app.add_stage_after(CoreStage::Update, SendHeartbeatsStage,
                SystemStage::parallel()
                .with_run_criteria(FixedTimestep::step(self.heartbeats_and_timeouts_timestep_in_seconds.unwrap_or(0.5)))
                .with_system(heartbeats_and_timeouts.system())
            );
        }
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
    server_channels: Arc<RwLock<HashMap<SocketAddr, Sender<Result<Packet, NetworkError>>>>>,

    runtime: TaskPoolRuntime,
    packet_pool: MuxPacketPool<BufferPacketPool<SimpleBufferPool>>,
    channels_builder_fn: Option<Box<dyn Fn(&mut ConnectionChannelsBuilder) + Send + Sync>>,
    message_flushing_strategy: MessageFlushingStrategy,
    idle_timeout_ms: Option<usize>,
    auto_heartbeat_ms: Option<usize>,

    link_conditioner: Option<LinkConditionerConfig>,
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // FIXME: remove this struct?
struct ServerListener {
    receiver_task: Task<()>,
    // needed to keep receiver_task alive
    sender: ServerSender,
    socket_address: SocketAddr,
}

#[derive(Debug)]
pub enum NetworkEvent {
    Connected(ConnectionHandle),
    Disconnected(ConnectionHandle),
    Packet(ConnectionHandle, Packet),
    Error(ConnectionHandle, NetworkError),
}

#[derive(Debug)]
pub enum NetworkError {
    TurbulenceChannelError(IncomingTrySendError<MultiplexedPacket>),
    IoError(Box<dyn Error + Sync + Send>),
    /// if we haven't seen a packet for the specified timeout
    MissedHeartbeat,
    Disconnected,
}

/// Turbulence will coalesce multiple small messages into a single packet when flush is called.
/// the default is `OnEverySend` - flushing after each message, which bypasses the coalescing.
/// You probably want to call flush once per tick instead, in your own system.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MessageFlushingStrategy {
    /// OnEverySend - flush immediately after calling send_message or send_broadcast.
    /// turbulence will never have a chance to coalesce multiple messages into a packet.
    OnEverySend,

    /// Never - you will want a system in (eg) PostUpdate which calls channels.flush for every channel type
    /// eg:
    ///
    /// pub fn flush_channels(mut net: ResMut<NetworkResource>) {
    ///     for (_handle, connection) in net.connections.iter_mut() {
    ///         let channels = connection.channels().unwrap();
    ///         channels.flush::<ClientNetMessage>();
    ///         channels.flush::<MyOtherMessageType>();
    ///         channels.flush::<AllMyTypesHere>();
    ///         ...
    ///     }
    /// }
    /// ...
    /// builder.add_system_to_stage(CoreStage::PostUpdate, flush_channels.system());
    ///
    Never,
}

impl Default for MessageFlushingStrategy {
    fn default() -> MessageFlushingStrategy {
        MessageFlushingStrategy::OnEverySend
    }
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for NetworkResource {}

#[cfg(target_arch = "wasm32")]
unsafe impl Sync for NetworkResource {}

impl NetworkResource {
    pub fn new( task_pool: TaskPool,
                link_conditioner: Option<LinkConditionerConfig>,
                message_flushing_strategy: MessageFlushingStrategy,
                idle_timeout_ms: Option<usize>,
                auto_heartbeat_ms: Option<usize>,
            ) -> Self
    {
        let runtime = TaskPoolRuntime::new(task_pool.clone());
        let packet_pool =
            MuxPacketPool::new(BufferPacketPool::new(SimpleBufferPool(MAX_PACKET_LEN)));

        NetworkResource {
            task_pool,
            connections: HashMap::new(),
            connection_sequence: atomic::AtomicU32::new(0),
            pending_connections: Arc::new(Mutex::new(Vec::new())),
            #[cfg(not(target_arch = "wasm32"))]
            listeners: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            server_channels: Arc::new(RwLock::new(HashMap::new())),
            runtime,
            packet_pool,
            channels_builder_fn: None,
            message_flushing_strategy,
            idle_timeout_ms,
            auto_heartbeat_ms,

            link_conditioner,
        }
    }

    /// The 3 listening addresses aren't strictly necessary, you can put the same IP address with a different port for the socket address; Unless you have some configuration issues with public and private addresses that need to be connected to.
    /// They also aren't necessary if you're using UDP, so you can put anything if that's the case.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn listen(
        &mut self,
        socket_address: SocketAddr,
        webrtc_listen_address: Option<SocketAddr>,
        public_webrtc_address: Option<SocketAddr>,
    ) {
        let mut server_socket = {
            let webrtc_listen_address = webrtc_listen_address.unwrap_or_else(|| {
                let mut listen_addr = socket_address;
                listen_addr.set_port(socket_address.port() + 1);
                listen_addr
            });
            let public_webrtc_address = public_webrtc_address.unwrap_or(webrtc_listen_address);
            let socket = futures_lite::future::block_on(ServerSocket::listen(
                socket_address,
                webrtc_listen_address,
                public_webrtc_address,
            ));

            if let Some(ref conditioner) = self.link_conditioner {
                socket.with_link_conditioner(conditioner)
            } else {
                socket
            }
        };
        let sender = server_socket.get_sender();
        let server_channels = self.server_channels.clone();
        let pending_connections = self.pending_connections.clone();
        let task_pool = self.task_pool.clone();

        let receiver_task = self.task_pool.spawn(async move {
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

                        let needs_new_channel = match server_channels
                            .read()
                            .expect("server channels lock is poisoned")
                            .get(&address)
                            .map(|channel| {
                                channel.send(Ok(Packet::copy_from_slice(packet.payload())))
                            }) {
                            Some(Ok(())) => false,
                            Some(Err(CrossbeamSendError(_packet))) => {
                                log::error!("Server can't send to channel, recreating");
                                // If we can't send to a channel, it's disconnected.
                                // We need to re-create the channel and re-try sending the message.
                                true
                            }
                            // This is a new connection, so we need to create a channel.
                            None => true,
                        };

                        if !needs_new_channel {
                            continue;
                        }

                        // We try to do a write lock only in case when a channel doesn't exist or
                        // has to be re-created. Trying to acquire a channel even for new
                        // connections is kind of a positive prediction to avoid doing a write
                        // lock.
                        let mut server_channels = server_channels
                            .write()
                            .expect("server channels lock is poisoned");
                        let (packet_tx, packet_rx): (
                            Sender<Result<Packet, NetworkError>>,
                            Receiver<Result<Packet, NetworkError>>,
                        ) = unbounded();
                        match packet_tx.send(Ok(Packet::copy_from_slice(packet.payload()))) {
                            Ok(()) => {
                                // It makes sense to store the channel only if it's healthy.
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
                            Err(error) => {
                                // This branch is unlikely to get called the second time (after
                                // re-creating a channel), but if for some strange reason it does,
                                // we'll just lose the message this time.
                                log::error!("Server Send Error (retry): {}", error);
                            }
                        }
                    }
                    Err(error) => {
                        log::error!("Server Receive Error: {}", error);
                    }
                }
            }
        });

        self.listeners.push(ServerListener {
            receiver_task,
            sender,
            socket_address,
        });
    }

    pub fn connect(&mut self, socket_address: SocketAddr) {
        let mut client_socket = {
            let socket = ClientSocket::connect(socket_address);

            if let Some(ref conditioner) = self.link_conditioner {
                socket.with_link_conditioner(conditioner)
            } else {
                socket
            }
        };
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

    // removes handle and connection, but doesn't signal peer in any way.
    // Peer will eventually do HeartbeatMissed and clean up.
    // (you should probably use the same idle timeout on server & client)
    pub fn disconnect(&mut self, handle: ConnectionHandle) {
        // on wasm32 we can't be a webrtc server, so cleanup is simpler
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                self.connections.remove(&handle);
            } else {
                if let Some(removed_connection) = self.connections.remove(&handle) {
                    if let Some(client_addr) = removed_connection.remote_address() {
                        self.server_channels.write().expect("server connections lock poisoned").remove(&client_addr);
                    }
                }
            }
        }
    }

    pub fn send(
        &mut self,
        handle: ConnectionHandle,
        payload: Packet,
    ) -> Result<(), Box<dyn Error + Sync + Send + 'static>> {
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
                if self.message_flushing_strategy == MessageFlushingStrategy::OnEverySend {
                    channels.flush::<M>();
                }
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
        for (handle, connection) in self.connections.iter_mut() {
            let channels = connection.channels().unwrap();
            let result = channels.send(message.clone());
            if self.message_flushing_strategy == MessageFlushingStrategy::OnEverySend {
                channels.flush::<M>();
            }
            if let Some(msg) = result {
                log::error!("Failed broadcast to [{}]: {:?}", handle, msg);
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

// check every connection for timeouts.
// ie. check how long since we last saw a packet.
pub fn heartbeats_and_timeouts(mut net: ResMut<NetworkResource>, mut network_events: ResMut<Events<NetworkEvent>>) {
    let mut silent_handles = Vec::new();
    let mut needs_hb_handles = Vec::new();
    let idle_limit = net.idle_timeout_ms;
    let heartbeat_limit = net.auto_heartbeat_ms;
    for (handle, connection) in net.connections.iter_mut() {
        let (rx_ms, tx_ms) = connection.last_packet_timings();
        log::debug!("millis since last rx: {} tx: {}", rx_ms, tx_ms);
        if idle_limit.is_some() && rx_ms > idle_limit.unwrap() as u128 {
            // idle-timeout this connection
            silent_handles.push(*handle);
        }
        if heartbeat_limit.is_some() && tx_ms > heartbeat_limit.unwrap() as u128 {
            // send a heartbeat packet.
            needs_hb_handles.push(*handle);
        }
    }
    for handle in needs_hb_handles {
        log::debug!("Sending hearbeat packet on h:{}", handle);
        // heartbeat packets are empty
        net.send(handle, Packet::new()).unwrap();
    }
    for handle in silent_handles {
        log::warn!("Idle disconnect for h:{}", handle);
        // Error doesn't imply Disconnected, so we send both
        network_events.send(NetworkEvent::Error(handle, NetworkError::MissedHeartbeat));
        network_events.send(NetworkEvent::Disconnected(handle));
        net.disconnect(handle);
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
                    // heartbeat packets are empty
                    if packet.len() == 0 {
                        log::debug!("Received heartbeat packet");
                        // discard without sending a NetworkEvent
                        continue;
                    }
                    let message = String::from_utf8_lossy(&packet);
                    log::debug!("Received on [{}] {} RAW: {}", handle, packet.len(), message);
                    if let Some(channels_rx) = connection.channels_rx() {
                        log::debug!("Processing as message");
                        let mut pool_packet = packet_pool.acquire();
                        pool_packet.resize(packet.len(), 0);
                        pool_packet[..].copy_from_slice(&*packet);
                        match channels_rx.try_send(pool_packet) {
                            Ok(()) => {
                                // cool
                            }
                            Err(err) => {
                                log::error!("Channel Incoming Error: {}", err);
                                network_events.send(NetworkEvent::Error(
                                    *handle,
                                    NetworkError::TurbulenceChannelError(err),
                                ));
                            }
                        }
                    } else {
                        log::debug!("Processing as packet");
                        network_events.send(NetworkEvent::Packet(*handle, packet));
                    }
                }
                Err(err) => {
                    log::error!("Receive Error: {:?}", err);
                    network_events.send(NetworkEvent::Error(*handle, err));
                }
            }
        }
    }
}

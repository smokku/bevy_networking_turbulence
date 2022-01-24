#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::Task;
use bevy::{prelude::error, tasks::TaskPool};
use bytes::Bytes;
use instant::{Duration, Instant};
use std::{
    error::Error,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use naia_client_socket::{
    ClientSocketTrait, MessageSender as ClientSender, Packet as ClientPacket,
};
#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{MessageSender as ServerSender, Packet as ServerPacket};

use turbulence::{
    buffer::BufferPacketPool,
    message_channels::{MessageChannels, MessageChannelsBuilder},
    packet::PacketPool,
    packet_multiplexer::{IncomingMultiplexedPackets, MuxPacket, MuxPacketPool, PacketMultiplexer},
};

#[cfg(not(target_arch = "wasm32"))]
use futures_lite::future::block_on;

use futures_lite::StreamExt;

use super::{
    channels::{SimpleBufferPool, TaskPoolRuntime},
    NetworkError,
};

pub type Packet = Bytes;
pub type MultiplexedPacket = MuxPacket<<BufferPacketPool<SimpleBufferPool> as PacketPool>::Packet>;
pub type ConnectionChannelsBuilder =
    MessageChannelsBuilder<TaskPoolRuntime, MuxPacketPool<BufferPacketPool<SimpleBufferPool>>>;

#[derive(Debug, Clone)]
pub struct PacketStats {
    pub packets_tx: usize,
    pub packets_rx: usize,
    pub bytes_tx: usize,
    pub bytes_rx: usize,
    pub last_tx: Instant,
    pub last_rx: Instant,
}

impl Default for PacketStats {
    fn default() -> Self {
        // default the last rx/tx to now.
        // not strictly true in use-udp mode, since we can "connect" without
        // exchanging packets. but always true for use-webrtc.
        let now = Instant::now();
        Self {
            packets_tx: 0,
            packets_rx: 0,
            bytes_tx: 0,
            bytes_rx: 0,
            last_tx: now,
            last_rx: now,
        }
    }
}

impl PacketStats {
    fn add_tx(&mut self, num_bytes: usize) {
        self.packets_tx += 1;
        self.bytes_tx += num_bytes;
        self.last_tx = Instant::now();
    }
    fn add_rx(&mut self, num_bytes: usize) {
        self.packets_rx += 1;
        self.bytes_rx += num_bytes;
        self.last_rx = Instant::now();
    }
    // returns Duration since last (rx, tx)
    fn idle_durations(&self) -> (Duration, Duration) {
        let now = Instant::now();
        let rx = now.duration_since(self.last_rx);
        let tx = now.duration_since(self.last_tx);
        (rx, tx)
    }
}

pub trait Connection: Send + Sync {
    fn remote_address(&self) -> Option<SocketAddr>;

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Sync + Send>>;

    fn receive(&mut self) -> Option<Result<Packet, NetworkError>>;

    fn build_channels(
        &mut self,
        builder_fn: &(dyn Fn(&mut ConnectionChannelsBuilder) + Send + Sync),
        runtime: TaskPoolRuntime,
        pool: MuxPacketPool<BufferPacketPool<SimpleBufferPool>>,
    );

    fn channels(&mut self) -> Option<&mut MessageChannels>;

    fn channels_rx(&mut self) -> Option<&mut IncomingMultiplexedPackets<MultiplexedPacket>>;

    fn stats(&self) -> PacketStats;

    /// returns milliseconds since last (rx, tx)
    fn last_packet_timings(&self) -> (u128, u128);
}

#[cfg(not(target_arch = "wasm32"))]
pub struct ServerConnection {
    task_pool: TaskPool,

    packet_rx: crossbeam_channel::Receiver<Result<Packet, NetworkError>>,
    sender: Option<ServerSender>,
    client_address: SocketAddr,
    stats: Arc<RwLock<PacketStats>>,

    channels: Option<MessageChannels>,
    channels_rx: Option<IncomingMultiplexedPackets<MultiplexedPacket>>,
    #[cfg(not(target_arch = "wasm32"))]
    channels_task: Option<Task<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ServerConnection {
    pub fn new(
        task_pool: TaskPool,
        packet_rx: crossbeam_channel::Receiver<Result<Packet, NetworkError>>,
        sender: ServerSender,
        client_address: SocketAddr,
    ) -> Self {
        ServerConnection {
            task_pool,
            packet_rx,
            sender: Some(sender),
            client_address,
            stats: Arc::new(RwLock::new(PacketStats::default())),
            channels: None,
            channels_rx: None,
            channels_task: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Connection for ServerConnection {
    fn remote_address(&self) -> Option<SocketAddr> {
        Some(self.client_address)
    }

    fn stats(&self) -> PacketStats {
        self.stats.read().expect("stats lock poisoned").clone()
    }

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Sync + Send>> {
        self.stats
            .write()
            .expect("stats lock poisoned")
            .add_tx(payload.len());
        block_on(
            self.sender
                .as_mut()
                .unwrap()
                .send(ServerPacket::new(self.client_address, payload.to_vec())),
        )
    }

    fn last_packet_timings(&self) -> (u128, u128) {
        let (rx_dur, tx_dur) = self
            .stats
            .read()
            .expect("stats lock poisoned")
            .idle_durations();
        (rx_dur.as_millis(), tx_dur.as_millis())
    }

    fn receive(&mut self) -> Option<Result<Packet, NetworkError>> {
        match self.packet_rx.try_recv() {
            Ok(payload) => match payload {
                Ok(packet) => {
                    self.stats
                        .write()
                        .expect("stats lock poisoned")
                        .add_rx(packet.len());
                    Some(Ok(packet))
                }
                Err(err) => Some(Err(err)),
            },
            Err(error) => match error {
                crossbeam_channel::TryRecvError::Empty => None,
                crossbeam_channel::TryRecvError::Disconnected => {
                    Some(Err(NetworkError::Disconnected))
                }
            },
        }
    }

    fn build_channels(
        &mut self,
        builder_fn: &(dyn Fn(&mut ConnectionChannelsBuilder) + Send + Sync),
        runtime: TaskPoolRuntime,
        pool: MuxPacketPool<BufferPacketPool<SimpleBufferPool>>,
    ) {
        let mut builder = MessageChannelsBuilder::new(runtime, pool);
        builder_fn(&mut builder);

        let mut multiplexer = PacketMultiplexer::new();
        self.channels = Some(builder.build(&mut multiplexer));
        let (channels_rx, mut channels_tx) = multiplexer.start();
        self.channels_rx = Some(channels_rx);

        let mut sender = self.sender.take().unwrap();
        let client_address = self.client_address;
        let stats = self.stats.clone();

        self.channels_task = Some(self.task_pool.spawn(async move {
            loop {
                let packet = channels_tx.next().await.unwrap();
                stats
                    .write()
                    .expect("stats lock poisoned")
                    .add_tx(packet.len());
                sender
                    .send(ServerPacket::new(client_address, (*packet).into()))
                    .await
                    .unwrap();
            }
        }));
    }

    fn channels(&mut self) -> Option<&mut MessageChannels> {
        self.channels.as_mut()
    }

    fn channels_rx(&mut self) -> Option<&mut IncomingMultiplexedPackets<MultiplexedPacket>> {
        self.channels_rx.as_mut()
    }
}

pub struct ClientConnection {
    task_pool: TaskPool,

    socket: Box<dyn ClientSocketTrait>,
    sender: Option<ClientSender>,
    stats: Arc<RwLock<PacketStats>>,

    channels: Option<MessageChannels>,
    channels_rx: Option<IncomingMultiplexedPackets<MultiplexedPacket>>,
    #[cfg(not(target_arch = "wasm32"))]
    channels_task: Option<Task<()>>,
}

impl ClientConnection {
    pub fn new(
        task_pool: TaskPool,
        socket: Box<dyn ClientSocketTrait>,
        sender: ClientSender,
    ) -> Self {
        ClientConnection {
            task_pool,
            socket,
            sender: Some(sender),
            stats: Arc::new(RwLock::new(PacketStats::default())),
            channels: None,
            channels_rx: None,
            #[cfg(not(target_arch = "wasm32"))]
            channels_task: None,
        }
    }
}

impl Connection for ClientConnection {
    fn remote_address(&self) -> Option<SocketAddr> {
        None
    }

    fn stats(&self) -> PacketStats {
        self.stats.read().expect("stats lock poisoned").clone()
    }

    fn last_packet_timings(&self) -> (u128, u128) {
        let (rx_dur, tx_dur) = self
            .stats
            .read()
            .expect("stats lock poisoned")
            .idle_durations();
        (rx_dur.as_millis(), tx_dur.as_millis())
    }

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Sync + Send>> {
        self.stats
            .write()
            .expect("stats lock poisoned")
            .add_tx(payload.len());
        self.sender
            .as_mut()
            .unwrap()
            .send(ClientPacket::new(payload.to_vec()))
    }

    fn receive(&mut self) -> Option<Result<Packet, NetworkError>> {
        match self.socket.receive() {
            Ok(event) => event.map(|packet| {
                self.stats
                    .write()
                    .expect("stats lock poisoned")
                    .add_rx(packet.payload().len());
                Ok(Packet::copy_from_slice(packet.payload()))
            }),
            Err(err) => Some(Err(NetworkError::IoError(Box::new(err)))),
        }
    }

    fn build_channels(
        &mut self,
        builder_fn: &(dyn Fn(&mut ConnectionChannelsBuilder) + Send + Sync),
        runtime: TaskPoolRuntime,
        pool: MuxPacketPool<BufferPacketPool<SimpleBufferPool>>,
    ) {
        let mut builder = MessageChannelsBuilder::new(runtime, pool);
        builder_fn(&mut builder);

        let mut multiplexer = PacketMultiplexer::new();
        self.channels = Some(builder.build(&mut multiplexer));
        let (channels_rx, mut channels_tx) = multiplexer.start();
        self.channels_rx = Some(channels_rx);

        let mut sender = self.sender.take().unwrap();
        let stats = self.stats.clone();

        let closure = async move {
            loop {
                match channels_tx.next().await {
                    Some(packet) => {
                        stats
                            .write()
                            .expect("stats lock poisoned")
                            .add_tx(packet.len());
                        sender.send(ClientPacket::new((*packet).into())).unwrap();
                    }
                    None => {
                        error!("Channel stream Disconnected");
                        return; // exit task
                    }
                }
            }
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.channels_task = Some(self.task_pool.spawn(closure));
        }
        #[cfg(target_arch = "wasm32")]
        self.task_pool.spawn(closure);
    }

    fn channels(&mut self) -> Option<&mut MessageChannels> {
        self.channels.as_mut()
    }

    fn channels_rx(&mut self) -> Option<&mut IncomingMultiplexedPackets<MultiplexedPacket>> {
        self.channels_rx.as_mut()
    }
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for ClientConnection {}

#[cfg(target_arch = "wasm32")]
unsafe impl Sync for ClientConnection {}

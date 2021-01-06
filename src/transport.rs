#[cfg(not(target_arch = "wasm32"))]
use bevy_tasks::Task;
use bevy_tasks::TaskPool;
use bytes::Bytes;
use std::{error::Error, net::SocketAddr};

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

use super::channels::{SimpleBufferPool, TaskPoolRuntime};

pub type Packet = Bytes;
type MultiplexedPacket = MuxPacket<<BufferPacketPool<SimpleBufferPool> as PacketPool>::Packet>;
pub type ConnectionChannelsBuilder =
    MessageChannelsBuilder<TaskPoolRuntime, MuxPacketPool<BufferPacketPool<SimpleBufferPool>>>;

pub trait Connection: Send + Sync {
    fn remote_address(&self) -> Option<SocketAddr>;

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Send>>;

    fn receive(&mut self) -> Option<Result<Packet, Box<dyn Error + Send>>>;

    fn build_channels(
        &mut self,
        builder_fn: &(dyn Fn(&mut ConnectionChannelsBuilder) + Send + Sync),
        runtime: TaskPoolRuntime,
        pool: MuxPacketPool<BufferPacketPool<SimpleBufferPool>>,
    );

    fn channels(&mut self) -> Option<&mut MessageChannels>;

    fn channels_rx(&mut self) -> Option<&mut IncomingMultiplexedPackets<MultiplexedPacket>>;
}

#[cfg(not(target_arch = "wasm32"))]
pub struct ServerConnection {
    task_pool: TaskPool,

    packet_rx: crossbeam_channel::Receiver<Packet>,
    sender: Option<ServerSender>,
    client_address: SocketAddr,

    channels: Option<MessageChannels>,
    channels_rx: Option<IncomingMultiplexedPackets<MultiplexedPacket>>,
    channels_task: Option<Task<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ServerConnection {
    pub fn new(
        task_pool: TaskPool,
        packet_rx: crossbeam_channel::Receiver<Packet>,
        sender: ServerSender,
        client_address: SocketAddr,
    ) -> Self {
        ServerConnection {
            task_pool,
            packet_rx,
            sender: Some(sender),
            client_address,
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

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Send>> {
        block_on(
            self.sender
                .as_mut()
                .unwrap()
                .send(ServerPacket::new(self.client_address, payload.to_vec())),
        )
    }

    fn receive(&mut self) -> Option<Result<Packet, Box<dyn Error + Send>>> {
        match self.packet_rx.try_recv() {
            Ok(payload) => Some(Ok(payload)),
            Err(error) => match error {
                crossbeam_channel::TryRecvError::Empty => None,
                err => Some(Err(Box::new(err))),
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
        self.channels_task = Some(self.task_pool.spawn(async move {
            loop {
                let packet = channels_tx.next().await.unwrap();
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

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Send>> {
        self.sender
            .as_mut()
            .unwrap()
            .send(ClientPacket::new(payload.to_vec()))
    }

    fn receive(&mut self) -> Option<Result<Packet, Box<dyn Error + Send>>> {
        match self.socket.receive() {
            Ok(event) => match event {
                Some(packet) => Some(Ok(Packet::copy_from_slice(packet.payload()))),
                None => None,
            },
            Err(err) => Some(Err(Box::new(err))),
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
        #[allow(unused_variables)]
        let channels_task = self.task_pool.spawn(async move {
            loop {
                match channels_tx.next().await {
                    Some(packet) => {
                        sender.send(ClientPacket::new((*packet).into())).unwrap();
                    }
                    None => {
                        log::error!("Channel stream Disconnected");
                        return; // exit task
                    }
                }
            }
        });

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.channels_task = Some(channels_task);
        }
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

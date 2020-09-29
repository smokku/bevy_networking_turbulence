use bytes::Bytes;
use std::{error::Error, fmt::Debug, net::SocketAddr};

use naia_client_socket::{
    ClientSocketTrait, MessageSender as ClientSender, Packet as ClientPacket,
};
#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{MessageSender as ServerSender, Packet as ServerPacket};

pub trait Connection: Debug + Send + Sync {
    fn send(&mut self, payload: &[u8]) -> Result<(), Box<dyn Error + Send>>;
    fn send_to(
        &mut self,
        payload: &[u8],
        destination: SocketAddr,
    ) -> Result<(), Box<dyn Error + Send>>;

    fn receive(&mut self) -> Option<Result<Packet, Box<dyn Error + Send>>>;
}

#[derive(Debug, Eq, PartialEq)]
pub struct Packet {
    /// The address from which it came
    address: Option<SocketAddr>,
    /// The payload of the packet
    payload: Bytes,
}

impl Packet {
    pub fn new(payload: &[u8]) -> Self {
        Packet {
            address: None,
            payload: Bytes::copy_from_slice(payload),
        }
    }

    pub fn new_from(payload: &[u8], from: SocketAddr) -> Self {
        Packet {
            address: Some(from),
            payload: Bytes::copy_from_slice(payload),
        }
    }

    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    pub fn address(&self) -> Option<SocketAddr> {
        self.address
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub struct ServerConnection {
    receiver_task: bevy::tasks::Task<()>,
    packet_rx: crossbeam_channel::Receiver<ServerPacket>,
    sender: ServerSender,
}

#[cfg(not(target_arch = "wasm32"))]
impl ServerConnection {
    pub fn new(
        receiver_task: bevy::tasks::Task<()>,
        packet_rx: crossbeam_channel::Receiver<ServerPacket>,
        sender: ServerSender,
    ) -> Self {
        ServerConnection {
            receiver_task,
            packet_rx,
            sender,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Debug for ServerConnection {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("ServerConnection")
            .field(
                "receiver_task",
                &format_args!("Task<{}>", std::any::type_name::<()>()),
            )
            .field("packet_rx", &self.packet_rx)
            .field("sender", &self.sender)
            .finish()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Connection for ServerConnection {
    fn send(&mut self, _payload: &[u8]) -> Result<(), Box<dyn Error + Send>> {
        panic!("ServerConnection does not support send() without destination");
    }
    fn send_to(
        &mut self,
        payload: &[u8],
        destination: SocketAddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        futures_lite::future::block_on(
            self.sender
                .send(ServerPacket::new(destination, payload.to_vec())),
        )
    }

    fn receive(&mut self) -> Option<Result<Packet, Box<dyn Error + Send>>> {
        match self.packet_rx.try_recv() {
            Ok(packet) => Some(Ok(Packet::new_from(packet.payload(), packet.address()))),
            Err(error) => match error {
                crossbeam_channel::TryRecvError::Empty => None,
                err => Some(Err(Box::new(err))),
            },
        }
    }
}

#[derive(Debug)]
pub struct ClientConnection {
    socket: Box<dyn ClientSocketTrait>,
    sender: ClientSender,
}

impl ClientConnection {
    pub fn new(socket: Box<dyn ClientSocketTrait>, sender: ClientSender) -> Self {
        ClientConnection { socket, sender }
    }
}

impl Connection for ClientConnection {
    fn send(&mut self, payload: &[u8]) -> Result<(), Box<dyn Error + Send>> {
        self.sender.send(ClientPacket::new(payload.to_vec()))
    }
    fn send_to(
        &mut self,
        _payload: &[u8],
        _destination: SocketAddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        panic!("ClientConnection can not send() to specific destination");
    }

    fn receive(&mut self) -> Option<Result<Packet, Box<dyn Error + Send>>> {
        match self.socket.receive() {
            Ok(event) => match event {
                Some(packet) => Some(Ok(Packet::new(packet.payload()))),
                None => None,
            },
            Err(err) => Some(Err(Box::new(err))),
        }
    }
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for ClientConnection {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for ClientConnection {}

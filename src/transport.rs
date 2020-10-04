use bytes::Bytes;
use std::{error::Error, fmt::Debug, net::SocketAddr};

use naia_client_socket::{
    ClientSocketTrait, MessageSender as ClientSender, Packet as ClientPacket,
};
#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{MessageSender as ServerSender, Packet as ServerPacket};

pub type Packet = Bytes;

pub trait Connection: Debug + Send + Sync {
    fn remote_address(&self) -> Option<SocketAddr>;

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Send>>;

    fn receive(&mut self) -> Option<Result<Packet, Box<dyn Error + Send>>>;
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct ServerConnection {
    packet_rx: crossbeam_channel::Receiver<Packet>,
    sender: ServerSender,
    client_address: SocketAddr,
}

#[cfg(not(target_arch = "wasm32"))]
impl ServerConnection {
    pub fn new(
        packet_rx: crossbeam_channel::Receiver<Packet>,
        sender: ServerSender,
        client_address: SocketAddr,
    ) -> Self {
        ServerConnection {
            packet_rx,
            sender,
            client_address,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Connection for ServerConnection {
    fn remote_address(&self) -> Option<SocketAddr> {
        Some(self.client_address)
    }

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Send>> {
        futures_lite::future::block_on(
            self.sender
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
    fn remote_address(&self) -> Option<SocketAddr> {
        None
    }

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Send>> {
        self.sender.send(ClientPacket::new(payload.to_vec()))
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
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for ClientConnection {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for ClientConnection {}

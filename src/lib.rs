use bevy_app::{AppBuilder, Plugin};

use async_compat::Compat;
use std::net::SocketAddr;

use naia_server_socket::{LinkConditionerConfig, ServerSocket, ServerSocketTrait};

pub struct NetworkingPlugin;

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut AppBuilder) {
        app.init_resource::<NetworkResource>();
    }
}

pub struct NetworkResource {
    server_socket: Option<Box<dyn ServerSocketTrait>>,
}

impl NetworkResource {
    pub fn listen(&mut self, socket_address: SocketAddr) {
        let server_socket = smol::block_on(Compat::new(ServerSocket::listen(socket_address)));
        self.server_socket =
            Some(server_socket.with_link_conditioner(&LinkConditionerConfig::good_condition()));
    }
}

impl Default for NetworkResource {
    fn default() -> Self {
        NetworkResource {
            server_socket: None,
        }
    }
}

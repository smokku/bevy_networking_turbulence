use bevy_app::{AppBuilder, Plugin};

pub struct NetworkingPlugin;

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut AppBuilder) {
        app.init_resource::<NetworkResource>();
    }
}

pub struct NetworkResource {}

impl NetworkResource {}

impl Default for NetworkResource {
    fn default() -> Self {
        NetworkResource {}
    }
}

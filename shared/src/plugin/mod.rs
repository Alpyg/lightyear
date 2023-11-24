use bevy::prelude::{App, Fixed, Plugin, Time};

use config::SharedConfig;
pub use replication::ReplicationData;
pub use sets::ReplicationSet;

pub mod config;
pub mod events;
pub(crate) mod log;
mod replication;
pub mod sets;
pub mod systems;

pub struct SharedPlugin {
    pub config: SharedConfig,
}

impl Plugin for SharedPlugin {
    fn build(&self, app: &mut App) {
        // RESOURCES
        // NOTE: this tick duration must be the same as any previous existing fixed timesteps
        app.insert_resource(Time::<Fixed>::from_seconds(
            self.config.tick.tick_duration.as_secs_f64(),
        ));
        app.init_resource::<ReplicationData>();
        // SYSTEMS
        // app.add_systems(FixedUpdate, increment_tick);

        // TODO: set log config
        let log_config = self.config.log.clone();
        app.add_plugins(log::LogPlugin {
            level: log_config.level,
            filter: log_config.filter,
        });
    }
}

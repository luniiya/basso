use bevy_app::{PluginGroup, PluginGroupBuilder};

pub mod attack;
pub mod block_update;
pub mod interact;
pub mod inventory;
pub mod loading;
pub mod mining;
pub mod movement;
pub mod task_pool;
pub mod tick_counter;

/// This plugin group will add all the default plugins necessary for the
/// simulated client to work.
pub struct DefaultPlugins;

impl PluginGroup for DefaultPlugins {
    fn build(self) -> PluginGroupBuilder {
        #[allow(unused_mut)]
        let mut group = PluginGroupBuilder::start::<Self>()
            .add(crate::client::AmbiguityLoggerPlugin)
            .add(bevy_time::TimePlugin)
            .add(crate::client::AzaleaPlugin)
            .add(basso_entity::EntityPlugin)
            .add(basso_physics::PhysicsPlugin)
            .add(task_pool::TaskPoolPlugin::default())
            .add(inventory::InventoryPlugin)
            .add(movement::MovementPlugin)
            .add(interact::InteractPlugin)
            .add(mining::MiningPlugin)
            .add(attack::AttackPlugin)
            .add(block_update::BlockUpdatePlugin)
            .add(loading::PlayerLoadedPlugin)
            .add(tick_counter::TickCounterPlugin);
        #[cfg(feature = "log")]
        {
            group = group.add(bevy_log::LogPlugin::default());
        }
        group
    }
}

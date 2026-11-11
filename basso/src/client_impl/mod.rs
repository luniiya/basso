use std::{
    collections::HashMap,
    sync::Arc,
};

use basso_client::{
    local_player::{Experience, Hunger, TabList, WorldHolder},
    packet::{SendGamePacketEvent, ServerboundGamePacket},
    player::{GameProfileComponent, PlayerInfo},
    tick_counter::TicksConnected,
};
use basso_core::{
    data_registry::{DataRegistryWithKey, ResolvableDataRegistry},
    entity_id::MinecraftEntityId,
};
use basso_entity::indexing::{EntityIdIndex, EntityUuidIndex};
use basso_inventory::game_profile::GameProfile;
use basso_registry::{DataRegistryKeyRef, identifier::Identifier};
use basso_world::{PartialWorld, World, WorldName};
use bevy_app::AppExit;
use bevy_ecs::{entity::Entity, resource::Resource};
use parking_lot::RwLock;
use uuid::Uuid;

use crate::{
    client_impl::error::AzaleaResult,
    entity_ref::EntityRef,
};

pub mod attack;
pub mod entity_query;
pub mod error;
pub mod interact;
pub mod inventory;
pub mod mining;
pub mod movement;

/// A Minecraft client instance that can interact with the world.
///
/// Note that `Client` is inaccessible from systems (i.e. plugins), but you can
/// achieve everything that client can do with ECS events.
#[derive(Clone)]
pub struct Client {
    /// The entity for this client in the ECS.
    pub entity: Entity,

    /// A mutually exclusive reference to the entity component system (ECS).
    ///
    /// You probably don't need to access this directly. Note that if you're
    /// using a shared world (i.e. a swarm), the ECS will also contain all
    /// entities in all worlds.
    ///
    /// You can nearly always use [`Self::component`], [`Self::query_self`],
    /// [`Self::query_entity`], or another one of those related functions to
    /// access the ECS instead.
    pub ecs: Arc<RwLock<bevy_ecs::world::World>>,
}

impl Client {
    /// Create a new client from the given ECS Entity and ECS World.
    pub fn new(entity: Entity, ecs: Arc<RwLock<bevy_ecs::world::World>>) -> Self {
        Self { entity, ecs }
    }

    /// Send an action to the game.
    ///
    /// The Horizon integration listens for the resulting
    /// [`SendGamePacketEvent`].
    pub fn write_packet(&self, packet: impl Into<ServerboundGamePacket>) {
        let packet = packet.into();
        self.ecs
            .write()
            .commands()
            .trigger(SendGamePacketEvent::new(self.entity, packet));
    }

    /// End the entire client, and return from the schedule runner.
    pub fn exit(&self) {
        self.ecs.write().write_message(AppExit::Success);
    }

    /// Get a resource from the ECS. This will clone the resource and return it.
    pub fn resource<T: Resource + Clone>(&self) -> T {
        self.ecs.read().resource::<T>().clone()
    }

    /// Get a required ECS resource and call the given function with it.
    pub fn map_resource<T: Resource, R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let ecs = self.ecs.read();
        let value = ecs.resource::<T>();
        f(value)
    }

    /// Get an optional ECS resource and call the given function with it.
    pub fn map_get_resource<T: Resource, R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R {
        let ecs = self.ecs.read();
        let value = ecs.get_resource::<T>();
        f(value)
    }

    /// Get an `RwLock` with a reference to our (potentially shared) Minecraft
    /// world.
    ///
    /// This gets the [`World`] from the client's [`WorldHolder`]
    /// component.
    pub fn world(&self) -> AzaleaResult<Arc<RwLock<World>>> {
        let world_holder = self.component::<WorldHolder>()?;
        Ok(world_holder.shared.clone())
    }

    /// Get an `RwLock` with a reference to the world that this client has
    /// loaded.
    pub fn partial_world(&self) -> AzaleaResult<Arc<RwLock<PartialWorld>>> {
        let world_holder = self.component::<WorldHolder>()?;
        Ok(world_holder.partial.clone())
    }

    /// Returns whether we have received enough data to be in a world.
    pub fn logged_in(&self) -> bool {
        // the client has a world name when it's in a world
        self.query_self::<&WorldName, _>(|_| {}).is_ok()
    }

    /// Returns the client as an [`EntityRef`], allowing you to treat it as any
    /// other entity.
    pub fn entity(&self) -> EntityRef {
        self.entity_ref_for(self.entity)
    }

    /// Create an [`EntityRef`] for the given ECS entity.
    pub fn entity_ref_for(&self, entity: Entity) -> EntityRef {
        EntityRef::new(self.clone(), entity)
    }
}

impl Client {
    /// Get the hunger level of this client, which includes both food and
    /// saturation.
    pub fn hunger(&self) -> AzaleaResult<Hunger> {
        Ok(self.component::<Hunger>()?.to_owned())
    }

    /// Get the experience of this client.
    pub fn experience(&self) -> AzaleaResult<Experience> {
        Ok((*self.component::<Experience>()?).clone())
    }

    /// Get a map of player UUIDs to their information in the tab list.
    pub fn tab_list(&self) -> AzaleaResult<HashMap<Uuid, PlayerInfo>> {
        Ok((**self.component::<TabList>()?).clone())
    }

    /// Returns the [`GameProfile`] for our client. This contains your username,
    /// UUID, and skin data.
    ///
    /// This is also available from the ECS as [`GameProfileComponent`].
    pub fn profile(&self) -> AzaleaResult<GameProfile> {
        Ok((**self.component::<GameProfileComponent>()?).clone())
    }

    /// Get the Minecraft UUID of this client.
    pub fn uuid(&self) -> AzaleaResult<Uuid> {
        Ok(self.profile()?.uuid)
    }

    /// Get an [`Entity`] in the world by its Minecraft UUID, if it's within
    /// render distance.
    pub fn entity_id_by_uuid(&self, uuid: Uuid) -> Option<Entity> {
        self.map_resource::<EntityUuidIndex, _>(|entity_uuid_index| entity_uuid_index.get(&uuid))
    }
    /// Get an [`EntityRef`] in the world by its Minecraft UUID, if it's within
    /// render distance.
    pub fn entity_by_uuid(&self, uuid: Uuid) -> Option<EntityRef> {
        self.entity_id_by_uuid(uuid).map(|e| self.entity_ref_for(e))
    }

    /// Get an [`Entity`] in the world by its [`MinecraftEntityId`].
    pub fn entity_id_by_minecraft_id(&self, id: MinecraftEntityId) -> AzaleaResult<Option<Entity>> {
        self.query_self::<&EntityIdIndex, _>(|entity_id_index| {
            entity_id_index.get_by_minecraft_entity(id)
        })
    }
    /// Get an [`EntityRef`] in the world by its [`MinecraftEntityId`].
    pub fn entity_by_minecraft_id(&self, id: MinecraftEntityId) -> AzaleaResult<Option<EntityRef>> {
        Ok(self
            .entity_id_by_minecraft_id(id)?
            .map(|e| EntityRef::new(self.clone(), e)))
    }

    /// Call the given function with the client's [`RegistryHolder`].
    ///
    /// Note that the player's world will be locked during this time, which may
    /// result in a deadlock if you try to access the world again while
    /// in the function.
    ///
    /// [`RegistryHolder`]: basso_core::registry_holder::RegistryHolder
    pub fn with_registry_holder<R>(
        &self,
        f: impl FnOnce(&basso_core::registry_holder::RegistryHolder) -> R,
    ) -> AzaleaResult<R> {
        let world = self.world()?;
        let registries = &world.read().registries;
        Ok(f(registries))
    }

    /// Resolve the given registry to its name.
    ///
    /// This is necessary for data-driven registries like [`Enchantment`].
    ///
    /// [`Enchantment`]: basso_registry::data::Enchantment
    #[deprecated = "use `bot.resolve_registry_key(registry).map(|r| r.into_ident())` instead."]
    pub fn resolve_registry_name(
        &self,
        registry: &impl ResolvableDataRegistry,
    ) -> AzaleaResult<Option<Identifier>> {
        self.with_registry_holder(|registries| registry.key(registries).map(|r| r.into_ident()))
    }

    /// Resolve the given registry entry to its key (aka name).
    ///
    /// This is necessary for data-driven registries like [`Enchantment`] and
    /// [`Biome`](basso_registry::data::Biome).
    ///
    /// To get the key as an [`Identifier`], you can map the return value like
    /// `.map(|r| r.into_ident())`.
    ///
    /// [`Enchantment`]: basso_registry::data::Enchantment
    pub fn resolve_registry_key<R: ResolvableDataRegistry>(
        &self,
        registry: &R,
    ) -> AzaleaResult<Option<R::Key>> {
        self.with_registry_holder(|registries| registry.key_owned(registries))
    }

    /// Resolve the given registry to its name and data and call the given
    /// function with it.
    ///
    /// This is necessary for data-driven registries like [`Enchantment`].
    ///
    /// If you just want the value name, use [`Self::resolve_registry_name`]
    /// instead.
    ///
    /// [`Enchantment`]: basso_registry::data::Enchantment
    pub fn with_resolved_registry<R: ResolvableDataRegistry, Ret>(
        &self,
        registry: R,
        f: impl FnOnce(&Identifier, &R::DeserializesTo) -> Ret,
    ) -> AzaleaResult<Option<Ret>> {
        self.with_registry_holder(|registries| {
            registry
                .resolve(registries)
                .map(|(name, data)| f(name, data))
        })
    }

    /// Returns the number of ticks since the world was entered, or 0 if the
    /// client isn't in the world.
    ///
    /// This is a shortcut for getting the [`TicksConnected`] component.
    pub fn ticks_connected(&self) -> u64 {
        self.component::<TicksConnected>().map(|c| c.0).unwrap_or(0)
    }
}

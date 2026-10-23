use bevy_ecs::prelude::*;
use chunkedge_ident::{Ident, ident};
use chunkedge_registry::{BiomeRegistry, DimensionTypeRegistry};
use chunkedge_server::entity::active_status_effects::{ActiveStatusEffect, ActiveStatusEffects};
use chunkedge_server::entity::attributes::{EntityAttribute, EntityAttributes};
use chunkedge_server::protocol::status_effects::StatusEffect;

use crate::Server;
use crate::client::{VisibleChunkLayer, VisibleEntityLayers};
use crate::entity::{EntityId, EntityLayerId};
use crate::layer::{ChunkLayer, EntityLayer};
use crate::op_level::OpLevel;
use crate::protocol::packets::play::game_event_s2c::GameEventKind;
use crate::protocol::packets::play::{
    EntityEventS2c, GameEventS2c, InitializeBorderS2c, LoginS2c, RespawnS2c, SetEntityDataS2c,
    SetHealthS2c, UpdateAttributesS2c, UpdateMobEffectS2c,
};
use crate::protocol::{Packet, VarInt};
use crate::testing::{MockClientHelper, PacketFrames, ScenarioSingleClient};
use crate::weather::Rain;
use crate::world_border::WorldBorderBundle;

/// Spawns a chunk and entity layer using `dimension_type`.
fn spawn_layer(app: &mut bevy_app::App, dimension_type: Ident<&str>) -> Entity {
    let chunk_layer = ChunkLayer::new(
        dimension_type,
        app.world().resource::<DimensionTypeRegistry>(),
        app.world().resource::<BiomeRegistry>(),
        app.world().resource::<Server>(),
    );
    let entity_layer = EntityLayer::new(app.world().resource::<Server>());
    app.world_mut().spawn((chunk_layer, entity_layer)).id()
}

/// Moves the client into `layer`, runs a tick and returns what it received.
fn switch_layer(
    app: &mut bevy_app::App,
    helper: &mut MockClientHelper,
    client: Entity,
    layer: Entity,
) -> PacketFrames {
    helper.clear_received();

    let mut client = app.world_mut().entity_mut(client);
    client.get_mut::<EntityLayerId>().unwrap().0 = layer;
    client.get_mut::<VisibleChunkLayer>().unwrap().0 = layer;
    let mut visible = client.get_mut::<VisibleEntityLayers>().unwrap();
    visible.0.clear();
    visible.0.insert(layer);

    app.update();
    helper.collect_received()
}

#[test]
fn respawn_resends_client_state() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    let dest = spawn_layer(&mut app, ident!("overworld"));
    app.update();

    let mut client_mut = app.world_mut().entity_mut(client);
    client_mut.get_mut::<OpLevel>().unwrap().set(2);
    client_mut
        .get_mut::<ActiveStatusEffects>()
        .unwrap()
        .apply(ActiveStatusEffect::from_effect(StatusEffect::Speed).with_duration(1000));
    client_mut
        .get_mut::<EntityAttributes>()
        .unwrap()
        .set_add_modifier(EntityAttribute::MovementSpeed, "test", 0.1);
    app.update();

    let entity_id = app.world().get::<EntityId>(client).unwrap().get();
    let recvd = switch_layer(&mut app, &mut helper, client, dest);

    recvd.assert_count::<RespawnS2c>(1);

    recvd.assert_count::<EntityEventS2c>(1);
    recvd.assert_order::<(RespawnS2c, EntityEventS2c)>();
    let op_event = recvd.first::<EntityEventS2c>();
    assert_eq!(op_event.entity_id, entity_id);
    assert_eq!(op_event.entity_status, 24 + 2);

    recvd.assert_count::<UpdateMobEffectS2c>(1);
    recvd.assert_order::<(RespawnS2c, UpdateMobEffectS2c)>();
    let effect = recvd.first::<UpdateMobEffectS2c>();
    assert_eq!(effect.entity_id, VarInt(entity_id));
    assert_eq!(
        effect.effect_id,
        VarInt(i32::from(StatusEffect::Speed.to_raw()))
    );

    recvd.assert_count::<SetEntityDataS2c>(1);
    recvd.assert_order::<(RespawnS2c, SetEntityDataS2c)>();
    assert_eq!(
        recvd.first::<SetEntityDataS2c>().entity_id,
        VarInt(entity_id)
    );

    recvd.assert_count::<UpdateAttributesS2c>(1);
    recvd.assert_order::<(RespawnS2c, UpdateAttributesS2c)>();
    let attributes = recvd.first::<UpdateAttributesS2c>();
    assert_eq!(attributes.entity_id, VarInt(entity_id));
    let speed = attributes
        .properties
        .iter()
        .find(|p| p.id == VarInt(EntityAttribute::MovementSpeed.get_id().into()))
        .expect("movement speed attribute is re-sent");
    assert_eq!(speed.modifiers.len(), 1);

    recvd.assert_count::<SetHealthS2c>(1);
    recvd.assert_order::<(RespawnS2c, SetHealthS2c)>();
}

#[test]
fn respawn_uses_layer_dimension_name() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    let nether = spawn_layer(&mut app, ident!("the_nether"));
    app.update();

    assert_eq!(
        helper.collect_received().first::<LoginS2c>().dimension_name,
        ident!("overworld")
    );

    let nether_type = app
        .world()
        .resource::<DimensionTypeRegistry>()
        .index_of(ident!("the_nether"))
        .unwrap();

    let recvd = switch_layer(&mut app, &mut helper, client, nether);
    let respawn = recvd.first::<RespawnS2c>();
    assert_eq!(respawn.dimension_name, ident!("the_nether"));
    assert_eq!(
        respawn.dimension_type,
        VarInt(nether_type.get_value().into())
    );
}

// A respawn into another dimension makes the client build a fresh level, so
// level state has to arrive after the respawn packet or it is lost.
#[test]
fn respawn_into_other_dimension_resends_level_state() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    let nether = spawn_layer(&mut app, ident!("the_nether"));
    app.world_mut()
        .entity_mut(nether)
        .insert((WorldBorderBundle::default(), Rain(1.0)));
    app.update();

    let recvd = switch_layer(&mut app, &mut helper, client, nether);

    recvd.assert_count::<InitializeBorderS2c>(1);
    recvd.assert_order::<(RespawnS2c, InitializeBorderS2c)>();

    let respawn_idx = recvd.0.iter().position(|f| f.id == RespawnS2c::ID).unwrap();
    let rain_after_respawn = recvd.0[respawn_idx..]
        .iter()
        .filter(|f| f.id == GameEventS2c::ID)
        .any(|f| f.decode::<GameEventS2c>().unwrap().kind == GameEventKind::RainLevelChange);
    assert!(rain_after_respawn, "rain level is sent after the respawn");
}

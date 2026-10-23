use bevy_ecs::prelude::*;
use chunkedge_ident::{Ident, ident};
use chunkedge_registry::{BiomeRegistry, DimensionTypeRegistry};

use crate::Server;
use crate::client::{VisibleChunkLayer, VisibleEntityLayers};
use crate::entity::EntityLayerId;
use crate::layer::{ChunkLayer, EntityLayer};
use crate::protocol::packets::play::game_event_s2c::GameEventKind;
use crate::protocol::packets::play::{GameEventS2c, InitializeBorderS2c, LoginS2c, RespawnS2c};
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

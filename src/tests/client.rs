use chunkedge_server::protocol::movement_flags::MovementFlags;

use crate::abilities::PlayerAbilitiesFlags;
use crate::client::{Client, ClientVelocityMessage};
use crate::ecs::message::Messages;
use crate::entity::avatar::PlayerModelParts;
use crate::entity::{EntityId, Position, Velocity};
use crate::layer::ChunkLayer;
use crate::layer::chunk::UnloadedChunk;
use crate::math::DVec3;
use crate::protocol::packets::play::{
    AcceptTeleportationC2s, EntityPositionSyncS2c, LoginS2c, MoveEntityPosS2c, MovePlayerPosRotC2s,
    PlayerCombatKillS2c, PlayerPositionS2c, SetEntityDataS2c, SetEntityMotionS2c,
};
use crate::protocol::{Packet, VarInt};
use crate::teleport::TeleportState;
use crate::testing::{ScenarioSingleClient, create_mock_client};
use crate::{ChunkPos, GameMode};

#[test]
fn client_teleport_and_move() {
    let ScenarioSingleClient {
        mut app,
        helper: mut helper_1,
        layer: layer_ent,
        ..
    } = ScenarioSingleClient::new();

    let mut layer = app.world_mut().get_mut::<ChunkLayer>(layer_ent).unwrap();

    for z in -10..10 {
        for x in -10..10 {
            layer.insert_chunk(ChunkPos::new(x, z), UnloadedChunk::new());
        }
    }

    let (mut bundle, mut helper_2) = create_mock_client(
        "other",
        &mut app
            .world_mut()
            .resource_mut::<chunkedge_server::entity::EntityManager>(),
    );

    bundle.layer.0 = layer_ent;
    bundle.visible_chunk_layer.0 = layer_ent;
    bundle.visible_entity_layers.0.insert(layer_ent);

    app.world_mut().spawn(bundle);

    app.update();

    // Client received an initial teleport.
    helper_1
        .collect_received()
        .assert_count::<PlayerPositionS2c>(1);

    // Confirm the initial teleport from the server.
    helper_1.send(&AcceptTeleportationC2s {
        teleport_id: 0.into(),
    });

    // Move a little.
    helper_1.send(&MovePlayerPosRotC2s {
        position: DVec3::new(1.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
        flags: MovementFlags::new().with_on_ground(true),
    });

    app.update();

    // Check that the other client saw the client moving.
    helper_2
        .collect_received()
        .assert_count::<MoveEntityPosS2c>(1);
}

#[test]
fn client_gamemode_changed_ability() {
    let mut scenario = ScenarioSingleClient::new();

    *scenario
        .app
        .world_mut()
        .get_mut::<GameMode>(scenario.client)
        .unwrap() = GameMode::Creative;

    scenario.app.update();

    let abilities = scenario
        .app
        .world_mut()
        .get::<PlayerAbilitiesFlags>(scenario.client)
        .unwrap();

    assert!(abilities.allow_flying());
    assert!(abilities.instant_break());
    assert!(abilities.invulnerable());

    *scenario
        .app
        .world_mut()
        .get_mut::<GameMode>(scenario.client)
        .unwrap() = GameMode::Adventure;

    scenario.app.update();

    let abilities = scenario
        .app
        .world_mut()
        .get::<PlayerAbilitiesFlags>(scenario.client)
        .unwrap();

    assert!(!abilities.allow_flying());
    assert!(!abilities.instant_break());
    assert!(!abilities.invulnerable());
}

#[test]
// Regression test for a scheduling race where derived player tracked data
// could be updated after tracked-data serialization during the initial join.
// That left a SetEntityDataS2c packet to be emitted on the next tick, which
// made tests that expected no unrelated packets after join flaky.
fn client_does_not_emit_delayed_tracked_data_after_initial_join() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    app.update();
    helper.clear_received();

    app.update();

    helper
        .collect_received()
        .assert_count::<SetEntityDataS2c>(0);
}

#[test]
fn client_knows_itself_by_its_entity_id() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    app.update();
    let entity_id = app.world().get::<EntityId>(client).unwrap().get();
    assert_ne!(entity_id, 0, "ID 0 means unassigned to the client");

    let recvd = helper.collect_received();
    recvd.assert_count::<LoginS2c>(1);
    assert_eq!(recvd.first::<LoginS2c>().entity_id, entity_id);

    app.world_mut()
        .get_mut::<Client>(client)
        .unwrap()
        .kill("test");
    app.update();

    let recvd = helper.collect_received();
    recvd.assert_count::<PlayerCombatKillS2c>(1);
    assert_eq!(
        recvd.first::<PlayerCombatKillS2c>().player_id,
        VarInt(entity_id)
    );
}

#[test]
// Regression test: the extracted default for the model-part mask was 127
// (taken from a mannequin), so a player showing every skin layer had that
// value dropped from its initial tracked data and others saw no layers.
fn client_skin_layers_are_sent_to_other_clients() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer: layer_ent,
        ..
    } = ScenarioSingleClient::new();

    app.update();
    helper.clear_received();

    let (mut bundle, _helper_2) = create_mock_client(
        "other",
        &mut app
            .world_mut()
            .resource_mut::<chunkedge_server::entity::EntityManager>(),
    );
    bundle.layer.0 = layer_ent;
    bundle.visible_chunk_layer.0 = layer_ent;
    bundle.visible_entity_layers.0.insert(layer_ent);
    bundle.player_model_parts = PlayerModelParts(0x7f);
    let other = app.world_mut().spawn(bundle).id();

    app.update();

    let other_id = app.world().get::<EntityId>(other).unwrap().get();
    let recvd = helper.collect_received();
    let sent_all_layers = recvd
        .0
        .iter()
        .filter(|frame| frame.id == SetEntityDataS2c::ID)
        .map(|frame| frame.decode::<SetEntityDataS2c>().unwrap())
        .filter(|pkt| pkt.entity_id.0 == other_id)
        // Index 16 (Avatar model parts), byte serializer (0), every layer.
        .any(|pkt| pkt.tracked_values.0.windows(3).any(|w| w == [16, 0, 0x7f]));

    assert!(sent_all_layers, "model-part mask missing from tracked data");
}

#[test]
fn client_teleport_sends_velocity_in_blocks_per_tick() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    app.update();
    helper.confirm_initial_pending_teleports();
    app.update();
    helper.clear_received();

    app.world_mut().get_mut::<Velocity>(client).unwrap().0 = DVec3::new(20.0, 40.0, -60.0);
    app.update();

    let recvd = helper.collect_received();
    recvd.assert_count::<PlayerPositionS2c>(1);
    let pkt = recvd.first::<PlayerPositionS2c>();
    assert_eq!(pkt.velocity, DVec3::new(1.0, 2.0, -3.0));
    assert!(!pkt.flags.x_vel() && !pkt.flags.y_vel() && !pkt.flags.z_vel());
}

#[test]
fn entity_position_sync_sends_velocity_in_blocks_per_tick() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        layer: layer_ent,
    } = ScenarioSingleClient::new();

    let mut layer = app.world_mut().get_mut::<ChunkLayer>(layer_ent).unwrap();

    for z in -10..10 {
        for x in -10..10 {
            layer.insert_chunk(ChunkPos::new(x, z), UnloadedChunk::new());
        }
    }

    let (mut bundle, mut other_helper) = create_mock_client(
        "other",
        &mut app
            .world_mut()
            .resource_mut::<chunkedge_server::entity::EntityManager>(),
    );

    bundle.layer.0 = layer_ent;
    bundle.visible_chunk_layer.0 = layer_ent;
    bundle.visible_entity_layers.0.insert(layer_ent);

    app.world_mut().spawn(bundle);

    app.update();
    helper.confirm_initial_pending_teleports();
    app.update();
    other_helper.clear_received();

    // Moving 8 blocks or more makes viewers resync the entity's absolute
    // position, and that packet carries its velocity.
    app.world_mut().get_mut::<Position>(client).unwrap().0 = DVec3::new(10.0, 0.0, 0.0);
    app.world_mut().get_mut::<Velocity>(client).unwrap().0 = DVec3::new(20.0, 40.0, -60.0);
    app.update();

    let recvd = other_helper.collect_received();
    recvd.assert_count::<EntityPositionSyncS2c>(1);
    assert_eq!(
        recvd.first::<EntityPositionSyncS2c>().velocity,
        DVec3::new(1.0, 2.0, -3.0)
    );
}

#[test]
fn set_velocity_is_reported_as_the_client_reads_it() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    app.update();
    helper.clear_received();

    let mut state = app.world_mut().get_mut::<Client>(client).unwrap();
    state.set_velocity([8.0, 7.2, -3.4]);
    state.set_velocity([250.0, -0.3, 0.0]);
    app.update();

    let reported: Vec<_> = app
        .world()
        .resource::<Messages<ClientVelocityMessage>>()
        .iter_current_update_messages()
        .copied()
        .collect();
    let received: Vec<_> = helper
        .collect_received()
        .0
        .into_iter()
        .filter(|frame| frame.id == SetEntityMotionS2c::ID)
        .map(|frame| frame.decode::<SetEntityMotionS2c>().unwrap().velocity)
        .collect();

    assert_eq!(reported.len(), 2);
    assert_eq!(received.len(), 2);
    for (message, wire) in reported.iter().zip(&received) {
        assert_eq!(message.client, client);
        assert_eq!(
            message.received().to_array().map(f64::to_bits),
            wire.0.map(f64::to_bits),
        );
    }

    app.update();
    assert_eq!(
        app.world()
            .resource::<Messages<ClientVelocityMessage>>()
            .iter_current_update_messages()
            .len(),
        0,
        "each velocity is reported once"
    );
}

#[test]
fn unexpected_teleport_confirmation_disconnects_without_panicking() {
    let ScenarioSingleClient {
        mut app,
        client,
        mut helper,
        ..
    } = ScenarioSingleClient::new();

    app.update();
    helper.confirm_initial_pending_teleports();
    app.update();

    // No teleport is pending, but the client confirms the next teleport ID
    // anyway. This used to underflow the pending teleport counter.
    let next_id = app
        .world()
        .get::<TeleportState>(client)
        .unwrap()
        .teleport_id_counter();
    helper.send(&AcceptTeleportationC2s {
        teleport_id: (next_id as i32).into(),
    });
    app.update();

    assert!(app.world().get::<Client>(client).is_none());
}

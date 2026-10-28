use chunkedge_inventory::Inventory;
use chunkedge_inventory::player_inventory::PlayerInventory;
use chunkedge_item::ItemComponent;

use crate::entity::entity::{self, Flags};
use crate::entity::{EntityId, OnGround, Pose};
use crate::protocol::packets::play::player_command_c2s::PlayerCommand;
use crate::protocol::packets::play::{PlayerCommandC2s, SetEntityDataS2c};
use crate::protocol::{Packet, VarInt};
use crate::testing::ScenarioSingleClient;
use crate::{ItemKind, ItemStack};

/// Tracked data holding only the shared flags (index 0, byte serializer)
/// without the fall flying bit, followed by the terminator.
const FLAGS_ONLY_NOT_FALL_FLYING: [u8; 4] = [0, 0, 0, 0xff];

#[test]
fn accepted_start_sets_fall_flying_and_pose() {
    let mut scenario = airborne_player_with_elytra();

    start_fall_flying(&mut scenario);
    scenario.app.update();

    assert!(flags(&scenario).fall_flying());
    assert_eq!(pose(&scenario), Pose::FallFlying);
    let sent_fall_flying = own_tracked_data(&mut scenario)
        .iter()
        .any(|values| values.windows(3).any(|w| w == [0, 0, 0x80]));
    assert!(sent_fall_flying, "fall flying flag not sent to the player");
}

#[test]
fn start_on_ground_is_rejected_and_resends_flags() {
    let mut scenario = airborne_player_with_elytra();
    set_on_ground(&mut scenario, true);
    scenario.app.update();
    scenario.helper.clear_received();

    start_fall_flying(&mut scenario);
    scenario.app.update();

    assert!(!flags(&scenario).fall_flying());
    assert_eq!(pose(&scenario), Pose::Standing);
    assert_eq!(
        own_tracked_data(&mut scenario),
        [FLAGS_ONLY_NOT_FALL_FLYING.to_vec()]
    );
}

#[test]
fn start_without_glider_is_rejected_and_resends_flags() {
    let mut scenario = airborne_player_with_elytra();
    set_chest(
        &mut scenario,
        ItemStack::new(ItemKind::DiamondChestplate, 1),
    );
    scenario.app.update();
    scenario.helper.clear_received();

    start_fall_flying(&mut scenario);
    scenario.app.update();

    assert!(!flags(&scenario).fall_flying());
    assert_eq!(
        own_tracked_data(&mut scenario),
        [FLAGS_ONLY_NOT_FALL_FLYING.to_vec()]
    );
}

#[test]
fn start_with_elytra_about_to_break_is_rejected() {
    let mut scenario = airborne_player_with_elytra();
    set_chest(
        &mut scenario,
        ItemStack::new(ItemKind::Elytra, 1)
            .with_components(vec![ItemComponent::Damage(VarInt(431))]),
    );
    scenario.app.update();

    start_fall_flying(&mut scenario);
    scenario.app.update();

    assert!(!flags(&scenario).fall_flying());
}

#[test]
fn landing_ends_the_glide_and_restores_the_pose() {
    let mut scenario = gliding_player();
    scenario
        .app
        .world_mut()
        .get_mut::<Flags>(scenario.client)
        .unwrap()
        .set_sneaking(true);

    set_on_ground(&mut scenario, true);
    scenario.app.update();

    assert!(!flags(&scenario).fall_flying());
    assert_eq!(pose(&scenario), Pose::Sneaking);
}

#[test]
fn removing_the_elytra_ends_the_glide() {
    let mut scenario = gliding_player();

    set_chest(&mut scenario, ItemStack::EMPTY);
    scenario.app.update();

    assert!(!flags(&scenario).fall_flying());
    assert_eq!(pose(&scenario), Pose::Standing);
    let sent_not_fall_flying = own_tracked_data(&mut scenario)
        .iter()
        .any(|values| values.windows(3).any(|w| w == [0, 0, 0]));
    assert!(sent_not_fall_flying, "cleared flag not sent to the player");
}

#[test]
fn gliding_continues_while_the_player_can_glide() {
    let mut scenario = gliding_player();

    scenario.app.update();
    scenario.app.update();

    assert!(flags(&scenario).fall_flying());
    assert_eq!(pose(&scenario), Pose::FallFlying);
    assert!(own_tracked_data(&mut scenario).is_empty());
}

fn gliding_player() -> ScenarioSingleClient {
    let mut scenario = airborne_player_with_elytra();
    start_fall_flying(&mut scenario);
    scenario.app.update();
    assert!(flags(&scenario).fall_flying());
    scenario.helper.clear_received();
    scenario
}

/// A player in the air with an elytra in the chest slot, past the join tick.
fn airborne_player_with_elytra() -> ScenarioSingleClient {
    let mut scenario = ScenarioSingleClient::new();
    scenario.app.update();

    set_chest(&mut scenario, ItemStack::new(ItemKind::Elytra, 1));
    scenario.app.update();
    scenario.helper.clear_received();

    assert!(
        !scenario
            .app
            .world()
            .get::<OnGround>(scenario.client)
            .unwrap()
            .0
    );
    scenario
}

fn start_fall_flying(scenario: &mut ScenarioSingleClient) {
    let entity_id = scenario
        .app
        .world()
        .get::<EntityId>(scenario.client)
        .unwrap()
        .get();
    scenario.helper.send(&PlayerCommandC2s {
        entity_id: VarInt(entity_id),
        action: PlayerCommand::StartFallFlying,
        jump_boost: VarInt(0),
    });
}

fn set_chest(scenario: &mut ScenarioSingleClient, stack: ItemStack) {
    scenario
        .app
        .world_mut()
        .get_mut::<Inventory>(scenario.client)
        .unwrap()
        .set_slot(PlayerInventory::SLOT_CHEST, stack);
}

fn set_on_ground(scenario: &mut ScenarioSingleClient, on_ground: bool) {
    scenario
        .app
        .world_mut()
        .get_mut::<OnGround>(scenario.client)
        .unwrap()
        .0 = on_ground;
}

fn flags(scenario: &ScenarioSingleClient) -> Flags {
    scenario
        .app
        .world()
        .get::<Flags>(scenario.client)
        .unwrap()
        .clone()
}

fn pose(scenario: &ScenarioSingleClient) -> Pose {
    scenario
        .app
        .world()
        .get::<entity::Pose>(scenario.client)
        .unwrap()
        .0
}

/// The tracked values of every `SetEntityDataS2c` the player received about
/// itself.
fn own_tracked_data(scenario: &mut ScenarioSingleClient) -> Vec<Vec<u8>> {
    let entity_id = scenario
        .app
        .world()
        .get::<EntityId>(scenario.client)
        .unwrap()
        .get();
    scenario
        .helper
        .collect_received()
        .0
        .iter()
        .filter(|frame| frame.id == SetEntityDataS2c::ID)
        .map(|frame| frame.decode::<SetEntityDataS2c>().unwrap())
        .filter(|pkt| pkt.entity_id.0 == entity_id)
        .map(|pkt| pkt.tracked_values.0.to_vec())
        .collect()
}

use bevy_ecs::message::Messages;
use chunkedge_server::abilities::{PlayerAbilitiesFlags, PlayerStartFlyingMessage};
use chunkedge_server::protocol::packets::play::PlayerAbilitiesC2s;

use crate::GameMode;
use crate::testing::ScenarioSingleClient;

#[test]
fn start_flying_without_permission_is_refused() {
    let mut scenario = ScenarioSingleClient::new();
    *scenario
        .app
        .world_mut()
        .get_mut::<GameMode>(scenario.client)
        .unwrap() = GameMode::Survival;
    scenario.app.update();

    scenario.helper.send(&PlayerAbilitiesC2s::StartFlying);
    scenario.app.update();

    let world = scenario.app.world();
    let flags = world.get::<PlayerAbilitiesFlags>(scenario.client).unwrap();
    assert!(!flags.allow_flying());
    assert!(!flags.flying());
    assert_eq!(
        world
            .resource::<Messages<PlayerStartFlyingMessage>>()
            .iter_current_update_messages()
            .count(),
        0
    );
}

#[test]
fn start_flying_with_permission_is_accepted() {
    let mut scenario = ScenarioSingleClient::new();
    *scenario
        .app
        .world_mut()
        .get_mut::<GameMode>(scenario.client)
        .unwrap() = GameMode::Creative;
    scenario.app.update();

    scenario.helper.send(&PlayerAbilitiesC2s::StartFlying);
    scenario.app.update();

    let world = scenario.app.world();
    let flags = world.get::<PlayerAbilitiesFlags>(scenario.client).unwrap();
    assert!(flags.allow_flying());
    assert!(flags.flying());
    assert_eq!(
        world
            .resource::<Messages<PlayerStartFlyingMessage>>()
            .iter_current_update_messages()
            .count(),
        1
    );
}

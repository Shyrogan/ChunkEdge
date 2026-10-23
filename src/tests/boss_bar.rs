use std::collections::HashSet;

use bevy_app::App;
use bevy_ecs::entity::Entity;
use chunkedge_boss_bar::{
    BossBarAction, BossBarBundle, BossBarColor, BossBarDivision, BossBarFlags, BossBarHealth,
    BossBarStyle, BossBarTitle,
};
use chunkedge_server::client::VisibleEntityLayers;
use chunkedge_server::entity::EntityLayerId;
use chunkedge_server::protocol::Packet;
use chunkedge_server::protocol::packets::play::BossEventS2c;
use chunkedge_server::text::IntoText;
use chunkedge_server::uuid::Uuid;
use chunkedge_server::{Despawned, UniqueId};

use crate::Text;
use crate::testing::{PacketFrames, ScenarioSingleClient};

#[test]
fn test_initialize_on_join() {
    let mut scenario = ScenarioSingleClient::new();

    // Insert a boss bar into the world
    scenario
        .app
        .world_mut()
        .entity_mut(scenario.layer)
        .insert(BossBarBundle {
            title: BossBarTitle("Boss Bar".into_text()),
            health: BossBarHealth(0.5),
            layer: EntityLayerId(scenario.layer),
            ..Default::default()
        });

    scenario.app.update();

    // We should receive a boss bar packet with the ADD action
    let frames = scenario.helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);
}

#[test]
fn test_despawn() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        ..
    } = prepare();

    // Despawn the boss bar
    app.world_mut().entity_mut(layer).insert(Despawned);

    app.update();

    // We should receive a boss bar packet with the REMOVE action
    let frames = helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);
}

#[test]
fn test_title_update() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        ..
    } = prepare();

    // Update the title
    app.world_mut()
        .entity_mut(layer)
        .insert(BossBarTitle(Text::text("Test 2")));

    app.update();

    // We should receive a boss bar packet with the UPDATE_TITLE action
    let frames = helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);
}

#[test]
fn test_health_update() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        ..
    } = prepare();

    // Update the health
    app.world_mut().entity_mut(layer).insert(BossBarHealth(0.5));

    app.update();

    // We should receive a boss bar packet with the UPDATE_HEALTH action
    let frames = helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);
}

#[test]
fn test_style_update() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        ..
    } = prepare();

    // Update the style
    app.world_mut().entity_mut(layer).insert(BossBarStyle {
        color: BossBarColor::Red,
        division: BossBarDivision::TenNotches,
    });

    app.update();

    // We should receive a boss bar packet with the UPDATE_STYLE action
    let frames = helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);
}

#[test]
fn test_flags_update() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        ..
    } = prepare();

    // Update the flags
    let mut new_flags = BossBarFlags::new();
    new_flags.set_create_fog(true);
    app.world_mut().entity_mut(layer).insert(new_flags);

    app.update();

    // We should receive a boss bar packet with the UPDATE_FLAGS action
    let frames = helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);
}

#[test]
fn test_client_layer_change() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        client,
    } = prepare();

    // Remove the layer from the client
    {
        let mut visible_entity_layers = app
            .world_mut()
            .get_mut::<VisibleEntityLayers>(client)
            .unwrap();
        visible_entity_layers.0.clear()
    };

    app.update();

    // We should receive a boss bar packet with the REMOVE action
    let frames = helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);

    // Add the layer back to the client
    {
        let mut visible_entity_layers = app
            .world_mut()
            .get_mut::<VisibleEntityLayers>(client)
            .unwrap();
        visible_entity_layers.0.insert(layer)
    };

    app.update();

    // We should receive a boss bar packet with the ADD action
    let frames = helper.collect_received();
    frames.assert_count::<BossEventS2c>(1);
}

#[test]
fn test_update_and_hide_same_tick() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        client,
    } = prepare();
    let id = app.world().get::<UniqueId>(layer).unwrap().0;

    // Change the bar and stop viewing its layer in the same tick.
    let mut new_flags = BossBarFlags::new();
    new_flags.set_darken_sky(true);
    app.world_mut().entity_mut(layer).insert(new_flags);
    hide_all_layers(&mut app, client);

    app.update();

    let shown = replay_boss_events(&helper.collect_received(), HashSet::from([id]));
    assert!(shown.is_empty());
}

#[test]
fn test_despawn_and_hide_same_tick() {
    let ScenarioSingleClient {
        mut app,
        mut helper,
        layer,
        client,
    } = prepare();
    let id = app.world().get::<UniqueId>(layer).unwrap().0;

    app.world_mut().entity_mut(layer).insert(Despawned);
    hide_all_layers(&mut app, client);

    app.update();

    let shown = replay_boss_events(&helper.collect_received(), HashSet::from([id]));
    assert!(shown.is_empty());
}

#[test]
fn test_spawn_while_viewing() {
    let mut scenario = ScenarioSingleClient::new();
    scenario.app.update();
    scenario.helper.clear_received();

    let bar = scenario
        .app
        .world_mut()
        .spawn(BossBarBundle {
            layer: EntityLayerId(scenario.layer),
            ..Default::default()
        })
        .id();

    scenario.app.update();

    let id = scenario.app.world().get::<UniqueId>(bar).unwrap().0;
    let shown = replay_boss_events(&scenario.helper.collect_received(), HashSet::new());
    assert_eq!(shown, HashSet::from([id]));
}

fn hide_all_layers(app: &mut App, client: Entity) {
    app.world_mut()
        .get_mut::<VisibleEntityLayers>(client)
        .unwrap()
        .0
        .clear();
}

/// Replays the boss bar packets the way the vanilla client applies them,
/// starting from the bars in `shown`, and returns the bars shown afterwards.
///
/// Panics on any packet the client cannot apply: the vanilla client crashes
/// with a `NullPointerException` on an update for a bar it does not show.
#[track_caller]
fn replay_boss_events(frames: &PacketFrames, mut shown: HashSet<Uuid>) -> HashSet<Uuid> {
    for frame in frames.0.iter().filter(|f| f.id == BossEventS2c::ID) {
        let BossEventS2c { id, action } = frame.decode::<BossEventS2c>().unwrap();
        match action {
            BossBarAction::Add { .. } => {
                assert!(shown.insert(id), "Add for boss bar {id} already shown");
            }
            BossBarAction::Remove => {
                assert!(shown.remove(&id), "Remove for boss bar {id} not shown");
            }
            action => {
                assert!(
                    shown.contains(&id),
                    "{action:?} for boss bar {id} not shown"
                );
            }
        }
    }
    shown
}

fn prepare() -> ScenarioSingleClient {
    let mut s = ScenarioSingleClient::new();

    // Process a tick to get past the "on join" logic.
    s.app.update();
    s.helper.clear_received();

    // Insert a boss bar into the world

    // Attach the new boss bar to the layer for convenience.
    s.app.world_mut().entity_mut(s.layer).insert(BossBarBundle {
        title: BossBarTitle("Boss Bar".into_text()),
        health: BossBarHealth(0.5),
        layer: EntityLayerId(s.layer),
        ..Default::default()
    });

    for _ in 0..2 {
        s.app.update();
    }

    s.helper.clear_received();
    s
}

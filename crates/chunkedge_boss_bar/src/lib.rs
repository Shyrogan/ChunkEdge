#![doc = include_str!("../README.md")]

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use chunkedge_server::client::{
    Client, OldView, OldVisibleEntityLayers, View, ViewDistance, VisibleEntityLayers,
    handle_layer_messages, update_view_and_layers,
};
use chunkedge_server::layer::UpdateLayersPreClientSet;
use chunkedge_server::protocol::packets::play::BossEventS2c;
pub use chunkedge_server::protocol::packets::play::boss_event_s2c::{
    BossBarAction, BossBarColor, BossBarDivision, BossBarFlags,
};
use chunkedge_server::protocol::{IntoTextComponent, WritePacket};
use chunkedge_server::{ChunkView, Despawned, EntityLayer, Layer, UniqueId};

mod components;
use chunkedge_entity::{EntityLayerId, Position};
pub use components::*;

pub struct BossBarPlugin;

impl Plugin for BossBarPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_systems(
            PostUpdate,
            (
                // Spawn, update and despawn packets are broadcast through the
                // entity layer, in this order.
                (
                    boss_bar_spawn,
                    update_boss_bar::<BossBarTitle>,
                    update_boss_bar::<BossBarHealth>,
                    update_boss_bar::<BossBarStyle>,
                    update_boss_bar::<BossBarFlags>,
                    boss_bar_despawn,
                )
                    .chain()
                    .before(UpdateLayersPreClientSet),
                // The layer broadcast reaches clients through their old view
                // and old layers, so the add or remove for a view change must
                // be written after it.
                update_boss_bar_view
                    .after(handle_layer_messages)
                    .before(update_view_and_layers),
            ),
        );
    }
}

/// Builds the packet that shows a boss bar with its current state.
fn add_packet<'a>(
    id: &UniqueId,
    title: &'a BossBarTitle,
    health: &BossBarHealth,
    style: &BossBarStyle,
    flags: BossBarFlags,
) -> BossEventS2c<'a> {
    BossEventS2c {
        id: id.0,
        action: BossBarAction::Add {
            title: (&title.0).into_cow_text_component(),
            health: health.0,
            color: style.color,
            division: style.division,
            flags,
        },
    }
}

fn write_to_layer(entity_layer: &mut EntityLayer, pos: Option<&Position>, packet: &BossEventS2c) {
    if let Some(pos) = pos {
        entity_layer.view_writer(pos.0).write_packet(packet);
    } else {
        entity_layer.write_packet(packet);
    }
}

fn boss_bar_spawn(
    boss_bars_query: Query<
        (
            &UniqueId,
            &BossBarTitle,
            &BossBarHealth,
            &BossBarStyle,
            &BossBarFlags,
            &EntityLayerId,
            Option<&Position>,
        ),
        Or<(
            Added<BossBarTitle>,
            Added<BossBarHealth>,
            Added<BossBarStyle>,
            Added<BossBarFlags>,
        )>,
    >,
    mut entity_layers_query: Query<&mut EntityLayer>,
) {
    for (id, title, health, style, flags, entity_layer_id, pos) in &boss_bars_query {
        if let Ok(mut entity_layer) = entity_layers_query.get_mut(entity_layer_id.0) {
            let packet = add_packet(id, title, health, style, *flags);
            write_to_layer(&mut entity_layer, pos, &packet);
        }
    }
}

fn update_boss_bar<T: Component + ToPacketAction>(
    boss_bars_query: Query<(&UniqueId, Ref<T>, &EntityLayerId, Option<&Position>), Changed<T>>,
    mut entity_layers_query: Query<&mut EntityLayer>,
) {
    for (id, part, entity_layer_id, pos) in &boss_bars_query {
        // `boss_bar_spawn` already sent the full state.
        if part.is_added() {
            continue;
        }
        if let Ok(mut entity_layer) = entity_layers_query.get_mut(entity_layer_id.0) {
            let packet = BossEventS2c {
                id: id.0,
                action: part.to_packet_action(),
            };
            write_to_layer(&mut entity_layer, pos, &packet);
        }
    }
}

/// Adds or removes boss bars for clients whose visible layers or view changed.
fn update_boss_bar_view(
    mut clients_query: Query<
        (
            &mut Client,
            &VisibleEntityLayers,
            &OldVisibleEntityLayers,
            View,
            OldView,
        ),
        Or<(
            Changed<VisibleEntityLayers>,
            Changed<Position>,
            Changed<ViewDistance>,
        )>,
    >,
    boss_bars_query: Query<
        (
            &UniqueId,
            &BossBarTitle,
            &BossBarHealth,
            &BossBarStyle,
            &BossBarFlags,
            &EntityLayerId,
            Option<&Position>,
        ),
        // Despawned bars are removed through the layer broadcast.
        Without<Despawned>,
    >,
) {
    for (mut client, visible_layers, old_visible_layers, view, old_view) in &mut clients_query {
        let view = view.get();
        let old_view = old_view.get();

        for (id, title, health, style, flags, entity_layer_id, pos) in &boss_bars_query {
            let in_view = |view: &ChunkView| pos.is_none_or(|pos| view.contains(pos.0.into()));
            let was_visible =
                old_visible_layers.get().contains(&entity_layer_id.0) && in_view(&old_view);
            let is_visible = visible_layers.0.contains(&entity_layer_id.0) && in_view(&view);

            if is_visible && !was_visible {
                client.write_packet(&add_packet(id, title, health, style, *flags));
            } else if was_visible && !is_visible {
                client.write_packet(&BossEventS2c {
                    id: id.0,
                    action: BossBarAction::Remove,
                });
            }
        }
    }
}

fn boss_bar_despawn(
    boss_bars_query: Query<(&UniqueId, &EntityLayerId, Option<&Position>), With<Despawned>>,
    mut entity_layers_query: Query<&mut EntityLayer>,
) {
    for (id, entity_layer_id, pos) in &boss_bars_query {
        if let Ok(mut entity_layer) = entity_layers_query.get_mut(entity_layer_id.0) {
            let packet = BossEventS2c {
                id: id.0,
                action: BossBarAction::Remove,
            };
            write_to_layer(&mut entity_layer, pos, &packet);
        }
    }
}

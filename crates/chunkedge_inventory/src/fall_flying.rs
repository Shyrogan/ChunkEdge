//! Elytra gliding the way the vanilla server handles it: the server decides
//! whether a player may start gliding, and ends the glide once the player can
//! no longer glide. Whether a player can glide depends on what they wear, so
//! this lives next to the player's [`Inventory`].

use bevy_ecs::prelude::*;
use chunkedge_item::{EquipSlot, ItemComponent, item_component_type};
use chunkedge_server::ItemStack;
use chunkedge_server::abilities::PlayerAbilitiesFlags;
use chunkedge_server::entity::active_status_effects::ActiveStatusEffects;
use chunkedge_server::entity::entity::{self, Flags};
use chunkedge_server::entity::{OnGround, Pose};
use chunkedge_server::event_loop::PacketMessage;
use chunkedge_server::passenger::Riding;
use chunkedge_server::protocol::VarInt;
use chunkedge_server::protocol::packets::play::PlayerCommandC2s;
use chunkedge_server::protocol::packets::play::player_command_c2s::PlayerCommand;
use chunkedge_server::protocol::status_effects::StatusEffect;

use crate::player_inventory::PlayerInventory;
use crate::{HeldItem, Inventory};

/// The components [`can_glide`] reads.
type GlideConditions = (
    &'static OnGround,
    &'static PlayerAbilitiesFlags,
    Has<Riding>,
    &'static ActiveStatusEffects,
    &'static Inventory,
    &'static HeldItem,
);

/// Handles the client asking to start gliding
/// (`ServerGamePacketListenerImpl#handlePlayerCommand`).
pub(crate) fn handle_start_fall_flying(
    mut packets: MessageReader<PacketMessage>,
    mut players: Query<(&mut Flags, &mut entity::Pose, GlideConditions)>,
) {
    for packet in packets.read() {
        let Some(pkt) = packet.decode::<PlayerCommandC2s>() else {
            continue;
        };
        if pkt.action != PlayerCommand::StartFallFlying {
            continue;
        }
        let Ok((mut flags, mut pose, conditions)) = players.get_mut(packet.client) else {
            continue;
        };

        // `Player#tryToStartFallFlying` also refuses a player in water.
        // ChunkEdge doesn't track which entities touch a fluid, so that check
        // is left out; the vanilla client doesn't ask to glide in water.
        if !flags.fall_flying() && can_glide(conditions) {
            flags.set_fall_flying(true);
        } else {
            // `LivingEntity#stopFallFlying` sets the flag and clears it again
            // so that the client gets its flags back even when they didn't
            // change, and stops the glide it started on its own. Writing
            // through `Mut` marks `Flags` as changed, which resends them.
            flags.set_fall_flying(false);
        }
        update_pose(&flags, &mut pose);
    }
}

/// Ends the glide of players who can no longer glide, every tick
/// (`LivingEntity#updateFallFlying`), and keeps their pose in step with it.
///
/// Vanilla also damages the glider every 20 ticks of gliding. ChunkEdge doesn't
/// damage items, so that part is left out.
pub(crate) fn update_fall_flying(
    mut players: Query<(&mut Flags, &mut entity::Pose, GlideConditions)>,
) {
    for (mut flags, mut pose, conditions) in &mut players {
        if flags.fall_flying() && !can_glide(conditions) {
            flags.set_fall_flying(false);
        }
        update_pose(&flags, &mut pose);
    }
}

/// `Player#canGlide` and `LivingEntity#canGlide`.
fn can_glide(
    (on_ground, abilities, riding, effects, inventory, held_item): (
        &OnGround,
        &PlayerAbilitiesFlags,
        bool,
        &ActiveStatusEffects,
        &Inventory,
        &HeldItem,
    ),
) -> bool {
    !abilities.flying()
        && !on_ground.0
        && !riding
        && !effects.has_effect(StatusEffect::Levitation)
        && equipment_slots(held_item.slot())
            .into_iter()
            .any(|(slot, idx)| can_glide_using(inventory.slot(idx), &slot))
}

/// The player inventory slot behind each equipment slot. Players never wear
/// anything in the body and saddle slots, so those are left out.
fn equipment_slots(held_slot: u16) -> [(EquipSlot, u16); 6] {
    [
        (EquipSlot::MainHand, held_slot),
        (EquipSlot::OffHand, PlayerInventory::SLOT_OFFHAND),
        (EquipSlot::Feet, PlayerInventory::SLOT_FEET),
        (EquipSlot::Legs, PlayerInventory::SLOT_LEGS),
        (EquipSlot::Chest, PlayerInventory::SLOT_CHEST),
        (EquipSlot::Head, PlayerInventory::SLOT_HEAD),
    ]
}

/// `LivingEntity#canGlideUsing`: the stack is a glider meant for the slot it
/// is in, and the next damage won't break it.
fn can_glide_using(stack: &ItemStack, slot: &EquipSlot) -> bool {
    stack
        .effective_component(item_component_type::GLIDER)
        .is_some()
        && matches!(
            stack.effective_component(item_component_type::EQUIPPABLE).as_deref(),
            Some(ItemComponent::Equippable { slot: equippable, .. }) if equippable == slot
        )
        && !next_damage_will_break(stack)
}

/// `ItemStack#nextDamageWillBreak`.
fn next_damage_will_break(stack: &ItemStack) -> bool {
    // `ItemStack#isDamageableItem`: a max damage and a damage, and not
    // unbreakable.
    let Some(&ItemComponent::MaxDamage(VarInt(max_damage))) = stack
        .effective_component(item_component_type::MAX_DAMAGE)
        .as_deref()
    else {
        return false;
    };
    let Some(&ItemComponent::Damage(VarInt(damage))) = stack
        .effective_component(item_component_type::DAMAGE)
        .as_deref()
    else {
        return false;
    };
    if stack
        .effective_component(item_component_type::UNBREAKABLE)
        .is_some()
    {
        return false;
    }

    // `ItemStack#getDamageValue` clamps the damage into `0..=max_damage`.
    damage.max(0).min(max_damage) >= max_damage - 1
}

/// The pose `Player#getDesiredPose` gives for the states ChunkEdge tracks:
/// gliding wins over standing and sneaking. Other poses are left alone.
fn update_pose(flags: &Flags, pose: &mut Mut<entity::Pose>) {
    pose.0 = match pose.0 {
        Pose::Standing | Pose::Sneaking if flags.fall_flying() => Pose::FallFlying,
        Pose::FallFlying if !flags.fall_flying() => {
            if flags.sneaking() {
                Pose::Sneaking
            } else {
                Pose::Standing
            }
        }
        _ => return,
    };
}

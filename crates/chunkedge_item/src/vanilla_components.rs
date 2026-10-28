use chunkedge_binary::{IdOr, VarInt};
use chunkedge_generated::item::ItemKind;
use chunkedge_generated::item_component_type as component_id;
use chunkedge_generated::sound::Sound;
use chunkedge_ident::ident;

use crate::components::{EquipSlot, Patchable};
use crate::{ItemComponent, NUM_ITEM_COMPONENTS};

pub(crate) trait ItemKindExt {
    /// Returns the default components for the [`ItemKind`].
    fn default_components(&self) -> [Patchable<Box<ItemComponent>>; NUM_ITEM_COMPONENTS];

    /// Returns the default component with the given ID for the [`ItemKind`],
    /// as far as ChunkEdge knows it.
    fn default_component(&self, id: usize) -> Option<ItemComponent>;
}

impl ItemKindExt for ItemKind {
    fn default_components(&self) -> [Patchable<Box<ItemComponent>>; NUM_ITEM_COMPONENTS] {
        // TODO: Implement via buildscript

        [const { Patchable::None }; NUM_ITEM_COMPONENTS]
    }

    fn default_component(&self, id: usize) -> Option<ItemComponent> {
        let mut defaults = self.default_components();
        if let Some(component) =
            std::mem::replace(defaults.get_mut(id)?, Patchable::None).to_option()
        {
            return Some(*component);
        }

        // Until `default_components` is generated, the only known defaults are
        // the durability ones, which follow from the extracted max durability,
        // and the elytra's gliding ones. They are kept out of
        // `default_components` because stacks built from it would no longer
        // compare equal to stacks built with `ItemStack::new`.
        let max_durability = self.max_durability();
        match id {
            component_id::MAX_DAMAGE if max_durability > 0 => {
                Some(ItemComponent::MaxDamage(VarInt(max_durability.into())))
            }
            component_id::DAMAGE if max_durability > 0 => Some(ItemComponent::Damage(VarInt(0))),
            component_id::GLIDER if *self == ItemKind::Elytra => Some(ItemComponent::Glider),
            component_id::EQUIPPABLE if *self == ItemKind::Elytra => Some(elytra_equippable()),
            _ => None,
        }
    }
}

fn elytra_equippable() -> ItemComponent {
    ItemComponent::Equippable {
        slot: EquipSlot::Chest,
        equip_sound: IdOr::id(Sound::ItemArmorEquipElytra),
        model: Some(ident!("elytra").into()),
        camera_overlay: None,
        allowed_entities: None,
        dispensable: true,
        swappable: true,
        damage_on_hurt: false,
        equip_on_interact: false,
        can_be_sheared: false,
        shearing_sound: IdOr::id(Sound::ItemShearsSnip),
    }
}
